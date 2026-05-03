> ✓ done — 2026-05-04 — implemented in commit

# 10-08: `--target=<triple>` CLI wiring

**Phase:** 10-driver    **Depends on:** 15-01    **Milestone:** M5

## Goal
Add a `--target=<triple>` CLI flag that selects the compilation
target. Parse the triple string, construct a `TargetInfo`, and
propagate it through the session to all consumers (preprocessor,
type-checker, layout, codegen).

## Scope
- In: CLI parsing for `--target=<triple>`. Construct
  `TargetInfo::from_triple()`. Store in the session/options and
  pass to preprocessor (for predefined macros), `LayoutCx`,
  codegen (LLVM target triple and data layout). Default to host
  triple when `--target` is absent.
- Out: cross-compilation linker selection (use host `cc` for now).

## Deliverables
- `--target` CLI flag.
- Session carries `TargetInfo`.
- All consumers read from `TargetInfo` instead of hardcoded values.
- Test: `--target=x86_64-unknown-linux-gnu` sets correct target.

## Acceptance
- `rcc --target=x86_64-unknown-linux-gnu hello.c --emit=llvm-ir`
  produces IR with `target triple = "x86_64-unknown-linux-gnu"`.
- `rcc --target=aarch64-unknown-linux-gnu` changes pointer size
  and predefined macros accordingly.
- Invalid triple produces a clear diagnostic.

## References
- LLVM target triple format documentation.
- Clang `--target` flag.
