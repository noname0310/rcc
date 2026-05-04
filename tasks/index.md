# Task tree — global progress

Agents land here after reading `../agent.md`. Pick the first phase
whose checkbox is `[ ]`; that is the **active phase**. Open that
phase's `index.md` next.

## Phases (in order)

- [x] [00-overview](00-overview/index.md) — meta / working agreement (always current until all phases done)
- [x] [01-test-infra](01-test-infra/index.md) — vendor suites + conformance harness
- [x] [02-diagnostics](02-diagnostics/index.md) — real diagnostic emitter + error-code registry
- [x] [03-lex](03-lex/index.md) — full C99 pp-token lexer
- [x] [04-preprocess](04-preprocess/index.md) — C preprocessor
- [x] [05-parse](05-parse/index.md) — recursive-descent + Pratt parser
- [x] [06-hir-lower](06-hir-lower/index.md) — AST → HIR
- [x] [07-typeck](07-typeck/index.md) — conversions + const-eval
- [x] [08-cfg](08-cfg/index.md) — HIR → MIR-style CFG
- [x] [09-codegen-llvm](09-codegen-llvm/index.md) — CFG → LLVM IR
- [x] [10-driver](10-driver/index.md) — `rcc` binary + test harness
- [x] [11-conformance](11-conformance/index.md) — KPI cells per milestone
- [ ] [12-fuzz-differential](12-fuzz-differential/index.md) — fuzz + csmith
- [ ] [13-quality](13-quality/index.md) — opt levels, benches, release
- [ ] [14-lang-extensions](14-lang-extensions/index.md) — preprocessor/parser extensions (pragmas, attributes, asm)
- [ ] [15-builtin-rt](15-builtin-rt/index.md) — target info, freestanding headers, builtins

## Legend

- `[ ]` pending — agents start here.
- `[~]` in-progress — at least one task inside is claimed.
- `[x]` done — every task inside is `[x]`.

## Rules

1. Phases must be worked in declared order.
2. Within a phase, tasks must also be worked in index order
   (dependencies are encoded by position).
3. A phase flips from `[ ]` → `[x]` only when every task inside is `[x]`.
4. If an agent finds every task in the active phase is `[x]` but the
   phase line still says `[ ]`, it flips the phase line and stops.
5. `00-overview` is reference material; its checkbox is pre-marked
   `[x]` because reading is passive — no implementation needed.
