# 06 — SQLite amalgamation

Status: manual PASS — `sqlite3.c` and `shell.c` compile to objects with `rcc`, link into a SQLite CLI with host `cc`, and pass an in-memory SQL smoke test.

Source: <https://www.sqlite.org/howtocompile.html>

Current local probe source: `sqlite/sqlite-amalgamation-3530000/`.
For a checked-in real-world wrapper, download the selected official amalgamation into `upstream/` and keep any adaptation in this directory as wrapper scripts or build-script-only patches. Do not edit upstream `.c` or `.h` files.

## Reproducible wrapper

```sh
bash real_world/projects/06-sqlite-amalgamation/scripts/run-cli-smoke.sh
```

The script writes generated objects, the linked CLI, logs, and smoke-test output under this project's ignored `build/`, `logs/`, and `artifacts/` directories. The latest recorded result is in `RESULTS.md`.

## Observed command sequence

Run from the repository root after building `target/debug/rcc` with LLVM support.

Common SQLite/rcc options used by both translation units:

```sh
--linux-gnu-hosted --std=c11 -w \
  -DSQLITE_THREADSAFE=0 \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  -DSQLITE_OMIT_PROGRESS_CALLBACK \
  -DSQLITE_OMIT_SHARED_CACHE \
  -DSQLITE_DEFAULT_MEMSTATUS=0
```

Compile the SQLite core object without `SQLITE_OMIT_VIRTUALTABLE`:

```sh
./target/debug/rcc sqlite/sqlite-amalgamation-3530000/sqlite3.c \
  -c -o sqlite/sqlite-amalgamation-3530000/sqlite3.rcc.o \
  --linux-gnu-hosted --std=c11 -w \
  -DSQLITE_THREADSAFE=0 \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  -DSQLITE_OMIT_PROGRESS_CALLBACK \
  -DSQLITE_OMIT_SHARED_CACHE \
  -DSQLITE_DEFAULT_MEMSTATUS=0
```

Compile the CLI shell object with the amalgamation include directory:

```sh
./target/debug/rcc sqlite/sqlite-amalgamation-3530000/shell.c \
  -c -o sqlite/sqlite-amalgamation-3530000/shell.rcc.o \
  --linux-gnu-hosted --std=c11 -w \
  -I sqlite/sqlite-amalgamation-3530000 \
  -DSQLITE_THREADSAFE=0 \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  -DSQLITE_OMIT_PROGRESS_CALLBACK \
  -DSQLITE_OMIT_SHARED_CACHE \
  -DSQLITE_DEFAULT_MEMSTATUS=0
```

Link the CLI with the host linker driver:

```sh
cc sqlite/sqlite-amalgamation-3530000/sqlite3.rcc.o \
   sqlite/sqlite-amalgamation-3530000/shell.rcc.o \
   -o sqlite/sqlite-amalgamation-3530000/sqlite3.rcc \
   -ldl -lm
```

Smoke test:

```sh
printf 'CREATE TABLE t(x); INSERT INTO t VALUES(1); SELECT * FROM t;\n' \
  | sqlite/sqlite-amalgamation-3530000/sqlite3.rcc :memory:
```

Expected output:

```text
1
```

## Compiler findings fixed by the probe

| ID | Classification | Status |
| --- | --- | --- |
| SQLITE-001 | Preprocessor `#if` evaluation of octal character escapes such as `#if 'A' == '\301'` | fixed in `crates/rcc_preprocess/src/if_eval.rs` |
| SQLITE-002 | HIR lowering of block-scope typedef names inside `va_arg` type names | fixed in `crates/rcc_hir_lower/src/lib.rs` |
| SQLITE-003 | HIR lowering of `sizeof` expression operands that traverse member, arrow, or index expressions | fixed in `crates/rcc_hir_lower/src/lib.rs` |
| SQLITE-004 | LLVM declaration handling for bodyless static function prototypes that remain referenced by lowered IR | fixed in `crates/rcc_codegen_llvm/src/lib.rs` |
| SQLITE-005 | Declarator/object qualifier lowering for pointer chains such as `const char *const *azArg` | fixed in `crates/rcc_hir_lower/src/lib.rs`; guarded by HIR/typeck regressions |

## Important option note

`SQLITE_OMIT_VIRTUALTABLE` must not be used for the CLI link probe. It lets the core object compile on older rcc revisions, but it also enables `SQLITE_OMIT_ALTERTABLE`, removes bodies such as `sqlite3AlterRenameTable` and virtual-table helpers, and leaves parser-action references that fail at link time.

Runtime ownership: SQLite function bodies come from the official amalgamation compiled by `rcc`; libc/libm/libdl behavior and process startup come from the host toolchain.

