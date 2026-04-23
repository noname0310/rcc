> ✓ done — 2026-04-23

# 03-04: Identifiers + universal character names

**Phase:** 03-lex    **Depends on:** 03-01    **Milestone:** M1

## Goal
Recognise C99 identifiers: `[_A-Za-z][_A-Za-z0-9]*` plus `\\uXXXX` and
`\\UXXXXXXXX` universal character names (C99 §6.4.2.1).

## Scope
- In: fast path over ASCII letters/digits/underscore; slow path for
  UCNs that lazily decodes the escape and merges into the identifier;
  emit E0005 for an ill-formed UCN (odd digit count, out of range).
- Out: keyword classification (phase 05 — here we emit plain `Ident`).

## Deliverables
- Identifier recognition producing a single `PpTokenKind::Ident` span
  covering the complete identifier body (including any UCNs).
- Table-driven unit test for: simple, underscore-leading,
  numeric-embedded, UCN `\\u00e9`, UCN `\\U0001F600` (valid), bad
  `\\u12` (error).

## Acceptance
- All c-testsuite sources produce Ident tokens where identifiers
  appear (snapshot test against a curated sample).
- E0005 diagnostic has a `help:` suggestion pointing to the exact
  bad escape bytes.

## References
- C99 §6.4.2.1 identifiers, §6.4.3 universal character names.
