# 09-07: Entry-block `alloca` hoisting

**Phase:** 09-codegen-llvm    **Depends on:** 09-06    **Milestone:** M3

## Goal
Every non-VLA local goes into a single `alloca` at the entry block
**before** any branch. This is LLVM's required pattern for `mem2reg`
to promote the slot to SSA.

## Scope
- In: builder sets insertion point to entry block for every
  `local_alloca` call, then restores; skip for VLAs (task 13).
- Out: zero-init of aggregate locals (uses `memset` — task 12).

## Deliverables
- Helper `alloca_in_entry(name, ty)`.
- Check: post-codegen verification passes LLVM's `FunctionPassManager`
  `verify`.

## Acceptance
- For every local, the `alloca` instruction appears before any
  non-`alloca` instruction in the entry block.
- Running `opt -mem2reg` eliminates every `load` / `store` introduced
  by local slots for a trivial function.

## References
- LLVM "Kaleidoscope" chapter 7.
