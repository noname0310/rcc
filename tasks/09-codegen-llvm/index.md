# 09-codegen-llvm: index

CFG -> LLVM IR via inkwell: ABI, layout, intrinsics. Behind the `llvm` feature.

## Upstream deps

- 08-cfg

## Tasks (pick in order)

- [x] [01-codegen-context-module-target](01-codegen-context-module-target.md)
- [x] [02-layoutcx-scalars](02-layoutcx-scalars.md)
- [ ] [03-layoutcx-records](03-layoutcx-records.md)
- [ ] [04-layoutcx-arrays](04-layoutcx-arrays.md)
- [ ] [05-llvm-type-lowering](05-llvm-type-lowering.md)
- [ ] [06-function-and-global-declarations](06-function-and-global-declarations.md)
- [ ] [07-sysv-abi-params](07-sysv-abi-params.md)
- [ ] [08-sysv-abi-returns](08-sysv-abi-returns.md)
- [ ] [09-place-address-load-store](09-place-address-load-store.md)
- [ ] [10-entry-alloca-and-local-materialization](10-entry-alloca-and-local-materialization.md)
- [ ] [11-global-initializer-materialization](11-global-initializer-materialization.md)
- [ ] [12-basic-block-and-terminator-wiring](12-basic-block-and-terminator-wiring.md)
- [ ] [13-call-emission-with-abi](13-call-emission-with-abi.md)
- [ ] [14-binop-and-unary-emission](14-binop-and-unary-emission.md)
- [ ] [15-cast-emission](15-cast-emission.md)
- [ ] [16-aggregate-copy-memset](16-aggregate-copy-memset.md)
- [ ] [17-vla-stack-and-len](17-vla-stack-and-len.md)
- [ ] [18-complex-rvalue-emission](18-complex-rvalue-emission.md)
- [ ] [19-varargs-va-intrinsics](19-varargs-va-intrinsics.md)
- [ ] [20-volatile-access](20-volatile-access.md)
- [ ] [21-bitfield-access-codegen](21-bitfield-access-codegen.md)
- [ ] [22-mem2reg-and-module-verifier](22-mem2reg-and-module-verifier.md)
- [ ] [23-llvm-ir-snapshots](23-llvm-ir-snapshots.md)
- [ ] [24-filecheck-tests](24-filecheck-tests.md)
- [ ] [25-debug-info-dwarf](25-debug-info-dwarf.md)

## Downstream

- 10-driver, 11-conformance, 12-fuzz-differential
