# 10-10: Linker flag forwarding

**Phase:** 10-driver    **Depends on:** 10-02    **Milestone:** M5

## Goal
Forward common linker-related flags to the host `cc` link step:
`-l<lib>`, `-L<path>`, `-Wl,<flags>`, `-shared`, `-static`,
`-pie`/`-no-pie`.

## Scope
- In: CLI parsing for all listed flags. Collect library names,
  library search paths, and raw linker flags. Pass them through
  to `pipeline::link()` which invokes the host C compiler as
  linker. `-shared` → produce shared library. `-static` → static
  link. `-pie`/`-no-pie` → position-independent executable control.
- Out: LTO flags, linker script support.

## Deliverables
- CLI parsing for `-l`, `-L`, `-Wl,`, `-shared`, `-static`,
  `-pie`, `-no-pie`.
- `pipeline::link()` extended to forward these flags.
- Tests: `-lm` links the math library, `-L/path` adds search dir.

## Acceptance
- `rcc hello.c -lm -o hello` successfully links with libm.
- `rcc -shared -o lib.so lib.c` produces a shared library.
- `-Wl,--version-script=map.txt` is forwarded verbatim to the
  linker.

## References
- GCC link options documentation.
