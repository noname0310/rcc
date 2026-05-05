#ifndef __RCC_DLFCN_H
#define __RCC_DLFCN_H

/*
 * Hosted Linux dynamic-loader declaration shim.
 *
 * rcc does not implement a dynamic linker.  These declarations let hosted
 * programs type-check calls; the final executable resolves the symbols through
 * the host runtime and explicit linker flags such as -ldl where the platform
 * still needs them.
 */

#define RTLD_LAZY 0x00001
#define RTLD_NOW 0x00002
#define RTLD_BINDING_MASK 0x00003
#define RTLD_NOLOAD 0x00004
#define RTLD_DEEPBIND 0x00008

#define RTLD_GLOBAL 0x00100
#define RTLD_LOCAL 0
#define RTLD_NODELETE 0x01000

#define RTLD_DEFAULT ((void *)0)
#define RTLD_NEXT ((void *)-1l)

typedef struct {
    const char *dli_fname;
    void *dli_fbase;
    const char *dli_sname;
    void *dli_saddr;
} Dl_info;

extern int dladdr(const void *, Dl_info *);
extern int dlclose(void *);
extern char *dlerror(void);
extern void *dlopen(const char *, int);
extern void *dlsym(void *__restrict, const char *__restrict);

#endif
