> ✓ done — 2026-04-23

# 01-02: Vendor chibicc tests

**Phase:** 01-test-infra    **Depends on:** 01-01    **Milestone:** M0.5

## Goal
Sparse-checkout only `chibicc/test/` (and `LICENSE`) rather than the
whole repo. The test files are numbered by feature (`arith.c`,
`control.c`, `pointer.c`, `struct.c`, ...) and drive our M2..M5
milestones.

## Scope
- In: sparse checkout pattern in `MANIFEST.toml`; verify `test/` shows
  up; copy `LICENSE` → `LICENSES/chibicc.txt`.
- Out: the adapter itself (task 09).

## Deliverables
- Updated sparse list in `MANIFEST.toml` for `chibicc`.
- `xtask/src/fetch.rs` validated for sparse-checkout path.

## Acceptance
- `cargo xtask fetch-testsuites --only chibicc` leaves only `test/`
  and `LICENSE` populated (no `chibicc.c`, `main.c`, etc.).
- `ls third_party/testsuites/chibicc/test/arith.c` exists.
- `LICENSES/chibicc.txt` is non-empty.

## References
- Plan §9.1 "chibicc 테스트".
- Upstream: https://github.com/rui314/chibicc
