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
- [x] [08-unit-tests](08-unit-tests.md)
- [x] [09-linux-multiarch-include-discovery](09-linux-multiarch-include-discovery.md)
- [x] [10-ctype-hosted-declarations](10-ctype-hosted-declarations.md)
- [x] [11-hosted-c99-header-audit](11-hosted-c99-header-audit.md)
- [x] [12-hosted-core-declaration-sweep](12-hosted-core-declaration-sweep.md)
- [x] [13-hosted-math-declaration-sweep](13-hosted-math-declaration-sweep.md)
- [x] [14-missing-hosted-header-files](14-missing-hosted-header-files.md)
- [ ] [15-math-classification-macros](15-math-classification-macros.md)
- [ ] [16-complex-fenv-tgmath-review](16-complex-fenv-tgmath-review.md)

## Downstream

- 11-conformance
