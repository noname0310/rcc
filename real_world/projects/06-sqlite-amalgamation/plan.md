# 06 - SQLite amalgamation probe plan

## Source snapshot

- Project: SQLite amalgamation
- Upstream: <https://www.sqlite.org/>
- Build reference: <https://www.sqlite.org/howtocompile.html>
- Local probe source:
  `real_world/projects/06-sqlite-amalgamation/upstream/sqlite-amalgamation-3530000/`
- Wrapper source policy: the checked-in wrapper downloads the selected official
  amalgamation zip into ignored project-local `upstream/` when it is missing.

The wrapper must not edit upstream `sqlite3.c`, `sqlite3.h`, `sqlite3ext.h`, or
`shell.c`. All adaptation belongs in wrapper scripts and build flags.

## Why This Project

SQLite is a large hosted C amalgamation with a real CLI. The probe exercises:

- very large single-translation-unit preprocessing, parsing, HIR lowering,
  type checking, and LLVM object generation;
- hosted Linux system headers through `--linux-gnu-hosted`;
- C11 mode on the amalgamation;
- multi-object CLI link (`sqlite3.c` + `shell.c`);
- host linker libraries `-ldl -lm`;
- a runtime SQL smoke that catches link-only or startup/runtime crashes.

## Probe Command

```sh
bash real_world/projects/06-sqlite-amalgamation/scripts/run-cli-smoke.sh
```

The script uses `target/debug/rcc` when it already exists. If it is missing, it
builds `rcc` with LLVM support first. Set `RCC_BUILD=1` to force rebuilding or
`RCC_BUILD=0` to require an existing `RCC`/`target/debug/rcc` binary.

Equivalent command sequence from the repository root:

```sh
./target/debug/rcc real_world/projects/06-sqlite-amalgamation/upstream/sqlite-amalgamation-3530000/sqlite3.c \
  -c -o real_world/projects/06-sqlite-amalgamation/build/sqlite3.rcc.o \
  --linux-gnu-hosted --std=c11 -w \
  -DSQLITE_THREADSAFE=0 \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  -DSQLITE_OMIT_PROGRESS_CALLBACK \
  -DSQLITE_OMIT_SHARED_CACHE \
  -DSQLITE_DEFAULT_MEMSTATUS=0

./target/debug/rcc real_world/projects/06-sqlite-amalgamation/upstream/sqlite-amalgamation-3530000/shell.c \
  -c -o real_world/projects/06-sqlite-amalgamation/build/shell.rcc.o \
  --linux-gnu-hosted --std=c11 -w \
  -I real_world/projects/06-sqlite-amalgamation/upstream/sqlite-amalgamation-3530000 \
  -DSQLITE_THREADSAFE=0 \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  -DSQLITE_OMIT_PROGRESS_CALLBACK \
  -DSQLITE_OMIT_SHARED_CACHE \
  -DSQLITE_DEFAULT_MEMSTATUS=0

cc real_world/projects/06-sqlite-amalgamation/build/sqlite3.rcc.o \
   real_world/projects/06-sqlite-amalgamation/build/shell.rcc.o \
   -o real_world/projects/06-sqlite-amalgamation/build/sqlite3.rcc \
   -ldl -lm

printf 'CREATE TABLE t(x); INSERT INTO t VALUES(1); SELECT * FROM t;\n' \
  | real_world/projects/06-sqlite-amalgamation/build/sqlite3.rcc :memory:
```

Expected stdout:

```text
1
```

## rcc flags

Common options used by both translation units:

```text
--linux-gnu-hosted --std=c11 -w
-DSQLITE_THREADSAFE=0
-DSQLITE_OMIT_LOAD_EXTENSION
-DSQLITE_OMIT_PROGRESS_CALLBACK
-DSQLITE_OMIT_SHARED_CACHE
-DSQLITE_DEFAULT_MEMSTATUS=0
```

`SQLITE_OMIT_VIRTUALTABLE` is intentionally not used. It can make older object
probes compile, but it also enables `SQLITE_OMIT_ALTERTABLE`, removes ALTER
TABLE / virtual-table helper bodies, and leaves parser-action references that
fail during the CLI link.

## Allowed local adaptation

- Wrapper scripts:
  - `scripts/run-cli-smoke.sh`
- Generated files:
  - `build/`
  - `artifacts/`
  - `logs/`
  - `scratch/`
- Local ignored source probe:
  - project-local `upstream/`
- Build flags listed above.

## Disallowed adaptation checklist

- [x] No upstream `.c` file modified
- [x] No upstream `.h` file modified
- [x] No SQLite parser/runtime source stubbed out
- [x] No runtime smoke weakened to hide an `rcc` bug
- [x] `SQLITE_OMIT_VIRTUALTABLE` is not used for the CLI link probe

## Failure log

| ID | Command | Symptom | Classification | Follow-up status |
| --- | --- | --- | --- | --- |
| SQLITE-001 | `rcc sqlite3.c` | `#if 'A' == '\301'` and related octal character escape expressions were mis-evaluated | preprocessor `#if` evaluator bug | fixed in `crates/rcc_preprocess/src/if_eval.rs` |
| SQLITE-002 | `rcc sqlite3.c` | block-scope typedef was not visible inside a `va_arg` type name | HIR lowering scope lookup bug | fixed in `crates/rcc_hir_lower/src/lib.rs` |
| SQLITE-003 | `rcc sqlite3.c` | `sizeof(pPager->dbFileVers)`-style operands were not lowered to the member type | HIR lowering expression-operand type bug | fixed in `crates/rcc_hir_lower/src/lib.rs` |
| SQLITE-004 | LLVM verifier / object emission | bodyless static function prototypes could remain referenced by lowered IR without a valid declaration | LLVM codegen declaration/linkage bug | fixed in `crates/rcc_codegen_llvm/src/lib.rs` |
| SQLITE-005 | `rcc sqlite3.c` without `SQLITE_OMIT_VIRTUALTABLE` | `const char *const *azArg; azArg = ...;` was rejected as assignment to a const-qualified object | declarator qualifier lowering bug | fixed in `crates/rcc_hir_lower/src/lib.rs` with HIR/typeck regressions |

## Exit criteria

- [x] `sqlite3.c` object is emitted by `rcc` without `SQLITE_OMIT_VIRTUALTABLE`
- [x] `shell.c` object is emitted by `rcc` with matching SQLite feature macros
- [x] host `cc` links the two objects into a CLI with `-ldl -lm`
- [x] CLI smoke on `:memory:` prints `1`
- [x] CLI smoke does not crash
- [x] Relevant compiler regressions pass
- [x] `RESULTS.md` updated to pass
