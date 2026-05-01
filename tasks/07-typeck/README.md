# 07-typeck

**Goal of the phase.** Make every `HirExpr` carry a real `TyId`,
insert every implicit conversion mandated by C99 §6.3, check that
assignment / return / call / initializer types are compatible, and
evaluate constant expressions where the standard requires.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-integer-promotion.md`](01-integer-promotion.md) | C99 §6.3.1.1. |
| 02 | [`02-usual-arithmetic-table.md`](02-usual-arithmetic-table.md) | §6.3.1.8. |
| 03 | [`03-array-function-decay.md`](03-array-function-decay.md) | §6.3.2.1. |
| 04 | [`04-lvalue-rvalue.md`](04-lvalue-rvalue.md) | Value-category rules. |
| 05 | [`05-assignment-constraint.md`](05-assignment-constraint.md) | §6.5.16 compatibility. |
| 06 | [`06-pointer-conversions.md`](06-pointer-conversions.md) | void*, null, compatible types. |
| 07 | [`07-implicit-convert-insertion.md`](07-implicit-convert-insertion.md) | Insert `Convert` nodes in HIR. |
| 08 | [`08-const-eval-integer.md`](08-const-eval-integer.md) | `#if`-compatible integer eval. |
| 09 | [`09-const-eval-extended.md`](09-const-eval-extended.md) | Floats, pointers-to-addr-constants. |
| 10 | [`10-init-constness.md`](10-init-constness.md) | Global init must be a `const-expr`. |
| 11 | [`11-unit-tests-truth-tables.md`](11-unit-tests-truth-tables.md) | Per-rule truth tables. |
| 12 | [`12-complex-arithmetic.md`](12-complex-arithmetic.md) | `_Complex` type arithmetic. |
| 13 | [`13-member-access-resolution.md`](13-member-access-resolution.md) | Resolve record/union member access to field type/index. |
| 14 | [`14-return-type-coercion.md`](14-return-type-coercion.md) | Check and coerce return statements. |
| 15 | [`15-coerce-to-diagnostics-gate.md`](15-coerce-to-diagnostics-gate.md) | Make failed coercions diagnostic instead of silent. |
| 16 | [`16-global-initializer-consteval.md`](16-global-initializer-consteval.md) | Fold static initializer expressions for globals. |
| 17 | [`17-control-and-conditional-constraints.md`](17-control-and-conditional-constraints.md) | Enforce scalar controls and full `?:` rules. |
| 18 | [`18-call-prototype-and-varargs-constraints.md`](18-call-prototype-and-varargs-constraints.md) | Apply prototype and default argument promotions. |
| 19 | [`19-no-error-type-pre-codegen-gate.md`](19-no-error-type-pre-codegen-gate.md) | Add final typed-HIR verifier before CFG. |
| 20 | [`20-object-qualifier-constraints.md`](20-object-qualifier-constraints.md) | Enforce const/volatile object qualifier semantics. |

## Exit criteria

- `rcc_typeck::check` leaves no `HirExpr::ty == TyCtxt::error` unless
  an error was already emitted.
- The conversion table test (§6.3) is exhaustive for every pair of
  scalar types.
- Every return, initializer, assignment, call argument, member access,
  and control expression has either a valid checked type or a diagnostic
  that stops CFG/codegen.
