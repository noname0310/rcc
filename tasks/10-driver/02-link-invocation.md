# 10-02: Link invocation

**Phase:** 10-driver    **Depends on:** 09-06    **Milestone:** M3

## Goal
Drive the final link. Emit an object file via LLVM (`llc` or inkwell's
built-in emit) then call `cc` as the linker so libc / crt startup is
handled correctly.

## Scope
- In: locate `cc` from `PATH`; build a `Command` passing our `.o`
  plus `-o <output>`; error on non-zero exit.
- Out: custom linker scripts (out of scope).

## Deliverables
- `pipeline::link(obj: &Path, output: &Path) -> Result<(), String>`.
- Fixture: compile `int main(){return 42;}` → run → exit 42.

## Acceptance
- On Linux / macOS / Windows, `rcc hello.c -o hello` and `./hello`
  works.

## References
- rustc `link.rs` for design inspiration (much simpler here).
