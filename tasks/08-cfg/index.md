# 08-cfg: index

Typed HIR -> MIR-style CFG: basic blocks, terminators, short-circuit lowering, VLA.

## Upstream deps

- 07-typeck

## Tasks (pick in order)

- [x] [01-body-builder](01-body-builder.md)
- [x] [02-local-allocation](02-local-allocation.md)
- [x] [03-expr-to-rvalue](03-expr-to-rvalue.md)
- [x] [04-place-projections](04-place-projections.md)
- [x] [05-short-circuit-lowering](05-short-circuit-lowering.md)
- [x] [06-if-else-lowering](06-if-else-lowering.md)
- [x] [07-loop-lowering](07-loop-lowering.md)
- [x] [08-switch-lowering](08-switch-lowering.md)
- [x] [09-goto-label-fixup](09-goto-label-fixup.md)
- [x] [10-call-lowering](10-call-lowering.md)
- [ ] [11-init-lowering](11-init-lowering.md)
- [ ] [12-storage-live-dead](12-storage-live-dead.md)
- [ ] [13-vla-lowering](13-vla-lowering.md)
- [ ] [14-snapshot-mir-emit](14-snapshot-mir-emit.md)
- [ ] [15-unit-tests](15-unit-tests.md)

## Downstream

- 09-codegen-llvm
