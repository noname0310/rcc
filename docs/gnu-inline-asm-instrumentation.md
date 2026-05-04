# GNU Inline Asm And Instrumentation

`rcc` accepts GNU inline asm under `-fgnu-inline-asm`. Full target-specific
template lowering remains a language-extension task, but the gcc-torture 15u
cluster only uses empty templates. For that shape, HIR lowering preserves the
observable C semantics:

- output operand with matching numeric input (`"=r"(dst) : "0"(src)`) lowers to
  `dst = src`, leaving type conversion to typeck;
- read/write operands (`"+r"(expr)`, `"+m"(expr)`) evaluate `expr` exactly once;
- input-only operands are evaluated for side effects;
- clobbers are treated as barriers for the cluster policy, not as optimizer
  metadata yet.

`-finstrument-functions` emits calls to
`__cyg_profile_func_enter(this_fn, 0)` at function entry and
`__cyg_profile_func_exit(this_fn, 0)` before each generated return. Functions
declared with GNU `no_instrument_function` are skipped, including profiling
hooks themselves.

Conformance note: the gcc-torture adapter reads `dg-options` lines and passes
`-finstrument-functions` only for cases that request it. This keeps
instrumentation out of ordinary C99 and GNU compatibility runs.
