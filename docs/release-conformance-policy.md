# Release conformance policy

`rcc` is a C99 compiler first. Release conformance gates therefore track the
supported C99 surface and keep non-C99 compatibility work visible without
letting it block a C99 release.

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
| `non-C99` | The fixture depends on C11, GNU, TinyCC, PCC, or another extension outside the current language target. |
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
| `c-testsuite::00046` | non-C99 | anonymous struct/union members are a compatibility extension outside C99 |
| `c-testsuite::00050` | non-C99 | anonymous union member inside struct is outside C99 |
| `c-testsuite::00216` | non-C99 | empty aggregate and anonymous aggregate extension forms are outside C99 |
| `c-testsuite::00219` | non-C99 | C11 `_Generic` is outside C99 |

### `tcc-tests2`

| Case | Category | Reason |
|------|----------|--------|
| `tcc-tests2::60_errors_and_warnings` | external-suite drift | TinyCC diagnostic mode checks TCC-specific diagnostics, not runtime C99 behavior |
| `tcc-tests2::70_floating_point_literals` | non-C99 | TinyCC-only binary floating constants under `__TINYC__` |
| `tcc-tests2::76_dollars_in_identifiers` | non-C99 | GNU/TinyCC `$` identifiers |
| `tcc-tests2::80_flexarray` | non-C99 | static initialization of a flexible array member, rejected by GCC under C99 pedantic mode |
| `tcc-tests2::83_utf8_in_identifiers` | non-C99 | raw UTF-8 identifier spelling extension |
| `tcc-tests2::85_asm-outside-function` | non-C99 | file-scope GNU assembly |
| `tcc-tests2::90_struct-init` | non-C99 | GNU empty structs, empty initializer lists, and global compound-literal initializer forms |
| `tcc-tests2::94_generic` | non-C99 | C11 `_Generic` |
| `tcc-tests2::96_nodata_wanted` | external-suite drift | TinyCC `-dt` data-section diagnostic mode |

`chibicc`, `llvm-test-suite`, `gcc-torture`, and `csmith` currently have empty
xfail lists in the committed release dashboard inputs.

## Exploratory suites

These runs are useful, but they are not release-blocking M7 rows:

- Full chibicc compile mode: broad extension-heavy upstream surface.
- `gcc-torture` smoke/full execute: important for future compatibility, but
  too broad for the first C99 release gate.
- `csmith` differential fuzzing: bounded bug-finding tool, not a stable
  deterministic release dashboard row.

Any non-xfailed failure discovered in the required dashboard is treated as a
compiler bug until proven otherwise. Do not raise aggregate percentages by
deleting tests, weakening adapters, or converting failures into skips without a
specific policy reason.
