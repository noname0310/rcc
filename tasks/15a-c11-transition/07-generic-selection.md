# 15a-07: Generic Selection

**Phase:** 15a-c11-transition  
**Depends on:** 15a-05-alignof-alignas  
**Milestone:** c11-transition

## Goal

Implement `_Generic` expressions so C11 type-generic macros can be parsed,
type-checked, and lowered without falling back to ad hoc preprocessor hacks.

## Scope

- In: AST expression for `_Generic(assignment-expression, association-list)`.
- In: association parsing for `type-name : assignment-expression` and
  `default : assignment-expression`.
- In: type compatibility matching after lvalue/function/array conversions on
  the controlling expression.
- In: typeck chooses exactly one association and assigns the selected
  expression's type/value category.
- In: lowering/codegen emits only the selected expression.
- Out: Clang's extension that permits a controlling type rather than a
  controlling expression.

## Acceptance

- [ ] `_Generic(1, int: 10, default: 20)` folds/selects the `int` arm.
- [ ] Duplicate compatible association types are diagnosed.
- [ ] Missing match without `default` is diagnosed.
- [ ] Non-selected arms are parsed but not evaluated for runtime codegen.
- [ ] `<tgmath.h>` remains compatible with or improves from this support.

## References

- N1570 6.5.1.1 generic selection.
- Clang C11 generic selection notes.
