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
- [x] [11-init-lowering](11-init-lowering.md)
- [x] [12-unit-tests](12-unit-tests.md)
- [x] [13-inline-linkage](13-inline-linkage.md)
- [x] [14-type-spec-to-ty-service](14-type-spec-to-ty-service.md)
- [x] [15-file-scope-def-type-finalization](15-file-scope-def-type-finalization.md)
- [x] [16-block-scope-typedef-defs](16-block-scope-typedef-defs.md)
- [x] [17-declarator-scope-timing](17-declarator-scope-timing.md)
- [x] [18-record-enum-tag-completion](18-record-enum-tag-completion.md)
- [x] [19-type-name-expression-lowering](19-type-name-expression-lowering.md)
- [x] [20-compound-literal-temporaries](20-compound-literal-temporaries.md)
- [x] [21-initializer-string-and-completeness](21-initializer-string-and-completeness.md)
- [x] [22-switch-case-collection](22-switch-case-collection.md)
- [x] [23-hir-placeholder-regression-gate](23-hir-placeholder-regression-gate.md)
- [x] [24-gnu-range-designator-lowering](24-gnu-range-designator-lowering.md)
- [x] [25-member-access-name-preservation](25-member-access-name-preservation.md)
- [x] [26-object-qualifier-preservation](26-object-qualifier-preservation.md)
- [x] [27-file-scope-function-prototypes](27-file-scope-function-prototypes.md)
- [ ] [28-block-scope-tag-shadowing](28-block-scope-tag-shadowing.md)
- [ ] [29-function-definition-nested-parameter-decls](29-function-definition-nested-parameter-decls.md)
- [ ] [30-file-scope-compound-literal-static-storage](30-file-scope-compound-literal-static-storage.md)
- [ ] [31-aggregate-brace-elision-cursor](31-aggregate-brace-elision-cursor.md)

## Downstream

- 07-typeck

## Reopened Review Findings

This phase was reopened after the CFG stabilization review found that
real source programs can still lose type information before typeck/CFG:

- `lower_declspecs_to_base_ty` ignores typedef / record / enum specs and
  falls back to `int`.
- file-scope typedef/global definitions remain `tcx.error` placeholders.
- block-scope typedefs only affect parser-level lookup and do not create
  real HIR defs.
- cast / `sizeof(type)` / compound literals drop their `TypeName`.
- real-source `switch` statements lower with an empty `cases` table.
- GNU range designators now parse as distinct AST nodes; lowering must
  expand them rather than silently collapsing to one array element.
- member access currently loses the requested field name before typeck
  can resolve the correct field index.
- declaration-level `const` / `volatile` qualifiers are not preserved
  for objects, which blocks const-assignment checks and volatile codegen.
- file-scope function prototypes can still be misclassified as global objects
  carrying direct `Ty::Func`, which later makes LLVM codegen treat a function
  type as a global object type.
