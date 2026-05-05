# rcc task tree

This directory is the **executable version of the architecture plan**. The
plan (`.cursor/plans/c_compiler_architecture_plan_*.plan.md`) answers
*"what is the design?"*; this tree answers *"in what order does the agent
swarm implement it, and how do we know each step is done?"*.

## Reading order

Phases are numbered to reflect dependency order: a later phase may assume
every task in every lower-numbered phase has shipped (or is explicitly
`xfail`-ed on the suites it uses). Files inside a phase are also numbered;
within one phase you may pick tasks in any order that respects the local
dependency notes.

```
tasks/
├── README.md                 ← you are here
├── 00-overview/              ← phase DAG, KPIs, working agreement
├── 01-test-infra/            ← vendoring + conformance harness (must land first)
├── 02-diagnostics/           ← real ariadne-based emitter, error codes
├── 03-lex/                   ← full C99 pp-token lexer + fuzz target
├── 04-preprocess/            ← hide-set, #include, conditionals, #define
├── 05-parse/                 ← pp-tokens → AST; typedef hack
├── 06-hir-lower/             ← AST → HIR; name resolution; declarator flatten
├── 07-typeck/                ← conversions, decay, const-eval
├── 08-cfg/                   ← HIR → MIR/CFG
├── 09-codegen-llvm/          ← CFG → LLVM IR via inkwell
├── 10-driver/                ← `rcc` binary, --emit, UI/snapshot/E2E
├── 11-conformance/           ← milestone-indexed KPI targets per suite
├── 12-fuzz-differential/     ← cargo-fuzz corpora + bounded csmith differential
├── 13-quality/               ← opt levels, diag polish, bench, release
├── 14-lang-extensions/       ← _Pragma, __attribute__, __has_include, asm, -U, -M
├── 15-builtin-rt/            ← TargetInfo, freestanding headers, __builtin_*, sysroot
└── 16-linux-glibc-compat/    ← glibc/POSIX header shims, -pthread, hosted Linux probes
```

## Phase dependency graph

```mermaid
flowchart LR
    infra[01-test-infra]
    diag [02-diagnostics]
    lex  [03-lex]
    pp   [04-preprocess]
    parse[05-parse]
    hir  [06-hir-lower]
    tyck [07-typeck]
    cfg  [08-cfg]
    cg   [09-codegen-llvm]
    drv  [10-driver]
    conf [11-conformance]
    fuzz [12-fuzz-differential]
    qual [13-quality]
    ext  [14-lang-extensions]
    brt  [15-builtin-rt]
    glibc[16-linux-glibc-compat]

    infra --> diag
    infra --> lex
    diag --> lex
    lex --> pp
    pp --> parse
    lex --> parse
    parse --> hir
    hir --> tyck
    tyck --> cfg
    cfg --> cg
    cg --> drv
    drv --> conf
    infra --> conf
    lex --> fuzz
    pp --> fuzz
    parse --> fuzz
    cg --> fuzz
    cg --> qual
    drv --> qual
    parse --> ext
    pp --> ext
    cg --> brt
    ext --> brt
    brt --> conf
    brt --> glibc
    ext --> glibc
    glibc --> conf
```

## Task file format

Every `NN-name.md` follows the same schema so an agent can pick one off
and execute it without hunting for context:

```md
# <task id>: <short title>

**Phase:** <NN-phase>    **Depends on:** <upstream task ids>    **Milestone:** M<n>

## Goal
One paragraph. What moves from "not done" to "done" after this.

## Scope
- In:  ...
- Out: ...  (explicitly lists what the *next* task will do, not this one)

## Deliverables
- Code files touched
- New / updated tests
- Documentation updates (if any)

## Acceptance
Checklist of observable criteria: test names, pass rates on a specific
conformance subset, snapshot fixtures, doc links to verify.

## References
- Plan §<n>
- C99 §<...> (when a spec section pins the rule)
- Prior art: rustc / chibicc / cproc / clang (when a file is worth reading)
```

## Agent workflow per task

1. Read the task file end-to-end.
2. Read the *Depends on* tasks to make sure their acceptance is green.
3. Write failing tests *first* for the acceptance criteria (unit / UI /
   conformance).
4. Implement.
5. Run `cargo test --workspace` and the conformance subset referenced by
   the task.
6. Update `docs/conformance.md` if pass rates shift.
7. Mark the task file with a `✓ done` banner at the top once every
   acceptance item is verified.

## KPI and milestones

Each task points at a milestone `M0.5` .. `M7` (see plan §10). The
rollup is maintained by the `rcc_conformance` crate and reported in
`docs/conformance.md`.
