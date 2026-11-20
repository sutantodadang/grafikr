#include "grafikr_client.h"
#include <string.h>

typedef struct request_check {
    gf_status status;
    bool stopped;
    bool structured_error;
    bool raw_rejected;
    bool failed_subscription_rejected;
    bool callback_mode_next_rejected;
} request_check;

static void on_event(gf_client *client, const gf_value *event, void *userdata) {
    (void)client;
    (void)event;
    (void)userdata;
}

static void setup(gf_client *client, void *userdata) {
    request_check *check = (request_check *)userdata;
    uint8_t buffer[1];
    size_t required = 0;
    check->callback_mode_next_rejected =
        gf_client_next(client, buffer, sizeof(buffer), &required, 0) == GRAFIKR_WRONG_LIFECYCLE;
    static const uint8_t raw_request[] = "{\"id\":\"900\",\"op\":\"state.get\",\"args\":{\"state\":\"999\"}}\n";
    check->raw_rejected = gf_client_submit(client, raw_request, sizeof(raw_request) - 1) == GRAFIKR_WRONG_LIFECYCLE;
    gf_value *args = gf_value_object();
    gf_value *result = NULL;
    (void)gf_value_object_set(args, "window", gf_value_string("9999"));
    gf_status request_status = gf_request(client, "window.close", args, &result);
    const gf_value *code = NULL;
    const char *code_text = NULL;
    if (request_status == GRAFIKR_BRIDGE_ERROR && result &&
        gf_value_object_get(result, "code", &code) == GRAFIKR_OK &&
        gf_value_get_string(code, &code_text) == GRAFIKR_OK &&
        strcmp(code_text, "invalid_handle") == 0)
        check->structured_error = true;
    gf_value_free(result);
    gf_subscription *subscription = NULL;
    request_status = gf_on(client, 9999, "click", on_event, check, &subscription);
    check->failed_subscription_rejected = request_status == GRAFIKR_BRIDGE_ERROR && subscription == NULL;
    gf_value_free(args);
    args = gf_value_object();
    result = NULL;
    check->status = gf_request(client, "shutdown", args, &result);
    const gf_value *stopped = NULL;
    if (check->status == GRAFIKR_OK &&
        gf_value_object_get(result, "stopped", &stopped) == GRAFIKR_OK)
        (void)gf_value_get_bool(stopped, &check->stopped);
    gf_value_free(args);
    gf_value_free(result);
}

int main(void) {
    gf_client *client = NULL;
    request_check check = {GRAFIKR_INTERNAL_FAILURE, false, false, false, false, false};
    if (gf_client_create(&client) != GRAFIKR_OK) return 1;
    const char *columns[] = {"label"};
    gf_collection_metadata metadata = {1, 24.0, columns, 1};
    gf_collection_provider *provider = NULL;
    gf_value *provider_error = NULL;
    gf_collection_metadata invalid_metadata = {1, 0.0, columns, 1};
    if (gf_collection_provider_create(client, 7, &invalid_metadata, &provider, &provider_error) != GRAFIKR_INVALID_ARGUMENT || provider || provider_error) return 5;
    if (gf_collection_provider_create(client, 7, &metadata, &provider, &provider_error) != GRAFIKR_WRONG_LIFECYCLE || provider || provider_error) return 6;
    if (gf_client_run(client, setup, &check) != GRAFIKR_OK) return 2;
    if (check.status != GRAFIKR_OK || !check.stopped || !check.structured_error ||
        !check.raw_rejected || !check.failed_subscription_rejected ||
        !check.callback_mode_next_rejected) return 3;
    if (gf_client_destroy(client) != GRAFIKR_OK) return 4;
    return 0;
}
