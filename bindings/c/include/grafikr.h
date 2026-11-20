#ifndef GRAFIKR_H
#define GRAFIKR_H

#include <stddef.h>
#include <stdint.h>

#if defined(GRAFIKR_STATIC)
#  define GRAFIKR_API
#  define GRAFIKR_CALL
#elif defined(_WIN32)
#  if defined(GRAFIKR_BUILDING_LIBRARY)
#    define GRAFIKR_API __declspec(dllexport)
#  else
#    define GRAFIKR_API __declspec(dllimport)
#  endif
#  define GRAFIKR_CALL __cdecl
#else
#  define GRAFIKR_API __attribute__((visibility("default")))
#  define GRAFIKR_CALL
#endif

#ifdef __cplusplus
extern "C" {
#endif

enum grafikr_status {
    GRAFIKR_OK = 0,
    GRAFIKR_INVALID_ARGUMENT = 1,
    GRAFIKR_INVALID_SESSION = 2,
    GRAFIKR_WRONG_THREAD = 3,
    GRAFIKR_WRONG_LIFECYCLE = 4,
    GRAFIKR_QUEUE_FULL = 5,
    GRAFIKR_BUFFER_TOO_SMALL = 6,
    GRAFIKR_TIMEOUT = 7,
    GRAFIKR_STOPPED = 8,
    GRAFIKR_INTERNAL_FAILURE = 9,
    GRAFIKR_READER_BUSY = 10,
    GRAFIKR_BRIDGE_ERROR = 11
};

/* ABI 1.0. Payloads are protocol v1 UTF-8 JSON Lines; schema:
 * bindings/protocol/v1/schema.json. Buffers remain owned by the caller.
 * A request frame must be complete and include its trailing LF.
 */
GRAFIKR_API uint32_t GRAFIKR_CALL grafikr_abi_version(void);
GRAFIKR_API int32_t GRAFIKR_CALL grafikr_session_create(uint64_t *session);
GRAFIKR_API int32_t GRAFIKR_CALL grafikr_submit(uint64_t session, const uint8_t *data, size_t length);
GRAFIKR_API int32_t GRAFIKR_CALL grafikr_run(uint64_t session);
GRAFIKR_API int32_t GRAFIKR_CALL grafikr_next_event(uint64_t session, uint8_t *data, size_t capacity,
                                                    size_t *required, uint32_t timeout_ms);
GRAFIKR_API int32_t GRAFIKR_CALL grafikr_request_stop(uint64_t session);
GRAFIKR_API int32_t GRAFIKR_CALL grafikr_session_destroy(uint64_t session);

#ifdef __cplusplus
}
#endif
#endif
