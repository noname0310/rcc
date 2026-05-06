#ifndef __RCC_ASSERT_H
#define __RCC_ASSERT_H

#ifdef NDEBUG
#define assert(expr) ((void)0)
#else
#include <stdlib.h>
#define assert(expr) ((expr) ? (void)0 : abort())
#endif

#ifndef __cplusplus
#define static_assert _Static_assert
#endif

#endif
