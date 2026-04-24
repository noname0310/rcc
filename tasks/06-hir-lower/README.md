# 06-hir-lower

**Goal of the phase.** Turn the AST into a fully name-resolved,
declarator-flattened HIR. This is where the C 이름 공간 분리 (ordinary
/ tag / label / members) becomes explicit and where declarators like
`int (*fp[3])(int,int)` become a single `Ty`.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-defid-assignment.md`](01-defid-assignment.md) | Assign `DefId` per top-level def. |
| 02 | [`02-name-resolution-ordinary.md`](02-name-resolution-ordinary.md) | Function / object / enumerator names. |
| 03 | [`03-name-resolution-tags.md`](03-name-resolution-tags.md) | `struct S`, `union U`, `enum E` tags. |
| 04 | [`04-name-resolution-labels.md`](04-name-resolution-labels.md) | Per-function label table. |
| 05 | [`05-typedef-expansion.md`](05-typedef-expansion.md) | Inline `typedef` when building `Ty`. |
| 06 | [`06-declarator-to-ty.md`](06-declarator-to-ty.md) | Fold derivation chain. |
| 07 | [`07-composite-lowering.md`](07-composite-lowering.md) | struct/union → `DefKind::Record`. |
| 08 | [`08-enum-lowering.md`](08-enum-lowering.md) | Constant-fold enumerators. |
| 09 | [`09-statement-lowering.md`](09-statement-lowering.md) | AST Stmt → HIR Stmt. |
| 10 | [`10-expression-lowering.md`](10-expression-lowering.md) | AST Expr → HIR Expr (no types yet). |
| 11 | [`11-init-lowering.md`](11-init-lowering.md) | Flatten initializer lists. |
| 12 | [`12-unit-tests.md`](12-unit-tests.md) | Declarator round-trip table. |
| 13 | [`13-inline-linkage.md`](13-inline-linkage.md) | C99 `inline` function linkage. |

## Exit criteria

- `rcc_hir_lower::lower` returns a `HirCrate` where:
  - Every `DefRef` / `LocalRef` / `Field` resolves.
  - No declarator appears (all are folded into `Ty`).
  - Every composite (struct / union / enum) has a `DefId`.
