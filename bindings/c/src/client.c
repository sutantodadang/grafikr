#include "grafikr_client.h"
#include "cJSON.h"

#if defined(_WIN32)
#include <windows.h>
typedef volatile LONG gf_atomic_int;
#define gf_atomic_init(p, value) (*(p) = (LONG)(value))
#define gf_atomic_exchange(p, value) InterlockedExchange((p), (LONG)(value))
#define gf_atomic_store(p, value) InterlockedExchange((p), (LONG)(value))
#define gf_atomic_load(p) InterlockedCompareExchange((p), 0, 0)
#else
#include <stdatomic.h>
typedef atomic_int gf_atomic_int;
#define gf_atomic_init(p, value) atomic_init((p), (value))
#define gf_atomic_exchange(p, value) atomic_exchange((p), (value))
#define gf_atomic_store(p, value) atomic_store((p), (value))
#define gf_atomic_load(p) atomic_load((p))
#endif

#include <stdio.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include "thread_compat.h"
#include <time.h>

#define GF_MAX_FRAME_BYTES (8u * 1024u * 1024u)
#define GF_MAX_PENDING_REQUESTS 256u
#define GF_MAX_PENDING_BYTES (16u * 1024u * 1024u)
#define GF_MAX_QUEUED_EVENTS 1024u
#define GF_MAX_QUEUED_EVENT_BYTES (16u * 1024u * 1024u)
#define GF_REQUEST_TIMEOUT_MS 30000u

typedef struct gf_pending_request {
    struct gf_pending_request *next;
    char id[21];
    size_t bytes;
    gf_status status;
    gf_value *value;
    int done;
} gf_pending_request;

typedef struct gf_queued_event {
    struct gf_queued_event *next;
    uint8_t *frame;
    size_t length;
} gf_queued_event;

struct gf_subscription {
    gf_client *client;
    struct gf_subscription *next;
    uint64_t local_id;
    char node[21];
    char handle[21];
    char *event;
    gf_handler_fn callback;
    void *userdata;
    int active;
    size_t in_flight;
    int released;
    int off_in_progress;
    int off_waiting;
};

struct gf_collection_provider {
    gf_client *client;
    gf_subscription *subscription;
    uint64_t node;
    mtx_t mutex;
    cnd_t changed;
    gf_collection_request request;
    gf_status failure;
    int has_request;
    int closed;
    int reader_waiting;
};

struct gf_client {
    uint64_t session;
    gf_atomic_int running;
    thrd_t creator;
    mtx_t mutex;
    cnd_t changed;
    uint64_t next_request_id;
    size_t pending_count;
    size_t pending_bytes;
    gf_pending_request *pending;
    gf_queued_event *events_head;
    gf_queued_event *events_tail;
    size_t event_count;
    size_t event_bytes;
    gf_subscription *subscriptions;
    size_t subscription_count;
    uint64_t next_subscription_id;
    size_t references;
    gf_status terminal_status;
    gf_status worker_status;
    gf_setup_fn setup;
    void *userdata;
    int high_level;
    int closing;
    int failed;
    int destroyed;
    thrd_t reader;
    thrd_t worker;
    int reader_started;
    int worker_started;
    int callback_dispatching;
    uint64_t dispatch_subscription_id;
};

static int gf_reader_main(void *userdata);
static int gf_worker_main(void *userdata);
static void gf_dispatch_event(gf_client *client, gf_queued_event *event);
static void gf_client_release_ref(gf_client *client);

static struct timespec gf_deadline(uint32_t milliseconds) {
    struct timespec deadline;
    (void)timespec_get(&deadline, TIME_UTC);
    deadline.tv_sec += (time_t)(milliseconds / 1000u);
    deadline.tv_nsec += (long)(milliseconds % 1000u) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_sec++;
        deadline.tv_nsec -= 1000000000L;
    }
    return deadline;
}

static void gf_pending_remove_locked(gf_client *client, gf_pending_request *pending) {
    gf_pending_request **cursor = &client->pending;
    while (*cursor && *cursor != pending) cursor = &(*cursor)->next;
    if (*cursor) *cursor = pending->next;
    if (!pending->done) {
        client->pending_count--;
        client->pending_bytes -= pending->bytes;
    }
    gf_value_free(pending->value);
}

static void gf_subscription_remove_locked(gf_client *client, gf_subscription *subscription) {
    gf_subscription **cursor = &client->subscriptions;
    while (*cursor && *cursor != subscription) cursor = &(*cursor)->next;
    if (*cursor) {
        *cursor = subscription->next;
        client->subscription_count--;
    }
    subscription->active = 0;
}

static void gf_client_release_ref(gf_client *client) {
    int release = 0;
    if (mtx_lock(&client->mutex) != thrd_success) return;
    if (client->references != 0) client->references--;
    release = client->references == 0;
    (void)mtx_unlock(&client->mutex);
    if (!release) return;
    gf_queued_event *event = client->events_head;
    while (event) {
        gf_queued_event *next = event->next;
        free(event->frame);
        free(event);
        event = next;
    }
    cnd_destroy(&client->changed);
    mtx_destroy(&client->mutex);
    free(client);
}

static void gf_subscription_free(gf_subscription *subscription) {
    if (!subscription) return;
    gf_client *client = subscription->client;
    free(subscription->event);
    free(subscription);
    gf_client_release_ref(client);
}

static void gf_fail_pending_locked(gf_client *client, gf_status status) {
    if (client->failed) return;
    client->failed = 1;
    client->terminal_status = status;
    client->closing = 1;
    for (gf_pending_request *pending = client->pending; pending; pending = pending->next) {
        if (!pending->done) {
            pending->done = 1;
            pending->status = status;
            client->pending_count--;
            client->pending_bytes -= pending->bytes;
        }
    }
    (void)cnd_broadcast(&client->changed);
}

static gf_status gf_enqueue_event(gf_client *client, uint8_t *frame, size_t length) {
    gf_queued_event *event = (gf_queued_event *)calloc(1, sizeof(*event));
    if (!event) return GRAFIKR_INTERNAL_FAILURE;
    event->frame = frame;
    event->length = length;

    if (mtx_lock(&client->mutex) != thrd_success) {
        free(event);
        return GRAFIKR_INTERNAL_FAILURE;
    }
    if (client->closing || client->failed || client->event_count >= GF_MAX_QUEUED_EVENTS ||
        length > GF_MAX_QUEUED_EVENT_BYTES - client->event_bytes) {
        gf_fail_pending_locked(client, GRAFIKR_QUEUE_FULL);
        (void)mtx_unlock(&client->mutex);
        free(event);
        return GRAFIKR_QUEUE_FULL;
    }
    if (client->events_tail) client->events_tail->next = event;
    else client->events_head = event;
    client->events_tail = event;
    client->event_count++;
    client->event_bytes += length;
    (void)cnd_broadcast(&client->changed);
    (void)mtx_unlock(&client->mutex);
    return GRAFIKR_OK;
}

static gf_status gf_complete_response(gf_client *client, const gf_value *message) {
    const gf_value *id_value = NULL;
    const gf_value *kind_value = NULL;
    const gf_value *ok_value = NULL;
    const gf_value *payload = NULL;
    const char *id = NULL;
    const char *kind = NULL;
    bool ok = false;
    if (gf_value_object_get(message, "kind", &kind_value) != GRAFIKR_OK ||
        gf_value_get_string(kind_value, &kind) != GRAFIKR_OK || strcmp(kind, "response") != 0 ||
        gf_value_object_get(message, "id", &id_value) != GRAFIKR_OK ||
        gf_value_get_string(id_value, &id) != GRAFIKR_OK ||
        gf_value_object_get(message, "ok", &ok_value) != GRAFIKR_OK ||
        gf_value_get_bool(ok_value, &ok) != GRAFIKR_OK ||
        gf_value_object_get(message, ok ? "result" : "error", &payload) != GRAFIKR_OK)
        return GRAFIKR_INTERNAL_FAILURE;

    if (mtx_lock(&client->mutex) != thrd_success) return GRAFIKR_INTERNAL_FAILURE;
    gf_pending_request *pending = client->pending;
    while (pending && strcmp(pending->id, id) != 0) pending = pending->next;
    if (pending && !pending->done) {
        pending->value = gf_value_clone(payload);
        if (!pending->value) {
            pending->status = GRAFIKR_INTERNAL_FAILURE;
        } else {
            pending->status = ok ? GRAFIKR_OK : GRAFIKR_BRIDGE_ERROR;
        }
        pending->done = 1;
        client->pending_count--;
        client->pending_bytes -= pending->bytes;
        (void)cnd_broadcast(&client->changed);
    }
    (void)mtx_unlock(&client->mutex);
    return GRAFIKR_OK;
}

static gf_status gf_process_frame(gf_client *client, uint8_t *frame, size_t length) {
    if (!frame || length < 2 || length > GF_MAX_FRAME_BYTES || frame[length - 1] != '\n')
        return GRAFIKR_INTERNAL_FAILURE;
    for (size_t i = 0; i + 1 < length; i++) {
        if (frame[i] == '\n' || frame[i] == '\r') return GRAFIKR_INTERNAL_FAILURE;
    }
    frame[length - 1] = '\0';
    gf_value *message = cJSON_ParseWithLengthOpts((const char *)frame, length, NULL, 1);
    if (!message) return GRAFIKR_INTERNAL_FAILURE;
    const gf_value *kind_value = NULL;
    const char *kind = NULL;
    gf_status status = GRAFIKR_INTERNAL_FAILURE;
    if (gf_value_object_get(message, "kind", &kind_value) == GRAFIKR_OK &&
        gf_value_get_string(kind_value, &kind) == GRAFIKR_OK) {
        if (strcmp(kind, "response") == 0) status = gf_complete_response(client, message);
        else if (strcmp(kind, "event") == 0) {
            frame[length - 1] = '\n';
            status = gf_enqueue_event(client, frame, length);
            if (status == GRAFIKR_OK) frame = NULL;
        }
    }
    gf_value_free(message);
    free(frame);
    return status;
}

static void gf_subscription_callback_done(gf_client *client, gf_subscription *subscription, uint64_t local_id) {
    int release = 0;
    if (mtx_lock(&client->mutex) != thrd_success) return;
    if (client->callback_dispatching && client->dispatch_subscription_id == local_id) {
        client->callback_dispatching = 0;
        client->dispatch_subscription_id = 0;
    }
    if (subscription->in_flight != 0) subscription->in_flight--;
    release = subscription->released && !subscription->off_in_progress &&
              !subscription->off_waiting && subscription->in_flight == 0;
    (void)cnd_broadcast(&client->changed);
    (void)mtx_unlock(&client->mutex);
    if (release) gf_subscription_free(subscription);
}

static void gf_set_terminal(gf_client *client, gf_status status) {
    if (mtx_lock(&client->mutex) != thrd_success) return;
    gf_fail_pending_locked(client, status);
    (void)mtx_unlock(&client->mutex);
}

static int gf_reader_main(void *userdata) {
    gf_client *client = (gf_client *)userdata;
    for (;;) {
        if (mtx_lock(&client->mutex) != thrd_success) break;
        int closing = client->closing;
        (void)mtx_unlock(&client->mutex);
        if (closing) break;

        size_t required = 0;
        gf_status status = grafikr_next_event(client->session, NULL, 0, &required, 100);
        if (status == GRAFIKR_TIMEOUT) continue;
        if (status == GRAFIKR_STOPPED) {
            gf_set_terminal(client, GRAFIKR_STOPPED);
            break;
        }
        if (status != GRAFIKR_BUFFER_TOO_SMALL || required < 2 || required > GF_MAX_FRAME_BYTES) {
            gf_set_terminal(client, status == GRAFIKR_OK ? GRAFIKR_INTERNAL_FAILURE : status);
            (void)grafikr_request_stop(client->session);
            break;
        }
        uint8_t *frame = (uint8_t *)malloc(required);
        if (!frame) {
            gf_set_terminal(client, GRAFIKR_INTERNAL_FAILURE);
            (void)grafikr_request_stop(client->session);
            break;
        }
        size_t actual = required;
        status = grafikr_next_event(client->session, frame, required, &actual, 0);
        if (status != GRAFIKR_OK || actual != required) {
            free(frame);
            if (status == GRAFIKR_STOPPED) gf_set_terminal(client, GRAFIKR_STOPPED);
            else {
                gf_set_terminal(client, status == GRAFIKR_OK ? GRAFIKR_INTERNAL_FAILURE : status);
                (void)grafikr_request_stop(client->session);
            }
            break;
        }
        status = gf_process_frame(client, frame, actual);
        if (status != GRAFIKR_OK) {
            gf_set_terminal(client, status);
            (void)grafikr_request_stop(client->session);
            break;
        }
    }
    return 0;
}

static void gf_dispatch_event(gf_client *client, gf_queued_event *event) {
    if (!event || event->length < 2 || event->frame[event->length - 1] != '\n') {
        if (event) {
            free(event->frame);
            free(event);
        }
        gf_set_terminal(client, GRAFIKR_INTERNAL_FAILURE);
        (void)grafikr_request_stop(client->session);
        return;
    }
    event->frame[event->length - 1] = '\0';
    gf_value *message = cJSON_ParseWithLengthOpts((const char *)event->frame, event->length, NULL, 1);
    free(event->frame);
    free(event);
    if (!message) {
        gf_set_terminal(client, GRAFIKR_INTERNAL_FAILURE);
        (void)grafikr_request_stop(client->session);
        return;
    }
    const gf_value *source_value = NULL;
    const gf_value *name_value = NULL;
    const char *source = NULL;
    const char *name = NULL;
    if (gf_value_object_get(message, "source", &source_value) != GRAFIKR_OK ||
        gf_value_get_string(source_value, &source) != GRAFIKR_OK ||
        gf_value_object_get(message, "event", &name_value) != GRAFIKR_OK ||
        gf_value_get_string(name_value, &name) != GRAFIKR_OK) {
        gf_value_free(message);
        gf_set_terminal(client, GRAFIKR_INTERNAL_FAILURE);
        (void)grafikr_request_stop(client->session);
        return;
    }

    uint64_t *local_ids = NULL;
    size_t count = 0;
    if (mtx_lock(&client->mutex) != thrd_success) {
        gf_value_free(message);
        gf_set_terminal(client, GRAFIKR_INTERNAL_FAILURE);
        (void)grafikr_request_stop(client->session);
        return;
    }
    for (gf_subscription *sub = client->subscriptions; sub; sub = sub->next) {
        if (sub->active && strcmp(sub->node, source) == 0 && strcmp(sub->event, name) == 0)
            count++;
    }
    if (count && count <= SIZE_MAX / sizeof(*local_ids))
        local_ids = (uint64_t *)malloc(count * sizeof(*local_ids));
    if (count && !local_ids) {
        gf_fail_pending_locked(client, GRAFIKR_INTERNAL_FAILURE);
        (void)mtx_unlock(&client->mutex);
        gf_value_free(message);
        (void)grafikr_request_stop(client->session);
        return;
    }
    size_t index = 0;
    for (gf_subscription *sub = client->subscriptions; sub; sub = sub->next) {
        if (sub->active && strcmp(sub->node, source) == 0 && strcmp(sub->event, name) == 0)
            local_ids[index++] = sub->local_id;
    }
    (void)mtx_unlock(&client->mutex);

    for (index = 0; index < count; index++) {
        gf_handler_fn callback = NULL;
        void *callback_userdata = NULL;
        gf_subscription *dispatch_subscription = NULL;
        if (mtx_lock(&client->mutex) != thrd_success) break;
        int closing = client->closing;
        for (gf_subscription *sub = client->subscriptions; sub; sub = sub->next) {
            if (sub->active && sub->local_id == local_ids[index]) {
                callback = sub->callback;
                callback_userdata = sub->userdata;
                dispatch_subscription = sub;
                sub->in_flight++;
                client->callback_dispatching = 1;
                client->dispatch_subscription_id = sub->local_id;
                break;
            }
        }
        (void)mtx_unlock(&client->mutex);
        if (closing) {
            if (dispatch_subscription) gf_subscription_callback_done(client, dispatch_subscription, local_ids[index]);
            break;
        }
        if (callback) callback(client, message, callback_userdata);
        if (dispatch_subscription) gf_subscription_callback_done(client, dispatch_subscription, local_ids[index]);
    }
    free(local_ids);
    gf_value_free(message);
}

static gf_value *gf_hello_args(void) {
    gf_value *args = gf_value_object();
    if (!args) return NULL;
    if (gf_value_object_set(args, "protocol_major", gf_value_i64(1)) != GRAFIKR_OK ||
        gf_value_object_set(args, "protocol_minor", gf_value_i64(0)) != GRAFIKR_OK ||
        gf_value_object_set(args, "sdk_version", gf_value_string("1.1.0")) != GRAFIKR_OK) {
        gf_value_free(args);
        return NULL;
    }
    return args;
}

static int gf_worker_main(void *userdata) {
    gf_client *client = (gf_client *)userdata;
    gf_value *args = gf_hello_args();
    gf_value *result = NULL;
    if (!args) client->worker_status = GRAFIKR_INTERNAL_FAILURE;
    else client->worker_status = gf_request(client, "hello", args, &result);
    gf_value_free(args);
    if (client->worker_status == GRAFIKR_OK) {
        const gf_value *protocol = NULL;
        const gf_value *major = NULL;
        double version = 0;
        if (!result || gf_value_object_get(result, "protocol", &protocol) != GRAFIKR_OK ||
            gf_value_object_get(protocol, "major", &major) != GRAFIKR_OK ||
            gf_value_get_f64(major, &version) != GRAFIKR_OK || version != 1.0) {
            client->worker_status = GRAFIKR_INVALID_ARGUMENT;
        }
    }
    gf_value_free(result);
    if (client->worker_status == GRAFIKR_OK && client->setup) {
        client->setup(client, client->userdata);
    }
    if (client->worker_status != GRAFIKR_OK) {
        gf_set_terminal(client, client->worker_status);
        (void)grafikr_request_stop(client->session);
        return 0;
    }
    for (;;) {
        if (mtx_lock(&client->mutex) != thrd_success) break;
        while (!client->events_head && !client->closing)
            if (cnd_wait(&client->changed, &client->mutex) != thrd_success) break;
        gf_queued_event *event = client->events_head;
        if (event) {
            client->events_head = event->next;
            if (!client->events_head) client->events_tail = NULL;
            client->event_count--;
            client->event_bytes -= event->length;
            (void)cnd_broadcast(&client->changed);
        }
        int closing = client->closing;
        (void)mtx_unlock(&client->mutex);
        if (!event) {
            if (closing) break;
            continue;
        }
        gf_dispatch_event(client, event);
    }
    return 0;
}

gf_status gf_client_create(gf_client **out) {
    if (!out) return GRAFIKR_INVALID_ARGUMENT;
    *out = NULL;
    gf_client *client = (gf_client *)calloc(1, sizeof(*client));
    if (!client) return GRAFIKR_INTERNAL_FAILURE;
    gf_atomic_init(&client->running, 0);
    client->creator = thrd_current();
    client->next_request_id = 1;
    client->next_subscription_id = 1;
    client->references = 1;
    client->terminal_status = GRAFIKR_STOPPED;
    client->worker_status = GRAFIKR_OK;
    if (mtx_init(&client->mutex, mtx_plain) != thrd_success) {
        free(client);
        return GRAFIKR_INTERNAL_FAILURE;
    }
    if (cnd_init(&client->changed) != thrd_success) {
        mtx_destroy(&client->mutex);
        free(client);
        return GRAFIKR_INTERNAL_FAILURE;
    }
    gf_status status = grafikr_session_create(&client->session);
    if (status != GRAFIKR_OK) {
        cnd_destroy(&client->changed);
        mtx_destroy(&client->mutex);
        free(client);
        return status;
    }
    *out = client;
    return GRAFIKR_OK;
}

gf_status gf_client_submit(gf_client *client, const uint8_t *request, size_t length) {
    if (!client) return GRAFIKR_INVALID_ARGUMENT;
    if (client->high_level) return GRAFIKR_WRONG_LIFECYCLE;
    return grafikr_submit(client->session, request, length);
}

gf_status gf_client_run(gf_client *client, gf_setup_fn setup, void *userdata) {
    if (!client) return GRAFIKR_INVALID_ARGUMENT;
    if (!thrd_equal(thrd_current(), client->creator)) return GRAFIKR_WRONG_THREAD;
    if (gf_atomic_exchange(&client->running, 1)) return GRAFIKR_WRONG_LIFECYCLE;
    client->setup = setup;
    client->userdata = userdata;
    client->high_level = setup != NULL;
    gf_status status = GRAFIKR_OK;
    if (client->high_level) {
        if (thrd_create(&client->reader, gf_reader_main, client) != thrd_success) {
            status = GRAFIKR_INTERNAL_FAILURE;
            goto finished;
        }
        client->reader_started = 1;
        if (thrd_create(&client->worker, gf_worker_main, client) != thrd_success) {
            status = GRAFIKR_INTERNAL_FAILURE;
            (void)grafikr_request_stop(client->session);
        } else {
            client->worker_started = 1;
        }
    }
    {
        gf_status run_status = grafikr_run(client->session);
        if (status == GRAFIKR_OK) status = run_status;
    }
    if (client->high_level) {
        if (mtx_lock(&client->mutex) == thrd_success) {
            gf_fail_pending_locked(client, GRAFIKR_STOPPED);
            (void)mtx_unlock(&client->mutex);
        }
        (void)grafikr_request_stop(client->session);
        if (client->reader_started) {
            int result = 0;
            (void)thrd_join(client->reader, &result);
            client->reader_started = 0;
        }
        if (client->worker_started) {
            int result = 0;
            (void)thrd_join(client->worker, &result);
            client->worker_started = 0;
        }
        if (status == GRAFIKR_OK && client->worker_status != GRAFIKR_OK)
            status = client->worker_status;
    }
finished:
    gf_atomic_store(&client->running, 0);
    return status;
}

gf_status gf_request(gf_client *client, const char *op, const gf_value *args, gf_value **out) {
    if (!client || !op || !args || !out || !cJSON_IsObject(args)) return GRAFIKR_INVALID_ARGUMENT;
    *out = NULL;
    if (!client->high_level || !gf_atomic_load(&client->running)) return GRAFIKR_WRONG_LIFECYCLE;

    gf_value *request = gf_value_object();
    if (!request) return GRAFIKR_INTERNAL_FAILURE;
    gf_status status = GRAFIKR_INTERNAL_FAILURE;
    char id[21];
    gf_pending_request pending = {0};
    uint8_t *frame = NULL;
    size_t frame_bytes = 0;
    size_t json_bytes = 0;
    size_t capacity = 0;

    if (mtx_lock(&client->mutex) != thrd_success) goto cleanup;
    if (client->closing || client->failed) {
        status = GRAFIKR_STOPPED;
        (void)mtx_unlock(&client->mutex);
        goto cleanup;
    }
    if (client->next_request_id == UINT64_MAX) {
        status = GRAFIKR_QUEUE_FULL;
        (void)mtx_unlock(&client->mutex);
        goto cleanup;
    }
    (void)snprintf(id, sizeof(id), "%llu", (unsigned long long)client->next_request_id++);
    (void)mtx_unlock(&client->mutex);

    if (gf_value_object_set(request, "id", gf_value_string(id)) != GRAFIKR_OK ||
        gf_value_object_set(request, "op", gf_value_string(op)) != GRAFIKR_OK ||
        gf_value_object_set(request, "args", gf_value_clone(args)) != GRAFIKR_OK)
        goto cleanup;
    status = gf_value_write_json(request, NULL, 0, &capacity);
    if (status != GRAFIKR_BUFFER_TOO_SMALL || capacity < 2 || capacity > GF_MAX_FRAME_BYTES)
        goto cleanup;
    json_bytes = capacity - 1;
    frame_bytes = json_bytes + 1;
    frame = (uint8_t *)malloc(frame_bytes);
    if (!frame) {
        status = GRAFIKR_INTERNAL_FAILURE;
        goto cleanup;
    }
    status = gf_value_write_json(request, (char *)frame, capacity, &capacity);
    if (status != GRAFIKR_OK) goto cleanup;
    frame[frame_bytes - 1] = '\n';

    (void)memcpy(pending.id, id, strlen(id) + 1);
    pending.bytes = frame_bytes;
    if (mtx_lock(&client->mutex) != thrd_success) {
        status = GRAFIKR_INTERNAL_FAILURE;
        goto cleanup;
    }
    if (client->closing || client->failed) status = GRAFIKR_STOPPED;
    else if (client->pending_count >= GF_MAX_PENDING_REQUESTS ||
             frame_bytes > GF_MAX_PENDING_BYTES - client->pending_bytes)
        status = GRAFIKR_QUEUE_FULL;
    else {
        pending.next = client->pending;
        client->pending = &pending;
        client->pending_count++;
        client->pending_bytes += frame_bytes;
        status = GRAFIKR_OK;
    }
    (void)mtx_unlock(&client->mutex);
    if (status != GRAFIKR_OK) goto cleanup;

    status = grafikr_submit(client->session, frame, frame_bytes);
    free(frame);
    frame = NULL;
    if (status != GRAFIKR_OK) {
        if (mtx_lock(&client->mutex) == thrd_success) {
            gf_pending_remove_locked(client, &pending);
            (void)mtx_unlock(&client->mutex);
        }
        goto cleanup;
    }

    struct timespec deadline = gf_deadline(GF_REQUEST_TIMEOUT_MS);
    if (mtx_lock(&client->mutex) != thrd_success) {
        status = GRAFIKR_INTERNAL_FAILURE;
        goto cleanup;
    }
    while (!pending.done) {
        int wait_status = cnd_timedwait(&client->changed, &client->mutex, &deadline);
        if (wait_status == thrd_timedout && !pending.done) {
            gf_pending_remove_locked(client, &pending);
            (void)mtx_unlock(&client->mutex);
            status = GRAFIKR_TIMEOUT;
            goto cleanup;
        }
        if (wait_status != thrd_success && wait_status != thrd_timedout) {
            gf_pending_remove_locked(client, &pending);
            (void)mtx_unlock(&client->mutex);
            status = GRAFIKR_INTERNAL_FAILURE;
            goto cleanup;
        }
    }
    gf_value *response_value = pending.value;
    pending.value = NULL;
    gf_pending_remove_locked(client, &pending);
    status = pending.status;
    *out = response_value;
    pending.value = NULL;
    (void)mtx_unlock(&client->mutex);

cleanup:
    free(frame);
    gf_value_free(request);
    gf_value_free(pending.value);
    return status;
}

gf_status gf_client_next(gf_client *client, uint8_t *buffer, size_t capacity, size_t *required, uint32_t timeout_ms) {
    if (!client) return GRAFIKR_INVALID_ARGUMENT;
    if (!client->high_level) return grafikr_next_event(client->session, buffer, capacity, required, timeout_ms);
    (void)buffer;
    (void)capacity;
    (void)required;
    (void)timeout_ms;
    return GRAFIKR_WRONG_LIFECYCLE;
}

gf_status gf_client_stop(gf_client *client) {
    if (!client) return GRAFIKR_INVALID_ARGUMENT;
    gf_status status = grafikr_request_stop(client->session);
    if (client->high_level && mtx_lock(&client->mutex) == thrd_success) {
        gf_fail_pending_locked(client, GRAFIKR_STOPPED);
        (void)mtx_unlock(&client->mutex);
    }
    return status;
}

gf_status gf_client_close(gf_client *client) { return gf_client_stop(client); }

gf_status gf_on(gf_client *client, uint64_t node, const char *event, gf_handler_fn callback,
                void *userdata, gf_subscription **out) {
    if (!client || !event || !*event || !callback || !out) return GRAFIKR_INVALID_ARGUMENT;
    *out = NULL;
    if (!client->high_level || !gf_atomic_load(&client->running)) return GRAFIKR_WRONG_LIFECYCLE;
    gf_subscription *subscription = (gf_subscription *)calloc(1, sizeof(*subscription));
    if (!subscription) return GRAFIKR_INTERNAL_FAILURE;
    subscription->event = (char *)malloc(strlen(event) + 1);
    if (!subscription->event) {
        free(subscription);
        return GRAFIKR_INTERNAL_FAILURE;
    }
    (void)strcpy(subscription->event, event);
    subscription->client = client;
    subscription->callback = callback;
    subscription->userdata = userdata;
    subscription->active = 1;
    (void)snprintf(subscription->node, sizeof(subscription->node), "%llu", (unsigned long long)node);

    if (mtx_lock(&client->mutex) != thrd_success) {
        free(subscription->event);
        free(subscription);
        return GRAFIKR_INTERNAL_FAILURE;
    }
    if (client->closing || client->failed || client->destroyed ||
        client->subscription_count >= GF_MAX_QUEUED_EVENTS || client->next_subscription_id == UINT64_MAX) {
        gf_status status = client->closing || client->failed ? GRAFIKR_STOPPED : GRAFIKR_QUEUE_FULL;
        (void)mtx_unlock(&client->mutex);
        free(subscription->event);
        free(subscription);
        return status;
    }
    subscription->local_id = client->next_subscription_id++;
    subscription->next = client->subscriptions;
    client->subscriptions = subscription;
    client->subscription_count++;
    client->references++;
    (void)mtx_unlock(&client->mutex);

    gf_value *args = gf_value_object();
    gf_value *result = NULL;
    gf_status status = args ? GRAFIKR_OK : GRAFIKR_INTERNAL_FAILURE;
    if (status == GRAFIKR_OK &&
        (gf_value_object_set(args, "node", gf_value_string(subscription->node)) != GRAFIKR_OK ||
         gf_value_object_set(args, "event", gf_value_string(event)) != GRAFIKR_OK))
        status = GRAFIKR_INTERNAL_FAILURE;
    if (status == GRAFIKR_OK) status = gf_request(client, "event.subscribe", args, &result);
    gf_value_free(args);
    if (status == GRAFIKR_OK) {
        const gf_value *handle = NULL;
        const char *text = NULL;
        if (gf_value_object_get(result, "subscription", &handle) != GRAFIKR_OK ||
            gf_value_get_string(handle, &text) != GRAFIKR_OK || strlen(text) >= sizeof(subscription->handle))
            status = GRAFIKR_INTERNAL_FAILURE;
        else (void)strcpy(subscription->handle, text);
    }
    gf_value_free(result);
    if (status != GRAFIKR_OK) {
        if (mtx_lock(&client->mutex) == thrd_success) {
            gf_subscription_remove_locked(client, subscription);
            (void)mtx_unlock(&client->mutex);
        }
        gf_subscription_free(subscription);
        return status;
    }
    *out = subscription;
    return GRAFIKR_OK;
}

gf_status gf_off(gf_subscription *subscription) {
    if (!subscription) return GRAFIKR_INVALID_ARGUMENT;
    gf_client *client = subscription->client;
    int active = 0;
    int destroyed = 0;
    if (mtx_lock(&client->mutex) != thrd_success) return GRAFIKR_INTERNAL_FAILURE;
    active = subscription->active;
    destroyed = client->destroyed;
    if (active) gf_subscription_remove_locked(client, subscription);
    subscription->released = 1;
    subscription->off_in_progress = 1;
    int called_from_callback = client->callback_dispatching &&
                              client->dispatch_subscription_id == subscription->local_id &&
                              thrd_equal(thrd_current(), client->worker);
    subscription->off_waiting = !called_from_callback;
    (void)mtx_unlock(&client->mutex);

    gf_status status = GRAFIKR_OK;
    if (active && !destroyed && gf_atomic_load(&client->running)) {
        gf_value *args = gf_value_object();
        gf_value *result = NULL;
        if (!args) status = GRAFIKR_INTERNAL_FAILURE;
        else if (gf_value_object_set(args, "subscription", gf_value_string(subscription->handle)) != GRAFIKR_OK)
            status = GRAFIKR_INTERNAL_FAILURE;
        else status = gf_request(client, "event.unsubscribe", args, &result);
        gf_value_free(args);
        gf_value_free(result);
    } else if (active && !destroyed) status = GRAFIKR_WRONG_LIFECYCLE;

    if (mtx_lock(&client->mutex) == thrd_success) {
        subscription->off_in_progress = 0;
        if (!called_from_callback) {
            while (subscription->in_flight != 0)
                (void)cnd_wait(&client->changed, &client->mutex);
            subscription->off_waiting = 0;
        }
        int release = subscription->in_flight == 0;
        (void)mtx_unlock(&client->mutex);
        if (release) gf_subscription_free(subscription);
        return status;
    }
    return GRAFIKR_INTERNAL_FAILURE;
}

static gf_status gf_collection_set(gf_value *object, const char *key, gf_value *value) {
    if (!object || !value) { gf_value_free(value); return GRAFIKR_INTERNAL_FAILURE; }
    gf_status status = gf_value_object_set(object, key, value);
    if (status != GRAFIKR_OK) gf_value_free(value);
    return status;
}

static gf_status gf_collection_validate(const gf_collection_metadata *metadata) {
    if (!metadata || metadata->row_count > (uint64_t)UINT32_MAX + 1u ||
        !isfinite(metadata->row_height) || metadata->row_height <= 0.0 ||
        !metadata->columns || metadata->column_count == 0 || metadata->column_count > 256)
        return GRAFIKR_INVALID_ARGUMENT;
    for (size_t i = 0; i < metadata->column_count; i++)
        if (!metadata->columns[i]) return GRAFIKR_INVALID_ARGUMENT;
    return GRAFIKR_OK;
}

static gf_value *gf_collection_metadata_value(const gf_collection_metadata *metadata) {
    gf_value *object = gf_value_object();
    gf_value *columns = gf_value_array();
    if (!object || !columns) { gf_value_free(object); gf_value_free(columns); return NULL; }
    if (gf_collection_set(object, "row_count", gf_value_i64((int64_t)metadata->row_count)) != GRAFIKR_OK ||
        gf_collection_set(object, "row_height", gf_value_f64(metadata->row_height)) != GRAFIKR_OK) {
        gf_value_free(object); gf_value_free(columns); return NULL;
    }
    for (size_t i = 0; i < metadata->column_count; i++) {
        gf_value *column = gf_value_string(metadata->columns[i]);
        if (!column || gf_value_array_append(columns, column) != GRAFIKR_OK) {
            gf_value_free(column); gf_value_free(columns); gf_value_free(object); return NULL;
        }
    }
    if (gf_collection_set(object, "columns", columns) != GRAFIKR_OK) {
        gf_value_free(object); return NULL;
    }
    return object;
}

static void gf_collection_provider_event(gf_client *client, const gf_value *event, void *userdata) {
    (void)client;
    gf_collection_provider *provider = (gf_collection_provider *)userdata;
    gf_collection_request request = {{0}, {0}, 0, 0};
    const gf_value *payload = NULL, *revision_value = NULL, *request_value = NULL;
    const gf_value *start_value = NULL, *count_value = NULL;
    const char *revision = NULL, *request_id = NULL;
    int64_t start = -1, count = -1;
    gf_status status = GRAFIKR_INTERNAL_FAILURE;
    if (event &&
        gf_value_object_get(event, "payload", &payload) == GRAFIKR_OK &&
        gf_value_object_get(payload, "revision", &revision_value) == GRAFIKR_OK &&
        gf_value_get_string(revision_value, &revision) == GRAFIKR_OK && strlen(revision) < sizeof(request.revision) &&
        gf_value_object_get(payload, "request", &request_value) == GRAFIKR_OK &&
        gf_value_get_string(request_value, &request_id) == GRAFIKR_OK && strlen(request_id) < sizeof(request.request) &&
        gf_value_object_get(payload, "start", &start_value) == GRAFIKR_OK &&
        gf_value_get_i64(start_value, &start) == GRAFIKR_OK && start >= 0 && (uint64_t)start <= UINT32_MAX &&
        gf_value_object_get(payload, "count", &count_value) == GRAFIKR_OK &&
        gf_value_get_i64(count_value, &count) == GRAFIKR_OK && count >= 0 && count <= 512) {
        (void)strcpy(request.revision, revision);
        (void)strcpy(request.request, request_id);
        request.start = (uint32_t)start;
        request.count = (uint32_t)count;
        status = GRAFIKR_OK;
    }

    if (mtx_lock(&provider->mutex) != thrd_success) return;
    if (!provider->closed) {
        if (status != GRAFIKR_OK) provider->failure = GRAFIKR_INTERNAL_FAILURE;
        else if (provider->has_request) provider->failure = GRAFIKR_QUEUE_FULL;
        else { provider->request = request; provider->has_request = 1; }
        (void)cnd_broadcast(&provider->changed);
    }
    (void)mtx_unlock(&provider->mutex);
}

gf_status gf_collection_provider_create(gf_client *client, uint64_t node,
                                        const gf_collection_metadata *metadata,
                                        gf_collection_provider **out,
                                        gf_value **result_or_error) {
    if (!client || !out) return GRAFIKR_INVALID_ARGUMENT;
    *out = NULL;
    if (result_or_error) *result_or_error = NULL;
    gf_status status = gf_collection_validate(metadata);
    if (status != GRAFIKR_OK) return status;

    gf_collection_provider *provider = (gf_collection_provider *)calloc(1, sizeof(*provider));
    if (!provider) return GRAFIKR_INTERNAL_FAILURE;
    if (mtx_init(&provider->mutex, mtx_plain) != thrd_success) { free(provider); return GRAFIKR_INTERNAL_FAILURE; }
    if (cnd_init(&provider->changed) != thrd_success) { mtx_destroy(&provider->mutex); free(provider); return GRAFIKR_INTERNAL_FAILURE; }
    provider->client = client;
    provider->node = node;

    status = gf_on(client, node, "data.request", gf_collection_provider_event, provider, &provider->subscription);
    if (status != GRAFIKR_OK) {
        cnd_destroy(&provider->changed); mtx_destroy(&provider->mutex); free(provider); return status;
    }

    gf_value *args = gf_collection_metadata_value(metadata);
    if (!args) { (void)gf_collection_provider_close(provider); (void)gf_collection_provider_destroy(provider); return GRAFIKR_INTERNAL_FAILURE; }
    char node_text[21];
    (void)snprintf(node_text, sizeof(node_text), "%llu", (unsigned long long)node);
    status = gf_collection_set(args, "node", gf_value_string(node_text));
    gf_value *response = NULL;
    if (status == GRAFIKR_OK) status = gf_request(client, "collection.create", args, &response);
    gf_value_free(args);
    if (status != GRAFIKR_OK) {
        if (result_or_error && status == GRAFIKR_BRIDGE_ERROR) *result_or_error = response;
        else gf_value_free(response);
        (void)gf_collection_provider_close(provider);
        (void)gf_collection_provider_destroy(provider);
        return status;
    }
    if (result_or_error) *result_or_error = response;
    else gf_value_free(response);
    *out = provider;
    return GRAFIKR_OK;
}

gf_status gf_collection_provider_next(gf_collection_provider *provider,
                                      gf_collection_request *out, uint32_t timeout_ms) {
    if (!provider || !out) return GRAFIKR_INVALID_ARGUMENT;
    if (mtx_lock(&provider->mutex) != thrd_success) return GRAFIKR_INTERNAL_FAILURE;
    if (provider->reader_waiting) { (void)mtx_unlock(&provider->mutex); return GRAFIKR_READER_BUSY; }
    provider->reader_waiting = 1;
    struct timespec deadline = gf_deadline(timeout_ms);
    while (!provider->has_request && provider->failure == GRAFIKR_OK && !provider->closed) {
        if (timeout_ms == 0 || cnd_timedwait(&provider->changed, &provider->mutex, &deadline) == thrd_timedout) {
            provider->reader_waiting = 0;
            (void)mtx_unlock(&provider->mutex);
            return GRAFIKR_TIMEOUT;
        }
    }
    provider->reader_waiting = 0;
    if (provider->has_request) {
        *out = provider->request;
        provider->has_request = 0;
        (void)mtx_unlock(&provider->mutex);
        return GRAFIKR_OK;
    }
    if (provider->failure != GRAFIKR_OK) {
        gf_status failure = provider->failure;
        provider->failure = GRAFIKR_STOPPED;
        (void)mtx_unlock(&provider->mutex);
        return failure;
    }
    (void)mtx_unlock(&provider->mutex);
    return GRAFIKR_STOPPED;
}

gf_status gf_collection_provider_provide(gf_collection_provider *provider,
                                         const gf_collection_request *request,
                                         const gf_value *rows, gf_value **result_or_error) {
    if (!provider || !request || !rows || !result_or_error ||
        !memchr(request->revision, '\0', sizeof(request->revision)) ||
        !memchr(request->request, '\0', sizeof(request->request))) return GRAFIKR_INVALID_ARGUMENT;
    *result_or_error = NULL;
    if (mtx_lock(&provider->mutex) != thrd_success) return GRAFIKR_INTERNAL_FAILURE;
    int closed = provider->closed;
    (void)mtx_unlock(&provider->mutex);
    if (closed) return GRAFIKR_STOPPED;
    gf_value *args = gf_value_object();
    if (!args) return GRAFIKR_INTERNAL_FAILURE;
    char node_text[21]; (void)snprintf(node_text, sizeof(node_text), "%llu", (unsigned long long)provider->node);
    gf_status status = gf_collection_set(args, "node", gf_value_string(node_text));
    if (status == GRAFIKR_OK) status = gf_collection_set(args, "revision", gf_value_string(request->revision));
    if (status == GRAFIKR_OK) status = gf_collection_set(args, "request", gf_value_string(request->request));
    if (status == GRAFIKR_OK) status = gf_collection_set(args, "start", gf_value_i64(request->start));
    if (status == GRAFIKR_OK) status = gf_collection_set(args, "rows", gf_value_clone(rows));
    if (status == GRAFIKR_OK) status = gf_request(provider->client, "data.provide", args, result_or_error);
    gf_value_free(args);
    return status;
}

gf_status gf_collection_provider_update(gf_collection_provider *provider,
                                       const gf_collection_metadata *metadata,
                                       gf_value **result_or_error) {
    if (!provider || !result_or_error) return GRAFIKR_INVALID_ARGUMENT;
    *result_or_error = NULL;
    gf_status status = gf_collection_validate(metadata);
    if (status != GRAFIKR_OK) return status;
    if (mtx_lock(&provider->mutex) != thrd_success) return GRAFIKR_INTERNAL_FAILURE;
    int closed = provider->closed;
    (void)mtx_unlock(&provider->mutex);
    if (closed) return GRAFIKR_STOPPED;
    gf_value *args = gf_value_object();
    gf_value *values = gf_collection_metadata_value(metadata);
    if (!args || !values) { gf_value_free(args); gf_value_free(values); return GRAFIKR_INTERNAL_FAILURE; }
    char node_text[21]; (void)snprintf(node_text, sizeof(node_text), "%llu", (unsigned long long)provider->node);
    status = gf_collection_set(args, "node", gf_value_string(node_text));
    if (status == GRAFIKR_OK) status = gf_collection_set(args, "metadata", values);
    else gf_value_free(values);
    if (status == GRAFIKR_OK) status = gf_request(provider->client, "collection.update", args, result_or_error);
    gf_value_free(args);
    return status;
}

gf_status gf_collection_provider_close(gf_collection_provider *provider) {
    if (!provider) return GRAFIKR_INVALID_ARGUMENT;
    if (mtx_lock(&provider->mutex) != thrd_success) return GRAFIKR_INTERNAL_FAILURE;
    if (provider->closed) { (void)mtx_unlock(&provider->mutex); return GRAFIKR_OK; }
    provider->closed = 1;
    provider->has_request = 0;
    gf_subscription *subscription = provider->subscription;
    provider->subscription = NULL;
    (void)cnd_broadcast(&provider->changed);
    (void)mtx_unlock(&provider->mutex);
    return subscription ? gf_off(subscription) : GRAFIKR_OK;
}

gf_status gf_collection_provider_destroy(gf_collection_provider *provider) {
    if (!provider) return GRAFIKR_INVALID_ARGUMENT;
    gf_status status = gf_collection_provider_close(provider);
    cnd_destroy(&provider->changed);
    mtx_destroy(&provider->mutex);
    free(provider);
    return status;
}

gf_status gf_client_destroy(gf_client *client) {
    if (!client) return GRAFIKR_INVALID_ARGUMENT;
    if (gf_atomic_load(&client->running)) return GRAFIKR_WRONG_LIFECYCLE;
    gf_status status = grafikr_session_destroy(client->session);
    if (status == GRAFIKR_OK) {
        if (mtx_lock(&client->mutex) != thrd_success) return GRAFIKR_INTERNAL_FAILURE;
        client->destroyed = 1;
        for (gf_subscription *sub = client->subscriptions; sub; sub = sub->next) sub->active = 0;
        client->subscriptions = NULL;
        client->subscription_count = 0;
        (void)mtx_unlock(&client->mutex);
        gf_client_release_ref(client);
    }
    return status;
}
