# SQLite amalgamation Results

Last verified: 2026-05-06 on Linux/WSL with LLVM 18.

Command:

```sh
bash real_world/projects/06-sqlite-amalgamation/scripts/run-cli-smoke.sh
```

Equivalent command sequence is recorded in `PROJECT.md` and `plan.md`. The
wrapper downloads the official amalgamation into this project's ignored
`upstream/` directory when it is missing.

Result:

- `rcc` build availability: success
- `sqlite3.c` object compile: success
- `shell.c` object compile: success
- host `cc` CLI link: success
- runtime SQL smoke: success
- final output: `1`

Runtime command:

```sh
printf 'CREATE TABLE t(x); INSERT INTO t VALUES(1); SELECT * FROM t;\n' \
  | real_world/projects/06-sqlite-amalgamation/build/sqlite3.rcc :memory:
```

Runtime stdout:

```text
1
```

## Compiler bugs found

| ID | Status | Symptom |
| --- | --- | --- |
| SQLITE-001 | fixed | Preprocessor `#if` octal character escape expressions such as `#if 'A' == '\301'` were not evaluated correctly |
| SQLITE-002 | fixed | Block-scope typedef names were not visible inside `va_arg` type names |
| SQLITE-003 | fixed | `sizeof` expression operands traversing member, arrow, or index expressions did not resolve to the operand member type |
| SQLITE-004 | fixed | Bodyless static function prototypes could produce invalid or missing LLVM declarations when referenced by lowered IR |
| SQLITE-005 | fixed | Pointer-chain qualifiers such as `const char *const *azArg` were confused with top-level object qualifiers, causing valid assignments to be rejected |

## Important option result

The passing CLI probe does not define `SQLITE_OMIT_VIRTUALTABLE`.

That macro allowed older object-only probes to progress, but it also enables
`SQLITE_OMIT_ALTERTABLE`, removes bodies such as `sqlite3AlterRenameTable` and
virtual-table helpers, and leaves parser-action references that fail the final
CLI link.

## Upstream source policy

The wrapper does not modify upstream C or header files. It downloads and
extracts the official `sqlite-amalgamation-3530000.zip` archive into this
project directory's ignored `upstream/` tree, with generated outputs under
ignored `build/`, `logs/`, `artifacts/`, or `scratch/` directories.
