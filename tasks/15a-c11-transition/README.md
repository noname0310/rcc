# 15a-c11-transition

**Goal of the phase.** Move `rcc` from a C99-only compiler to a
C11-capable compiler without hiding C11 syntax behind project-local macros.
This phase exists because real hosted Linux code, including Toybox, uses C11
spellings such as `_Noreturn` directly in project headers. Treating those as
preprocessor hacks makes later HIR, CFG, diagnostics, and LLVM codegen
incorrect.

## Source Notes

The public WG14 standards page identifies WG14 N1570 as the latest publicly
available C11 draft and says it reflects what became ISO/IEC 9899:2011 at the
time of issue. WG14's project list maps C11 to ISO/IEC 9899:2011 and N1570.
N1570's foreword lists the large C11 additions over C99: optional features,
threads and atomics, alignment, Unicode strings, type-generic expressions,
static assertions, and anonymous structures/unions. Clang's documentation is
useful as an implementation reference: it treats `_Alignas`, `_Alignof`,
`_Atomic`, `_Generic`, `_Noreturn`, `_Static_assert`, and `_Thread_local` as C
keywords with C semantics and documents the C11 feature buckets it exposes.

References:

- WG14 approved standards: <https://open-std.org/jtc1/sc22/wg14/www/standards.html>
- WG14 project status: <https://www.open-std.org/jtc1/sc22/wg14/www/projects>
- N1570 rendered draft: <https://www.iso-9899.info/n1570.html>
- Clang language extensions: <https://clang.llvm.org/docs/LanguageExtensions.html>

## Policy

- `-std=c99` remains supported while the transition is underway.
- `-std=c11` enables standard C11 syntax and updates standard predefined
  macros; it does not imply GNU syntax.
- `-std=gnu11` may be introduced only as an explicit GNU dialect alias once
  the strict C11 mode exists.
- Do not solve C11 syntax by injecting fake macros from `--linux-gnu-hosted`.
  Headers such as `stdnoreturn.h` may define convenience macros, but the
  compiler must still parse the underlying keyword.
- Annex K bounds-checking interfaces and full C11 threads runtime behavior are
  optional/deferred unless a real-world probe needs them. Hosted C11 library
  headers should come from the real target sysroot; do not add approximate libc
  shims under `lib/rcc/include`.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-language-standard-mode.md`](01-language-standard-mode.md) | Add first-class C99/C11 language-standard selection. |
| 02 | [`02-c11-keyword-tokenization.md`](02-c11-keyword-tokenization.md) | Tokenize C11 keywords without stealing C99 diagnostics. |
| 03 | [`03-noreturn-function-specifier.md`](03-noreturn-function-specifier.md) | Implement `_Noreturn` as a real function specifier. |
| 04 | [`04-static-assert-declarations.md`](04-static-assert-declarations.md) | Parse and evaluate `_Static_assert` declarations. |
| 05 | [`05-alignof-alignas.md`](05-alignof-alignas.md) | Implement `_Alignof`, `_Alignas`, and `stdalign.h`. |
| 06 | [`06-anonymous-records-standard-mode.md`](06-anonymous-records-standard-mode.md) | Treat anonymous structs/unions as standard C11. |
| 07 | [`07-generic-selection.md`](07-generic-selection.md) | Implement `_Generic` expression typing and lowering. |
| 08 | [`08-atomic-types-and-stdatomic.md`](08-atomic-types-and-stdatomic.md) | Add `_Atomic` type syntax and minimal `<stdatomic.h>`. |
| 09 | [`09-thread-local-and-threads-header.md`](09-thread-local-and-threads-header.md) | Add `_Thread_local` and declaration-only `<threads.h>`. |
| 10 | [`10-unicode-character-and-string-literals.md`](10-unicode-character-and-string-literals.md) | Add C11 Unicode literal prefixes and `<uchar.h>`. |
| 11 | [`11-c11-library-header-sweep.md`](11-c11-library-header-sweep.md) | Add C11 headers/macros not covered by language tasks. |
| 12 | [`12-c11-conformance-and-realworld-gates.md`](12-c11-conformance-and-realworld-gates.md) | Wire C11 tests and unblock Toybox without macro hacks. |
| 13 | [`13-real-host-c11-library-headers.md`](13-real-host-c11-library-headers.md) | Keep C11 coverage while replacing shimmed library headers with real host sysroot headers. |

## Exit Criteria

- `rcc -std=c11` accepts representative strict C11 snippets for every task in
  this phase.
- `rcc -std=c99` still diagnoses or warns for C11-only constructs according to
  the task's compatibility policy.
- `__STDC_VERSION__` is `199901L` in C99 mode and `201112L` in C11 mode.
- Toybox no longer needs `_Noreturn` to be faked by `-D_Noreturn=`.
- C11-specific tests live in ordinary crate tests and at least one driver e2e
  gate, not only in real-world scripts.
