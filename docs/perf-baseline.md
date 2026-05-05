# Runtime Performance Baseline

Date: 2026-05-05

Host: Linux 6.6.87.2-microsoft-standard-WSL2 x86_64 GNU/Linux

Command:

```text
cargo xtask bench-runtime --rcc target/release/rcc --host-cc cc --iterations 3 --out docs/perf-baseline.md
```

Criterion compile-speed checks:

```text
cargo bench -p rcc_lexer --bench lex
cargo bench -p rcc_preprocess --bench preprocess
cargo bench -p rcc_parse --bench parse
cargo bench -p rcc_driver --bench pipeline
```

Compile time and generated-code runtime are deliberately separate. These numbers are a baseline, not a pass/fail threshold.

| program | compiler | compile ms | runtime avg us | runs | stdout |
| --- | --- | ---: | ---: | ---: | --- |
| sum_loop | rcc | 517.226 | 11465.038 | 3 | `79974\n` |
| sum_loop | host-cc | 356.033 | 11892.247 | 3 | `79974\n` |
| fib_iter | rcc | 335.436 | 11568.033 | 3 | `46368\n` |
| fib_iter | host-cc | 305.130 | 11840.602 | 3 | `46368\n` |
| prime_count | rcc | 356.954 | 11557.779 | 3 | `95\n` |
| prime_count | host-cc | 310.440 | 11975.063 | 3 | `95\n` |
| array_mix | rcc | 338.381 | 11822.196 | 3 | `2660083\n` |
| array_mix | host-cc | 304.519 | 11532.818 | 3 | `2660083\n` |
| switch_table | rcc | 347.687 | 11583.786 | 3 | `21660\n` |
| switch_table | host-cc | 315.868 | 11668.522 | 3 | `21660\n` |
