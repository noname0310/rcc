# 09-11: Globals and string literals

**Phase:** 09-codegen-llvm    **Depends on:** 09-02    **Milestone:** M4

## Goal
Emit `DefKind::Global` as `llvm::GlobalValue` with the right linkage
(internal vs external), initializer value (from `ConstEval`), and
alignment. String literals use deduplicated `@.str.N` internal
globals.

## Scope
- In: `emit_global(def)`; string literal interning keyed by
  byte-content + encoding.
- Out: TLS (`_Thread_local`) — C11, out of scope.

## Deliverables
- `GlobalCx` helper with intern table.
- Fixture with two `const char *s = "hi";` sharing the same `@.str`.

## Acceptance
- Two identical string literals in different functions resolve to
  the same `GlobalValue`.
- `static int x = 5;` emits `@x = internal global i32 5`.

## References
- LLVM LangRef globals.
