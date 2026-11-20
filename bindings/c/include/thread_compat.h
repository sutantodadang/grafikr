#ifndef GRAFIKR_THREAD_COMPAT_H
#define GRAFIKR_THREAD_COMPAT_H

#if defined(__APPLE__)
/* macOS SDKs do not ship C11 <threads.h>; map the used API to pthreads. */
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <time.h>

typedef pthread_t thrd_t;
typedef pthread_mutex_t mtx_t;
typedef pthread_cond_t cnd_t;
typedef pthread_once_t once_flag;
typedef int (*thrd_start_t)(void *);

enum { thrd_success = 0, thrd_busy = 1, thrd_error = 2, thrd_nomem = 3, thrd_timedout = 4 };
enum { mtx_plain = 0 };
#define ONCE_FLAG_INIT PTHREAD_ONCE_INIT

typedef struct { thrd_start_t function; void *argument; } gf_thread_start;

static inline void *gf_thread_start_main(void *argument) {
    gf_thread_start start = *(gf_thread_start *)argument;
    free(argument);
    return (void *)(intptr_t)start.function(start.argument);
}

static inline int thrd_create(thrd_t *thread, thrd_start_t function, void *argument) {
    gf_thread_start *start = (gf_thread_start *)malloc(sizeof(*start));
    if (!start) return thrd_nomem;
    start->function = function;
    start->argument = argument;
    int result = pthread_create(thread, NULL, gf_thread_start_main, start);
    if (result != 0) free(start);
    return result == 0 ? thrd_success : thrd_error;
}

static inline int thrd_join(thrd_t thread, int *result) {
    void *value = NULL;
    int status = pthread_join(thread, &value);
    if (status == 0 && result) *result = (int)(intptr_t)value;
    return status == 0 ? thrd_success : thrd_error;
}

static inline thrd_t thrd_current(void) { return pthread_self(); }
static inline int thrd_equal(thrd_t left, thrd_t right) { return pthread_equal(left, right); }
static inline int mtx_init(mtx_t *mutex, int type) { (void)type; return pthread_mutex_init(mutex, NULL) == 0 ? thrd_success : thrd_error; }
static inline int mtx_lock(mtx_t *mutex) { return pthread_mutex_lock(mutex) == 0 ? thrd_success : thrd_error; }
static inline int mtx_unlock(mtx_t *mutex) { return pthread_mutex_unlock(mutex) == 0 ? thrd_success : thrd_error; }
static inline void mtx_destroy(mtx_t *mutex) { (void)pthread_mutex_destroy(mutex); }
static inline int cnd_init(cnd_t *condition) { return pthread_cond_init(condition, NULL) == 0 ? thrd_success : thrd_error; }
static inline int cnd_wait(cnd_t *condition, mtx_t *mutex) { return pthread_cond_wait(condition, mutex) == 0 ? thrd_success : thrd_error; }
static inline int cnd_timedwait(cnd_t *condition, mtx_t *mutex, const struct timespec *deadline) {
    int result = pthread_cond_timedwait(condition, mutex, deadline);
    return result == 0 ? thrd_success : result == ETIMEDOUT ? thrd_timedout : thrd_error;
}
static inline int cnd_broadcast(cnd_t *condition) { return pthread_cond_broadcast(condition) == 0 ? thrd_success : thrd_error; }
static inline void cnd_destroy(cnd_t *condition) { (void)pthread_cond_destroy(condition); }
static inline void call_once(once_flag *flag, void (*function)(void)) { (void)pthread_once(flag, function); }
#else
#include <threads.h>
#endif

#endif
