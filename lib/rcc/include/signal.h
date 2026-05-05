#ifndef __RCC_SIGNAL_H
#define __RCC_SIGNAL_H

#include <sys/types.h>

typedef int sig_atomic_t;
typedef void (*__rcc_sighandler_t)(int);
typedef __rcc_sighandler_t sighandler_t;

#define SIG_DFL ((__rcc_sighandler_t)0)
#define SIG_ERR ((__rcc_sighandler_t)-1)
#define SIG_IGN ((__rcc_sighandler_t)1)

#define SIGABRT 6
#define SIGFPE 8
#define SIGILL 4
#define SIGINT 2
#define SIGSEGV 11
#define SIGTERM 15

extern __rcc_sighandler_t signal(int, __rcc_sighandler_t);
extern int raise(int);
extern int kill(pid_t, int);

#endif
