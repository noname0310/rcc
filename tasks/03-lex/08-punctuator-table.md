# 03-08: Punctuator table (max-munch)

**Phase:** 03-lex    **Depends on:** 03-01    **Milestone:** M1

## Goal
Turn the full C99 §6.4.6 punctuator list into a `match`-based
maximal-munch matcher. All 3-char punctuators (`<<=`, `>>=`, `...`)
then 2-char (`->`, `++`, `==`, `<=`, `>=`, `!=`, `&&`, `||`, `<<`,
`>>`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `##`) then
single chars.

## Scope
- In: cover every `Punct` variant in `rcc_lexer::Punct`; any byte not
  matching any punctuator or previous rules becomes `Unknown` and
  reports E0009.
- Out: trigraph digraph (`%:`, `<:`) — *not* implemented per plan.

## Deliverables
- `punctuator()` fn with explicit match for each punctuator.
- Exhaustive unit test iterating every `Punct` variant.

## Acceptance
- Feeding the string obtained by concatenating every punctuator
  (separated by spaces) round-trips through the lexer losslessly.
- `...` is preferred over `..` + `.` (max-munch demonstrated by test).

## References
- C99 §6.4.6.
