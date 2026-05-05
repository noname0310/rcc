# MuJS Results

Last verified: 2026-05-06 on WSL/Linux with LLVM 18.

Command:

```sh
LLVM_SYS_181_PREFIX=/usr/lib/llvm-18 \
  bash real_world/projects/07-mujs/scripts/run-smoke.sh
```

Result:

- host build: success
- rcc build/link: success
- runtime comparison: success
- final output line: `mujs smoke ok`

The wrapper compiles upstream `main.c` and `one.c` with both host `cc` and
`rcc`, links with host `libm`, and runs the same generated JavaScript smoke
through both binaries.  The smoke covers loops, closures, objects, arrays,
JSON, regular expressions, strings, and math.

No upstream `.c` or `.h` files are modified.
