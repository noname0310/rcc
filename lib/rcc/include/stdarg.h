#ifndef __RCC_STDARG_H
#define __RCC_STDARG_H

typedef char *va_list;

#define va_start(ap, last) ((void)0)
#define va_end(ap) ((void)0)
#define va_copy(dest, src) ((dest) = (src))
#define va_arg(ap, type) (*(type *)0)

#endif
