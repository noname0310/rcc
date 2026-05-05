#ifndef __RCC_FCNTL_H
#define __RCC_FCNTL_H

#include <sys/stat.h>
#include <sys/types.h>

#define O_ACCMODE 0003
#define O_RDONLY 00
#define O_WRONLY 01
#define O_RDWR 02
#define O_CREAT 0100
#define O_EXCL 0200
#define O_NOCTTY 0400
#define O_TRUNC 01000
#define O_APPEND 02000
#define O_NONBLOCK 04000
#define O_NDELAY O_NONBLOCK
#define O_ASYNC 020000
#define O_DSYNC 010000
#define O_SYNC 04010000
#define O_FSYNC O_SYNC
#define O_DIRECTORY 0200000
#define O_NOFOLLOW 0400000
#define O_CLOEXEC 02000000
#define O_DIRECT 040000
#define O_NOATIME 01000000
#define O_PATH 010000000
#define O_SEARCH O_PATH
#define O_TMPFILE 020200000

#define F_DUPFD 0
#define F_GETFD 1
#define F_SETFD 2
#define F_GETFL 3
#define F_SETFL 4
#define FD_CLOEXEC 1

#define AT_FDCWD (-100)
#define AT_SYMLINK_NOFOLLOW 0x100

extern int creat(const char *, mode_t);
extern int fcntl(int, int, ...);
extern int open(const char *, int, ...);
extern int openat(int, const char *, int, ...);

#endif
