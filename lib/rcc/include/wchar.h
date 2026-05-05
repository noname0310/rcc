#ifndef __RCC_WCHAR_H
#define __RCC_WCHAR_H

#include <stddef.h>

#ifndef __RCC_WINT_T_DEFINED
#define __RCC_WINT_T_DEFINED
typedef unsigned int wint_t;
#endif

#define WEOF ((wchar_t)-1)

extern int wcwidth(wchar_t);

#endif
