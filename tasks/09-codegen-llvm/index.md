# 09-codegen-llvm: index

CFG -> LLVM IR via inkwell: ABI, layout, intrinsics. Behind the `llvm` feature.

## Upstream deps

- 08-cfg

## Tasks (pick in order)

- [x] [01-codegen-context-module-target](01-codegen-context-module-target.md)
- [x] [02-layoutcx-scalars](02-layoutcx-scalars.md)
- [x] [03-layoutcx-records](03-layoutcx-records.md)
- [x] [04-layoutcx-arrays](04-layoutcx-arrays.md)
- [x] [05-llvm-type-lowering](05-llvm-type-lowering.md)
- [x] [06-function-and-global-declarations](06-function-and-global-declarations.md)
- [x] [07-sysv-abi-params](07-sysv-abi-params.md)
- [x] [08-sysv-abi-returns](08-sysv-abi-returns.md)
- [x] [09-place-address-load-store](09-place-address-load-store.md)
- [x] [10-entry-alloca-and-local-materialization](10-entry-alloca-and-local-materialization.md)
- [x] [11-global-initializer-materialization](11-global-initializer-materialization.md)
- [x] [12-basic-block-and-terminator-wiring](12-basic-block-and-terminator-wiring.md)
- [x] [13-call-emission-with-abi](13-call-emission-with-abi.md)
- [x] [14-binop-and-unary-emission](14-binop-and-unary-emission.md)
- [x] [15-cast-emission](15-cast-emission.md)
- [x] [16-aggregate-copy-memset](16-aggregate-copy-memset.md)
- [x] [17-vla-stack-and-len](17-vla-stack-and-len.md)
- [x] [18-complex-rvalue-emission](18-complex-rvalue-emission.md)
- [x] [19-varargs-va-intrinsics](19-varargs-va-intrinsics.md)
- [x] [20-volatile-access](20-volatile-access.md)
- [x] [21-bitfield-access-codegen](21-bitfield-access-codegen.md)
- [ ] [22-mem2reg-and-module-verifier](22-mem2reg-and-module-verifier.md)
- [ ] [23-llvm-ir-snapshots](23-llvm-ir-snapshots.md)
- [ ] [24-filecheck-tests](24-filecheck-tests.md)
- [ ] [25-debug-info-dwarf](25-debug-info-dwarf.md)

## Downstream

- 10-driver, 11-conformance, 12-fuzz-differential
