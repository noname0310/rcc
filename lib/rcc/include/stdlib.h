#ifndef __RCC_STDLIB_H
#define __RCC_STDLIB_H

#include <stddef.h>

typedef struct {
    int quot;
    int rem;
} div_t;

typedef struct {
    long quot;
    long rem;
} ldiv_t;

typedef struct {
    long long quot;
    long long rem;
} lldiv_t;

extern double atof(const char *);
extern int atoi(const char *);
extern long atol(const char *);
extern long long atoll(const char *);
extern double strtod(const char *, char **);
extern float strtof(const char *, char **);
extern long double strtold(const char *, char **);
extern long strtol(const char *, char **, int);
extern long long strtoll(const char *, char **, int);
extern unsigned long strtoul(const char *, char **, int);
extern unsigned long long strtoull(const char *, char **, int);

extern int rand(void);
extern void srand(unsigned int);

extern void *malloc(size_t);
extern void *calloc(size_t, size_t);
extern void *realloc(void *, size_t);
extern void free(void *);

extern void exit(int);
extern void _Exit(int);
extern void abort(void);
extern int atexit(void (*)(void));

extern char *getenv(const char *);
extern int system(const char *);

extern void *bsearch(const void *, const void *, size_t, size_t, int (*)(const void *, const void *));
extern void qsort(void *, size_t, size_t, int (*)(const void *, const void *));

extern int abs(int);
extern long labs(long);
extern long long llabs(long long);
extern div_t div(int, int);
extern ldiv_t ldiv(long, long);
extern lldiv_t lldiv(long long, long long);

extern int mblen(const char *, size_t);
extern int mbtowc(wchar_t *, const char *, size_t);
extern int wctomb(char *, wchar_t);
extern size_t mbstowcs(wchar_t *, const char *, size_t);
extern size_t wcstombs(char *, const wchar_t *, size_t);

#endif
