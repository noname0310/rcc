# CFG Semantics

This document records semantic choices made by `rcc_cfg` when typed HIR is
lowered to the MIR-style CFG. It is a backend contract: codegen and
conformance tests should rely on this file rather than inferring policy from
incidental statement order.

## Evaluation Order

C99 leaves the order of evaluation unspecified for several expression forms,
most importantly function-call arguments and many binary-operator operands.
`rcc_cfg` chooses one deterministic order so MIR dumps are debuggable and
later compiler stages receive a stable representation.

The policy is:

- Function call callee expression: evaluated before every argument.
- Function call arguments: evaluated left-to-right in source order.
- Non-short-circuit binary operators: left operand, then right operand, then
  the binary rvalue assignment.
- Assignment: destination place is computed first, then the right-hand value is
  evaluated, then the store is emitted.
- Comma expression: left expression is evaluated and discarded before the
  right expression.
- `&&`, `||`, and `?:`: preserve C sequencing and short-circuit behavior with
  explicit `SwitchInt` diamonds.
- Local initializer leaf stores: emitted in the HIR initializer walk order,
  after any aggregate `ZeroInit` store.
- Scope lifetime markers: `StorageLive` precedes every local initializer;
  `StorageDead` is emitted in reverse lexical lifetime order on scope exits.

This is not a promise that C source with unspecified or undefined behavior has
a portable runtime result. The CFG order is an implementation choice used for
debuggability and backend determinism.

## Conformance Policy

Differential execution against host `cc` must not classify a case as a compiler
failure when the expected stdout depends only on unspecified evaluation order.
Such cases should be skipped or demoted using `rcc_conformance::metadata`
rather than being added as ordinary expected failures.

The important distinction:

- Missing feature or known implementation bug: use `xfail.toml`.
- Non-portable fixture whose output changes with a valid evaluation order:
  use the unspecified-evaluation-order metadata skip.

This keeps pass-rate reports focused on C99 conformance gaps instead of
rewarding accidental agreement with a particular host compiler.
