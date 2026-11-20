#include "grafikr.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <threads.h>

#define MAX_FRAME_BYTES (8u * 1024u * 1024u)

typedef struct stream_context {
    uint64_t session;
} stream_context;

static int fail(uint64_t session, const char *message, int status);

static int stream_input(void *opaque) {
    stream_context *context = (stream_context *)opaque;
    uint8_t *frame = (uint8_t *)malloc(MAX_FRAME_BYTES);
    if (!frame) {
        (void)grafikr_request_stop(context->session);
        return 1;
    }
    while (fgets((char *)frame, (int)MAX_FRAME_BYTES, stdin)) {
        size_t length = strlen((char *)frame);
        if (!length || frame[length - 1] != '\n' ||
            grafikr_submit(context->session, frame, length) != GRAFIKR_OK) {
            fprintf(stderr, "grafikr-native-scenario: stream submit failed\n");
            (void)grafikr_request_stop(context->session);
            free(frame);
            return 1;
        }
    }
    int failed = ferror(stdin) != 0;
    free(frame);
    (void)grafikr_request_stop(context->session);
    return failed;
}

static int stream_output(void *opaque) {
    stream_context *context = (stream_context *)opaque;
    for (;;) {
        size_t required = 0;
        int32_t status = grafikr_next_event(context->session, NULL, 0, &required, 100);
        if (status == GRAFIKR_STOPPED) break;
        if (status == GRAFIKR_TIMEOUT) continue;
        if (status != GRAFIKR_BUFFER_TOO_SMALL || required == 0 || required > MAX_FRAME_BYTES) {
            fprintf(stderr, "grafikr-native-scenario: stream output probe failed (%d)\n", status);
            (void)grafikr_request_stop(context->session);
            return 1;
        }
        uint8_t *output = (uint8_t *)malloc(required);
        if (!output) {
            (void)grafikr_request_stop(context->session);
            return 1;
        }
        status = grafikr_next_event(context->session, output, required, &required, 0);
        if (status != GRAFIKR_OK || fwrite(output, 1, required, stdout) != required || fflush(stdout) != 0) {
            free(output);
            (void)grafikr_request_stop(context->session);
            return 1;
        }
        free(output);
    }
    return 0;
}

static int run_stream(void) {
    uint64_t session = 0;
    int32_t status = grafikr_session_create(&session);
    if (status != GRAFIKR_OK) return fail(0, "session create failed", status);
    stream_context context = {session};
    thrd_t input_thread, output_thread;
    if (thrd_create(&input_thread, stream_input, &context) != thrd_success)
        return fail(session, "cannot create input thread", GRAFIKR_INTERNAL_FAILURE);
    if (thrd_create(&output_thread, stream_output, &context) != thrd_success) {
        (void)grafikr_request_stop(session);
        (void)thrd_join(input_thread, NULL);
        return fail(session, "cannot create output thread", GRAFIKR_INTERNAL_FAILURE);
    }
    status = grafikr_run(session);
    int input_result = 0, output_result = 0;
    (void)thrd_join(input_thread, &input_result);
    (void)thrd_join(output_thread, &output_result);
    if (status != GRAFIKR_OK) return fail(session, "native run failed", status);
    status = grafikr_session_destroy(session);
    if (status != GRAFIKR_OK) return fail(session, "session destroy failed", status);
    if (input_result || output_result) {
        fprintf(stderr, "grafikr-native-scenario: native stream failed\n");
        return 1;
    }
    return 0;
}

static int fail(uint64_t session, const char *message, int status) {
    fprintf(stderr, "grafikr-native-scenario: %s (%d)\n", message, status);
    if (session) {
        (void)grafikr_request_stop(session);
        (void)grafikr_session_destroy(session);
    }
    return 1;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--stream") == 0) return run_stream();
    if (argc != 3 || strcmp(argv[1], "--scenario") != 0) {
        fprintf(stderr, "usage: grafikr_native_scenario --scenario <jsonl> | --stream\n");
        return 2;
    }
    FILE *scenario = fopen(argv[2], "rb");
    if (!scenario) return fail(0, "cannot open scenario", GRAFIKR_INVALID_ARGUMENT);
    uint64_t session = 0;
    int32_t status = grafikr_session_create(&session);
    if (status != GRAFIKR_OK) {
        fclose(scenario);
        return fail(0, "session create failed", status);
    }
    uint8_t *frame = (uint8_t *)malloc(MAX_FRAME_BYTES);
    if (!frame) {
        fclose(scenario);
        return fail(session, "frame allocation failed", GRAFIKR_INTERNAL_FAILURE);
    }
    while (fgets((char *)frame, (int)MAX_FRAME_BYTES, scenario)) {
        size_t length = strlen((char *)frame);
        if (!length || frame[length - 1] != '\n') {
            free(frame);
            fclose(scenario);
            return fail(session, "scenario frame is incomplete or oversized", GRAFIKR_INVALID_ARGUMENT);
        }
        status = grafikr_submit(session, frame, length);
        if (status != GRAFIKR_OK) {
            free(frame);
            fclose(scenario);
            return fail(session, "scenario submit failed", status);
        }
    }
    if (ferror(scenario)) {
        free(frame);
        fclose(scenario);
        return fail(session, "scenario read failed", GRAFIKR_INTERNAL_FAILURE);
    }
    free(frame);
    fclose(scenario);
    status = grafikr_run(session);
    if (status != GRAFIKR_OK) return fail(session, "native run failed", status);

    for (;;) {
        size_t required = 0;
        status = grafikr_next_event(session, NULL, 0, &required, 100);
        if (status == GRAFIKR_STOPPED) break;
        if (status == GRAFIKR_TIMEOUT) continue;
        if (status != GRAFIKR_BUFFER_TOO_SMALL || required == 0 || required > MAX_FRAME_BYTES)
            return fail(session, "native output probe failed", status);
        uint8_t *output = (uint8_t *)malloc(required);
        if (!output) return fail(session, "output allocation failed", GRAFIKR_INTERNAL_FAILURE);
        status = grafikr_next_event(session, output, required, &required, 0);
        if (status != GRAFIKR_OK || fwrite(output, 1, required, stdout) != required) {
            free(output);
            return fail(session, "native output read failed", status);
        }
        free(output);
    }
    status = grafikr_session_destroy(session);
    return status == GRAFIKR_OK ? 0 : fail(session, "session destroy failed", status);
}
