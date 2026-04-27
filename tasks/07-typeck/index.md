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
- [ ] [12-complex-arithmetic](12-complex-arithmetic.md)

## Downstream

- 08-cfg
