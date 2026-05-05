# 16-22: gnulib Function Declaration Macro Surface

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

- [ ] Reduced `_GL_FUNCDECL_RPL(name, ret, (args), attrs)` fixtures preprocess
      into parseable declarations.
- [ ] K&R parser diagnostics do not fire on macro-generated prototypes.
- [ ] The coreutils `run-true-probe.sh` no longer reports E0030/E0063 cascades
      from `_gl_cxxalias_dummy` or gnulib declaration helpers.
