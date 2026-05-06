> ✓ done — 2026-05-06

# 15a-01: Language Standard Mode

**Phase:** 15a-c11-transition  
**Depends on:** 10-driver  
**Milestone:** c11-transition

## Goal

Introduce a first-class language standard setting so the compiler can
distinguish C99, C11, and future GNU dialect decisions without scattering
boolean feature flags across the frontend.

## Scope

- In: `rcc_session::Options` gets a structured language-standard enum.
- In: driver parsing for `-std=c99` and `-std=c11`.
- In: decide whether `-std=gnu11` is rejected initially or aliases to C11 plus
  already-explicit `-fgnu-*` flags only after a later task.
- In: `__STDC_VERSION__` changes from `199901L` to `201112L` only in C11 mode.
- In: tests proving `--linux-gnu-hosted` does not silently switch language
  standard.
- Out: implementing any individual C11 construct.

## Acceptance

- [ ] `rcc -std=c99 -E` reports `__STDC_VERSION__` as `199901L`.
- [ ] `rcc -std=c11 -E` reports `__STDC_VERSION__` as `201112L`.
- [ ] unsupported standard names still produce clear usage errors.
- [ ] Existing C99 tests pass unchanged.
- [ ] `docs/hosted-linux.md` says hosted mode and language standard are
      separate controls.

## References

- N1570 6.10.8 predefined macros.
- WG14 C11/N1570 status page.
