# 15a-04: Static Assert Declarations

**Phase:** 15a-c11-transition  
**Depends on:** 15a-02-c11-keyword-tokenization  
**Milestone:** c11-transition

## Goal

Implement C11 `_Static_assert` declarations so headers can express compile-time
layout and feature assumptions without falling into parser recovery.

## Scope

- In: parse `static_assert-declaration` at file scope, block scope, and inside
  struct/union declaration lists.
- In: evaluate the integer constant expression with existing const-eval.
- In: emit a diagnostic containing the string literal message on failure.
- In: accept `static_assert` macro from `<assert.h>` only through header work,
  not as a parser special case.
- Out: C23 single-argument static assertions.

## Acceptance

- [ ] `_Static_assert(1, "ok");` is accepted at file scope and block scope.
- [ ] `_Static_assert(sizeof(int) == 4, "int size");` evaluates through the
      same constant-expression path used by array bounds and initializers.
- [ ] A false assertion fails before codegen with a stable diagnostic.
- [ ] Struct-scope static assertions do not create fields or affect layout.
- [ ] C99-mode behavior is explicit and tested.

## References

- N1570 6.7.10 static assertions.
