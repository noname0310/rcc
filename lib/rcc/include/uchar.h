#ifndef __RCC_UCHAR_H
#define __RCC_UCHAR_H

#include <stddef.h>
#include <stdint.h>

/*
 * C11 <uchar.h> declaration shim.
 *
 * Conversion routines are provided by the hosted C library.  rcc owns the
 * declarations and the char16_t/char32_t typedef surface, not locale or
 * conversion runtime behavior.
 */

typedef uint_least16_t char16_t;
typedef uint_least32_t char32_t;

#ifndef __RCC_MBSTATE_T_DEFINED
#define __RCC_MBSTATE_T_DEFINED
typedef struct {
    unsigned int __rcc_state[4];
} mbstate_t;
#endif

extern size_t mbrtoc16(char16_t *__restrict, const char *__restrict, size_t,
                       mbstate_t *__restrict);
extern size_t c16rtomb(char *__restrict, char16_t, mbstate_t *__restrict);
extern size_t mbrtoc32(char32_t *__restrict, const char *__restrict, size_t,
                       mbstate_t *__restrict);
extern size_t c32rtomb(char *__restrict, char32_t, mbstate_t *__restrict);

#endif
