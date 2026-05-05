#ifndef __RCC_BITS_POSIX_TYPES_H
#define __RCC_BITS_POSIX_TYPES_H

#include <stddef.h>

/*
 * POSIX hosted Linux scalar type shims.
 *
 * These match the current LP64 glibc-oriented target surface used by rcc's
 * hosted probes.  Do not add layout-sensitive typedefs here without adding a
 * target-info backed test.
 */

#ifndef __rcc_clock_t_defined
typedef long clock_t;
#define __rcc_clock_t_defined 1
#endif

#ifndef __rcc_time_t_defined
typedef long time_t;
#define __rcc_time_t_defined 1
#endif

#ifndef __rcc_clockid_t_defined
typedef int clockid_t;
#define __rcc_clockid_t_defined 1
#endif

#ifndef __rcc_pid_t_defined
typedef int pid_t;
#define __rcc_pid_t_defined 1
#endif

#ifndef __rcc_uid_t_defined
typedef unsigned int uid_t;
#define __rcc_uid_t_defined 1
#endif

#ifndef __rcc_gid_t_defined
typedef unsigned int gid_t;
#define __rcc_gid_t_defined 1
#endif

#ifndef __rcc_mode_t_defined
typedef unsigned int mode_t;
#define __rcc_mode_t_defined 1
#endif

#ifndef __rcc_dev_t_defined
typedef unsigned long dev_t;
#define __rcc_dev_t_defined 1
#endif

#ifndef __rcc_ino_t_defined
typedef unsigned long ino_t;
#define __rcc_ino_t_defined 1
#endif

#ifndef __rcc_nlink_t_defined
typedef unsigned long nlink_t;
#define __rcc_nlink_t_defined 1
#endif

#ifndef __rcc_off_t_defined
typedef long off_t;
#define __rcc_off_t_defined 1
#endif

#ifndef __rcc___off64_t_defined
typedef long __off64_t;
#define __rcc___off64_t_defined 1
#endif

#ifndef __rcc_off64_t_defined
typedef __off64_t off64_t;
#define __rcc_off64_t_defined 1
#endif

#ifndef __rcc_ssize_t_defined
typedef long ssize_t;
#define __rcc_ssize_t_defined 1
#endif

#ifndef __rcc_blksize_t_defined
typedef long blksize_t;
#define __rcc_blksize_t_defined 1
#endif

#ifndef __rcc_blkcnt_t_defined
typedef long blkcnt_t;
#define __rcc_blkcnt_t_defined 1
#endif

#ifndef __rcc_fsblkcnt_t_defined
typedef unsigned long fsblkcnt_t;
#define __rcc_fsblkcnt_t_defined 1
#endif

#ifndef __rcc_fsfilcnt_t_defined
typedef unsigned long fsfilcnt_t;
#define __rcc_fsfilcnt_t_defined 1
#endif

#ifndef __rcc_useconds_t_defined
typedef unsigned int useconds_t;
#define __rcc_useconds_t_defined 1
#endif

#ifndef __rcc_suseconds_t_defined
typedef long suseconds_t;
#define __rcc_suseconds_t_defined 1
#endif

#endif
