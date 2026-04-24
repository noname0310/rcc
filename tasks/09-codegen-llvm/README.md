# 09-codegen-llvm

**Goal of the phase.** Emit a valid LLVM IR module that, once passed
through `opt -O2` + `llc`, produces a correct object file. All work
lives behind the `llvm` feature flag; the skeleton path remains
active for developers without an LLVM install.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-layoutcx-scalars.md`](01-layoutcx-scalars.md) | Size/align for every scalar `Ty`. |
| 02 | [`02-layoutcx-records.md`](02-layoutcx-records.md) | Struct / union layout, bitfields. |
| 03 | [`03-layoutcx-arrays.md`](03-layoutcx-arrays.md) | Arrays, flexible array member. |
| 04 | [`04-sysv-abi-params.md`](04-sysv-abi-params.md) | System V x86-64 parameter classification. |
| 05 | [`05-sysv-abi-returns.md`](05-sysv-abi-returns.md) | Return by register / `sret`. |
| 06 | [`06-function-body-emission.md`](06-function-body-emission.md) | `Body` → `FunctionValue`. |
| 07 | [`07-alloca-entry-block.md`](07-alloca-entry-block.md) | Centralise allocas. |
| 08 | [`08-basic-block-translation.md`](08-basic-block-translation.md) | One LLVM BB per rcc BB. |
| 09 | [`09-binop-emission.md`](09-binop-emission.md) | Map `rcc_cfg::BinOp` to LLVM ops. |
| 10 | [`10-cast-emission.md`](10-cast-emission.md) | `CastKind` → LLVM instructions. |
| 11 | [`11-globals-and-strings.md`](11-globals-and-strings.md) | Emit `@str.0` etc. |
| 12 | [`12-memcpy-memset-intrinsic.md`](12-memcpy-memset-intrinsic.md) | Aggregate moves. |
| 13 | [`13-varargs-va-intrinsics.md`](13-varargs-va-intrinsics.md) | `va_start` / `va_arg`. |
| 14 | [`14-mem2reg-verify.md`](14-mem2reg-verify.md) | Run `opt -mem2reg` in tests. |
| 15 | [`15-llvm-ir-snapshot.md`](15-llvm-ir-snapshot.md) | `--emit=llvm-ir` snapshots. |
| 16 | [`16-filecheck-tests.md`](16-filecheck-tests.md) | LLVM-style `// CHECK:` tests. |
| 17 | [`17-debug-info-dwarf.md`](17-debug-info-dwarf.md) | DWARF debug metadata via DIBuilder. |
| 18 | [`18-volatile-access.md`](18-volatile-access.md) | `volatile` load/store codegen. |
| 19 | [`19-bitfield-access-codegen.md`](19-bitfield-access-codegen.md) | Bitfield read/write shift-mask sequences. |

## Exit criteria

- `rcc --features llvm --emit=llvm-ir hello.c` produces an IR that
  `llvm-as` parses without error.
- `mem2reg` + `instcombine` hand produced IR that `llc` assembles
  and the linked binary `./a.out` runs.
