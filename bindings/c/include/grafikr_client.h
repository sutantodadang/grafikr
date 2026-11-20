#ifndef GRAFIKR_CLIENT_H
#define GRAFIKR_CLIENT_H
#include "grafikr.h"
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef struct gf_client gf_client;
typedef struct gf_subscription gf_subscription;
typedef struct gf_collection_provider gf_collection_provider;
typedef int32_t gf_status;
typedef struct cJSON gf_value;
typedef struct gf_collection_metadata {
    uint64_t row_count;
    double row_height;
    const char *const *columns;
    size_t column_count;
} gf_collection_metadata;
typedef struct gf_collection_request {
    char revision[65];
    char request[65];
    uint32_t start;
    uint32_t count;
} gf_collection_request;
typedef void (GRAFIKR_CALL *gf_setup_fn)(gf_client *client, void *userdata);
typedef void (GRAFIKR_CALL *gf_handler_fn)(gf_client *client, const gf_value *event, void *userdata);
#define GF_CLIENT_API

GF_CLIENT_API gf_status GRAFIKR_CALL gf_client_create(gf_client **out);
/* `request` is one complete protocol JSON line including LF. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_client_submit(gf_client *client, const uint8_t *request, size_t length);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_client_run(gf_client *client, gf_setup_fn setup, void *userdata);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_client_next(gf_client *client, uint8_t *buffer, size_t capacity, size_t *required, uint32_t timeout_ms);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_client_stop(gf_client *client);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_client_close(gf_client *client);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_client_destroy(gf_client *client);
/* On GRAFIKR_BRIDGE_ERROR, out owns the structured bridge error object. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_request(gf_client *client, const char *op, const gf_value *args, gf_value **out);
/* Register locally before the subscribe request; event is borrowed only during callback. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_on(gf_client *client, uint64_t node, const char *event, gf_handler_fn callback, void *userdata, gf_subscription **out);
/* Unsubscribes when the client is running, then releases the subscription handle. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_off(gf_subscription *subscription);
/* Create subscribes to data.request before collection.create. Close the
 * provider before destroying its client. The optional output owns the create
 * result on success or the structured bridge error on GRAFIKR_BRIDGE_ERROR. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_collection_provider_create(
    gf_client *client, uint64_t node, const gf_collection_metadata *metadata,
    gf_collection_provider **out, gf_value **result_or_error);
/* Returns GRAFIKR_TIMEOUT when no range arrives before timeout_ms, or
 * GRAFIKR_STOPPED after provider close. Only one reader may wait at a time. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_collection_provider_next(
    gf_collection_provider *provider, gf_collection_request *out, uint32_t timeout_ms);
/* rows must be an array; result_or_error follows gf_request ownership rules. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_collection_provider_provide(
    gf_collection_provider *provider, const gf_collection_request *request,
    const gf_value *rows, gf_value **result_or_error);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_collection_provider_update(
    gf_collection_provider *provider, const gf_collection_metadata *metadata,
    gf_value **result_or_error);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_collection_provider_close(gf_collection_provider *provider);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_collection_provider_destroy(gf_collection_provider *provider);

/* Value builders return owned values. Object/array insertion transfers the
 * child only on success; getters return borrowed child pointers. */
GF_CLIENT_API gf_value *GRAFIKR_CALL gf_value_object(void);
GF_CLIENT_API gf_value *GRAFIKR_CALL gf_value_array(void);
GF_CLIENT_API gf_value *GRAFIKR_CALL gf_value_bool(bool value);
GF_CLIENT_API gf_value *GRAFIKR_CALL gf_value_i64(int64_t value);
GF_CLIENT_API gf_value *GRAFIKR_CALL gf_value_f64(double value);
GF_CLIENT_API gf_value *GRAFIKR_CALL gf_value_string(const char *value);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_object_set(gf_value *object, const char *key, gf_value *value);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_array_append(gf_value *array, gf_value *value);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_object_get(const gf_value *object, const char *key, const gf_value **out);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_array_get(const gf_value *array, size_t index, const gf_value **out);
GF_CLIENT_API size_t GRAFIKR_CALL gf_value_array_size(const gf_value *array);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_get_bool(const gf_value *value, bool *out);
/* Parsed JSON numbers are accepted only when integral and within the exact
 * IEEE-754 integer range; values built with gf_value_i64 retain full i64. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_get_i64(const gf_value *value, int64_t *out);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_get_f64(const gf_value *value, double *out);
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_get_string(const gf_value *value, const char **out);
GF_CLIENT_API gf_value *GRAFIKR_CALL gf_value_clone(const gf_value *value);
/* Compact JSON output; required includes the trailing NUL byte. */
GF_CLIENT_API gf_status GRAFIKR_CALL gf_value_write_json(const gf_value *value, char *buffer, size_t capacity, size_t *required);
GF_CLIENT_API void GRAFIKR_CALL gf_value_free(gf_value *value);

#ifdef __cplusplus
}
#endif
#endif
