#ifndef GRAFIKR_TEST_MOCK_FFI_H
#define GRAFIKR_TEST_MOCK_FFI_H

enum mock_ffi_mode {
    MOCK_FFI_OUT_OF_ORDER,
    MOCK_FFI_EARLY_EVENT,
    MOCK_FFI_UNSUBSCRIBE_STALE,
    MOCK_FFI_EOF_PENDING_256,
    MOCK_FFI_BLOCKED_HANDLER
};

#ifdef __cplusplus
extern "C" {
#endif

void mock_ffi_set_mode(enum mock_ffi_mode mode);

#ifdef __cplusplus
}
#endif

#endif
