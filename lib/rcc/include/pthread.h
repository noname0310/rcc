#ifndef __RCC_PTHREAD_H
#define __RCC_PTHREAD_H

#include <stddef.h>
#include <sys/cdefs.h>

/*
 * Hosted Linux pthread declaration shim.
 *
 * The runtime implementation belongs to host glibc/libpthread.  The object
 * types below reserve ABI-sized storage for common x86_64 glibc use cases so
 * frontend tests can type-check and simple programs can link/run through the
 * host pthread implementation.  They are not pthread implementations.
 */

typedef unsigned long pthread_t;
typedef unsigned int pthread_key_t;
typedef int pthread_once_t;

typedef union {
    char __rcc_size[56];
    long __rcc_align;
} pthread_attr_t;

typedef union {
    char __rcc_size[40];
    long __rcc_align;
} pthread_mutex_t;

typedef union {
    char __rcc_size[48];
    long long __rcc_align;
} pthread_cond_t;

typedef union {
    char __rcc_size[4];
    int __rcc_align;
} pthread_mutexattr_t;

typedef union {
    char __rcc_size[4];
    int __rcc_align;
} pthread_condattr_t;

#define PTHREAD_ONCE_INIT 0
#define PTHREAD_MUTEX_INITIALIZER { { 0 } }
#define PTHREAD_COND_INITIALIZER { { 0 } }

#define PTHREAD_CREATE_JOINABLE 0
#define PTHREAD_CREATE_DETACHED 1

#define PTHREAD_MUTEX_NORMAL 0
#define PTHREAD_MUTEX_RECURSIVE 1
#define PTHREAD_MUTEX_ERRORCHECK 2
#define PTHREAD_MUTEX_DEFAULT PTHREAD_MUTEX_NORMAL

extern int pthread_create(pthread_t *__restrict, const pthread_attr_t *__restrict,
                          void *(*)(void *), void *__restrict);
extern int pthread_join(pthread_t, void **);
extern int pthread_detach(pthread_t);
extern pthread_t pthread_self(void);
extern int pthread_equal(pthread_t, pthread_t);
extern void pthread_exit(void *);

extern int pthread_once(pthread_once_t *, void (*)(void));

extern int pthread_attr_init(pthread_attr_t *);
extern int pthread_attr_destroy(pthread_attr_t *);
extern int pthread_attr_getdetachstate(const pthread_attr_t *, int *);
extern int pthread_attr_setdetachstate(pthread_attr_t *, int);
extern int pthread_attr_getstacksize(const pthread_attr_t *, size_t *);
extern int pthread_attr_setstacksize(pthread_attr_t *, size_t);

extern int pthread_mutex_init(pthread_mutex_t *__restrict, const pthread_mutexattr_t *__restrict);
extern int pthread_mutex_destroy(pthread_mutex_t *);
extern int pthread_mutex_lock(pthread_mutex_t *);
extern int pthread_mutex_trylock(pthread_mutex_t *);
extern int pthread_mutex_unlock(pthread_mutex_t *);

extern int pthread_mutexattr_init(pthread_mutexattr_t *);
extern int pthread_mutexattr_destroy(pthread_mutexattr_t *);
extern int pthread_mutexattr_settype(pthread_mutexattr_t *, int);
extern int pthread_mutexattr_gettype(const pthread_mutexattr_t *, int *);

extern int pthread_cond_init(pthread_cond_t *__restrict, const pthread_condattr_t *__restrict);
extern int pthread_cond_destroy(pthread_cond_t *);
extern int pthread_cond_signal(pthread_cond_t *);
extern int pthread_cond_broadcast(pthread_cond_t *);
extern int pthread_cond_wait(pthread_cond_t *__restrict, pthread_mutex_t *__restrict);

extern int pthread_condattr_init(pthread_condattr_t *);
extern int pthread_condattr_destroy(pthread_condattr_t *);

extern int pthread_key_create(pthread_key_t *, void (*)(void *));
extern int pthread_key_delete(pthread_key_t);
extern void *pthread_getspecific(pthread_key_t);
extern int pthread_setspecific(pthread_key_t, const void *);

#endif
