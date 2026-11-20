#include "grafikr_client.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(condition) do { if (!(condition)) return __LINE__; } while (0)

static int check_queue_limits(void) {
    gf_client *client = NULL;
    CHECK(gf_client_create(&client) == GRAFIKR_OK);
    for (unsigned i = 0; i < 1024; i++) {
        char frame[96];
        int length = snprintf(frame, sizeof(frame), "{\"id\":\"c%u\",\"op\":\"state.get\",\"args\":{\"state\":\"0\"}}\n", i);
        CHECK(length > 0 && (size_t)length < sizeof(frame));
        CHECK(gf_client_submit(client, (const uint8_t *)frame, (size_t)length) == GRAFIKR_OK);
    }
    static const uint8_t small[] = "{\"id\":\"overflow\",\"op\":\"state.get\",\"args\":{\"state\":\"0\"}}\n";
    CHECK(gf_client_submit(client, small, sizeof(small) - 1) == GRAFIKR_QUEUE_FULL);
    CHECK(gf_client_close(client) == GRAFIKR_OK);
    CHECK(gf_client_run(client, NULL, NULL) == GRAFIKR_OK);
    CHECK(gf_client_destroy(client) == GRAFIKR_OK);

    CHECK(gf_client_create(&client) == GRAFIKR_OK);
    const size_t frame_bytes = 8u * 1024u * 1024u;
    const char suffix[] = "\"}}}\n";
    for (unsigned i = 1; i <= 2; i++) {
        uint8_t *frame = (uint8_t *)malloc(frame_bytes);
        CHECK(frame != NULL);
        char prefix[128];
        int prefix_length = snprintf(prefix, sizeof(prefix),
            "{\"id\":\"b%u\",\"op\":\"state.create\",\"args\":{\"value\":{\"type\":\"string\",\"value\":\"", i);
        CHECK(prefix_length > 0 && (size_t)prefix_length < sizeof(prefix));
        size_t suffix_length = sizeof(suffix) - 1;
        CHECK((size_t)prefix_length + suffix_length < frame_bytes);
        size_t padding = frame_bytes - (size_t)prefix_length - suffix_length;
        memcpy(frame, prefix, (size_t)prefix_length);
        memset(frame + prefix_length, 'x', padding);
        memcpy(frame + prefix_length + padding, suffix, suffix_length);
        gf_status status = gf_client_submit(client, frame, frame_bytes);
        free(frame);
        CHECK(status == GRAFIKR_OK);
    }
    CHECK(gf_client_submit(client, small, sizeof(small) - 1) == GRAFIKR_QUEUE_FULL);
    CHECK(gf_client_close(client) == GRAFIKR_OK);
    CHECK(gf_client_run(client, NULL, NULL) == GRAFIKR_OK);
    CHECK(gf_client_destroy(client) == GRAFIKR_OK);
    return 0;
}

int main(void) {
    gf_client *client = NULL;
    CHECK(grafikr_abi_version() == 0x00010000u);
    CHECK(gf_client_create(&client) == GRAFIKR_OK);
    CHECK(gf_client_close(client) == GRAFIKR_OK);
    CHECK(gf_client_run(client, NULL, NULL) == GRAFIKR_OK);
    CHECK(gf_client_destroy(client) == GRAFIKR_OK);
    int queue_status = check_queue_limits();
    if (queue_status != 0) {
        fprintf(stderr, "queue quota test failed at line %d\n", queue_status);
        return queue_status;
    }
    return 0;
}
