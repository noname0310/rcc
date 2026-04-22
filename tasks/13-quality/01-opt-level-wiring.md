# 13-01: Optimisation level wiring

**Phase:** 13-quality    **Depends on:** 09-14    **Milestone:** M7

## Goal
Map `OptLevel::{None, Less, Default, Aggressive}` → LLVM pass
manager levels O0/O1/O2/O3 using `inkwell::passes::PassManagerBuilder`.
Also plumb through `-flto=thin` via `lto = "thin"` in Cargo
profile.

## Scope
- In: `PassManager` construction in driver before codegen emits
  object; verify via `-emit=asm` snapshots across `-O` levels.
- Out: PGO (profile-guided optimisation; future work).

## Deliverables
- Driver opt-level wiring.
- ASM snapshot diff: `-O0` vs `-O2` for a simple loop shows
  vectorisation.

## Acceptance
- `rcc -O2 bench.c -o bench.o` runtime within 2× of host `cc -O2`.

## References
- LLVM `PassManagerBuilder`.
