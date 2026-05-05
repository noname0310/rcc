# 01 — inih

Status: stage 1 passed

Source: <https://github.com/benhoyt/inih>

Start by cloning into `upstream/`, then create `plan.md` from the fetched build
files. Do not edit upstream `.c` or `.h` files. Any adaptation must live in this
directory as wrapper scripts or build-script-only patches.

Initial target: compile the C parser and one minimal example/test object.

Current stage: `ini.c + tests/unittest.c` builds and links with `rcc`, runs, and
matches upstream `tests/baseline_multi.txt`.
