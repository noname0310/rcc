# 00-01: Phase ordering

**Phase:** 00-overview    **Depends on:** —    **Milestone:** M0.5

## Goal
Freeze the phase DAG so parallel agents never collide on required
interfaces. The phases in `tasks/` are numbered `01..13`; an edge
`X → Y` means "every acceptance item in X must pass before Y is
merge-eligible".

## The DAG

```
                          01-test-infra
                         ┌──────┬──────────┐
                         │      │          │
                     02-diag  03-lex    11-conf (seeded empty)
                         │      │
                         └──────▶────────┐
                                         │
                                      04-pp
                                         │
                                      05-parse
                                         │
                                      06-hir-lower
                                         │
                                      07-typeck
                                         │
                                      08-cfg
                                         │
                                      09-codegen-llvm
                                         │
                                      10-driver
                                         │
                                  ┌──────┴──────┐
                               11-conf       13-quality
                                 │
                            (suites ramp M2..M7)
                                 ▲
                                 │
                           12-fuzz-differential
                                 ▲
              (parallel with 03/04/05/09; uses their binaries)
```

## Why each edge exists

- **01 → everything.** Without a working `cargo xtask fetch-testsuites`
  and a `rcc_conformance` harness that enumerates a suite, no downstream
  task can state "I made pass rate X go up by Y".
- **02 → 03.** The lexer emits its first diagnostic (stray `\0`, invalid
  UCN) the moment it starts consuming real files, so the `ariadne`
  emitter and the stable error-code registry must be there first.
- **03 → 04 → 05.** Standard frontend order. The parser sees tokens
  only *after* the preprocessor has expanded them (C99 §5.1.1.2).
- **05 → 06 → 07 → 08 → 09.** rustc's classic middle-end chain:
  AST → HIR → typed HIR → MIR → LLVM IR. Each stage's public types
  are frozen by the skeleton; tasks fill bodies.
- **09 → 10.** The driver can't `--emit=llvm-ir` until codegen is real.
- **10 → 11.** Conformance runs end-to-end; it needs a driver that
  ingests a `.c` file and produces a binary.
- **12 parallel with 03/04/05/09.** Fuzz targets only need a buildable
  upstream crate; they don't block downstream work.
- **13 tail.** Quality work (optimisation, benches, release) comes last
  because it optimises something that already exists.

## Acceptance
- The diagram and the edge rationale in this file match
  [`tasks/README.md`](../README.md).
- CI's `conformance` job (from [`01-test-infra/13-ci-wire-conformance.md`](../01-test-infra/13-ci-wire-conformance.md))
  runs the *minimal subset* required by the current milestone and
  blocks commit approval on it.

## References
- Plan §9 "External C99 테스트셋 벤더링"
- Plan §10 "마일스톤"
- rustc's `compiler/` module graph in `rustc_driver/src/lib.rs`
