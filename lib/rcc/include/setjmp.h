#ifndef __RCC_SETJMP_H
#define __RCC_SETJMP_H

#if defined(__linux__) && defined(__x86_64__)
typedef struct {
    long __jmpbuf[8];
    int __mask_was_saved;
    long __saved_mask[16];
} __rcc_jmp_buf_tag;
typedef __rcc_jmp_buf_tag jmp_buf[1];
#else
typedef long jmp_buf[64];
#endif

extern int setjmp(jmp_buf);
extern void longjmp(jmp_buf, int);

#endif
