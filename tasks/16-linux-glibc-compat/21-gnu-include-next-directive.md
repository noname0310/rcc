# 16-21: GNU include_next Directive

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
- Out: broad GCC warning policy for every non-C99 directive.

## Acceptance

- [ ] A generated-header fixture in `dir1/string.h` using
      `#include_next <string.h>` resolves `dir2/string.h`, not itself.
- [ ] The normal `#include <string.h>` search order remains unchanged.
- [ ] Missing `#include_next` targets emit an actionable diagnostic with the
      skipped directory named in a note.
- [ ] The coreutils `run-true-probe.sh` no longer reports E0019 for
      `include_next`.
