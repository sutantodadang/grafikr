#include "common.h"

typedef struct dashboard_state {
    char status[21];
    unsigned refreshes;
    gf_subscription *subscription;
} dashboard_state;

static void on_refresh(gf_client *client, const gf_value *event, void *userdata) {
    (void)event;
    dashboard_state *state = (dashboard_state *)userdata;
    char text[64];
    (void)snprintf(text, sizeof(text), "Updated %u times", ++state->refreshes);
    gf_value *properties = gf_value_object();
    gf_value *args = gf_value_object();
    if (!properties || !args || !gf_example_set(properties, "text", gf_value_string(text))) {
        gf_value_free(properties);
        gf_value_free(args);
        (void)gf_client_stop(client);
        return;
    }
    if (!gf_example_set(args, "node", gf_value_string(state->status)) ||
        !gf_example_set(args, "properties", properties)) {
        gf_value_free(args);
        (void)gf_client_stop(client);
        return;
    }
    gf_value *result = gf_example_request(client, "property.update", args);
    if (!result) (void)gf_client_stop(client);
    gf_value_free(result);
}

static void setup(gf_client *client, void *userdata) {
    dashboard_state *state = (dashboard_state *)userdata;
    char window[21], root[21], column[21], heading[21], status[21], button[21];
    if (!gf_example_window(client, "Grafikr dashboard", window, root)) goto failed;
    gf_value *layout = gf_value_object();
    if (!layout || !gf_example_set(layout, "gap", gf_value_i64(10)) ||
        !gf_example_node(client, window, "Column", layout, column)) goto failed;
    gf_value *heading_props = gf_value_object();
    if (!heading_props || !gf_example_set(heading_props, "text", gf_value_string("Operations dashboard")) ||
        !gf_example_set(heading_props, "size", gf_value_i64(24)) ||
        !gf_example_node(client, window, "Text", heading_props, heading)) goto failed;
    gf_value *status_props = gf_value_object();
    if (!status_props || !gf_example_set(status_props, "text", gf_value_string("Ready")) ||
        !gf_example_node(client, window, "Text", status_props, status)) goto failed;
    gf_value *button_props = gf_value_object();
    if (!button_props || !gf_example_set(button_props, "label", gf_value_string("Refresh status")) ||
        !gf_example_node(client, window, "Button", button_props, button)) goto failed;
    if (!gf_example_attach(client, column, root) || !gf_example_attach(client, heading, column) ||
        !gf_example_attach(client, status, column) || !gf_example_attach(client, button, column)) goto failed;
    (void)strcpy(state->status, status);
    if (gf_on(client, gf_example_node_id(button), "click", on_refresh, state, &state->subscription) != GRAFIKR_OK)
        goto failed;
    return;
failed:
    (void)gf_client_stop(client);
}

int main(void) {
    gf_client *client = NULL;
    dashboard_state state = {{0}, 0, NULL};
    if (gf_client_create(&client) != GRAFIKR_OK) return 1;
    gf_status status = gf_client_run(client, setup, &state);
    (void)gf_client_destroy(client);
    if (state.subscription) (void)gf_off(state.subscription);
    return status == GRAFIKR_OK ? 0 : 1;
}
