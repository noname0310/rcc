#ifndef __RCC_STDDEF_H
#define __RCC_STDDEF_H

#if defined(_WIN64)
typedef unsigned long long size_t;
typedef long long ptrdiff_t;
typedef unsigned short wchar_t;
typedef double max_align_t;
#else
typedef unsigned long size_t;
typedef long ptrdiff_t;
typedef int wchar_t;
typedef long double max_align_t;
#endif

#define NULL ((void *)0)
#define offsetof(type, member) __builtin_offsetof(type, member)

#endif
