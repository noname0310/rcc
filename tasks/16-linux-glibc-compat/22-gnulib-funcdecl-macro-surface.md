# 16-22: gnulib Function Declaration Macro Surface

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-21-gnu-include-next-directive  
**Milestone:** hosted-linux

## Goal

Parse generated gnulib declaration helpers such as `_GL_FUNCDECL_RPL`,
`_GL_FUNCDECL_SYS`, and `_GL_CXXALIAS_*` without treating their macro-expanded
declarations as K&R function definitions.

## Scope

- In: reduced fixtures from generated `stdio.h`, `string.h`, `unistd.h`,
  `sys/stat.h`, and related replacement headers.
- In: preprocessor macro-expansion correctness or parser recovery fixes needed
  by those reduced fixtures.
- Out: copying gnulib generated headers into `lib/rcc/include`.

## Acceptance

- [x] Reduced `_GL_FUNCDECL_RPL(name, ret, (args), attrs)` fixtures preprocess
      into parseable declarations.
- [x] K&R parser diagnostics do not fire on macro-generated prototypes.
- [x] The coreutils `run-true-probe.sh` no longer reports E0030/E0063 cascades
      from `_gl_cxxalias_dummy` or gnulib declaration helpers.

## Result

- Added hosted parser regression fixtures for reduced `_GL_FUNCDECL_SYS` and
  `_GL_CXXALIAS_SYS` expansions.
- Tightened old-style K&R definition recovery so a malformed parenthesized
  function declaration does not consume following prototypes as a K&R
  declaration-list unless a function body or declaration-specifier-like token
  follows.
- Added hosted `off64_t` / `__off64_t` declaration shims because generated
  gnulib prototypes surface `off64_t` before host headers provide the type.
- Re-ran the GNU coreutils `src/true` probe: E0030/E0063 cascades from
  `_gl_cxxalias_dummy` and gnulib declaration helpers are gone.

## Follow-up

The next coreutils blocker is GNU `__extension__ static __inline` header
function syntax in glibc `<bits/byteswap.h>`. It is tracked separately as
`22a-gnu-extension-inline-header-functions.md` so task 23 can stay focused on
missing hosted declarations/macros.
