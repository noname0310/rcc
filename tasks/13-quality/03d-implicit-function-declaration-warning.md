> ✓ done — 2026-05-05

# 13-03d: `-Wimplicit-function-declaration`

**Phase:** 13-quality    **Depends on:** 13-03    **Milestone:** M7

## Goal
Diagnose calls to undeclared functions with a named warning/error policy.
C99 removed implicit `int`, so default policy may remain an error; this task
defines the compatibility warning surface for users who need it.

## Scope
- In:
  - Identify the current undeclared-function diagnostic path.
  - Decide whether strict C99 keeps a hard error and GNU compatibility lowers
    it to `Wimplicit-function-declaration`.
  - Add warning-control tests for suppression/promotion.
- Out:
  - Inventing full K&R implicit-int semantics beyond the documented mode.

## Deliverables
- Policy docs and implementation.
- Tests for strict C99 and compatibility mode.

## Acceptance
- Strict C99 does not silently accept undeclared calls.
- Compatibility mode emits a warning named `implicit-function-declaration`.
- `-Werror=implicit-function-declaration` promotes the compatibility warning.
