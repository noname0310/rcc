# GNU Builtin Libcall Policy

This document records the policy used by `-fgnu-builtin-libcalls`. Strict C99
does not reserve these names for the compiler, so every builtin behavior must be
behind the explicit GNU compatibility flag.

## Policy Table

| Name or case | Policy | Notes |
| --- | --- | --- |
| `__builtin_abort`, `__builtin_exit` | Alias to libc symbol | Preprocessor maps the GNU builtin spelling to `abort` / `exit`; HIR lowering injects prototypes when needed. |
| `__builtin_printf`, `__builtin_sprintf`, `__builtin_snprintf` | Alias to libc symbol | Variadic prototypes are injected by HIR lowering. |
| `__builtin_memcpy`, `__builtin_memset`, `__builtin_memcmp` | Alias to libc symbol | No intrinsic folding yet; object semantics remain delegated to libc. |
| `__builtin_strcmp`, `__builtin_strcpy`, `__builtin_strncpy`, `__builtin_strchr`, `__builtin_strlen` | Alias to libc symbol | Used by gcc-torture and chibicc compatibility fixtures. |
| `__builtin_prefetch` | Fold to no-op expression | Preprocessor expands it to `((void)(addr))`; no backend operation is emitted. |
| `__builtin_add_overflow`, `__builtin_mul_overflow` | Lower specially in HIR/CFG/codegen | Produces checked overflow rvalues and stores the wrapped result. |
| `__builtin_add_overflow_p`, `__builtin_mul_overflow_p` | Lower specially in HIR/CFG/codegen | Evaluates to the overflow predicate without storing. |
| `__builtin_offsetof` | Lower specially in HIR | Produces a layout-derived integer constant for field/index paths. |
| `llabs(long long)` | Fold/lower specially in LLVM codegen | Exact GNU builtin-libcall signature lowers to a signed absolute value, even when a later local definition named `llabs` exists. |
| Fortify wrappers such as `__printf_chk`, `__fprintf_chk`, `__vprintf_chk`, `__vfprintf_chk` | Ordinary user functions unless explicitly declared by the TU | The gcc-torture fortify cases define these wrappers themselves; rcc must not rewrite or optimize them away. |

## 11-15t Result

WSL LLVM 18 command:

```text
cargo run -p rcc_conformance --bin rcc_conformance_run -- \
  --rcc target/wsl/debug/rcc \
  --suite gcc-torture --include-gpl \
  --case gcc-torture::execute::20021127-1 \
  --case gcc-torture::execute::fprintf-chk-1 \
  --case gcc-torture::execute::printf-chk-1 \
  --case gcc-torture::execute::vfprintf-1 \
  --case gcc-torture::execute::vfprintf-chk-1 \
  --case gcc-torture::execute::vprintf-1 \
  --case gcc-torture::execute::vprintf-chk-1 \
  --case gcc-torture::execute::pr103255 \
  --output target/wsl/gcc-builtin-libcalls-15t.json
```

Expected result: `8 pass, 0 fail, 0 xfail, 0 skip`.
