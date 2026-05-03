#ifndef __RCC_STDIO_H
#define __RCC_STDIO_H

#include <stddef.h>

typedef struct __rcc_FILE FILE;

#define EOF (-1)

extern int printf(const char *, ...);
extern int sprintf(char *, const char *, ...);
extern int puts(const char *);
extern int putchar(int);

extern FILE *fopen(const char *, const char *);
extern int fclose(FILE *);
extern int fgetc(FILE *);
extern int getc(FILE *);
extern char *fgets(char *, int, FILE *);
extern size_t fread(void *, size_t, size_t, FILE *);
extern size_t fwrite(const void *, size_t, size_t, FILE *);

#endif
