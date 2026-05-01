> ✓ done — 2026-05-01

# 06-23: HIR placeholder regression gate

**Phase:** 06-hir-lower    **Depends on:** 06-22    **Milestone:** M5 stabilization

## Goal
Add a source-driven regression suite that prevents supported HIR shapes
from quietly falling back to `tcx.error`, `tcx.int`, or `IntConst(0)`
placeholders.

## Scope
- In: source fixtures that run parse -> HIR lower -> typeck where needed.
- In: assertions over HIR defs, locals, expression kinds, and switch
  case tables.
- In: negative tests that assert diagnostics for unsupported or invalid
  constructs.
- Out: full conformance scoring.

## Deliverables
- A focused `rcc_hir_lower` integration test module or fixtures under
  `tests/fixtures/hir_lower`.
- Helper assertions for "no unsupported placeholder in supported
  construct".
- Documentation in this task file listing any placeholders intentionally
  kept after completion.

## Acceptance
- Supported source programs contain no `SizeofType -> IntConst(0)`.
- Supported declaration paths contain no accidental `tcx.int` fallback
  for typedef / record / enum specs.
- File-scope globals and typedefs have non-error types.
- Real-source switches have populated `cases`.
- The test suite includes at least one end-to-end fixture for
  `struct S { char c; int i; }; unsigned long f(void) { struct S s; return sizeof s; }`.

## References
- All reopened 06-hir-lower stabilization tasks.
- The CFG 08 review that exposed `sizeof` and record type loss through
  source-to-MIR fixtures.

## Placeholder policy after completion

The regression gate treats the following as intentional boundaries, not
silent lowering failures:

- Expression node `ty: tcx.error` is still allowed immediately after HIR
  lowering; `rcc_typeck::check` must replace it for clean supported
  fixtures.
- `Field { field_index: 0 }` remains a pre-typeck placeholder because
  field lookup depends on the base expression type.
- Invalid constructs may still lower to recovery nodes after emitting a
  diagnostic, for example bad initializer designators emitting E0079.
