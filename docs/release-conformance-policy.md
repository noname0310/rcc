# Release conformance policy

`rcc` is now a C99/C11 compiler. Release conformance gates therefore track the
supported strict C99 release surface, C11 core-language coverage, and hosted
C11 library-header coverage separately while keeping GNU/TinyCC compatibility
work visible without letting it block an ISO C release.

## Required release dashboard

The release dashboard is regenerated with the command sequence in
[`conformance.md`](conformance.md). The required M7 rows are:

| Suite | Mode | Release rule |
|-------|------|--------------|
| `c-testsuite` | compile/link/run | `Fail = 0`, pass rate `>= 100.0%` after xfails |
| `chibicc` | `stage-1-3` | `Fail = 0`, pass rate `>= 100.0%` |
| `tcc-tests2` | compile/link/run | `Fail = 0`, pass rate `>= 95.0%` after xfails/skips |
| `llvm-test-suite` | curated SingleSource subset | `Fail = 0`, pass rate `>= 100.0%` |

`scripts/ci/check_kpi.py` enforces those rules for `docs/milestone.txt = M7`.
The check fails if a required row is missing, if any case has status `fail`,
or if the pass-rate threshold is not met.

## XFail categories

Every `xfail.toml` entry must use one of these categories in its reason text:

| Category | Meaning |
|----------|---------|
| `outside release target` | The fixture depends on GNU, TinyCC, PCC, or another extension outside the current ISO C release gates. |
| `implementation gap` | The fixture is valid for the supported target, but `rcc` has not implemented it yet. This must have a follow-up task. |
| `external-suite drift` | The vendored expected output or fixture behavior disagrees with GCC/Clang/TCC for reasons outside `rcc`. |
| `platform/runtime limitation` | The fixture depends on host headers, system calls, signals, or runtime behavior outside the portable release gate. |

Expected failures count as passing for KPI percentages, but they are not
allowed to hide compiler bugs. If an xfail is an `implementation gap`, it must
point to a concrete task or be fixed before release.

## Current xfail review

### `c-testsuite`

| Case | Category | Reason |
|------|----------|--------|
| `c-testsuite::00050` | outside release target | anonymous union member inside struct still needs alias/layout semantics beyond parser support |
| `c-testsuite::00216` | outside release target | empty aggregate and anonymous aggregate extension forms are outside strict ISO C initializer coverage |
| `c-testsuite::00219` | implementation gap | C11 `_Generic` is covered by focused C11 gates; this legacy suite case still needs full-pipeline ownership |

### `tcc-tests2`

| Case | Category | Reason |
|------|----------|--------|
| `tcc-tests2::60_errors_and_warnings` | external-suite drift | TinyCC diagnostic mode checks TCC-specific diagnostics, not runtime C99 behavior |
| `tcc-tests2::70_floating_point_literals` | outside release target | TinyCC-only binary floating constants under `__TINYC__` |
| `tcc-tests2::76_dollars_in_identifiers` | outside release target | GNU/TinyCC `$` identifiers |
| `tcc-tests2::80_flexarray` | outside release target | static initialization of a flexible array member, rejected by GCC under strict ISO modes |
| `tcc-tests2::83_utf8_in_identifiers` | outside release target | raw UTF-8 identifier spelling extension |
| `tcc-tests2::85_asm-outside-function` | outside release target | file-scope GNU assembly |
| `tcc-tests2::90_struct-init` | outside release target | GNU empty structs, empty initializer lists, and global compound-literal initializer forms |
| `tcc-tests2::94_generic` | implementation gap | C11 `_Generic` is covered by focused C11 gates; this TinyCC fixture remains a separate full-pipeline target |
| `tcc-tests2::96_nodata_wanted` | external-suite drift | TinyCC `-dt` data-section diagnostic mode |

`chibicc`, `llvm-test-suite`, `gcc-torture`, and `csmith` currently have empty
xfail lists in the committed release dashboard inputs.

## Exploratory suites

These runs are useful, but they are not release-blocking M7 rows:

- Full chibicc compile mode: broad extension-heavy upstream surface.
- `gcc-torture` smoke/full execute: important for future compatibility, but
  too broad for the first ISO release gate.
- `csmith` differential fuzzing: bounded bug-finding tool, not a stable
  deterministic release dashboard row.

Any non-xfailed failure discovered in the required dashboard is treated as a
compiler bug until proven otherwise. Do not raise aggregate percentages by
deleting tests, weakening adapters, or converting failures into skips without a
specific policy reason.
