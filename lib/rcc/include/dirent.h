#ifndef __RCC_DIRENT_H
#define __RCC_DIRENT_H

#include <sys/types.h>

typedef struct __rcc_DIR DIR;

struct dirent {
    ino_t d_ino;
    off_t d_off;
    unsigned short d_reclen;
    unsigned char d_type;
    char d_name[256];
};

#define D_INO_IN_DIRENT 1
#define _D_EXACT_NAMLEN(dp) __rcc_dirent_namlen((dp)->d_name)

#define DT_UNKNOWN 0
#define DT_FIFO 1
#define DT_CHR 2
#define DT_DIR 4
#define DT_BLK 6
#define DT_REG 8
#define DT_LNK 10
#define DT_SOCK 12
#define DT_WHT 14

static inline unsigned long __rcc_dirent_namlen(const char *name) {
    unsigned long n = 0;
    while (name[n])
        ++n;
    return n;
}

extern int closedir(DIR *);
extern DIR *fdopendir(int);
extern DIR *opendir(const char *);
extern struct dirent *readdir(DIR *);
extern void rewinddir(DIR *);
extern void seekdir(DIR *, long);
extern long telldir(DIR *);

#endif
