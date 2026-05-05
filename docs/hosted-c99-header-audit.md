# Hosted C99 Header Audit

Date: 2026-05-05

`rcc` does not implement libc. The headers under `lib/rcc/include/` are
declaration shims so the frontend can parse, type-check, lower, and call symbols
provided by the target host libc or libm.

This audit exists because real-world probes started finding missing declarations
one at a time. That is the wrong loop: declaration coverage should be swept by
header family, then real-world probes should expose compiler bugs beyond simple
missing prototypes.

## Current header files

Present:

- `ctype.h`
- `assert.h`
- `errno.h`
- `float.h`
- `inttypes.h`
- `iso646.h`
- `limits.h`
- `locale.h`
- `math.h`
- `setjmp.h`
- `signal.h`
- `stdarg.h`
- `stdbool.h`
- `stddef.h`
- `stdint.h`
- `stdio.h`
- `stdlib.h`
- `string.h`
- `time.h`
- `wchar.h`
- `wctype.h`

Absent hosted C99/C95 headers:

- `complex.h`
- `fenv.h`
- `tgmath.h`

`complex.h`, `fenv.h`, and `tgmath.h` need separate compiler-support review.
The others can start as ABI-facing declaration shims.

## Function declaration coverage after `15-13`

This table counts only representative C99 function names in already-present
headers. It does not count required types/macros, and it undercounts `<math.h>`
because the float/long-double suffixed variants need a separate sweep.

| Header | Current | Expected representative set | Missing |
| --- | ---: | ---: | --- |
| `ctype.h` | 14 | 14 | none |
| `string.h` | 22 | 22 | none |
| `stdlib.h` | 36 | 36 | none |
| `stdio.h` | 46 | 46 | none |
| `math.h` | 171 | 171 | none for function declarations; classification/comparison macros remain task `15-15` |

## Real-world hits so far

| Project | Missing surface | Resolution path |
| --- | --- | --- |
| `inih` | `ctype.h` missed `isspace` | fixed by `15-10` as the complete C99 ctype set |
| `cJSON` | `stdlib.h` missed `strtod`; `stdio.h` missed `sscanf` | fixed as part of the `15-12` hosted core declaration sweep |

## Task split

1. `15-14-missing-hosted-header-files`
   - Add minimal shims for absent hosted headers.
   - Split out `complex.h`, `fenv.h`, and `tgmath.h` when compiler support is
     required.
2. `15-15-math-classification-macros`
   - Add C99 math classification/comparison macros only with sound frontend
     semantics.
3. `15-16-complex-fenv-tgmath-review`
   - Review and either implement or explicitly block the remaining C99 hosted
     headers that require compiler semantics beyond declarations.

Completed:

- `15-12-hosted-core-declaration-sweep`
  - Swept `stdio.h`, `stdlib.h`, and `string.h`.
  - Kept the implementation declaration-only.
  - Added a representative compile/link/run fixture.
- `15-13-hosted-math-declaration-sweep`
  - Swept double, float, and long-double C99 math function-family declarations.
  - Added a hosted math fixture linked with `-lm`.
  - Left classification/comparison macros as `15-15` instead of faking them.
- `15-14-missing-hosted-header-files`
  - Added minimal ABI-facing shims for `assert.h`, `errno.h`, `inttypes.h`,
    `locale.h`, `setjmp.h`, `signal.h`, `time.h`, and `wctype.h`.
  - Added a compile/link/run fixture covering representative declarations,
    types, and macros.
  - Left `complex.h`, `fenv.h`, and `tgmath.h` to `15-16` because they need
    semantics review.

## Policy

Do not copy system headers into the repository. Do not add function bodies.
Do not add POSIX/GNU declarations unless a real-world project exposes them and
the extension is explicitly classified.

The pass condition for these headers is not "all libc exists in rcc"; it is:

- the frontend can parse and type-check standard hosted C99 declarations;
- linking remains delegated to the target libc/libm;
- real-world probes stop failing on simple missing C99 prototypes and start
  exposing actual compiler behavior bugs.
