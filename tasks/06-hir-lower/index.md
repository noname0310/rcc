# 06-hir-lower: index

AST -> HIR: resolve every name, fold every declarator into a Ty, materialise composites.

## Upstream deps

- 05-parse

## Tasks (pick in order)

- [x] [01-defid-assignment](01-defid-assignment.md)
- [x] [02-name-resolution-ordinary](02-name-resolution-ordinary.md)
- [x] [03-name-resolution-tags](03-name-resolution-tags.md)
- [x] [04-name-resolution-labels](04-name-resolution-labels.md)
- [x] [05-typedef-expansion](05-typedef-expansion.md)
- [x] [06-declarator-to-ty](06-declarator-to-ty.md)
- [x] [07-composite-lowering](07-composite-lowering.md)
- [x] [08-enum-lowering](08-enum-lowering.md)
- [x] [09-statement-lowering](09-statement-lowering.md)
- [x] [10-expression-lowering](10-expression-lowering.md)
- [ ] [11-init-lowering](11-init-lowering.md)
- [ ] [12-unit-tests](12-unit-tests.md)
- [ ] [13-inline-linkage](13-inline-linkage.md)

## Downstream

- 07-typeck
