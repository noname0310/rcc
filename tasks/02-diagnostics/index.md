# 02-diagnostics: index

Replace the placeholder `StderrEmitter` with a real ariadne-backed
renderer and establish the stable `E0XXX` registry every later
diagnostic will use.

## Upstream deps

- `01-test-infra` complete (CI and coverage tooling in place).

## Tasks (pick in order)

- [x] [01-ariadne-emitter](01-ariadne-emitter.md)
- [x] [02-error-codes-registry](02-error-codes-registry.md)
- [x] [03-multi-file-rendering](03-multi-file-rendering.md)
- [ ] [04-capture-emitter-tests](04-capture-emitter-tests.md)

## Downstream

`03-lex` and every subsequent phase rely on the error-code registry
and the `ariadne` emitter for user-facing diagnostics.
