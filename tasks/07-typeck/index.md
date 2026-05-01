# 07-typeck: index

Every HirExpr gets a real TyId, every C99 (S)6.3 conversion is inserted, constant expressions evaluate.

## Upstream deps

- 06-hir-lower

## Tasks (pick in order)

- [x] [01-integer-promotion](01-integer-promotion.md)
- [x] [02-usual-arithmetic-table](02-usual-arithmetic-table.md)
- [x] [03-array-function-decay](03-array-function-decay.md)
- [x] [04-lvalue-rvalue](04-lvalue-rvalue.md)
- [x] [05-assignment-constraint](05-assignment-constraint.md)
- [x] [06-pointer-conversions](06-pointer-conversions.md)
- [x] [07-implicit-convert-insertion](07-implicit-convert-insertion.md)
- [x] [08-const-eval-integer](08-const-eval-integer.md)
- [x] [09-const-eval-extended](09-const-eval-extended.md)
- [x] [10-init-constness](10-init-constness.md)
- [x] [11-unit-tests-truth-tables](11-unit-tests-truth-tables.md)
- [x] [12-complex-arithmetic](12-complex-arithmetic.md)
- [x] [13-member-access-resolution](13-member-access-resolution.md)
- [x] [14-return-type-coercion](14-return-type-coercion.md)
- [x] [15-coerce-to-diagnostics-gate](15-coerce-to-diagnostics-gate.md)
- [ ] [16-global-initializer-consteval](16-global-initializer-consteval.md)
- [ ] [17-control-and-conditional-constraints](17-control-and-conditional-constraints.md)
- [ ] [18-call-prototype-and-varargs-constraints](18-call-prototype-and-varargs-constraints.md)
- [ ] [19-no-error-type-pre-codegen-gate](19-no-error-type-pre-codegen-gate.md)
- [ ] [20-object-qualifier-constraints](20-object-qualifier-constraints.md)

## Downstream

- 08-cfg

## Reopened Review Findings

This phase is reopened before 09-codegen-llvm because the completed
baseline still allows semantically invalid or placeholder-typed HIR to
reach CFG:

- member access is not resolved to the actual field type/index.
- return statements are not checked against the enclosing function's
  return type.
- failed pointer/object coercions can fall through without diagnostics.
- static initializers are checked by helper APIs but not wired into the
  crate-level pipeline.
- controlling expressions and `?:` still use permissive placeholder
  rules in some cases.
- function calls need prototype/varargs coercion before ABI lowering.
- codegen needs a final no-`Ty::Error` gate.
- object qualifiers preserved by HIR must drive const/volatile rules.
