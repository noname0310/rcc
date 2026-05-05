# 02 — cJSON

Status: stage 1 passed

Source: <https://github.com/DaveGamble/cJSON>

Start by cloning into `upstream/`, then create `plan.md` from the fetched build
files. Do not edit upstream `.c` or `.h` files. Any adaptation must live in this
directory as wrapper scripts or build-script-only patches.

Initial target: compile `cJSON.c` and run a tiny JSON round-trip probe.

Current stage: `cJSON.c + generated roundtrip.c` builds and links with `rcc`,
runs, and matches the host compiler baseline output.
