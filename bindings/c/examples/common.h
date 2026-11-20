#ifndef GRAFIKR_EXAMPLE_COMMON_H
#define GRAFIKR_EXAMPLE_COMMON_H
#include "grafikr_client.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int gf_example_set(gf_value *object, const char *key, gf_value *value) {
    gf_status status = gf_value_object_set(object, key, value);
    if (status != GRAFIKR_OK) gf_value_free(value);
    return status == GRAFIKR_OK;
}

static gf_value *gf_example_request(gf_client *client, const char *op, gf_value *args) {
    gf_value *result = NULL;
    gf_status status = args ? gf_request(client, op, args, &result) : GRAFIKR_INTERNAL_FAILURE;
    gf_value_free(args);
    if (status != GRAFIKR_OK) {
        fprintf(stderr, "Grafikr request '%s' failed (%d)\n", op, status);
        gf_value_free(result);
        return NULL;
    }
    return result;
}

static int gf_example_copy_field(const gf_value *object, const char *key, char out[21]) {
    const gf_value *field = NULL;
    const char *value = NULL;
    if (gf_value_object_get(object, key, &field) != GRAFIKR_OK ||
        gf_value_get_string(field, &value) != GRAFIKR_OK || strlen(value) >= 21) return 0;
    (void)strcpy(out, value);
    return 1;
}

static uint64_t gf_example_node_id(const char *handle) {
    return (uint64_t)strtoull(handle, NULL, 10);
}

static int gf_example_window(gf_client *client, const char *title, char window[21], char root[21]) {
    gf_value *args = gf_value_object();
    if (!args || !gf_example_set(args, "title", gf_value_string(title)) ||
        !gf_example_set(args, "width", gf_value_i64(560)) ||
        !gf_example_set(args, "height", gf_value_i64(400))) {
        gf_value_free(args);
        return 0;
    }
    gf_value *result = gf_example_request(client, "window.create", args);
    if (!result) return 0;
    int ok = gf_example_copy_field(result, "window", window) &&
             gf_example_copy_field(result, "root", root);
    gf_value_free(result);
    return ok;
}

static int gf_example_node(gf_client *client, const char *window, const char *widget,
                           gf_value *properties, char node[21]) {
    gf_value *args = gf_value_object();
    if (!properties) properties = gf_value_object();
    if (!args || !properties) {
        gf_value_free(args);
        gf_value_free(properties);
        return 0;
    }
    if (!gf_example_set(args, "window", gf_value_string(window)) ||
        !gf_example_set(args, "widget", gf_value_string(widget))) {
        gf_value_free(args);
        gf_value_free(properties);
        return 0;
    }
    if (!gf_example_set(args, "properties", properties)) {
        gf_value_free(args);
        return 0;
    }
    gf_value *result = gf_example_request(client, "node.create", args);
    if (!result) return 0;
    int ok = gf_example_copy_field(result, "node", node);
    gf_value_free(result);
    return ok;
}

static int gf_example_attach(gf_client *client, const char *node, const char *parent) {
    gf_value *args = gf_value_object();
    if (!args || !gf_example_set(args, "node", gf_value_string(node)) ||
        !gf_example_set(args, "parent", gf_value_string(parent))) {
        gf_value_free(args);
        return 0;
    }
    gf_value *result = gf_example_request(client, "node.attach", args);
    if (!result) return 0;
    gf_value_free(result);
    return 1;
}

#endif
