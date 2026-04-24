# 09-codegen-llvm: index

CFG -> LLVM IR via inkwell: ABI, layout, intrinsics. Behind the `llvm` feature.

## Upstream deps

- 08-cfg

## Tasks (pick in order)

- [ ] [01-layoutcx-scalars](01-layoutcx-scalars.md)
- [ ] [02-layoutcx-records](02-layoutcx-records.md)
- [ ] [03-layoutcx-arrays](03-layoutcx-arrays.md)
- [ ] [04-sysv-abi-params](04-sysv-abi-params.md)
- [ ] [05-sysv-abi-returns](05-sysv-abi-returns.md)
- [ ] [06-function-body-emission](06-function-body-emission.md)
- [ ] [07-alloca-entry-block](07-alloca-entry-block.md)
- [ ] [08-basic-block-translation](08-basic-block-translation.md)
- [ ] [09-binop-emission](09-binop-emission.md)
- [ ] [10-cast-emission](10-cast-emission.md)
- [ ] [11-globals-and-strings](11-globals-and-strings.md)
- [ ] [12-memcpy-memset-intrinsic](12-memcpy-memset-intrinsic.md)
- [ ] [13-varargs-va-intrinsics](13-varargs-va-intrinsics.md)
- [ ] [14-mem2reg-verify](14-mem2reg-verify.md)
- [ ] [15-llvm-ir-snapshot](15-llvm-ir-snapshot.md)
- [ ] [16-filecheck-tests](16-filecheck-tests.md)
- [ ] [17-debug-info-dwarf](17-debug-info-dwarf.md)
- [ ] [18-volatile-access](18-volatile-access.md)
- [ ] [19-bitfield-access-codegen](19-bitfield-access-codegen.md)

## Downstream

- 10-driver, 11-conformance, 12-fuzz-differential
