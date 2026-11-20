#include "common.h"

typedef struct window_slot {
    char label[21];
    unsigned clicks;
    gf_subscription *subscription;
} window_slot;

static void on_click(gf_client *client, const gf_value *event, void *userdata) {
    (void)event;
    window_slot *slot = (window_slot *)userdata;
    char text[64];
    (void)snprintf(text, sizeof(text), "Window clicks: %u", ++slot->clicks);
    gf_value *properties = gf_value_object();
    gf_value *args = gf_value_object();
    if (!properties || !args || !gf_example_set(properties, "text", gf_value_string(text))) {
        gf_value_free(properties);
        gf_value_free(args);
        (void)gf_client_stop(client);
        return;
    }
    if (!gf_example_set(args, "node", gf_value_string(slot->label)) ||
        !gf_example_set(args, "properties", properties)) {
        gf_value_free(args);
        (void)gf_client_stop(client);
        return;
    }
    gf_value *result = gf_example_request(client, "property.update", args);
    if (!result) (void)gf_client_stop(client);
    gf_value_free(result);
}

static int create_window(gf_client *client, const char *title, window_slot *slot) {
    char window[21], root[21], label[21], button[21];
    if (!gf_example_window(client, title, window, root)) return 0;
    gf_value *label_props = gf_value_object();
    if (!label_props || !gf_example_set(label_props, "text", gf_value_string(title)) ||
        !gf_example_node(client, window, "Text", label_props, label)) return 0;
    gf_value *button_props = gf_value_object();
    if (!button_props || !gf_example_set(button_props, "label", gf_value_string("Update this window")) ||
        !gf_example_node(client, window, "Button", button_props, button)) return 0;
    if (!gf_example_attach(client, label, root) || !gf_example_attach(client, button, root)) return 0;
    (void)strcpy(slot->label, label);
    return gf_on(client, gf_example_node_id(button), "click", on_click, slot, &slot->subscription) == GRAFIKR_OK;
}

static void setup(gf_client *client, void *userdata) {
    window_slot *windows = (window_slot *)userdata;
    if (!create_window(client, "Grafikr window one", &windows[0]) ||
        !create_window(client, "Grafikr window two", &windows[1]))
        (void)gf_client_stop(client);
}

int main(void) {
    gf_client *client = NULL;
    window_slot windows[2] = {0};
    if (gf_client_create(&client) != GRAFIKR_OK) return 1;
    gf_status status = gf_client_run(client, setup, windows);
    (void)gf_client_destroy(client);
    for (size_t i = 0; i < 2; i++) if (windows[i].subscription) (void)gf_off(windows[i].subscription);
    return status == GRAFIKR_OK ? 0 : 1;
}
