#ifndef __RCC_ERRNO_H
#define __RCC_ERRNO_H

#define EDOM 33
#define EILSEQ 84
#define ERANGE 34

#if defined(_WIN32)
extern int *__errno(void);
#define errno (*__errno())
#else
extern int *__errno_location(void);
#define errno (*__errno_location())
#endif

#endif
