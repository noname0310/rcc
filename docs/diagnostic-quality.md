# Diagnostic Quality Checklist

This checklist is the release gate for user-facing diagnostics. It complements
the mechanical registry check in `cargo xtask check-error-codes`.

## Mechanical Registry

- Every diagnostic code declared in `crates/rcc_errors/src/codes.rs` must have
  a `## EXXXX` or `## WXXXX` entry in `docs/error-codes.md`.
- Every documented code must exist in the registry.
- Every quoted diagnostic code in Rust source must exist in the registry.
- The check covers both hard errors (`E`) and warnings (`W`).
- Warning names, group membership, and detector contracts live in
  `docs/warnings.md`.

Command:

```bash
cargo xtask check-error-codes
```

## Message Rubric

Every emitted diagnostic should satisfy the applicable rows below:

| Item | Rule |
|------|------|
| Primary span | Points at the offending token or directive, not the whole file. |
| Secondary span | Used when prior context matters, such as previous definitions. |
| Help | Present when the user can take a concrete corrective action. |
| Note | Used for standard references, implementation policy, or phase ownership. |
| Warning control | Warnings mention the relevant `-W...` or `-f...` control when one exists. |
| Recovery | Parser/HIR/typeck recovery diagnostics should avoid cascades from one root cause. |
| Internal invariant | Explains which upstream phase should have rejected the invalid state. |

## Current Coverage Ownership

| Code range | Primary owner | Regression surface |
|------------|---------------|--------------------|
| `E0001`-`E0012`, `W0002`, `W0003` | lexer / phase 7 literal decoding | lexer unit tests and parser literal tests |
| `E0013`-`E0029`, `W0001`, `W0006` | preprocessor | preprocessor unit tests and chibicc preprocess fixtures |
| `E0030`-`E0032`, `E0060`-`E0063`, `W0004`, `W0005`, `W0013`-`W0025` | parser | parser unit tests and driver UI parse snapshots |
| `E0070`-`E0079`, `W0007`, `W0022`, `W0023`, `W0029` | HIR lowering | `rcc_hir_lower` / driver warning tests |
| `E0080`-`E0084`, `E0087`, `E0088`, `W0008`-`W0012`, `W0026`-`W0028`, `W0030` | type checker / const evaluator | `rcc_typeck` and driver warning tests |
| `E0085`, `E0086` | CFG/layout boundary | CFG and driver tests |

## Release Policy

- A new diagnostic code must land with registry entry, docs entry, and at
  least one regression test.
- A diagnostic can be listed as "not yet emitted" only if the registry comment
  says which future task owns it.
- UI snapshots are required for diagnostics whose rendered span quality is the
  point of the feature; pure semantic table rows can stay as unit tests.
