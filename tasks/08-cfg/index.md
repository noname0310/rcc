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
- [x] [11-init-lowering](11-init-lowering.md)
- [x] [12-storage-live-dead](12-storage-live-dead.md)
- [x] [13-vla-lowering](13-vla-lowering.md)
- [x] [14-snapshot-mir-emit](14-snapshot-mir-emit.md)
- [x] [15-unit-tests](15-unit-tests.md)
- [x] [16-inc-dec-lowering](16-inc-dec-lowering.md)
- [x] [17-goto-scope-lifetimes](17-goto-scope-lifetimes.md)
- [x] [18-sizeof-layout-service](18-sizeof-layout-service.md)
- [x] [19-complex-conversion-rvalues](19-complex-conversion-rvalues.md)
- [x] [20-cfg-verifier-release-gate](20-cfg-verifier-release-gate.md)
- [ ] [21-eval-order-conformance-policy](21-eval-order-conformance-policy.md)
- [ ] [22-source-pipeline-edge-fixtures](22-source-pipeline-edge-fixtures.md)

## Downstream

- 09-codegen-llvm
