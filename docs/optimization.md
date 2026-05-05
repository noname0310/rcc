# Optimization

`rcc` accepts the GCC-style optimization spellings below:

| CLI spelling | Internal level | LLVM pipeline |
|--------------|----------------|---------------|
| `-O0`        | `OptLevel::None` | no optimization pipeline |
| `-O`, `-O1`  | `OptLevel::Less` | `default<O1>` |
| `-O2`        | `OptLevel::Default` | `default<O2>` |
| `-O3`        | `OptLevel::Aggressive` | `default<O3>` |

The driver also accepts the explicit clap form:

```bash
rcc --opt-level=aggressive file.c
```

Optimization runs after LLVM IR generation and module verification, before
`--emit=llvm-ir`, assembly emission, or object emission. This makes optimized
IR observable: `rcc -O2 --emit=llvm-ir file.c` should not silently fall back to
the O0 alloca-heavy shape.

## `restrict` and LLVM `noalias`

For function definitions, `rcc` preserves object-level `restrict` qualifiers on
pointer parameters through HIR and CFG locals. During LLVM function declaration,
those parameters are emitted with LLVM's `noalias` parameter attribute:

```c
void copy(int * restrict dst, int * restrict src);
```

is represented as pointer parameters carrying `noalias`. This only applies when
the C parameter itself is a pointer object and the SysV ABI lowers it to exactly
one LLVM pointer parameter.

Current non-goals for `restrict` optimization:

- Prototype-only declarations without a body. HIR currently stores parameter
  object qualifiers on function-body locals, not on `DefKind::Function`.
- Local pointer variables declared with `restrict`; scoped noalias metadata for
  loads/stores is a separate alias-analysis task.
- Non-pointer or ABI-expanded parameters. `restrict` is only meaningful for
  pointer object types and must not be attached to neighboring split LLVM
  arguments.

Current non-goals:

- PGO.
- LTO.
- Target-specific tuning beyond the selected target triple and LLVM's default
  pipeline.
