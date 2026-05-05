# Ordered project probes

The order starts with tiny single-library projects, then moves toward projects
with larger build systems and more platform assumptions.

| Order | Project | Source | First goal | Runtime oracle | Why here |
| --- | --- | --- | --- | --- | --- |
| 01 | inih | <https://github.com/benhoyt/inih> | compile the C parser and one example/test object | run a tiny INI parse program and compare output | very small C surface, minimal preprocessor pressure |
| 02 | cJSON | <https://github.com/DaveGamble/cJSON> | compile `cJSON.c` | run a tiny JSON round-trip probe | single-file library, realistic strings/pointers/structs |
| 03 | zlib | <https://github.com/madler/zlib> | compile core library objects without generated configure edits | compress/decompress one buffer | classic portable C library, moderate macros |
| 04 | LibTomMath | <https://github.com/libtom/libtommath> | compile the library objects | run a small arithmetic smoke program | many translation units, integer-heavy code |
| 05 | Lua | <https://www.lua.org/source/> | compile the Lua core library first, then the interpreter if reachable | run one small script if linkage works | real VM code, parser/runtime tables, portable C |
| 06 | SQLite amalgamation | <https://www.sqlite.org/howtocompile.html> | compile `sqlite3.c` as an object before attempting shell linkage | run one in-memory query if shell/library linkage works | single huge translation unit, strong stress test |
| 07 | MuJS | <https://mujs.com/introduction.html> | compile core objects only | run a tiny expression if executable linkage works | small embeddable JS engine, more complex control flow |
| 08 | QuickJS | <https://bellard.org/quickjs/> | compile selected core objects only | run a tiny expression only after GNU/platform blockers are resolved | hard target with GNU/platform assumptions |
| 09 | GNU coreutils | <https://github.com/coreutils/coreutils> | host-bootstrap/configure first, then compile one small utility with `rcc` | run that utility against host output once linkage works | glibc/POSIX/GNU userland target with heavy gnulib and hosted-header assumptions |

## Start rule

Do not pre-write a detailed build plan for every project. When a project starts,
clone that project, inspect its current build files, then create
`real_world/projects/NN-name/plan.md` from the fetched tree.

## Stop rule

Stop and create a compiler task when a project exposes a real `rcc` defect.
Do not paper over compiler bugs by weakening the project probe.

## Result rule

Every completed project stage must leave enough information for another agent
to reproduce it:

- upstream commit or archive checksum
- host compiler command and observed output
- `rcc` command and observed output
- runtime command and output comparison, when applicable
- compiler task links for every discovered `rcc` defect
