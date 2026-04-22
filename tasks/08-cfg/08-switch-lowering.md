# 08-08: `switch` lowering

**Phase:** 08-cfg    **Depends on:** 08-06    **Milestone:** M3

## Goal
Collect every `case` label's integer constant and the `default` inside
the switch body, then terminate the dispatch block with a `SwitchInt`
targeting those blocks.

## Scope
- In: two-pass over switch body: first collect cases (each case body
  gets its own block), then wire fallthroughs; `break` pops to the
  switch-join block.
- Out: jump tables (LLVM handles).

## Deliverables
- `lower_switch` + fixtures including fallthrough and nested switch.

## Acceptance
- CFG for a dispatch `switch(x) { case 1: a(); break; case 2: b(); default: c(); }`
  has a `SwitchInt` with targets `[(1, bb_case1), (2, bb_case2), (None, bb_default)]`.

## References
- C99 §6.8.4.2.
