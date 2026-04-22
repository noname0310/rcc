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

## Exit criteria

- `rcc_typeck::check` leaves no `HirExpr::ty == TyCtxt::error` unless
  an error was already emitted.
- The conversion table test (§6.3) is exhaustive for every pair of
  scalar types.
