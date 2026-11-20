#include "common.h"

typedef struct counter_state {
    char label[21];
    int64_t count;
    gf_subscription *subscription;
} counter_state;

static void on_add(gf_client *client, const gf_value *event, void *userdata) {
    (void)event;
    counter_state *state = (counter_state *)userdata;
    gf_value *properties = gf_value_object();
    char text[64];
    (void)snprintf(text, sizeof(text), "Count: %lld", (long long)++state->count);
    gf_value *args = gf_value_object();
    if (!properties || !args || !gf_example_set(properties, "text", gf_value_string(text))) {
        gf_value_free(properties);
        gf_value_free(args);
        (void)gf_client_stop(client);
        return;
    }
    if (!gf_example_set(args, "node", gf_value_string(state->label)) ||
        !gf_example_set(args, "properties", properties)) {
        gf_value_free(args);
        (void)gf_client_stop(client);
        return;
    }
    gf_value *result = gf_example_request(client, "property.update", args);
    if (!result) {
        (void)gf_client_stop(client);
        return;
    }
    gf_value_free(result);
}

static void setup(gf_client *client, void *userdata) {
    counter_state *state = (counter_state *)userdata;
    char root[21], window[21], row[21], label[21], button[21], field[21];
    if (!gf_example_window(client, "Grafikr counter form", window, root)) goto failed;
    gf_value *row_properties = gf_value_object();
    if (!row_properties || !gf_example_set(row_properties, "gap", gf_value_i64(12)) ||
        !gf_example_node(client, window, "Column", row_properties, row)) goto failed;
    gf_value *label_properties = gf_value_object();
    if (!label_properties || !gf_example_set(label_properties, "text", gf_value_string("Count: 0")) ||
        !gf_example_node(client, window, "Text", label_properties, label)) goto failed;
    gf_value *button_properties = gf_value_object();
    if (!button_properties || !gf_example_set(button_properties, "label", gf_value_string("Add one")) ||
        !gf_example_node(client, window, "Button", button_properties, button)) goto failed;
    gf_value *field_properties = gf_value_object();
    if (!field_properties || !gf_example_set(field_properties, "text", gf_value_string("Type here")) ||
        !gf_example_set(field_properties, "width", gf_value_i64(240)) ||
        !gf_example_node(client, window, "TextField", field_properties, field)) goto failed;
    if (!gf_example_attach(client, row, root) || !gf_example_attach(client, field, row) ||
        !gf_example_attach(client, label, row) ||
        !gf_example_attach(client, button, row)) goto failed;
    (void)strcpy(state->label, label);
    if (gf_on(client, gf_example_node_id(button), "click", on_add, state, &state->subscription) != GRAFIKR_OK)
        goto failed;
    return;
failed:
    (void)gf_client_stop(client);
}

int main(void) {
    gf_client *client = NULL;
    counter_state state = {{0}, 0, NULL};
    if (gf_client_create(&client) != GRAFIKR_OK) return 1;
    gf_status status = gf_client_run(client, setup, &state);
    (void)gf_client_destroy(client);
    if (state.subscription) (void)gf_off(state.subscription);
    return status == GRAFIKR_OK ? 0 : 1;
}
