> ✓ done — 2026-05-04

# 11-04: c-testsuite residual bug triage

**Phase:** 11-conformance    **Depends on:** 11-03    **Milestone:** M6+

## Goal
Stop treating c-testsuite success as only a percentage. Every non-pass TU in
the latest report must be classified as one of:

- ISO C compiler bug: must be fixed or scheduled as a blocking task.
- Outside-release extension: may stay xfail with a precise reason.
- Test-policy case: may skip/xfail only with a documented semantic reason.

## Current non-pass TUs
Source: `docs/conformance.json` generated after task `11-03`
(`204 pass / 11 fail / 5 xfail`).

### Failures to triage as compiler bugs unless proven otherwise

| TU | Current symptom |
|---|---|
| `c-testsuite::00044` | `E0070` redeclaration of `T` in the same scope |
| `c-testsuite::00053` | `E0070` redeclaration of `T` in the same scope |
| `c-testsuite::00124` | `E0071` undeclared identifier `a` |
| `c-testsuite::00149` | `E0084` non-constant expression in static initializer |
| `c-testsuite::00150` | `E0084` non-constant expression in static initializer |
| `c-testsuite::00199` | compiler panic in CFG build: goto into local scope |
| `c-testsuite::00204` | compiler panic in CFG lowering: non-lvalue place |
| `c-testsuite::00205` | `E0081` initializer/type assignment failure |
| `c-testsuite::00207` | compiler panic in CFG build: goto into local scope |
| `c-testsuite::00213` | stdout mismatch |
| `c-testsuite::00218` | stdout mismatch |

### Existing xfails that must be reviewed

| TU | Current xfail reason | Initial policy |
|---|---|---|
| `c-testsuite::00050` | anonymous union member inside struct | outside-release extension; parser support exists, alias/layout semantics remain |
| `c-testsuite::00152` | macro-expanded `#line` directive | likely ISO C compiler bug; should not stay hidden |
| `c-testsuite::00216` | empty aggregate extension / anonymous aggregate members | mixed extension; split reason if needed |
| `c-testsuite::00219` | C11 `_Generic` | historical xfail; now owned by C11 gates and pending full-pipeline release classification |

## Scope
- In:
  - Re-run c-testsuite locally and confirm the current non-pass list.
  - Refresh `docs/conformance.md` if it is stale relative to
    `docs/conformance.json`.
  - For each non-pass TU, decide whether it is an ISO C bug, extension,
    or documented policy case.
  - Create focused follow-up tasks in the owning phase for every ISO C compiler
    bug.
  - Remove or rewrite any xfail entry that hides an ISO C compiler bug.
- Out:
  - Fixing every listed bug in this task. This is a classification and tasking
    task; fixes happen in the owner tasks it creates.

## Deliverables
- Updated failure matrix with owner phase/crate and next task for each TU.
- Follow-up task files for every ISO C compiler bug.
- Clean xfail policy: no ISO C compiler bug remains xfailed without an owning
  fix task.

## Classification result

| TU | Classification | Owner task |
|---|---|---|
| `00044` | ISO C compiler bug: tag namespace is not block-scoped. | `06-28` |
| `00053` | ISO C compiler bug: inner block tag shadowing cascades into field lookup errors. | `06-28` |
| `00124` | ISO C compiler bug: function-definition parameters are collected from the wrong declarator level for function-pointer returns. | `06-29` |
| `00149` | ISO C compiler bug: file-scope compound literal static storage/address constant missing. | `06-30` |
| `00150` | ISO C compiler bug: same file-scope compound-literal issue, plus nested designated global initializer coverage. | `06-30` |
| `00152` | ISO C compiler bug hidden as xfail: `#line` operands must be macro-expanded. | `04-21` |
| `00199` | ISO C compiler bug: CFG panics on valid goto into ordinary block scope. | `08-26` |
| `00204` | Fixed ISO C compiler bug: aggregate-rvalue member access, SysV direct aggregate ABI, aggregate `va_arg`, and hex integer constant signedness are all green as of `07-22`. | `07-21`, `08-27`, `09-28`, `09-29`, `07-22` |
| `00205` | ISO C compiler bug: brace elision for nested aggregate initializers missing. | `06-31` |
| `00207` | ISO C compiler bug: CFG panic on goto into ordinary block scope; VLA legality remains guarded by the same task. | `08-26` |
| `00213` | GNU extension semantics bug: parsed statement expressions are not semantically lowered, causing missing output. | `11-07` |
| `00218` | Implementation-defined bitfield policy gap: enum bitfield should zero-extend for this suite. | `09-27` |

Existing xfails retained as outside-release/future-scope unless a later task elects to
support the extension: `00050`, `00216`, `00219`. `00152` should be
removed from xfail by `04-21`.

## Acceptance
- The latest c-testsuite report has no unclassified fail/xfail entries.
- Compiler panics are always classified as compiler bugs.
- `docs/conformance.md` and `docs/conformance.json` do not disagree about the
  current pass/fail counts.
- The next active task after this one is unambiguous.

## References
- `docs/conformance.json`
- `third_party/testsuites/c-testsuite/xfail.toml`
- `tasks/00-overview/03-working-agreement.md`
