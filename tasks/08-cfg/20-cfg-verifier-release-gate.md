# 08-20: CFG verifier release gate

**Phase:** 08-cfg    **Depends on:** 08-19    **Milestone:** M3 stabilization

## Goal
Move critical CFG invariants out of debug-only assertions and into a
callable verifier that tests, driver debug modes, and future codegen can
run in every build profile.

## Scope
- In: verify every reachable block has an intentional terminator.
- In: verify branch targets are valid block ids.
- In: verify every `Place` base local exists and projections are
  structurally valid.
- In: verify `StorageLive`/`StorageDead` balance for straightforward
  lexical paths, with documented limitations for path-sensitive cases.
- In: return structured verification errors rather than panicking.
- Out: full dataflow borrow/lifetime analysis.

## Deliverables
- `rcc_cfg::verify::verify_body(&Body, &TyCtxt) -> Result<(), Vec<CfgError>>`.
- Driver/test hook that runs the verifier after CFG lowering in debug
  and test configurations.
- Unit tests that intentionally build malformed CFG bodies and assert
  verifier diagnostics.
- Replace or supplement `BodyBuilder::finish` debug-only checks with
  verifier coverage.

## Acceptance
- A reachable block with default `Unreachable` due to missing lowering
  is caught by a normal `cargo test`, not only by debug assertions.
- Invalid block/local references are reported as `CfgError`.
- `cargo test -p rcc_cfg verify` passes.

## References
- `crates/rcc_cfg/src/build.rs` `BodyBuilder::finish`.
- `crates/rcc_cfg/tests/cfg.rs` invariant helpers.
