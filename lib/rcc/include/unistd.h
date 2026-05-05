#ifndef __RCC_UNISTD_H
#define __RCC_UNISTD_H

#include <stddef.h>
#include <sys/types.h>

#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2

#define F_OK 0
#define X_OK 1
#define W_OK 2
#define R_OK 4

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

#define _SC_OPEN_MAX 4

extern char **environ;

extern int access(const char *, int);
extern int chdir(const char *);
extern int close(int);
extern int dup(int);
extern int dup2(int, int);
extern int execv(const char *, char *const[]);
extern int execve(const char *, char *const[], char *const[]);
extern int execvp(const char *, char *const[]);
extern void _exit(int);
extern int fchownat(int, const char *, uid_t, gid_t, int);
extern pid_t fork(void);
extern char *getcwd(char *, size_t);
extern gid_t getegid(void);
extern uid_t geteuid(void);
extern gid_t getgid(void);
extern int gethostname(char *, size_t);
extern pid_t getpid(void);
extern pid_t getppid(void);
extern uid_t getuid(void);
extern int isatty(int);
extern off_t lseek(int, off_t, int);
extern int pipe(int[2]);
extern ssize_t read(int, void *, size_t);
extern ssize_t readlink(const char *__restrict, char *__restrict, size_t);
extern int rmdir(const char *);
extern int setgid(gid_t);
extern int setuid(uid_t);
extern unsigned int sleep(unsigned int);
extern long sysconf(int);
extern int symlink(const char *, const char *);
extern int unlink(const char *);
extern int usleep(useconds_t);
extern ssize_t write(int, const void *, size_t);

#endif
