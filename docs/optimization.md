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

Current non-goals:

- PGO.
- LTO.
- Target-specific tuning beyond the selected target triple and LLVM's default
  pipeline.
