#ifndef __RCC_SYS_SELECT_H
#define __RCC_SYS_SELECT_H

#include <sys/types.h>

struct timeval;

#define FD_SETSIZE 1024
#define __RCC_NFDBITS (8 * (int)sizeof(long))

typedef struct {
    unsigned long fds_bits[FD_SETSIZE / __RCC_NFDBITS];
} fd_set;

#define FD_ZERO(set) do { \
    int __rcc_i; \
    for (__rcc_i = 0; __rcc_i < (int)(FD_SETSIZE / __RCC_NFDBITS); __rcc_i++) \
        (set)->fds_bits[__rcc_i] = 0; \
} while (0)

#define FD_SET(fd, set) \
    ((set)->fds_bits[(fd) / __RCC_NFDBITS] |= (1UL << ((fd) % __RCC_NFDBITS)))

#define FD_CLR(fd, set) \
    ((set)->fds_bits[(fd) / __RCC_NFDBITS] &= ~(1UL << ((fd) % __RCC_NFDBITS)))

#define FD_ISSET(fd, set) \
    (((set)->fds_bits[(fd) / __RCC_NFDBITS] & (1UL << ((fd) % __RCC_NFDBITS))) != 0)

extern int select(int, fd_set *__restrict, fd_set *__restrict, fd_set *__restrict,
                  struct timeval *__restrict);

#endif
