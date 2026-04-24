# rcc Error Codes

Every user-facing diagnostic emitted by `rcc` carries a stable error code.
Errors use `EXXXX`; non-fatal warnings use `WXXXX`. This page is the
canonical reference. If a code appears in compiler output but is missing
here, that is a bug — CI will catch it via
`cargo xtask check-error-codes`.

---

## E0001 — unexpected character

The lexer encountered a byte that cannot begin any C99 token.

```c
int x = @;  // error[E0001]: unexpected character '@'
```

## E0002 — unterminated string literal

A `"` was opened but never closed before end of line or file.

```c
char *s = "hello;  // error[E0002]: unterminated string literal
```

## E0003 — nested block comment

A `/*` appeared inside another `/* ... */` block comment. C99 block
comments do not nest (§6.4.9); the outer comment silently closes at the
first `*/`, which is almost always a mistake.

```c
/* outer
   /* inner */          // error[E0003]: nested block comment
   still inside? */
```

## E0004 — unterminated block comment

A `/*` was opened but the matching `*/` was never found.

```c
/* this comment never ends
int x = 1;  // error[E0004]: unterminated block comment
```

## E0005 — invalid escape sequence

A backslash sequence in a string or character literal is not recognised
by C99.

```c
char *s = "\q";  // error[E0005]: invalid escape sequence '\q'
```

## E0006 — unterminated character constant

A `'` was opened but never closed before end of line or end of file.
C99 §6.4.4.4 forbids a literal newline inside a character constant.

```c
char c = 'a  // error[E0006]: unterminated character constant
```

## E0007 — invalid escape sequence

A backslash inside a character or string literal is followed by a
character that is not a recognised C99 escape. Valid simple escapes are
`\' \" \? \\ \a \b \f \n \r \t \v`; also accepted are octal `\NNN`,
hex `\xHH+`, and universal character names `\uXXXX` / `\UXXXXXXXX`.

Note: multi-character character constants (e.g. `'ab'`) are
implementation-defined per C99 §6.4.4.4p10 and are NOT diagnosed by the
lexer.

```c
char c = '\q';  // error[E0007]: invalid escape sequence '\q'
```

## E0008 — unterminated string literal

A `"` was opened but never closed before end of line or end of file.
C99 §6.4.5 forbids a literal newline inside a string literal; use line
splicing (`\<newline>`) or string concatenation at the source level.

```c
char *s = "hello  // error[E0008]: unterminated string literal
```

## E0009 — integer literal overflow

The value of an integer literal exceeds the range of the widest
representable type (`unsigned long long`).

```c
int x = 99999999999999999999;  // error[E0009]: integer literal overflow
```

## E0010 — unterminated header name

A `#include` header name (`<...>` or `"..."`) was opened but its
matching closing delimiter was not found before the end of the logical
line or end of file. Per C99 §6.4.7 a header-name token must be closed
on the same logical line.

The lexer only emits this after the preprocessor has entered header-name
context (i.e. directly after `#include`); elsewhere `<` and `"` are
ordinary punctuator / string-literal starts.

```c
#include <stdio.h   // error[E0010]: unterminated header name
#include "missing   // error[E0010]: unterminated header name
```

## E0011 — invalid octal digit

An octal literal (leading `0`) contains the digit `8` or `9`.

```c
int x = 089;  // error[E0011]: invalid octal digit '9'
```

## E0012 — invalid hex escape

A `\x` escape in a string or character literal is not followed by any
hexadecimal digit.

```c
char *s = "\xZZ";  // error[E0012]: invalid hex escape
```

## E0013 — malformed #include directive

An `#include` directive does not have a valid `"..."` or `<...>` path.

```c
#include foo.h  // error[E0013]: malformed #include directive
```

## E0014 — invalid #define directive

The token following `#define` is not a valid identifier.

```c
#define 123 x  // error[E0014]: invalid #define directive
```

## E0015 — expected identifier after #ifdef/#ifndef

The conditional-compilation directive requires an identifier operand.

```c
#ifdef    // error[E0015]: expected identifier after #ifdef/#ifndef
#endif
```

## E0016 — unmatched #endif

An `#endif` was found without a corresponding `#if` / `#ifdef` /
`#ifndef`.

```c
#endif  // error[E0016]: unmatched #endif
```

## E0017 — unmatched #else/#elif

An `#else` or `#elif` appears in a position the C99 §6.10.1 state
machine does not allow. Three distinct constraint violations share
this code:

- **Bare `#else` / `#elif`** — no conditional group is currently
  open, so there is nothing to branch off of.
- **`#elif` after `#else`** — once the `#else` group has opened, no
  further branches may be introduced; only `#endif` may close the
  group.
- **Duplicate `#else`** — a conditional group may contain at most
  one `#else`.

```c
#else                         // error[E0017]: unmatched `#else`
#endif

#if 0
#else
#elif 1                       // error[E0017]: `#elif` after `#else`
#endif

#if 0
#else
#else                         // error[E0017]: duplicate `#else`
#endif
```

The diagnostic always labels the offending directive's keyword so
the user can locate the transgressor at a glance. The conditional
stack remains internally consistent after the report — a matching
`#endif` later in the file still pops the group — so downstream
parsing is not destabilised by a single misplaced branch.

## E0018 — missing #endif at end of file

An `#if` / `#ifdef` / `#ifndef` was opened but never closed. One
diagnostic is emitted per still-open frame, each labelled at its
own originating keyword so nested groups are easy to match up.

```c
#ifdef FOO
int x = 1;
// error[E0018]: missing `#endif` at end of file
```

## E0019 — unknown preprocessor directive

A `#` is followed by a token that is not a recognised C99 directive.

```c
#foobar  // error[E0019]: unknown preprocessor directive
```

## E0020 — #error directive encountered

The user explicitly triggered a compilation error via `#error` (C99
§6.10.5). The body tokens are surfaced verbatim in the diagnostic
message so the user's reason appears in compiler output. `#error` is
**fatal**: after emitting E0020 the preprocessor halts for the rest
of the translation unit — no further tokens reach the parser, no
later directive side effects apply, and subsequent would-be
diagnostics (malformed `#define`, missing `#endif`, unknown
`#pragma`) are suppressed.

```c
#error unsupported platform  // error[E0020]: #error: unsupported platform
```

Dead-branch `#error` directives are exempt: §6.10p5 says skipped
groups execute nothing, so a `#error` inside a `#if 0 ... #endif`
block is silently dropped, not raised.

## E0021 — cannot find header

A `#include` directive names a header that was not found in any of the
configured search directories. Per C99 §6.10.2, the `"..."` form
searches the current file's directory first and then the command-line
include paths (`-I`); the `<...>` form searches only the include paths.

```c
#include <missing.h>  // error[E0021]: cannot find header `missing.h`
```

## E0022 — macro redefined with a different body

A macro name appears in a second `#define` directive whose replacement
list differs from the first. C99 §6.10.3p1 permits *benign*
redefinition — a repeated `#define` with a replacement list that
matches the original in token count, ordering, spelling, and
whitespace separation is silently accepted — but any substantive
difference is an error. Use `#undef` before redefining.

```c
#define FOO 42
#define FOO 43  // error[E0022]: macro `FOO` redefined with a different body
```

## E0023 — duplicate macro parameter name

A function-like `#define` lists the same identifier more than once in
its parameter list. C99 §6.10.3p6 requires each parameter name to be
distinct so that parameter references inside the replacement list are
unambiguous.

```c
#define FOO(a, a) a  // error[E0023]: duplicate macro parameter name `a`
```

## E0024 — `#` is not followed by a macro parameter

Inside a function-like macro replacement list, the stringize operator
`#` must be immediately followed by the name of one of the macro's
parameters. C99 §6.10.3.2p1 makes any other form a constraint
violation.

```c
#define BAD(x) #y    // error[E0024]: `#` is not followed by a macro parameter
#define ALSO(x) #    // error[E0024]: `#` is not followed by a macro parameter
```

The operator only applies in function-like macros; a `#` inside an
object-like macro body is preserved as an ordinary punctuator.

## E0025 — pasting forms an invalid token

The token-paste operator `##` concatenates the spellings of its left
and right operands and the result must be a single preprocessing
token (C99 §6.10.3.3). When the combined text re-lexes to more than
one pp-token — e.g. pasting two unrelated punctuators — the paste is
ill-formed.

```c
#define BAD(a, b) a##b
BAD(+, ;)  // error[E0025]: pasting forms an invalid token
```

The same code is emitted for the §6.10.3.3p1 positional constraint
violation: `##` shall not appear at the very beginning or the very
end of a replacement list for either macro form.

```c
#define LEAD ## x   // error[E0025]: `##` at the beginning of a replacement list
#define TAIL x ##   // error[E0025]: `##` at the end of a replacement list
```

## E0026 — `__VA_ARGS__` outside a variadic macro

The identifier `__VA_ARGS__` is reserved by C99 §6.10.3p5 for use
inside the replacement list of a *variadic* function-like macro —
one whose parameter list ends with `...`. Using the name anywhere
else (in an object-like macro, in a non-variadic function-like
macro, or as an ordinary identifier in regular source) is a
constraint violation.

```c
#define F(x) __VA_ARGS__    // error[E0026]: `__VA_ARGS__` outside a variadic macro

#define OBJ __VA_ARGS__     // error[E0026]: `__VA_ARGS__` outside a variadic macro
```

Variadic macros opt in by ending the parameter list with `...`; the
body may then refer to `__VA_ARGS__` as a stand-in for the trailing
comma-separated arguments:

```c
#define LOG(fmt, ...) printf(fmt, __VA_ARGS__)    // OK
```

When zero trailing arguments are supplied (`LOG("a")`) `__VA_ARGS__`
expands to an empty token sequence by default, leaving the preceding
comma in place (`printf("a", )`). The GNU extension `, ##
__VA_ARGS__` — which drops the preceding comma when the variadic
argument list is empty — is gated behind the
`Options::gnu_va_args_elision` flag and is off by default.

## E0027 — cannot redefine or undefine a predefined macro

C99 §6.10.8p2 makes the predefined macros (`__DATE__`, `__FILE__`,
`__LINE__`, `__STDC__`, `__STDC_HOSTED__`, `__STDC_VERSION__`,
`__TIME__`) off-limits to user `#define` and `#undef` — each such
attempt is a constraint violation. `rcc` seeds its macro table with
these entries at the start of every translation unit (see
`Preprocessor::install_predefined`); the table marks them so the
`#define` / `#undef` paths can diagnose any tampering.

```c
#define __LINE__ 42   // error[E0027]: cannot redefine predefined macro `__LINE__`
#undef __STDC__       // error[E0027]: cannot `#undef` predefined macro `__STDC__`
```

Command-line `-D NAME[=VALUE]` flags install ordinary object-like
macros *before* the predefined set is seeded, so the predefined
entries always win on name collisions (`-D __STDC__=0` silently
loses to the built-in `__STDC__ = 1`). The identifier `__func__` is
**not** a predefined macro — per C99 §6.4.2.2 it is a predeclared
identifier materialised by the parser inside every function
definition — and is therefore not covered by E0027.

## E0028 — invalid `#if` expression

C99 §6.10.1 gives the preprocessor its own integer-only constant
expression language, independent of C's general expression parser:
no floats, no casts, no pointers, no `sizeof`, no enumeration
constants. The `#if` / `#elif` controlling expression must parse
cleanly in that restricted grammar and must not exhibit any
constraint violation that only the preprocessor can catch. E0028 is
the umbrella code for every such failure.

The concrete cases that raise it:

- **Division or remainder by zero in a live branch.** `#if 1/0` or
  `#if 1 % 0` is undefined per §6.5.5p5; the evaluator refuses to
  produce a value and emits E0028 pointing at the operator.
  Short-circuited dead sides are exempt — `#if 1 ? 42 : 1/0`
  evaluates cleanly because §6.5.15p4 says the unused operand is
  not evaluated.
- **Malformed `defined` operator.** The only legal shapes are
  `defined IDENT` and `defined ( IDENT )` — anything else (e.g.
  `defined 42`, `defined (`, `defined(A, B)`) is ill-formed per
  §6.10.1p1.
- **Unexpected tokens in the expression.** Leftover punctuators,
  unbalanced parens, missing operands, and trailing garbage after
  the expression all fall under this code.
- **Floating-point or other forbidden literals.** A pp-number
  containing `.`, `e` / `E` (decimal exponent), or `p` / `P`
  (hex-float exponent) is a float by pp-tokenisation and therefore
  §6.10.1p4-forbidden in `#if`; the same goes for malformed integer
  suffixes.

```c
#if 1/0                  // error[E0028]: division by zero in #if expression
#if defined 123          // error[E0028]: malformed `defined` operator
#if 1.0 + 2              // error[E0028]: floating-point literal `1.0` not allowed
#if (1 + 2               // error[E0028]: expected `)` in #if expression
```

Identifiers that survive macro expansion are **not** an error —
§6.10.1p4 silently replaces them with the pp-number `0`, so
`#if NO_SUCH_MACRO == 0` is always true rather than ill-formed.
The `defined` operator sees raw spellings before expansion, which
is how `#if defined FOO` can distinguish an undefined `FOO` from
one `#define FOO 0`'d to zero.

## E0029 — `#line` argument out of range

C99 §6.10.4p3: the digit sequence of a `#line` directive "shall not
specify zero, nor a number greater than 2147483647". Both bounds
are constraint violations.

```c
#line 0                        // error[E0029]: `#line` argument out of range
#line 2147483648               // error[E0029]: `#line` argument out of range
```

A missing or non-numeric argument (`#line`, `#line abc`) is a
different error — see E0015.

## E0030 — unexpected token

The parser encountered a token that does not belong to any valid
statement, declaration, or expression at the current position.
Recovery skips forward to the next `;` or `}` so that subsequent
constructs can still be diagnosed independently.

```c
int main(void) {
    ) ;          // error[E0030]: unexpected token
    int x = 1;  // still parsed normally after recovery
}
```

## E0040 — integer literal too large

The magnitude of an integer constant exceeds the range of `u128`, the
widest unsigned type `rcc` uses to hold a decoded literal before the
typeck pass selects a concrete C type per C99 §6.4.4.1p5. A literal
this large has no representation at any standard C integer type, so
the parser rejects it at decode time.

```c
unsigned long long x =
    0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF;  // error[E0040]: integer literal too large
```

Contrast with lexer code E0009, which covers the narrower case of a
literal that fits `u128` but still exceeds the language-level widest
type — that check is performed later, when typeck walks the §6.4.4.1p5
ladder.

## E0041 — incompatible string literal encodings

Two adjacent string-literal tokens have encoding prefixes that C99
§6.4.5p5 does not permit to be concatenated. The standard spells out
exactly one promotion — a narrow (unprefixed) literal and an
`L`-prefixed wide literal concatenate, in either order, into a wide
literal — and says every other cross-prefix mix is undefined
behavior. `rcc` rejects the undefined cases at parse time.

```c
wchar_t *ok = L"a" "b";       // OK: narrow + wide → wide
wchar_t *bad = L"a" U"b";     // error[E0041]: incompatible string literal encodings
char32_t *also = "a" U"b";    // error[E0041]: incompatible string literal encodings
```

The primary label points at the offending (later) literal; the first
literal of the current run gets a secondary "previous string literal
here" label so the user can see exactly where the encoding mismatch
began. After emitting the diagnostic the parser splits the run at the
conflict and continues — any subsequent well-formed concatenation
still merges normally.

## E0060 — conflicting storage-class specifiers

C99 §6.7.1p2: "At most, one storage-class specifier may be given in
the declaration specifiers in a declaration." Any second storage-class
keyword — whether a real conflict or a self-duplicate — is reported at
the offending keyword and the first-chosen specifier is kept.

```c
static extern int x;    // error[E0060]: cannot combine `extern` with previous storage-class specifier
static static int y;    // error[E0060]: duplicate `static` storage-class specifier
```

The parser still continues through the specifier list after emitting
the diagnostic so subsequent tokens (qualifiers, the type specifier,
the declarator) get a chance to parse and report their own problems
independently.

## E0061 — invalid combination of type specifiers

C99 §6.7.2p2 enumerates the legal multisets of type-specifier keywords
in a single declaration — e.g. `unsigned long long int`, `long
double`, `_Complex float`, `signed char`. Anything outside that table
is a constraint violation.

```c
short long x;           // error[E0061]: cannot combine `long` with `short`
long long long y;       // error[E0061]: `long long long` is not a valid type specifier
int int z;              // error[E0061]: cannot combine `int` with previous type specifier
signed unsigned w;      // error[E0061]: cannot combine `unsigned` with opposite sign specifier
float int v;            // error[E0061]: cannot combine `int` with previous type specifier
```

A `struct`/`union`/`enum` specifier with neither a tag nor a `{…}`
body is also E0061 — §6.7.2.1 / §6.7.2.2 both require at least one of
the two.

```c
struct ;                // error[E0061]: `struct` specifier needs a tag or a `{` body
```

The parser reports the first token that breaks the combination and
keeps going so the rest of the declaration still gets parsed (which
usually surfaces more useful follow-up diagnostics than bailing at
the first error).

## E0062 — abstract declarator cannot contain a name

C99 §6.7.6 defines a `type-name` as a specifier-qualifier-list
optionally followed by an *abstract* declarator — the kind that has
no identifier atom. Type names appear in casts `(T)e`, `sizeof(T)`,
compound literals `(T){...}`, and (with the same shape) in parameter
type lists. A name written in that slot is a constraint violation:

```c
int *x = (int *p)0;    // error[E0062]: abstract declarator cannot contain a name
                       //                              ^^^^
```

The parser recovers by discarding the name and keeping the rest of
the declarator (pointer / array / function chain) so later passes
can still report any further mistakes in the surrounding expression.

## E0070 — conflicting redeclaration

A redeclaration of an identifier with conflicting linkage or type.
C99 §6.2.2p7: if within a translation unit the same identifier
appears with both internal and external linkage the behaviour is
undefined. `rcc` rejects this at lowering time.

```c
static int x;
extern int x;   // error[E0070]: conflicting redeclaration of `x`
```

## E0071 — undeclared identifier

An identifier is used that has not been declared in any visible scope.
C99 §6.5.1p2: an identifier shall designate an entity visible in the
current scope. A `help:` line suggests similarly-named symbols if any
exist within edit-distance 3.

```c
int main(void) {
    return coutn;  // error[E0071]: use of undeclared identifier `coutn`
                   //   help: did you mean `count`?
}
```

## E0072 — tag kind mismatch

A struct, union, or enum tag was previously declared with a different
kind. C99 §6.7.2.3 requires that every use of a particular tag agrees
on whether it names a struct, union, or enum.

```c
struct S { int x; };
union S;             // error[E0072]: use of `S` as `union` but previously declared as `struct`
```

## E0073 — undeclared label

A `goto` statement references a label that does not exist anywhere in
the enclosing function. C99 §6.8.6.1p1 requires that the identifier in
a `goto` name a label located somewhere in the same function body.
Forward references are allowed — the label may appear after the `goto`.

```c
void f(void) {
    goto missing;  // error[E0073]: use of undeclared label `missing`
}
```

## E0074 — duplicate label

Two labels with the same name appear in the same function body. C99
§6.8.1p3 requires that label names be unique within a function.

```c
void f(void) {
    a: ;
    a: ;  // error[E0074]: duplicate label `a`
}
```

## E0075 — typedef cycle detected

A typedef directly or indirectly refers to itself through a chain of
other typedefs. C99 §6.7.7 requires typedef names to denote a complete,
acyclic type. `rcc` detects cycles during expansion and reports this
error rather than looping forever.

```c
typedef T T;              // error[E0075]: typedef cycle detected for `T`
typedef U V; typedef V U; // error[E0075]: typedef cycle detected for `U`
```

---

## W0001 — unknown #pragma directive

C99 §6.10.6 lets an implementation ignore any `#pragma` it does not
understand. `rcc` recognises two:

- `#pragma once` — include-once header hint (handled at `#include`
  time by a raw pre-pass; accepted silently by the directive
  dispatcher too).
- `#pragma STDC ...` — the standard reserved family
  (`FP_CONTRACT`, `FENV_ACCESS`, `CX_LIMITED_RANGE`). Every `STDC`
  form is accepted silently; `rcc` does not currently act on any
  of them, but §6.10.6p2 explicitly allows that.

Any other pragma — including a bare `#pragma` with a single unknown
identifier — emits W0001 and is ignored; compilation continues. A
totally empty `#pragma` (no body tokens) is silently dropped.

```c
#pragma mystery          // warning[W0001]: unknown pragma `mystery`
#pragma GCC diagnostic   // warning[W0001]: unknown pragma `GCC`

#pragma once             // accepted silently
#pragma STDC FP_CONTRACT ON  // accepted silently
```

W0001 does **not** count as an error for `Handler::has_errors`, so a
translation unit with only unknown-pragma warnings still compiles
cleanly.

## W0004 — duplicate type qualifier or function specifier

C99 §6.7.3p4 explicitly permits repeating the same type qualifier in
a declaration ("If the same qualifier appears more than once in the
same specifier-qualifier-list … the behavior is the same as if it
appeared only once"), and §6.7.4p5 says the same thing about
`inline`. Repetition is therefore well-formed — the declaration
compiles — but it is almost always a copy-paste mistake.

```c
const const int x;       // warning[W0004]: duplicate `const` type qualifier
inline inline void f();  // warning[W0004]: duplicate `inline` function specifier
```

Like every warning, W0004 does not count toward `Handler::has_errors`.

---

## E0063 — K&R declaration names unknown parameter

In an old-style (K&R) function definition the declaration-list between
the closing `)` and the opening `{` may only declare names that appear
in the identifier-list of the function declarator. Naming a parameter
that was never listed is a constraint violation (C99 §6.9.1p6):

```c
int f(x, y)
    int x;
    int z;   // error[E0063]: K&R declaration names unknown parameter `z`
{ return x; }
```

---

## W0005 — K&R function definition is obsolete

C99 §6.11.7 marks the use of function definitions with an
identifier-list (K&R style) as an obsolescent feature. rcc emits a
warning whenever it encounters one:

```c
int f(x, y) int x; double y; { return x; }
//          ^^^^^^^^^^^^^^^ warning[W0005]: K&R function definition is obsolete
//                          help: rewrite using prototype syntax
```

Like every warning, W0005 does not count toward `Handler::has_errors`.

## W0006 — macro redefined with a different body (permissive)

When `gnu_permissive_redefinition` is enabled, a non-identical `#define`
is accepted with a warning instead of the strict C99 E0022 error.
The new definition silently replaces the old one, matching GCC / Clang
behaviour.

```c
#define X 1
#define X 2   // warning[W0006]: macro redefined with a different body (permissive)
```

Like every warning, W0006 does not count toward `Handler::has_errors`.
