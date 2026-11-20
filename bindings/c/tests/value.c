#include "grafikr_client.h"
#include "cJSON.h"
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#define CHECK(condition) do { if (!(condition)) return __LINE__; } while (0)

int main(void) {
    gf_value *parsed_integer = cJSON_Parse("512");
    CHECK(parsed_integer != NULL);
    int64_t parsed_value = 0;
    CHECK(gf_value_get_i64(parsed_integer, &parsed_value) == GRAFIKR_OK);
    CHECK(parsed_value == 512);
    gf_value_free(parsed_integer);
    parsed_integer = cJSON_Parse("-9007199254740991");
    CHECK(parsed_integer != NULL);
    CHECK(gf_value_get_i64(parsed_integer, &parsed_value) == GRAFIKR_OK);
    CHECK(parsed_value == -INT64_C(9007199254740991));
    gf_value_free(parsed_integer);
    parsed_integer = cJSON_Parse("9007199254740992");
    CHECK(parsed_integer != NULL);
    CHECK(gf_value_get_i64(parsed_integer, &parsed_value) == GRAFIKR_INVALID_ARGUMENT);
    gf_value_free(parsed_integer);
    parsed_integer = cJSON_Parse("1.5");
    CHECK(parsed_integer != NULL);
    CHECK(gf_value_get_i64(parsed_integer, &parsed_value) == GRAFIKR_INVALID_ARGUMENT);
    gf_value_free(parsed_integer);

    gf_value *object = gf_value_object();
    CHECK(object != NULL);
    CHECK(gf_value_object_set(object, "minimum", gf_value_i64(INT64_MIN)) == GRAFIKR_OK);
    CHECK(gf_value_object_set(object, "maximum", gf_value_i64(INT64_MAX)) == GRAFIKR_OK);
    CHECK(gf_value_object_set(object, "enabled", gf_value_bool(true)) == GRAFIKR_OK);

    const gf_value *minimum = NULL;
    int64_t value = 0;
    CHECK(gf_value_object_get(object, "minimum", &minimum) == GRAFIKR_OK);
    CHECK(gf_value_get_i64(minimum, &value) == GRAFIKR_OK);
    CHECK(value == INT64_MIN);
    const gf_value *maximum = NULL;
    CHECK(gf_value_object_get(object, "maximum", &maximum) == GRAFIKR_OK);
    CHECK(gf_value_get_i64(maximum, &value) == GRAFIKR_OK && value == INT64_MAX);

    const gf_value *enabled = NULL;
    bool flag = false;
    CHECK(gf_value_object_get(object, "enabled", &enabled) == GRAFIKR_OK);
    CHECK(gf_value_get_bool(enabled, &flag) == GRAFIKR_OK && flag);
    CHECK(gf_value_get_bool(minimum, &flag) == GRAFIKR_INVALID_ARGUMENT);
    const char expected[] = "{\"minimum\":-9223372036854775808,\"maximum\":9223372036854775807,\"enabled\":true}";
    size_t required = 0;
    CHECK(gf_value_write_json(object, NULL, 0, &required) == GRAFIKR_BUFFER_TOO_SMALL);
    CHECK(required == sizeof(expected));
    char json[128];
    CHECK(gf_value_write_json(object, json, sizeof(json), &required) == GRAFIKR_OK);
    CHECK(strcmp(json, expected) == 0);
    gf_value *duplicate = gf_value_bool(false);
    CHECK(gf_value_object_set(object, "enabled", duplicate) == GRAFIKR_INVALID_ARGUMENT);
    gf_value_free(duplicate);

    gf_value *array = gf_value_array();
    CHECK(gf_value_array_append(array, gf_value_f64(2.5)) == GRAFIKR_OK);
    CHECK(gf_value_array_append(array, gf_value_string("雪")) == GRAFIKR_OK);
    CHECK(gf_value_f64(INFINITY) == NULL);
    CHECK(gf_value_array_size(array) == 2);
    const gf_value *number = NULL;
    double decimal = 0;
    CHECK(gf_value_array_get(array, 0, &number) == GRAFIKR_OK);
    CHECK(gf_value_get_f64(number, &decimal) == GRAFIKR_OK && decimal == 2.5);
    const gf_value *text = NULL;
    const char *string = NULL;
    CHECK(gf_value_array_get(array, 1, &text) == GRAFIKR_OK);
    CHECK(gf_value_get_string(text, &string) == GRAFIKR_OK);
    CHECK(string[0] == (char)0xE9);

    gf_value *clone = gf_value_clone(object);
    CHECK(clone != NULL);
    gf_value_free(object);
    gf_value_free(clone);
    gf_value_free(array);
    return 0;
}
