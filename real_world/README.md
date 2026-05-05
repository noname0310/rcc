# Real-world compile-and-run probes

This directory tracks real open-source C projects that should be compiled with
`rcc` after the conformance suites are green enough to justify broader probes.

The goal is not to vendor these projects. Each project is cloned dynamically
when its turn starts, inspected in place, and given a small local plan based on
the source tree that was actually fetched.

The primary purpose is compiler testing. A project probe is valuable when it
finds an `rcc` frontend, lowering, typeck, CFG, codegen, runtime, or driver bug
that the curated conformance suites missed.

## Non-negotiable rules

1. Do not edit upstream C or header files.
2. Do not commit cloned upstream sources, build directories, logs, or generated
   binaries.
3. Build adaptation must be limited to wrapper scripts, invocation scripts,
   generated config headers, or build-script-only patches.
4. If a build requires changing upstream `.c` or `.h` files, stop and file an
   `rcc` compiler/runtime task instead.
5. If a project depends on non-C99 extensions, record the exact extension and
   decide whether it belongs in `14-lang-extensions`, `15-builtin-rt`, or a
   project-local skip.

## Per-project loop

For a project `real_world/projects/NN-name`:

1. Read `PROJECT.md` only.
2. Clone the source into `upstream/` at the recorded URL and record the resolved
   commit in a new `plan.md`.
3. Read the upstream build files and the minimum source files needed to
   understand the build entry point.
4. Create or update `plan.md` with the exact compile target, expected command,
   required `rcc` flags, expected host tools, test oracle, and known blockers.
5. Build and run a host-compiler baseline when the project has a small runnable
   test or example.
6. Add wrapper scripts or build-script-only patches under the project directory.
7. Run the smallest meaningful `rcc` compile probe first.
8. If an executable can be linked, run it and compare exit status/stdout/stderr
   with the host-compiler baseline.
9. If the failure is an `rcc` bug, stop project integration and create the
   smallest compiler regression first.
10. Fix `rcc`, commit that compiler fix, then rerun the project probe.
11. When the probe passes, record the command and result in `RESULTS.md`.

## Failure loop

Classify every failure before changing project integration:

| Failure class | Action |
| --- | --- |
| C99 feature missing or implemented incorrectly | add a focused task under the owning phase, add a minimized regression, fix `rcc`, then rerun |
| Wrong diagnostics for valid C99 | add parser/typeck/preprocess regression before changing the wrapper |
| Wrong runtime behavior | add CFG/codegen/driver regression and compare against the host compiler |
| Missing hosted-library surface | add a `15-builtin-rt` task or document the external libc dependency |
| Non-C99 extension | record the exact extension and decide between `14-lang-extensions` or a project-local skip |
| Build-system assumption | wrapper or build-script-only patch is allowed |

Do not mark a project probe as passing by deleting a failing upstream test,
editing upstream C, loosening the oracle, or suppressing diagnostics. If a
failure is a real compiler bug, the next unit of work is the compiler bug, not
the project integration.

## Pass definition

A project stage is passing only when all applicable checks are true:

- the selected upstream source compiles with `rcc`
- the same source compiles with the host compiler baseline
- the linked executable runs when the project has a runnable smoke target
- observable behavior matches the baseline, or a documented project limitation
  explains why no runtime oracle exists
- any newly discovered compiler bug has a committed regression test

## Patch policy

Allowed tracked files:

- `plan.md`
- `RESULTS.md`
- wrapper scripts owned by this repository
- `patches/*.patch` when the patch touches build scripts only
- generated config templates used by wrappers

Disallowed tracked files:

- upstream source clones
- modified upstream C files
- modified upstream headers
- build products
- runtime logs

## Project order

Use [ordered-projects.md](ordered-projects.md). Do not jump to a harder project
until the current project either passes or has a documented blocker with a
follow-up `rcc` task.

Hosted Linux status lives in [hosted-linux-dashboard.md](hosted-linux-dashboard.md).
It records stage-level pass/blocker cells for MuJS and GNU coreutils without
turning compiler failures into aggregate percentages.
