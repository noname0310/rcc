# 15-builtin-rt: index

Compiler-owned support surface: target abstraction, freestanding headers, builtin lowering, system header discovery. Hosted libc/glibc/MSVCRT bodies are not implemented by rcc.

## Upstream deps

- 14-lang-extensions, 09-codegen-llvm

## Tasks (pick in order)

- [x] [01-target-info](01-target-info.md)
- [x] [02-stddef-header](02-stddef-header.md)
- [x] [03-stdarg-header](03-stdarg-header.md)
- [x] [04-remaining-freestanding](04-remaining-freestanding.md)
- [x] [05-builtin-va-functions](05-builtin-va-functions.md)
- [x] [06-builtin-common](06-builtin-common.md)
- [x] [07-system-header-search](07-system-header-search.md)
- [ ] [08-unit-tests](08-unit-tests.md)

## Downstream

- 11-conformance
