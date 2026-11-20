#include "grafikr_client.h"
#include "mock_ffi.h"

#include <stdio.h>
#include <string.h>
#include "thread_compat.h"

typedef struct request_call {
    gf_client *client;
    const char *state;
    char value[32];
    gf_status status;
} request_call;

typedef struct event_check {
    gf_client *client;
    gf_subscription *subscription;
    int count;
} event_check;

typedef struct pending_batch {
    request_call calls[256];
    thrd_t threads[256];
    size_t started;
} pending_batch;

typedef struct blocked_handler_check {
    gf_client *client;
    gf_subscription *subscription;
    mtx_t mutex;
    cnd_t changed;
    thrd_t feeder;
    int entered;
    int release;
    int thread_started;
    int callback_count;
    gf_status request_status;
} blocked_handler_check;

static int get_state(void *userdata) {
    request_call *call = (request_call *)userdata;
    gf_value *args = gf_value_object();
    gf_value *result = NULL;
    const gf_value *value = NULL;
    const char *text = NULL;
    (void)gf_value_object_set(args, "state", gf_value_string(call->state));
    call->status = gf_request(call->client, "state.get", args, &result);
    if (call->status == GRAFIKR_OK &&
        (gf_value_object_get(result, "value", &value) != GRAFIKR_OK ||
         gf_value_get_string(value, &text) != GRAFIKR_OK)) call->status = GRAFIKR_INTERNAL_FAILURE;
    if (text) (void)snprintf(call->value, sizeof(call->value), "%s", text);
    gf_value_free(result);
    return 0;
}

static void on_click(gf_client *client, const gf_value *event, void *userdata) {
    (void)event;
    event_check *check = (event_check *)userdata;
    check->count++;
    (void)gf_client_stop(client);
}

static void out_of_order_setup(gf_client *client, void *userdata) {
    request_call *calls = (request_call *)userdata;
    calls[0].client = calls[1].client = client;
    thrd_t first, second;
    if (thrd_create(&first, get_state, &calls[0]) != thrd_success ||
        thrd_create(&second, get_state, &calls[1]) != thrd_success) {
        calls[0].status = calls[1].status = GRAFIKR_INTERNAL_FAILURE;
    } else {
        int result = 0;
        (void)thrd_join(first, &result);
        (void)thrd_join(second, &result);
    }
    (void)gf_client_stop(client);
}

static void early_delivery_setup(gf_client *client, void *userdata) {
    event_check *check = (event_check *)userdata;
    if (gf_on(client, 7, "click", on_click, check, &check->subscription) != GRAFIKR_OK) {
        check->count = -1;
        (void)gf_client_stop(client);
    }
}

static void unsubscribe_stale_setup(gf_client *client, void *userdata) {
    event_check *check = (event_check *)userdata;
    if (gf_on(client, 7, "click", on_click, check, &check->subscription) != GRAFIKR_OK ||
        gf_off(check->subscription) != GRAFIKR_OK) check->count = -1;
    check->subscription = NULL;
    (void)gf_client_stop(client);
}

static void pending_eof_setup(gf_client *client, void *userdata) {
    pending_batch *batch = (pending_batch *)userdata;
    for (; batch->started < 256; batch->started++) {
        batch->calls[batch->started].client = client;
        batch->calls[batch->started].state = "pending";
        if (thrd_create(&batch->threads[batch->started], get_state,
                        &batch->calls[batch->started]) != thrd_success) break;
    }
    if (batch->started != 256) (void)gf_client_stop(client);
    for (size_t i = 0; i < batch->started; i++) {
        int result = 0;
        (void)thrd_join(batch->threads[i], &result);
    }
    (void)gf_client_stop(client);
}

static void on_blocked_click(gf_client *client, const gf_value *event, void *userdata) {
    (void)client;
    (void)event;
    blocked_handler_check *check = (blocked_handler_check *)userdata;
    (void)mtx_lock(&check->mutex);
    check->callback_count++;
    check->entered = 1;
    (void)cnd_broadcast(&check->changed);
    while (!check->release) (void)cnd_wait(&check->changed, &check->mutex);
    (void)mtx_unlock(&check->mutex);
}

static int trigger_overflow(void *userdata) {
    blocked_handler_check *check = (blocked_handler_check *)userdata;
    (void)mtx_lock(&check->mutex);
    while (!check->entered) (void)cnd_wait(&check->changed, &check->mutex);
    (void)mtx_unlock(&check->mutex);
    gf_value *args = gf_value_object();
    gf_value *result = NULL;
    check->request_status = gf_request(check->client, "state.get", args, &result);
    gf_value_free(args);
    gf_value_free(result);
    (void)mtx_lock(&check->mutex);
    check->release = 1;
    (void)cnd_broadcast(&check->changed);
    (void)mtx_unlock(&check->mutex);
    return 0;
}

static void blocked_handler_setup(gf_client *client, void *userdata) {
    blocked_handler_check *check = (blocked_handler_check *)userdata;
    check->client = client;
    if (gf_on(client, 7, "click", on_blocked_click, check, &check->subscription) != GRAFIKR_OK ||
        thrd_create(&check->feeder, trigger_overflow, check) != thrd_success) {
        (void)gf_client_stop(client);
        return;
    }
    check->thread_started = 1;
}

static int run_client(enum mock_ffi_mode mode, gf_setup_fn setup, void *userdata) {
    mock_ffi_set_mode(mode);
    gf_client *client = NULL;
    if (gf_client_create(&client) != GRAFIKR_OK) return 1;
    gf_status status = gf_client_run(client, setup, userdata);
    if (gf_client_destroy(client) != GRAFIKR_OK) return 2;
    return status == GRAFIKR_OK ? 0 : 3;
}

int main(void) {
    request_call calls[2] = {{.state = "first"}, {.state = "second"}};
    if (run_client(MOCK_FFI_OUT_OF_ORDER, out_of_order_setup, calls) ||
        calls[0].status != GRAFIKR_OK || calls[1].status != GRAFIKR_OK ||
        strcmp(calls[0].value, "first") || strcmp(calls[1].value, "second")) return 1;

    event_check early = {0};
    if (run_client(MOCK_FFI_EARLY_EVENT, early_delivery_setup, &early) || early.count != 1) return 2;

    event_check stale = {0};
    if (run_client(MOCK_FFI_UNSUBSCRIBE_STALE, unsubscribe_stale_setup, &stale) || stale.count != 0) return 3;

    pending_batch batch = {0};
    if (run_client(MOCK_FFI_EOF_PENDING_256, pending_eof_setup, &batch) || batch.started != 256) return 4;
    for (size_t i = 0; i < batch.started; i++)
        if (batch.calls[i].status != GRAFIKR_STOPPED) return 5;

    blocked_handler_check blocked = {0};
    if (mtx_init(&blocked.mutex, mtx_plain) != thrd_success || cnd_init(&blocked.changed) != thrd_success) return 6;
    int blocked_status = run_client(MOCK_FFI_BLOCKED_HANDLER, blocked_handler_setup, &blocked);
    if (blocked.thread_started) {
        int result = 0;
        (void)thrd_join(blocked.feeder, &result);
    }
    cnd_destroy(&blocked.changed);
    mtx_destroy(&blocked.mutex);
    if (blocked_status || !blocked.thread_started || blocked.callback_count != 1 ||
        blocked.request_status != GRAFIKR_QUEUE_FULL) return 7;
    return 0;
}
