# 07-typeck: index

Every HirExpr gets a real TyId, every C99 (S)6.3 conversion is inserted, constant expressions evaluate.

## Upstream deps

- 06-hir-lower

## Tasks (pick in order)

- [x] [01-integer-promotion](01-integer-promotion.md)
- [ ] [02-usual-arithmetic-table](02-usual-arithmetic-table.md)
- [ ] [03-array-function-decay](03-array-function-decay.md)
- [ ] [04-lvalue-rvalue](04-lvalue-rvalue.md)
- [ ] [05-assignment-constraint](05-assignment-constraint.md)
- [ ] [06-pointer-conversions](06-pointer-conversions.md)
- [ ] [07-implicit-convert-insertion](07-implicit-convert-insertion.md)
- [ ] [08-const-eval-integer](08-const-eval-integer.md)
- [ ] [09-const-eval-extended](09-const-eval-extended.md)
- [ ] [10-init-constness](10-init-constness.md)
- [ ] [11-unit-tests-truth-tables](11-unit-tests-truth-tables.md)
- [ ] [12-complex-arithmetic](12-complex-arithmetic.md)

## Downstream

- 08-cfg
