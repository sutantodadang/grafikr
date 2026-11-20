#include "grafikr_client.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "thread_compat.h"

typedef struct scenario {
    gf_client *client;
    char window[65];
    gf_collection_provider *provider;
    gf_subscription *nested_subscription;
    gf_subscription *noop_subscription;
    thrd_t reader;
    int reader_started;
    gf_status result;
    uint32_t received_count;
    uint32_t received_requests;
    int nested_request_succeeded;
    const char *stage;
} scenario;

static gf_status set(gf_value *object, const char *key, gf_value *value) {
    gf_status status = gf_value_object_set(object, key, value);
    if (status != GRAFIKR_OK) gf_value_free(value);
    return status;
}

static gf_status request(gf_client *client, const char *op, gf_value *args, gf_value **result) {
    gf_status status = args ? gf_request(client, op, args, result) : GRAFIKR_INTERNAL_FAILURE;
    gf_value_free(args);
    return status;
}

static void check_nested_request(gf_client *client, const gf_value *event, void *userdata) {
    (void)event;
    scenario *state = (scenario *)userdata;
    gf_value *args = gf_value_object();
    gf_value *result = NULL;
    const gf_value *code_value = NULL;
    const char *code = NULL;
    if (!args || set(args, "state", gf_value_string("0")) != GRAFIKR_OK) {
        gf_value_free(args);
        return;
    }
    gf_status status = gf_request(client, "state.get", args, &result);
    if (status == GRAFIKR_BRIDGE_ERROR && result &&
        gf_value_object_get(result, "code", &code_value) == GRAFIKR_OK &&
        gf_value_get_string(code_value, &code) == GRAFIKR_OK && strcmp(code, "invalid_handle") == 0)
        state->nested_request_succeeded = 1;
    gf_value_free(result);
    if (state->nested_subscription) {
        gf_status off_status = gf_off(state->nested_subscription);
        state->nested_subscription = NULL;
        if (off_status != GRAFIKR_OK) state->nested_request_succeeded = 0;
    }
}

static void ignore_data_request(gf_client *client, const gf_value *event, void *userdata) {
    (void)client;
    (void)event;
    (void)userdata;
}

static int provide_visible_rows(void *userdata) {
    scenario *state = (scenario *)userdata;
    gf_status status = GRAFIKR_OK;
    for (uint32_t request_index = 0; request_index < 1 && status == GRAFIKR_OK; request_index++) {
        state->stage = "waiting for visible range";
        gf_collection_request range;
        status = gf_collection_provider_next(state->provider, &range, 30000);
        if (status != GRAFIKR_OK || range.count == 0 || range.count > 512) break;
        gf_value *rows = gf_value_array();
        for (uint32_t i = 0; rows && i < range.count; i++) {
            char label[32];
            (void)snprintf(label, sizeof(label), "row-%u", range.start + i);
            gf_value *row = gf_value_array();
            if (!row || gf_value_array_append(row, gf_value_string(label)) != GRAFIKR_OK ||
                gf_value_array_append(rows, row) != GRAFIKR_OK) {
                gf_value_free(row);
                status = GRAFIKR_INTERNAL_FAILURE;
                break;
            }
        }
        gf_value *result = NULL;
        if (status == GRAFIKR_OK && rows)
            status = gf_collection_provider_provide(state->provider, &range, rows, &result);
        else if (!rows) status = GRAFIKR_INTERNAL_FAILURE;
        gf_value_free(rows);
        gf_value_free(result);
        result = NULL;
        if (status == GRAFIKR_OK) {
            state->received_count += range.count;
            state->received_requests++;
        }
        gf_value_free(result);
    }
    state->result = status;
    if (status != GRAFIKR_OK) state->stage = "serving visible range";
    (void)gf_collection_provider_close(state->provider);
    if (state->noop_subscription) {
        (void)gf_off(state->noop_subscription);
        state->noop_subscription = NULL;
    }
    gf_value *shutdown_result = NULL;
    gf_status shutdown_status = request(state->client, "shutdown", gf_value_object(), &shutdown_result);
    gf_value_free(shutdown_result);
    if (state->result == GRAFIKR_OK) state->result = shutdown_status;
    return 0;
}

static void setup(gf_client *client, void *userdata) {
    scenario *state = (scenario *)userdata;
    state->client = client;
    state->stage = "creating window";
    gf_value *args = gf_value_object();
    gf_value *result = NULL;
    if (set(args, "title", gf_value_string("C collection provider")) != GRAFIKR_OK ||
        set(args, "width", gf_value_i64(400)) != GRAFIKR_OK ||
        set(args, "height", gf_value_i64(300)) != GRAFIKR_OK ||
        request(client, "window.create", args, &result) != GRAFIKR_OK) goto failed;
    const gf_value *window_value = NULL, *root_value = NULL;
    const char *window = NULL, *root = NULL;
    if (gf_value_object_get(result, "window", &window_value) != GRAFIKR_OK ||
        gf_value_get_string(window_value, &window) != GRAFIKR_OK ||
        gf_value_object_get(result, "root", &root_value) != GRAFIKR_OK ||
        gf_value_get_string(root_value, &root) != GRAFIKR_OK) goto failed;
    char window_text[65], root_text[65];
    if (strlen(window) >= sizeof(window_text) || strlen(root) >= sizeof(root_text)) goto failed;
    (void)strcpy(window_text, window);
    (void)strcpy(state->window, window);
    (void)strcpy(root_text, root);
    gf_value_free(result); result = NULL;

    args = gf_value_object();
    state->stage = "creating virtual list";
    gf_value *properties = gf_value_object();
    if (set(args, "window", gf_value_string(window_text)) != GRAFIKR_OK ||
        set(args, "widget", gf_value_string("VirtualList")) != GRAFIKR_OK ||
        set(properties, "row_count", gf_value_i64(50000)) != GRAFIKR_OK ||
        set(properties, "row_height", gf_value_f64(24.0)) != GRAFIKR_OK ||
        set(properties, "height", gf_value_f64(120.0)) != GRAFIKR_OK ||
        set(args, "properties", properties) != GRAFIKR_OK ||
        request(client, "node.create", args, &result) != GRAFIKR_OK) goto failed;
    const gf_value *node_value = NULL;
    const char *node_text = NULL;
    uint64_t node;
    char node_text_buffer[65];
    if (gf_value_object_get(result, "node", &node_value) != GRAFIKR_OK ||
        gf_value_get_string(node_value, &node_text) != GRAFIKR_OK) goto failed;
    if (strlen(node_text) >= sizeof(node_text_buffer)) goto failed;
    (void)strcpy(node_text_buffer, node_text);
    node = (uint64_t)strtoull(node_text_buffer, NULL, 10);
    gf_value_free(result); result = NULL;

    args = gf_value_object();
    state->stage = "attaching virtual list";
    if (set(args, "node", gf_value_string(node_text_buffer)) != GRAFIKR_OK ||
        set(args, "parent", gf_value_string(root_text)) != GRAFIKR_OK ||
        request(client, "node.attach", args, &result) != GRAFIKR_OK) goto failed;
    gf_value_free(result); result = NULL;
    state->stage = "subscribing nested request check";
    if (gf_on(client, node, "data.request", check_nested_request, state, &state->nested_subscription) != GRAFIKR_OK)
        goto failed;
    if (gf_on(client, node, "data.request", ignore_data_request, state, &state->noop_subscription) != GRAFIKR_OK)
        goto failed;
    const char *columns[] = {"label"};
    const gf_collection_metadata metadata = {50000, 24.0, columns, 1};
    state->stage = "creating provider";
    if (gf_collection_provider_create(client, node, &metadata, &state->provider, &result) != GRAFIKR_OK) goto failed;
    gf_value_free(result); result = NULL;
    args = gf_value_object();
    state->stage = "refreshing provider window";
    if (set(args, "window", gf_value_string(state->window)) != GRAFIKR_OK ||
        set(args, "width", gf_value_f64(400.0)) != GRAFIKR_OK ||
        set(args, "height", gf_value_f64(300.0)) != GRAFIKR_OK ||
        request(client, "window.update", args, &result) != GRAFIKR_OK) goto failed;
    gf_value_free(result); result = NULL;
    state->result = GRAFIKR_INTERNAL_FAILURE;
    if (thrd_create(&state->reader, provide_visible_rows, state) != thrd_success) goto failed;
    state->reader_started = 1;
    return;

failed:
    char detail[1024];
    size_t detail_size = 0;
    if (gf_value_write_json(result, detail, sizeof(detail), &detail_size) != GRAFIKR_OK)
        (void)strcpy(detail, "<no structured error>");
    (void)fprintf(stderr, "C provider scenario failed while %s: %s\n",
                  state->stage ? state->stage : "initializing", detail);
    gf_value_free(result);
    state->result = GRAFIKR_INTERNAL_FAILURE;
    (void)gf_client_stop(client);
}

int main(void) {
    scenario state = {0};
    gf_client *client = NULL;
    if (gf_client_create(&client) != GRAFIKR_OK) return 1;
    state.client = client;
    gf_status status = gf_client_run(client, setup, &state);
    if (state.reader_started) {
        int result = 0;
        (void)thrd_join(state.reader, &result);
    }
    if (state.provider) (void)gf_collection_provider_destroy(state.provider);
    if (gf_client_destroy(client) != GRAFIKR_OK) return 2;
    if (status != GRAFIKR_OK || state.result != GRAFIKR_OK || state.received_count == 0 ||
        state.received_requests != 1 || !state.nested_request_succeeded) {
        (void)fprintf(stderr, "C provider scenario ended at %s (run=%d, scenario=%d, rows=%u, requests=%u, nested=%d)\n",
                      state.stage ? state.stage : "unknown", status, state.result, state.received_count,
                      state.received_requests, state.nested_request_succeeded);
        return 3;
    }
    return 0;
}
