#include "grafikr_client.h"
#include "cJSON.h"

#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

gf_value *gf_value_object(void) { return cJSON_CreateObject(); }
gf_value *gf_value_array(void) { return cJSON_CreateArray(); }
gf_value *gf_value_bool(bool value) { return cJSON_CreateBool(value); }

gf_value *gf_value_i64(int64_t value) {
    char decimal[32];
    (void)snprintf(decimal, sizeof(decimal), "%" PRId64, value);
    return cJSON_CreateRaw(decimal);
}

gf_value *gf_value_f64(double value) {
    return isfinite(value) ? cJSON_CreateNumber(value) : NULL;
}

gf_value *gf_value_string(const char *value) {
    return value ? cJSON_CreateString(value) : NULL;
}

gf_status gf_value_object_set(gf_value *object, const char *key, gf_value *value) {
    if (!cJSON_IsObject(object) || !key || !value || cJSON_GetObjectItemCaseSensitive(object, key))
        return GRAFIKR_INVALID_ARGUMENT;
    return cJSON_AddItemToObject(object, key, value) ? GRAFIKR_OK : GRAFIKR_INTERNAL_FAILURE;
}

gf_status gf_value_array_append(gf_value *array, gf_value *value) {
    if (!cJSON_IsArray(array) || !value) return GRAFIKR_INVALID_ARGUMENT;
    return cJSON_AddItemToArray(array, value) ? GRAFIKR_OK : GRAFIKR_INTERNAL_FAILURE;
}

gf_status gf_value_object_get(const gf_value *object, const char *key, const gf_value **out) {
    if (!cJSON_IsObject(object) || !key || !out) return GRAFIKR_INVALID_ARGUMENT;
    const cJSON *child = cJSON_GetObjectItemCaseSensitive(object, key);
    if (!child) return GRAFIKR_INVALID_ARGUMENT;
    *out = child;
    return GRAFIKR_OK;
}

gf_status gf_value_array_get(const gf_value *array, size_t index, const gf_value **out) {
    if (!cJSON_IsArray(array) || !out || index > INT_MAX) return GRAFIKR_INVALID_ARGUMENT;
    const cJSON *child = cJSON_GetArrayItem(array, (int)index);
    if (!child) return GRAFIKR_INVALID_ARGUMENT;
    *out = child;
    return GRAFIKR_OK;
}

size_t gf_value_array_size(const gf_value *array) {
    return cJSON_IsArray(array) ? (size_t)cJSON_GetArraySize(array) : 0;
}

gf_status gf_value_get_bool(const gf_value *value, bool *out) {
    if (!value || !out || !cJSON_IsBool(value)) return GRAFIKR_INVALID_ARGUMENT;
    *out = cJSON_IsTrue(value);
    return GRAFIKR_OK;
}

gf_status gf_value_get_i64(const gf_value *value, int64_t *out) {
    if (!value || !out) return GRAFIKR_INVALID_ARGUMENT;
    if (cJSON_IsRaw(value) && value->valuestring) {
        errno = 0;
        char *end = NULL;
        intmax_t parsed = strtoimax(value->valuestring, &end, 10);
        if (errno == ERANGE || end == value->valuestring || *end != '\0' ||
            parsed < INT64_MIN || parsed > INT64_MAX) return GRAFIKR_INVALID_ARGUMENT;
        *out = (int64_t)parsed;
        return GRAFIKR_OK;
    }
    if (!cJSON_IsNumber(value) || !isfinite(value->valuedouble) ||
        trunc(value->valuedouble) != value->valuedouble ||
        value->valuedouble < -9007199254740991.0 || value->valuedouble > 9007199254740991.0)
        return GRAFIKR_INVALID_ARGUMENT;
    *out = (int64_t)value->valuedouble;
    return GRAFIKR_OK;
}

gf_status gf_value_get_f64(const gf_value *value, double *out) {
    if (!value || !out || !cJSON_IsNumber(value) || !isfinite(value->valuedouble))
        return GRAFIKR_INVALID_ARGUMENT;
    *out = value->valuedouble;
    return GRAFIKR_OK;
}

gf_status gf_value_get_string(const gf_value *value, const char **out) {
    if (!value || !out || !cJSON_IsString(value) || !value->valuestring)
        return GRAFIKR_INVALID_ARGUMENT;
    *out = value->valuestring;
    return GRAFIKR_OK;
}

gf_value *gf_value_clone(const gf_value *value) {
    return value ? cJSON_Duplicate(value, 1) : NULL;
}

gf_status gf_value_write_json(const gf_value *value, char *buffer, size_t capacity, size_t *required) {
    if (!value || !required || (capacity != 0 && !buffer)) return GRAFIKR_INVALID_ARGUMENT;
    char *json = cJSON_PrintUnformatted(value);
    if (!json) return GRAFIKR_INTERNAL_FAILURE;
    size_t length = strlen(json) + 1;
    *required = length;
    if (capacity < length) {
        cJSON_free(json);
        return GRAFIKR_BUFFER_TOO_SMALL;
    }
    memcpy(buffer, json, length);
    cJSON_free(json);
    return GRAFIKR_OK;
}

void gf_value_free(gf_value *value) { cJSON_Delete(value); }
