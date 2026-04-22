# 06-hir-lower: index

AST -> HIR: resolve every name, fold every declarator into a Ty, materialise composites.

## Upstream deps

- 05-parse

## Tasks (pick in order)

- [ ] [01-defid-assignment](01-defid-assignment.md)
- [ ] [02-name-resolution-ordinary](02-name-resolution-ordinary.md)
- [ ] [03-name-resolution-tags](03-name-resolution-tags.md)
- [ ] [04-name-resolution-labels](04-name-resolution-labels.md)
- [ ] [05-typedef-expansion](05-typedef-expansion.md)
- [ ] [06-declarator-to-ty](06-declarator-to-ty.md)
- [ ] [07-composite-lowering](07-composite-lowering.md)
- [ ] [08-enum-lowering](08-enum-lowering.md)
- [ ] [09-statement-lowering](09-statement-lowering.md)
- [ ] [10-expression-lowering](10-expression-lowering.md)
- [ ] [11-init-lowering](11-init-lowering.md)
- [ ] [12-unit-tests](12-unit-tests.md)

## Downstream

- 07-typeck
