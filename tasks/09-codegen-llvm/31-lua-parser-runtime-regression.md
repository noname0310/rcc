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

- [ ] A minimized regression captures the same bad behavior without the full
  Lua source tree.
- [ ] `build/rcc/lua -e 'print(42)'` prints `42`.
- [ ] `scripts/run-smoke.sh` exits 0 and records matching host/rcc stdout.
- [ ] `real_world/projects/05-lua/RESULTS.md` is updated from blocked to pass.

## References

- `real_world/projects/05-lua/plan.md`
- `real_world/projects/05-lua/RESULTS.md`
