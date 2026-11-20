#include "grafikr.h"
#include "cJSON.h"
#include "mock_ffi.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <threads.h>
#include <time.h>

typedef struct frame {
    struct frame *next;
    size_t length;
    uint8_t data[];
} frame;

typedef struct saved_request {
    char id[21];
    char value[32];
} saved_request;

static once_flag initialized = ONCE_FLAG_INIT;
static mtx_t mutex;
static cnd_t changed;
static enum mock_ffi_mode mode;
static frame *head;
static frame *tail;
static int stopped;
static saved_request pending[2];
static size_t pending_count;
static size_t state_get_count;
static char subscribed_node[21];
static char subscribed_event[32];

static void initialize(void) {
    (void)mtx_init(&mutex, mtx_plain);
    (void)cnd_init(&changed);
}

static void copy_string(char *target, size_t capacity, const cJSON *value) {
    const char *text = cJSON_IsString(value) ? value->valuestring : "";
    (void)snprintf(target, capacity, "%s", text);
}

static void push_locked(const char *text) {
    size_t length = strlen(text);
    frame *item = (frame *)malloc(sizeof(*item) + length);
    if (!item) return;
    item->next = NULL;
    item->length = length;
    memcpy(item->data, text, length);
    if (tail) tail->next = item;
    else head = item;
    tail = item;
    (void)cnd_broadcast(&changed);
}

static void response_locked(const char *id, const char *result) {
    char text[512];
    (void)snprintf(text, sizeof(text), "{\"kind\":\"response\",\"id\":\"%s\",\"ok\":true,\"result\":%s}\n", id, result);
    push_locked(text);
}

static void event_locked(const char *node, const char *event) {
    char text[256];
    (void)snprintf(text, sizeof(text), "{\"kind\":\"event\",\"sequence\":\"1\",\"event\":\"%s\",\"source\":\"%s\",\"payload\":{}}\n", event, node);
    push_locked(text);
}

void mock_ffi_set_mode(enum mock_ffi_mode next) {
    call_once(&initialized, initialize);
    (void)mtx_lock(&mutex);
    mode = next;
    (void)mtx_unlock(&mutex);
}

uint32_t GRAFIKR_CALL grafikr_abi_version(void) { return 0x00010000u; }

int32_t GRAFIKR_CALL grafikr_session_create(uint64_t *session) {
    call_once(&initialized, initialize);
    if (!session) return GRAFIKR_INVALID_ARGUMENT;
    (void)mtx_lock(&mutex);
    while (head) {
        frame *next = head->next;
        free(head);
        head = next;
    }
    tail = NULL;
    stopped = 0;
    pending_count = 0;
    state_get_count = 0;
    subscribed_node[0] = '\0';
    subscribed_event[0] = '\0';
    *session = 1;
    (void)mtx_unlock(&mutex);
    return GRAFIKR_OK;
}

int32_t GRAFIKR_CALL grafikr_submit(uint64_t session, const uint8_t *data, size_t length) {
    if (session != 1 || !data || !length) return GRAFIKR_INVALID_ARGUMENT;
    cJSON *request = cJSON_ParseWithLengthOpts((const char *)data, length, NULL, 0);
    if (!request) return GRAFIKR_INVALID_ARGUMENT;
    char id[21], op[32], value[32], node[21], event[32];
    copy_string(id, sizeof(id), cJSON_GetObjectItemCaseSensitive(request, "id"));
    copy_string(op, sizeof(op), cJSON_GetObjectItemCaseSensitive(request, "op"));
    const cJSON *args = cJSON_GetObjectItemCaseSensitive(request, "args");
    copy_string(value, sizeof(value), cJSON_GetObjectItemCaseSensitive(args, "state"));
    copy_string(node, sizeof(node), cJSON_GetObjectItemCaseSensitive(args, "node"));
    copy_string(event, sizeof(event), cJSON_GetObjectItemCaseSensitive(args, "event"));
    (void)mtx_lock(&mutex);
    if (strcmp(op, "hello") == 0) response_locked(id, "{\"protocol\":{\"major\":1,\"minor\":0}}");
    else if (strcmp(op, "event.subscribe") == 0) {
        (void)snprintf(subscribed_node, sizeof(subscribed_node), "%s", node);
        (void)snprintf(subscribed_event, sizeof(subscribed_event), "%s", event);
        if (mode == MOCK_FFI_EARLY_EVENT) event_locked(node, event);
        response_locked(id, "{\"subscription\":\"77\"}");
        if (mode == MOCK_FFI_BLOCKED_HANDLER) event_locked(node, event);
    } else if (strcmp(op, "event.unsubscribe") == 0) {
        if (mode == MOCK_FFI_UNSUBSCRIBE_STALE) event_locked(subscribed_node, subscribed_event);
        response_locked(id, "{}");
    } else if (strcmp(op, "state.get") == 0 && mode == MOCK_FFI_OUT_OF_ORDER) {
        if (pending_count < 2) {
            (void)snprintf(pending[pending_count].id, sizeof(pending[pending_count].id), "%s", id);
            (void)snprintf(pending[pending_count].value, sizeof(pending[pending_count].value), "%s", value);
            pending_count++;
        }
        if (pending_count == 2) {
            char result[128];
            (void)snprintf(result, sizeof(result), "{\"value\":\"%s\"}", pending[1].value);
            response_locked(pending[1].id, result);
            (void)snprintf(result, sizeof(result), "{\"value\":\"%s\"}", pending[0].value);
            response_locked(pending[0].id, result);
            pending_count = 0;
        }
    } else if (strcmp(op, "state.get") == 0 && mode == MOCK_FFI_EOF_PENDING_256) {
        state_get_count++;
        if (state_get_count == 256) {
            stopped = 1;
            (void)cnd_broadcast(&changed);
        }
    } else if (strcmp(op, "state.get") == 0 && mode == MOCK_FFI_BLOCKED_HANDLER) {
        for (size_t i = 0; i < 1025; i++) event_locked(subscribed_node, subscribed_event);
        response_locked(id, "{}");
    } else response_locked(id, "{}");
    (void)mtx_unlock(&mutex);
    cJSON_Delete(request);
    return GRAFIKR_OK;
}

int32_t GRAFIKR_CALL grafikr_run(uint64_t session) {
    if (session != 1) return GRAFIKR_INVALID_SESSION;
    (void)mtx_lock(&mutex);
    while (!stopped) (void)cnd_wait(&changed, &mutex);
    (void)mtx_unlock(&mutex);
    return GRAFIKR_OK;
}

int32_t GRAFIKR_CALL grafikr_next_event(uint64_t session, uint8_t *data, size_t capacity,
                                       size_t *required, uint32_t timeout_ms) {
    if (session != 1 || !required || (capacity && !data)) return GRAFIKR_INVALID_ARGUMENT;
    struct timespec deadline;
    (void)timespec_get(&deadline, TIME_UTC);
    deadline.tv_sec += timeout_ms / 1000u;
    deadline.tv_nsec += (long)(timeout_ms % 1000u) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) { deadline.tv_sec++; deadline.tv_nsec -= 1000000000L; }
    (void)mtx_lock(&mutex);
    while (!head && !stopped) {
        if (!timeout_ms || cnd_timedwait(&changed, &mutex, &deadline) == thrd_timedout) {
            (void)mtx_unlock(&mutex);
            return GRAFIKR_TIMEOUT;
        }
    }
    if (!head) { (void)mtx_unlock(&mutex); return GRAFIKR_STOPPED; }
    *required = head->length;
    if (!capacity || capacity < head->length) { (void)mtx_unlock(&mutex); return GRAFIKR_BUFFER_TOO_SMALL; }
    frame *item = head;
    head = item->next;
    if (!head) tail = NULL;
    memcpy(data, item->data, item->length);
    (void)mtx_unlock(&mutex);
    free(item);
    return GRAFIKR_OK;
}

int32_t GRAFIKR_CALL grafikr_request_stop(uint64_t session) {
    if (session != 1) return GRAFIKR_INVALID_SESSION;
    (void)mtx_lock(&mutex);
    stopped = 1;
    (void)cnd_broadcast(&changed);
    (void)mtx_unlock(&mutex);
    return GRAFIKR_OK;
}

int32_t GRAFIKR_CALL grafikr_session_destroy(uint64_t session) {
    if (session != 1) return GRAFIKR_INVALID_SESSION;
    return GRAFIKR_OK;
}
