# zlib results

## 2026-05-05 smoke

**Snapshot:** `f9dd6009be3ed32415edf1e89d1bc38380ecb95d`

**Command:**

```sh
bash real_world/projects/03-zlib/scripts/run-smoke.sh
```

**Result:** pass.

- Host baseline: `gcc -std=c99 -Wall`
- Host stdout: `zlib smoke ok`
- `rcc` command: `target/release/rcc --std=c99 -Wall`
- `rcc` stdout: `zlib smoke ok`
- Runtime oracle: exact stdout comparison with the host compiler baseline

The wrapper compiles these zlib core sources with the generated smoke program:

```text
adler32.c crc32.c deflate.c infback.c inffast.c inflate.c inftrees.c
trees.c zutil.c compress.c uncompr.c ../scratch/zlib_smoke.c
```

## Compiler bugs found

| ID | Fixed by | Symptom |
| --- | --- | --- |
| ZLIB-001 | `tasks/04-preprocess/22-multiline-function-macro-invocation.md` | multiline function-like macro invocations leaked unexpanded tokens |
| ZLIB-002 | `tasks/09-codegen-llvm/30-external-incomplete-array-globals.md` | external incomplete array globals were rejected before LLVM codegen |
| ZLIB-003 | `tasks/08-cfg/28-string-literal-index-place.md` | CFG panicked on string literal subscript lvalue lowering |
| ZLIB-004 | `tasks/07-typeck/24-casted-string-global-initializer.md` | casted string literal pointer initializers stayed as error leaves |

## Upstream source policy

The wrapper does not modify upstream C or header files. The local `upstream/`
clone is ignored by git. On Windows/WSL mixed checkouts, an existing clone may
show CRLF-only working-tree noise; new clones created by the wrapper set
`core.autocrlf=false`.
