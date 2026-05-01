# 08-16: Increment/decrement lowering

**Phase:** 08-cfg    **Depends on:** 08-15    **Milestone:** M3 stabilization

## Goal
Remove the remaining `todo!` path for `++` and `--` expressions in CFG
lowering. Parser, HIR lowering, and typeck already accept these
operators, so CFG must lower them instead of panicking.

## Scope
- In: `PreInc`, `PreDec`, `PostInc`, and `PostDec` for scalar lvalues.
- In: integer and pointer increment/decrement, using typeck's final
  expression type and pointer element stride.
- In: correct value result: prefix yields the new value; postfix yields
  the old value.
- Out: atomic/volatile side-effect ordering beyond the existing CFG
  memory model.

## Deliverables
- Replace the `todo!("inc/dec lowering ...")` arm in `rcc_cfg`.
- Add lowering helpers that emit explicit read-modify-write statements.
- Add unit tests for `++i`, `i++`, `--i`, `i--`, `*p++`, and array-index
  lvalues.
- Add a regression fixture for `for (i = 0; i < n; ++i)`.

## Acceptance
- `cargo test -p rcc_cfg inc_dec` passes.
- `rg "inc/dec lowering" crates/rcc_cfg/src` returns no `todo!`.
- `int f(void) { int i = 0; return i++; }` lowers without panic.
- `int f(int *p) { return (*p)++; }` emits one store to `*p` and returns
  the old value.

## References
- C99 §6.5.2.4 postfix increment/decrement.
- C99 §6.5.3.1 prefix increment/decrement.
- `crates/rcc_cfg/src/lower.rs` current `HirUnOp` arm.
