# 16-12: Dlfcn And Runtime Linking

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-11-fcntl-dirent-stat-shims  
**Milestone:** hosted-linux

## Goal

Make dynamic-loader declarations and runtime library flags work for hosted
projects that need `dlopen` or related APIs.

## Scope

- In: `<dlfcn.h>` declarations, `-ldl` handling where needed, and diagnostic
  tests for missing link libraries.
- In: Linux behavior only.
- Out: implementing a dynamic linker.

## Acceptance

- [ ] A small `dlopen` smoke program compiles and links on Linux.
- [ ] Driver link planning preserves explicit `-ldl`.
- [ ] Missing-symbol diagnostics remain actionable.
- [ ] Documentation states that the host runtime resolves these symbols.
