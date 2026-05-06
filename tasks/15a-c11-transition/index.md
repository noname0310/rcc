# 15a-c11-transition: index

C11 transition tasks. Complete these before resuming phase 16 hosted Linux
work, because Toybox and similar Linux projects expose C11 syntax directly.

## Upstream deps

- 05-parse
- 06-hir-lower
- 07-typeck
- 08-cfg
- 09-codegen-llvm
- 10-driver
- 14-lang-extensions
- 15-builtin-rt

## Tasks (pick in order)

- [x] [01-language-standard-mode](01-language-standard-mode.md)
- [x] [02-c11-keyword-tokenization](02-c11-keyword-tokenization.md)
- [x] [03-noreturn-function-specifier](03-noreturn-function-specifier.md)
- [x] [04-static-assert-declarations](04-static-assert-declarations.md)
- [x] [05-alignof-alignas](05-alignof-alignas.md)
- [x] [06-anonymous-records-standard-mode](06-anonymous-records-standard-mode.md)
- [x] [07-generic-selection](07-generic-selection.md)
- [x] [08-atomic-types-and-stdatomic](08-atomic-types-and-stdatomic.md)
- [x] [09-thread-local-and-threads-header](09-thread-local-and-threads-header.md)
- [ ] [10-unicode-character-and-string-literals](10-unicode-character-and-string-literals.md)
- [ ] [11-c11-library-header-sweep](11-c11-library-header-sweep.md)
- [ ] [12-c11-conformance-and-realworld-gates](12-c11-conformance-and-realworld-gates.md)

## Downstream

- 16-linux-glibc-compat/25-toybox-applet-hosted-surface
- real_world/projects/10-toybox
- real_world/projects/11-libuv
