# 16-21: GNU include_next Directive

> ✓ done — 2026-05-06

**Phase:** 16-linux-glibc-compat  
**Depends on:** 16-17-gnu-coreutils-single-utility-probe  
**Milestone:** hosted-linux

## Goal

Support GNU `#include_next` well enough for generated gnulib replacement
headers to include the next header with the same spelling after the current
include directory.

## Scope

- In: directive parsing, include search starting after the including header's
  directory, dependency recording, and regression tests.
- In: `#include_next <h>` and `#include_next "h"` as a GNU hosted extension.
- Out: broad GCC warning policy for every outside-release directive.

## Acceptance

- [x] A generated-header fixture in `dir1/string.h` using
      `#include_next <string.h>` resolves `dir2/string.h`, not itself.
- [x] The normal `#include <string.h>` search order remains unchanged.
- [x] Missing `#include_next` targets emit an actionable diagnostic with the
      skipped directory named in a note.
- [x] The coreutils `run-true-probe.sh` no longer reports E0019 for
      `include_next`.

## Result

- Added `Directive::IncludeNext` parsing and dispatch through
  `Preprocessor::process_include_next`.
- Implemented GNU include-next search from the include root that produced the
  current header. This matters for generated headers such as
  `<sys/stat.h>`, where the search root is `build/lib`, not the physical
  `build/lib/sys` directory.
- Added regression tests for direct header names, subdirectory header names,
  ordinary include search preservation, and missing-target diagnostics.
- Re-ran the GNU coreutils `src/true.c` probe under WSL. The probe no longer
  reports E0019 or recursive include cycles for `include_next`; the next
  concrete blocker is `16-22-gnulib-funcdecl-macro-surface`.
