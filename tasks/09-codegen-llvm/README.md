# 09-codegen-llvm

**Goal of the phase.** Emit a valid LLVM IR module that, once passed
through `opt -O2` + `llc`, produces a correct object file. All work
lives behind the `llvm` feature flag; the skeleton path remains
active for developers without an LLVM install.

## Tasks

| # | File | Summary |
|---|------|---------|
| 01 | [`01-codegen-context-module-target.md`](01-codegen-context-module-target.md) | `CodegenCx`, LLVM context/module, target triple, verifier baseline. |
| 02 | [`02-layoutcx-scalars.md`](02-layoutcx-scalars.md) | Size/align for every scalar `Ty`. |
| 03 | [`03-layoutcx-records.md`](03-layoutcx-records.md) | Struct / union layout, including bitfield layout metadata. |
| 04 | [`04-layoutcx-arrays.md`](04-layoutcx-arrays.md) | Fixed arrays, incomplete arrays, flexible array members, VLA layout sentinel. |
| 05 | [`05-llvm-type-lowering.md`](05-llvm-type-lowering.md) | Lower `TyId` to LLVM types with recursive caches. |
| 06 | [`06-function-and-global-declarations.md`](06-function-and-global-declarations.md) | Declare functions/globals with linkage before emitting bodies. |
| 07 | [`07-sysv-abi-params.md`](07-sysv-abi-params.md) | System V x86-64 parameter classification. |
| 08 | [`08-sysv-abi-returns.md`](08-sysv-abi-returns.md) | Return by register / `sret`. |
| 09 | [`09-place-address-load-store.md`](09-place-address-load-store.md) | Central place address, operand load, and store helpers. |
| 10 | [`10-entry-alloca-and-local-materialization.md`](10-entry-alloca-and-local-materialization.md) | Entry-block allocas and parameter/local slots. |
| 11 | [`11-global-initializer-materialization.md`](11-global-initializer-materialization.md) | Lower `GlobalInit` and string literals to LLVM constants/globals. |
| 12 | [`12-basic-block-and-terminator-wiring.md`](12-basic-block-and-terminator-wiring.md) | One LLVM BB per rcc BB plus branch/switch/return wiring. |
| 13 | [`13-call-emission-with-abi.md`](13-call-emission-with-abi.md) | Emit call terminators using ABI param/return lowering. |
| 14 | [`14-binop-and-unary-emission.md`](14-binop-and-unary-emission.md) | Map `rcc_cfg::BinOp` and `UnOp` to LLVM ops. |
| 15 | [`15-cast-emission.md`](15-cast-emission.md) | `CastKind` -> LLVM instructions. |
| 16 | [`16-aggregate-copy-memset.md`](16-aggregate-copy-memset.md) | Aggregate moves and zero-init via intrinsics. |
| 17 | [`17-vla-stack-and-len.md`](17-vla-stack-and-len.md) | Dynamic stack allocation and `Rvalue::Len` for VLAs. |
| 18 | [`18-complex-rvalue-emission.md`](18-complex-rvalue-emission.md) | C99 `_Complex` construction/extraction and arithmetic representation. |
| 19 | [`19-varargs-va-intrinsics.md`](19-varargs-va-intrinsics.md) | Variadic calls/functions and `va_start` / `va_arg`. |
| 20 | [`20-volatile-access.md`](20-volatile-access.md) | `volatile` load/store codegen. |
| 21 | [`21-bitfield-access-codegen.md`](21-bitfield-access-codegen.md) | Bitfield read/write shift-mask sequences. |
| 22 | [`22-mem2reg-and-module-verifier.md`](22-mem2reg-and-module-verifier.md) | LLVM module verifier and mem2reg proof fixtures. |
| 23 | [`23-llvm-ir-snapshots.md`](23-llvm-ir-snapshots.md) | Stable `--emit=llvm-ir` snapshots. |
| 24 | [`24-windows-llvm-c-linking.md`](24-windows-llvm-c-linking.md) | Link the official Windows LLVM archive through `LLVM-C.lib`. |
| 25 | [`25-filecheck-tests.md`](25-filecheck-tests.md) | LLVM-style `// CHECK:` tests. |
| 26 | [`26-debug-ir-metadata.md`](26-debug-ir-metadata.md) | LLVM debug metadata via DIBuilder; object DWARF smoke moves to driver. |

## Exit criteria

- `rcc --features llvm --emit=llvm-ir hello.c` produces an IR that
  `llvm-as` parses without error.
- `mem2reg` + `instcombine` hand produced IR that `llc` assembles
  and the linked binary `./a.out` runs.
- Every `rcc_cfg::Rvalue`, `TerminatorKind`, `Projection`, and
  `GlobalInitValue` variant has an explicit codegen owner task above.
