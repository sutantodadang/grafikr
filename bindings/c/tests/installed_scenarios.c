#include "grafikr_client.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "thread_compat.h"

typedef struct scenario {
    gf_client *client;
    const char *mode;
    char window[65], second_window[65], root[65];
    char button[65], label[65], field[65], state[65], count_state[65], binding[65];
    gf_subscription *click, *change, *selection, *modal_change;
    gf_collection_provider *provider;
    thrd_t data_thread;
    int data_thread_started;
    int clicked, text_changed;
} scenario;

static void report(const char *step) {
    const char *path = getenv("GRAFIKR_SCENARIO_REPORT");
    FILE *file = path ? fopen(path, "ab") : NULL;
    if (file) { (void)fprintf(file, "%s\n", step); (void)fclose(file); }
}

static gf_value *object(void) { return gf_value_object(); }

static int set(gf_value *value, const char *key, gf_value *child) {
    if (!value || !child) { gf_value_free(child); return 0; }
    if (gf_value_object_set(value, key, child) != GRAFIKR_OK) { gf_value_free(child); return 0; }
    return 1;
}

static int append(gf_value *value, gf_value *child) {
    if (!value || !child) { gf_value_free(child); return 0; }
    if (gf_value_array_append(value, child) != GRAFIKR_OK) { gf_value_free(child); return 0; }
    return 1;
}

static int token(const gf_value *value, const char *key, char *out, size_t capacity) {
    const gf_value *field = NULL;
    const char *text = NULL;
    return gf_value_object_get(value, key, &field) == GRAFIKR_OK &&
        gf_value_get_string(field, &text) == GRAFIKR_OK &&
        snprintf(out, capacity, "%s", text) > 0;
}

static gf_value *request(scenario *state, const char *op, gf_value *args) {
    gf_value *result = NULL;
    gf_status status = args ? gf_request(state->client, op, args, &result) : GRAFIKR_INTERNAL_FAILURE;
    gf_value_free(args);
    if (status != GRAFIKR_OK) {
        fprintf(stderr, "installed C scenario %s failed: %d\n", op, status);
        gf_value_free(result);
        return NULL;
    }
    return result;
}

static int window_create(scenario *state, const char *title, char *window, size_t window_size, char *root, size_t root_size) {
    gf_value *args = object();
    if (!set(args, "title", gf_value_string(title)) || !set(args, "width", gf_value_f64(480)) || !set(args, "height", gf_value_f64(320))) {
        gf_value_free(args); return 0;
    }
    gf_value *result = request(state, "window.create", args);
    if (!result || !token(result, "window", window, window_size) || !token(result, "root", root, root_size)) {
        gf_value_free(result); return 0;
    }
    gf_value_free(result);
    return 1;
}

static int node_create(scenario *state, const char *window, const char *widget, gf_value *properties, char *node, size_t node_size) {
    gf_value *args = object();
    if (!set(args, "window", gf_value_string(window)) || !set(args, "widget", gf_value_string(widget)) || !set(args, "properties", properties)) {
        gf_value_free(args); return 0;
    }
    gf_value *result = request(state, "node.create", args);
    int ok = result && token(result, "node", node, node_size);
    gf_value_free(result);
    return ok;
}

static int attach(scenario *state, const char *node, const char *parent) {
    gf_value *args = object();
    if (!set(args, "node", gf_value_string(node)) || !set(args, "parent", gf_value_string(parent))) { gf_value_free(args); return 0; }
    gf_value *result = request(state, "node.attach", args);
    int ok = result != NULL;
    gf_value_free(result);
    return ok;
}

static int destroy(scenario *state, const char *op, const char *field, const char *handle) {
    gf_value *args = object();
    if (!set(args, field, gf_value_string(handle))) { gf_value_free(args); return 0; }
    gf_value *result = request(state, op, args);
    int ok = result != NULL;
    gf_value_free(result);
    return ok;
}

static int create_string_state(scenario *state, const char *initial) {
    gf_value *value = object();
    if (!set(value, "type", gf_value_string("string")) || !set(value, "value", gf_value_string(initial))) { gf_value_free(value); return 0; }
    gf_value *args = object();
    if (!set(args, "value", value)) { gf_value_free(args); return 0; }
    gf_value *result = request(state, "state.create", args);
    int ok = result && token(result, "state", state->state, sizeof(state->state));
    gf_value_free(result);
    return ok;
}

static int create_count_state(scenario *state) {
    gf_value *value = object();
    if (!set(value, "type", gf_value_string("i64")) || !set(value, "value", gf_value_string("0"))) { gf_value_free(value); return 0; }
    gf_value *args = object();
    if (!set(args, "value", value)) { gf_value_free(args); return 0; }
    gf_value *result = request(state, "state.create", args);
    int ok = result && token(result, "state", state->count_state, sizeof(state->count_state));
    gf_value_free(result);
    return ok;
}

static int state_value(scenario *state, char *out, size_t capacity) {
    gf_value *args = object();
    if (!set(args, "state", gf_value_string(state->state))) { gf_value_free(args); return 0; }
    gf_value *result = request(state, "state.get", args);
    const gf_value *value = NULL;
    const char *text = NULL;
    int ok = result && gf_value_object_get(result, "value", &value) == GRAFIKR_OK && gf_value_get_string(value, &text) == GRAFIKR_OK && snprintf(out, capacity, "%s", text) > 0;
    gf_value_free(result);
    return ok;
}

static int state_set(scenario *state, const char *value) {
    gf_value *wire = object();
    if (!set(wire, "type", gf_value_string("string")) || !set(wire, "value", gf_value_string(value))) { gf_value_free(wire); return 0; }
    gf_value *args = object();
    if (!set(args, "state", gf_value_string(state->state)) || !set(args, "value", wire)) { gf_value_free(args); return 0; }
    gf_value *result = request(state, "state.set", args);
    int ok = result != NULL;
    gf_value_free(result);
    return ok;
}

static int bind_text(scenario *state) {
    gf_value *args = object();
    if (!set(args, "state", gf_value_string(state->state)) || !set(args, "node", gf_value_string(state->field)) ||
        !set(args, "property", gf_value_string("text")) || !set(args, "direction", gf_value_string("two_way"))) { gf_value_free(args); return 0; }
    gf_value *result = request(state, "binding.bind", args);
    int ok = result && token(result, "binding", state->binding, sizeof(state->binding));
    gf_value_free(result);
    return ok;
}

static void finish_counter(scenario *state) {
    if (!state->clicked || !state->text_changed) return;
    if (state->click) { (void)gf_off(state->click); state->click = NULL; }
    if (state->change) { (void)gf_off(state->change); state->change = NULL; }
    (void)destroy(state, "binding.unbind", "binding", state->binding);
    (void)destroy(state, "state.destroy", "state", state->state);
    (void)destroy(state, "state.destroy", "state", state->count_state);
    (void)destroy(state, "window.close", "window", state->window);
    (void)gf_client_stop(state->client);
}

static void on_click(gf_client *client, const gf_value *event, void *userdata) {
    (void)event;
    scenario *state = (scenario *)userdata;
    gf_value *get = object();
    if (!set(get, "state", gf_value_string(state->count_state))) { (void)gf_client_stop(client); return; }
    gf_value *current = request(state, "state.get", get);
    const gf_value *raw = NULL;
    const char *number = NULL;
    if (!current || gf_value_object_get(current, "value", &raw) != GRAFIKR_OK || gf_value_get_string(raw, &number) != GRAFIKR_OK) {
        gf_value_free(current); (void)gf_client_stop(client); return;
    }
    char next[32]; (void)snprintf(next, sizeof(next), "%lld", (long long)(strtoll(number, NULL, 10) + 1));
    gf_value_free(current);
    gf_value *wire = object();
    if (!set(wire, "type", gf_value_string("i64")) || !set(wire, "value", gf_value_string(next))) { gf_value_free(wire); (void)gf_client_stop(client); return; }
    gf_value *args = object();
    if (!set(args, "state", gf_value_string(state->count_state)) || !set(args, "value", wire)) { gf_value_free(args); (void)gf_client_stop(client); return; }
    gf_value *reply = request(state, "state.set", args);
    if (!reply) { (void)gf_client_stop(client); return; }
    gf_value_free(reply);
    gf_value *props = object();
    if (!set(props, "text", gf_value_string(next))) { gf_value_free(props); (void)gf_client_stop(client); return; }
    args = object();
    if (!set(args, "node", gf_value_string(state->label)) || !set(args, "properties", props)) { gf_value_free(args); (void)gf_client_stop(client); return; }
    reply = request(state, "property.update", args);
    if (!reply) { (void)gf_client_stop(client); return; }
    gf_value_free(reply);
    state->clicked = 1;
    report("counter.click-state-update");
    finish_counter(state);
}

static void on_text_change(gf_client *client, const gf_value *event, void *userdata) {
    scenario *state = (scenario *)userdata;
    const gf_value *payload = NULL, *value = NULL;
    const char *text = NULL;
    if (gf_value_object_get(event, "payload", &payload) != GRAFIKR_OK ||
        gf_value_object_get(payload, "value", &value) != GRAFIKR_OK || gf_value_get_string(value, &text) != GRAFIKR_OK) return;
    const char *expected = strcmp(state->mode, "counter") == 0 ? "Input 雪" : "Confirm 雪";
    if (strcmp(text, expected) != 0) return;
    char bound[64];
    if (!state_value(state, bound, sizeof(bound)) || strcmp(bound, text) != 0) { (void)gf_client_stop(client); return; }
    state->text_changed = 1;
    if (strcmp(state->mode, "counter") == 0) {
        report("counter.unicode-binding");
        finish_counter(state);
    } else {
        report("multi-window.keyboard-binding");
    }
}

static void on_modal_change(gf_client *client, const gf_value *event, void *userdata) {
    (void)client;
    scenario *state = (scenario *)userdata;
    const gf_value *payload = NULL, *raw = NULL;
    bool open = true;
    if (gf_value_object_get(event, "payload", &payload) != GRAFIKR_OK ||
        gf_value_object_get(payload, "value", &raw) != GRAFIKR_OK ||
        gf_value_get_bool(raw, &open) != GRAFIKR_OK || open) return;
    report("multi-window.modal-escape");
    if (state->change) { (void)gf_off(state->change); state->change = NULL; }
    if (state->modal_change) { (void)gf_off(state->modal_change); state->modal_change = NULL; }
    (void)destroy(state, "binding.unbind", "binding", state->binding);
    (void)destroy(state, "state.destroy", "state", state->state);
    (void)destroy(state, "window.close", "window", state->second_window);
    report("multi-window-modal-close");
    (void)gf_client_stop(state->client);
}

static int setup_counter(scenario *state) {
    char root[65];
    if (!window_create(state, "Grafikr installed counter", state->window, sizeof(state->window), root, sizeof(root)) || !create_string_state(state, "Grafikr 雪") || !create_count_state(state)) return 0;
    gf_value *props = object();
    if (!set(props, "text", gf_value_string("Count: 0")) || !node_create(state, state->window, "Text", props, state->label, sizeof(state->label))) return 0;
    props = object();
    if (!set(props, "label", gf_value_string("Increment")) || !node_create(state, state->window, "Button", props, state->button, sizeof(state->button))) return 0;
    props = object();
    if (!set(props, "text", gf_value_string("Grafikr 雪")) || !set(props, "width", gf_value_i64(240)) || !node_create(state, state->window, "TextField", props, state->field, sizeof(state->field))) return 0;
    if (!attach(state, state->label, root) || !attach(state, state->button, root) || !attach(state, state->field, root) || !bind_text(state)) return 0;
    uint64_t button = strtoull(state->button, NULL, 10), field = strtoull(state->field, NULL, 10);
    if (gf_on(state->client, button, "click", on_click, state, &state->click) != GRAFIKR_OK ||
        gf_on(state->client, field, "change", on_text_change, state, &state->change) != GRAFIKR_OK) return 0;
    report("counter.ready");
    return 1;
}

static gf_value *rect_commands(void) {
    gf_value *commands = gf_value_array(), *command = object(), *rect = object();
    if (!set(rect, "x", gf_value_f64(0)) || !set(rect, "y", gf_value_f64(0)) || !set(rect, "width", gf_value_f64(32)) || !set(rect, "height", gf_value_f64(24)) ||
        !set(command, "kind", gf_value_string("rect")) || !set(command, "rect", rect) || !set(command, "color", gf_value_string("#336699")) ||
        !set(command, "radius", gf_value_f64(0)) || !append(commands, command)) { gf_value_free(commands); return NULL; }
    return commands;
}

static int dashboard_data(void *userdata) {
    scenario *state = (scenario *)userdata;
    gf_collection_request range;
    gf_status status = gf_collection_provider_next(state->provider, &range, 5000);
    if (status != GRAFIKR_OK || range.start != 0 || range.count == 0 || range.count > 512) {
        report("dashboard.initial-range-failed");
        (void)gf_client_stop(state->client);
        return status == GRAFIKR_OK ? GRAFIKR_INVALID_ARGUMENT : status;
    }
    gf_value *rows = gf_value_array();
    if (!rows) return GRAFIKR_INTERNAL_FAILURE;
    for (uint32_t i = 0; i < range.count; i++) {
        char label[32]; (void)snprintf(label, sizeof(label), "Row %u", i);
        gf_value *row = gf_value_array();
        if (!row || !append(row, gf_value_string(label)) || !append(rows, row)) { gf_value_free(row); gf_value_free(rows); return GRAFIKR_INTERNAL_FAILURE; }
    }
    gf_value *reply = NULL;
    status = gf_collection_provider_provide(state->provider, &range, rows, &reply);
    char error_text[1024] = {0}; size_t error_size = 0;
    if (reply) (void)gf_value_write_json(reply, error_text, sizeof(error_text), &error_size);
    gf_value_free(reply); gf_value_free(rows);
    if (status != GRAFIKR_OK) {
        fprintf(stderr, "installed C dashboard initial provide failed: status=%d start=%u count=%u error=%s\n", status, range.start, range.count, error_text);
        report("dashboard.setup-failed");
        (void)gf_client_stop(state->client);
        return status;
    }

    report("dashboard.initial-range");
    status = gf_collection_provider_next(state->provider, &range, 10000);
    if (status != GRAFIKR_OK || range.start < 49000 || range.start + range.count != 50000 || range.count == 0 || range.count > 512) {
        report("dashboard.scroll-range-failed");
        (void)gf_client_stop(state->client);
        return status == GRAFIKR_OK ? GRAFIKR_INVALID_ARGUMENT : status;
    }
    rows = gf_value_array();
    if (!rows) { (void)gf_client_stop(state->client); return GRAFIKR_INTERNAL_FAILURE; }
    for (uint32_t i = 0; i < range.count; i++) {
        char label[32]; (void)snprintf(label, sizeof(label), "Row %llu", (unsigned long long)(range.start + i));
        gf_value *row = gf_value_array();
        if (!row || !append(row, gf_value_string(label)) || !append(rows, row)) { gf_value_free(row); gf_value_free(rows); (void)gf_client_stop(state->client); return GRAFIKR_INTERNAL_FAILURE; }
    }
    reply = NULL;
    status = gf_collection_provider_provide(state->provider, &range, rows, &reply);
    gf_value_free(reply); gf_value_free(rows);
    if (status != GRAFIKR_OK) { report("dashboard.setup-failed"); (void)gf_client_stop(state->client); return status; }
    report("dashboard.scroll-range");
    if (state->selection) { (void)gf_off(state->selection); state->selection = NULL; }
    (void)destroy(state, "window.close", "window", state->window);
    report("dashboard.multi-resource-cleanup");
    (void)gf_client_stop(state->client);
    return status;
}

static void on_dashboard_select(gf_client *client, const gf_value *event, void *userdata) {
    (void)client;
    (void)userdata;
    const gf_value *payload = NULL, *index = NULL;
    const char *text = NULL;
    if (gf_value_object_get(event, "payload", &payload) == GRAFIKR_OK &&
        gf_value_object_get(payload, "index", &index) == GRAFIKR_OK &&
        gf_value_get_string(index, &text) == GRAFIKR_OK && strtoull(text, NULL, 10) == 49999) {
        report("dashboard.selection");
    }
}

static int setup_dashboard(scenario *state) {
    char root[65];
    if (!window_create(state, "Grafikr installed dashboard", state->window, sizeof(state->window), root, sizeof(root))) return 0;
    gf_value *props = object();
    if (!set(props, "row_count", gf_value_i64(50000)) || !set(props, "row_height", gf_value_f64(24)) ||
        !set(props, "width", gf_value_f64(600)) || !set(props, "height", gf_value_f64(360)) || !node_create(state, state->window, "VirtualList", props, state->field, sizeof(state->field)) ||
        !attach(state, state->field, root)) return 0;
    uint64_t node = strtoull(state->field, NULL, 10);
    const char *columns[] = {"label"};
    gf_collection_metadata metadata = {50000, 24, columns, 1};
    gf_value *create_error = NULL;
    if (gf_collection_provider_create(state->client, node, &metadata, &state->provider, &create_error) != GRAFIKR_OK) { gf_value_free(create_error); return 0; }
    gf_value_free(create_error);
    if (gf_on(state->client, node, "select", on_dashboard_select, state, &state->selection) != GRAFIKR_OK) return 0;

    props = object();
    if (!set(props, "commands", gf_value_array()) || !set(props, "label", gf_value_string("Dashboard canvas")) ||
        !set(props, "width", gf_value_f64(320)) || !set(props, "height", gf_value_f64(120)) || !node_create(state, state->window, "Canvas", props, state->label, sizeof(state->label)) ||
        !attach(state, state->label, root)) return 0;
    gf_value *canvas_args = object();
    if (!set(canvas_args, "node", gf_value_string(state->label)) || !set(canvas_args, "commands", rect_commands())) { gf_value_free(canvas_args); return 0; }
    gf_value *reply = request(state, "canvas.update", canvas_args);
    if (!reply) return 0;
    gf_value_free(reply); report("dashboard.canvas");

    gf_value *args = object();
    if (!set(args, "width", gf_value_i64(1)) || !set(args, "height", gf_value_i64(1)) || !set(args, "byte_length", gf_value_i64(4))) return 0;
    reply = request(state, "image.begin", args);
    char upload[65];
    if (!reply || !token(reply, "upload", upload, sizeof(upload))) { gf_value_free(reply); return 0; }
    gf_value_free(reply);
    args = object();
    if (!set(args, "upload", gf_value_string(upload)) || !set(args, "offset", gf_value_i64(0)) || !set(args, "data", gf_value_string("AAAA/w=="))) { gf_value_free(args); return 0; }
    reply = request(state, "image.chunk", args); if (!reply) return 0; gf_value_free(reply);
    args = object(); if (!set(args, "upload", gf_value_string(upload))) { gf_value_free(args); return 0; }
    reply = request(state, "image.finalize", args);
    char image[65];
    if (!reply || !token(reply, "image", image, sizeof(image))) { gf_value_free(reply); return 0; }
    gf_value_free(reply);
    props = object();
    if (!set(props, "image", gf_value_string(image)) || !set(props, "width", gf_value_i64(1)) || !set(props, "height", gf_value_i64(1)) ||
        !set(props, "label", gf_value_string("Pixel")) || !node_create(state, state->window, "Image", props, state->button, sizeof(state->button)) || !attach(state, state->button, root)) return 0;
    if (!destroy(state, "image.destroy", "image", image)) return 0;
    report("dashboard.image-resource-cleanup");
    report("dashboard.ready");
    if (thrd_create(&state->data_thread, dashboard_data, state) != thrd_success) return 0;
    state->data_thread_started = 1;
    return 1;
}

static int setup_multi_window(scenario *state) {
    char root[65];
    if (!window_create(state, "Grafikr installed multi-window", state->window, sizeof(state->window), root, sizeof(root))) return 0;
    char second_root[65];
    if (!window_create(state, "Grafikr second window", state->second_window, sizeof(state->second_window), second_root, sizeof(second_root))) return 0;
    gf_value *props = object();
    if (!set(props, "open", gf_value_bool(true)) || !set(props, "width", gf_value_f64(300)) || !node_create(state, state->second_window, "Modal", props, state->label, sizeof(state->label)) || !attach(state, state->label, second_root)) return 0;
    if (gf_on(state->client, strtoull(state->label, NULL, 10), "change", on_modal_change, state, &state->modal_change) != GRAFIKR_OK) return 0;
    props = object();
    if (!set(props, "text", gf_value_string("Type here")) || !set(props, "width", gf_value_f64(240)) || !node_create(state, state->second_window, "TextField", props, state->field, sizeof(state->field)) || !attach(state, state->field, state->label)) return 0;
    if (!create_string_state(state, "Type here") || !bind_text(state)) return 0;
    if (!destroy(state, "window.close", "window", state->window)) return 0;
    if (!state_set(state, "state-survives-first-close")) return 0;
    char value[64];
    if (!state_value(state, value, sizeof(value)) || strcmp(value, "state-survives-first-close") != 0) return 0;
    if (!state_set(state, "Type here")) return 0;
    report("multi-window-state-survives-first-close");
    uint64_t field = strtoull(state->field, NULL, 10);
    if (gf_on(state->client, field, "change", on_text_change, state, &state->change) != GRAFIKR_OK) return 0;
    report("multi-window.ready");
    return 1;
}

static void setup(gf_client *client, void *userdata) {
    scenario *state = (scenario *)userdata;
    state->client = client;
    int ok = strcmp(state->mode, "counter") == 0 ? setup_counter(state) :
        strcmp(state->mode, "dashboard") == 0 ? setup_dashboard(state) : setup_multi_window(state);
    if (!ok) { report("scenario.setup-failed"); (void)gf_client_stop(client); }
}

int main(int argc, char **argv) {
    if (argc != 2 || (strcmp(argv[1], "counter") != 0 && strcmp(argv[1], "dashboard") != 0 && strcmp(argv[1], "multi") != 0)) return 2;
    scenario state = {0};
    state.mode = argv[1];
    gf_client *client = NULL;
    if (gf_client_create(&client) != GRAFIKR_OK) return 4;
    gf_status status = gf_client_run(client, setup, &state);
    if (state.data_thread_started) { int result = 0; (void)thrd_join(state.data_thread, &result); if (result != GRAFIKR_OK) status = result; }
    if (state.selection) { (void)gf_off(state.selection); state.selection = NULL; }
    if (state.provider) { (void)gf_collection_provider_close(state.provider); (void)gf_collection_provider_destroy(state.provider); }
    (void)gf_client_destroy(client);
    return status == GRAFIKR_OK ? 0 : 5;
}
