#ifndef __RCC_STDLIB_H
#define __RCC_STDLIB_H

#include <stddef.h>

extern void *malloc(size_t);
extern void *calloc(size_t, size_t);
extern void *realloc(void *, size_t);
extern void free(void *);
extern int atoi(const char *);
extern void exit(int);
extern void abort(void);

#endif
