#ifndef __RCC_STDIO_H
#define __RCC_STDIO_H

#include <stddef.h>
#include <stdarg.h>

typedef struct __rcc_FILE FILE;
#if defined(_WIN64)
typedef long long fpos_t;
#else
typedef struct {
    long __rcc_storage[2];
} fpos_t;
#endif

#define BUFSIZ 8192
#define EOF (-1)
#define FILENAME_MAX 4096
#define FOPEN_MAX 16
#define L_tmpnam 20
#define TMP_MAX 238328

#define _IOFBF 0
#define _IOLBF 1
#define _IONBF 2

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

extern int remove(const char *);
extern int rename(const char *, const char *);
extern FILE *tmpfile(void);
extern char *tmpnam(char *);

extern int fclose(FILE *);
extern int fflush(FILE *);
extern FILE *fopen(const char *, const char *);
extern FILE *freopen(const char *, const char *, FILE *);
extern void setbuf(FILE *, char *);
extern int setvbuf(FILE *, char *, int, size_t);

extern int printf(const char *, ...);
extern int fprintf(FILE *, const char *, ...);
extern int fscanf(FILE *, const char *, ...);
extern int scanf(const char *, ...);
extern int snprintf(char *, size_t, const char *, ...);
extern int sprintf(char *, const char *, ...);
extern int sscanf(const char *, const char *, ...);
extern int vfprintf(FILE *, const char *, va_list);
extern int vfscanf(FILE *, const char *, va_list);
extern int vprintf(const char *, va_list);
extern int vscanf(const char *, va_list);
extern int vsnprintf(char *, size_t, const char *, va_list);
extern int vsprintf(char *, const char *, va_list);
extern int vsscanf(const char *, const char *, va_list);
extern int vasprintf(char **, const char *, va_list);

extern int fgetc(FILE *);
extern char *fgets(char *, int, FILE *);
extern int fputc(int, FILE *);
extern int fputs(const char *, FILE *);
extern int fputs_unlocked(const char *, FILE *);
extern int getc(FILE *);
extern int getchar(void);
extern char *gets(char *);
extern int puts(const char *);
extern int putc(int, FILE *);
extern int putchar(int);
extern int ungetc(int, FILE *);

extern size_t fread(void *, size_t, size_t, FILE *);
extern size_t fwrite(const void *, size_t, size_t, FILE *);
extern size_t fwrite_unlocked(const void *, size_t, size_t, FILE *);

extern int fgetpos(FILE *, fpos_t *);
extern int fseek(FILE *, long, int);
extern int fsetpos(FILE *, const fpos_t *);
extern long ftell(FILE *);
extern void rewind(FILE *);

extern void clearerr(FILE *);
extern void clearerr_unlocked(FILE *);
extern int fflush_unlocked(FILE *);
extern int feof(FILE *);
extern int ferror(FILE *);
extern int fpurge(FILE *);
extern void perror(const char *);

extern FILE *stdin;
extern FILE *stdout;
extern FILE *stderr;

#endif
