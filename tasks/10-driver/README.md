# 10-driver

**Goal of the phase.** Stitch the whole pipeline into a polished
`rcc` binary: `--emit=` stages actually emit, `-o file.o` invokes
the linker, and the test harnesses (UI / snapshot / E2E) run from a
single driver.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-emit-stages-dump.md`](01-emit-stages-dump.md) | Wire every `EmitKind`. |
| 02 | [`02-link-invocation.md`](02-link-invocation.md) | Call `cc` / `ld` for final link. |
| 03 | [`03-ui-test-harness.md`](03-ui-test-harness.md) | `tests/ui/**/*.c` runner. |
| 04 | [`04-insta-snapshot-harness.md`](04-insta-snapshot-harness.md) | Shared snapshot helper. |
| 05 | [`05-e2e-runner.md`](05-e2e-runner.md) | Build + run programs in tests. |
| 06 | [`06-differential-cc.md`](06-differential-cc.md) | Compare against host `cc`. |
| 07 | [`07-standard-stop-flags.md`](07-standard-stop-flags.md) | `-c`, `-S`, `-E` stop flags. |
| 08 | [`08-target-cli-wiring.md`](08-target-cli-wiring.md) | `--target=<triple>` CLI flag. |
| 09 | [`09-warning-control.md`](09-warning-control.md) | `-Wall`, `-Werror`, `-Wno-*` flags. |
| 10 | [`10-linker-flags.md`](10-linker-flags.md) | `-l`, `-L`, `-Wl,`, `-shared`, `-static`. |
| 11 | [`11-multi-file.md`](11-multi-file.md) | Multiple `.c` input files. |
| 12 | [`12-misc-cli-flags.md`](12-misc-cli-flags.md) | `-std=c99`, `-ansi`, and common ignored `-f` flags. |
| 13 | [`13-debug-info-object-smoke.md`](13-debug-info-object-smoke.md) | `-g` driver wiring plus object-level DWARF smoke tests. |
| 14 | [`14-output-and-temp-artifact-policy.md`](14-output-and-temp-artifact-policy.md) | Default outputs, temp dirs, and `--save-temps`. |
| 15 | [`15-exit-status-and-diagnostics-contract.md`](15-exit-status-and-diagnostics-contract.md) | Stable driver exit-code contract. |
| 16 | [`16-tool-discovery-and-verbose-trace.md`](16-tool-discovery-and-verbose-trace.md) | Host tool lookup, dry-run command specs, and `-v`. |
| 17 | [`17-response-files.md`](17-response-files.md) | `@file` response-file expansion. |
| 18 | [`18-dependency-file-emission.md`](18-dependency-file-emission.md) | Make-compatible `-M*` dependency files. |

## Exit criteria

- `rcc foo.c -o foo` produces a runnable binary.
- `cargo test -p rcc_driver`: UI, snapshot, and E2E tests green on
  the M3+ fixture set.
- Build-system integration flags (`-c`, `-S`, `-E`, `-MMD`, response
  files, linker flags, and `-v`) have deterministic behavior and tests.
