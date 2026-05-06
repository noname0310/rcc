#ifndef __RCC_THREADS_H
#define __RCC_THREADS_H

#include <pthread.h>
#include <time.h>

/*
 * Hosted Linux C11 threads declaration shim.
 *
 * rcc does not implement a thread runtime.  These declarations expose the
 * C11 <threads.h> surface and rely on the host libc/pthread implementation at
 * link and runtime.  The type aliases intentionally match the pthread shim's
 * storage surface for the Linux hosted target.
 */

#ifndef __cplusplus
#define thread_local _Thread_local
#endif

#define TSS_DTOR_ITERATIONS 4

typedef pthread_t thrd_t;
typedef int (*thrd_start_t)(void *);

typedef pthread_mutex_t mtx_t;
typedef pthread_cond_t cnd_t;
typedef pthread_key_t tss_t;
typedef void (*tss_dtor_t)(void *);
typedef pthread_once_t once_flag;

#define ONCE_FLAG_INIT PTHREAD_ONCE_INIT

enum {
    thrd_success = 0,
    thrd_busy = 1,
    thrd_error = 2,
    thrd_nomem = 3,
    thrd_timedout = 4
};

enum {
    mtx_plain = 0,
    mtx_recursive = 1,
    mtx_timed = 2
};

extern int thrd_create(thrd_t *, thrd_start_t, void *);
extern int thrd_equal(thrd_t, thrd_t);
extern thrd_t thrd_current(void);
extern int thrd_sleep(const struct timespec *, struct timespec *);
extern _Noreturn void thrd_exit(int);
extern int thrd_detach(thrd_t);
extern int thrd_join(thrd_t, int *);
extern void thrd_yield(void);

extern int mtx_init(mtx_t *, int);
extern int mtx_lock(mtx_t *);
extern int mtx_timedlock(mtx_t *__restrict, const struct timespec *__restrict);
extern int mtx_trylock(mtx_t *);
extern int mtx_unlock(mtx_t *);
extern void mtx_destroy(mtx_t *);

extern void call_once(once_flag *, void (*)(void));

extern int cnd_init(cnd_t *);
extern int cnd_signal(cnd_t *);
extern int cnd_broadcast(cnd_t *);
extern int cnd_wait(cnd_t *, mtx_t *);
extern int cnd_timedwait(cnd_t *__restrict, mtx_t *__restrict, const struct timespec *__restrict);
extern void cnd_destroy(cnd_t *);

extern int tss_create(tss_t *, tss_dtor_t);
extern void *tss_get(tss_t);
extern int tss_set(tss_t, void *);
extern void tss_delete(tss_t);

#endif
