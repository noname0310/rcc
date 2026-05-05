> ✓ done — 2026-05-06

# 09-31: Lua parser runtime regression

**Phase:** 09-codegen-llvm    **Depends on:** 09-30, 06-33, 15-20    **Milestone:** real-world/lua

## Goal

Reduce and fix the runtime miscompile exposed by the Lua interpreter smoke.

## Trigger

The Lua 5.5.0 interpreter now compiles and links with `rcc`, but executing even
a tiny chunk fails:

```sh
build/rcc/lua -e 'print(42)'
```

The host compiler prints `42`; the `rcc` binary reports:

```text
(command line):1: unexpected symbol
```

An empty `-e ''` chunk has also reproduced a segmentation fault. This points to
a runtime codegen/ABI/layout bug rather than a missing declaration or upstream
build adaptation.

## Scope

- In:
  - Create a reduced C regression before changing codegen.
  - Triage likely areas: aggregate/union layout, pointer qualifiers that affect
    stores, function pointer calls, or stack/local materialization in Lua's
    parser/lexer path.
  - Add a fast regression that fails before the fix and passes after it.
  - Re-run `real_world/projects/05-lua/scripts/run-smoke.sh`.
- Out:
  - Editing Lua upstream C or header files.
  - Disabling parser/runtime code in Lua.
  - Treating `lua -v` as sufficient; the oracle must execute a Lua chunk.

## Acceptance

- [x] A minimized regression captures the same bad behavior without the full
  Lua source tree.
- [x] `build/rcc/lua -e 'print(42)'` prints `42`.
- [x] `scripts/run-smoke.sh` exits 0 and records matching host/rcc stdout.
- [x] `real_world/projects/05-lua/RESULTS.md` is updated from blocked to pass.

## Resolution

Root cause: LLVM type lowering represented unions as `{ [N x i8] }`, which has
alignment 1. A struct containing such a union was therefore too small in LLVM's
natural layout. Lua's `LexState` contains `Token`, which contains `SemInfo`
union; LLVM saw the stack object as 120 bytes while rcc's C layout metadata
correctly used 128 bytes for initialization. LLVM placed the live `firstchar`
parameter slot immediately after the too-small alloca, and rcc's 128-byte
zero-init clobbered it before calling `luaX_setinput`.

Fix: force records that are unions, or contain unions, through explicit packed
layout lowering so C offsets, padding, and storage size are represented in the
LLVM type shape. A driver e2e regression now covers the reduced
union-in-struct stack-slot clobber without depending on Lua.

## References

- `real_world/projects/05-lua/plan.md`
- `real_world/projects/05-lua/RESULTS.md`
