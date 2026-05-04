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

## E0031 — malformed attribute syntax

GNU `__attribute__((...))` syntax requires the double-parenthesized
wrapper and a comma-separated list of attribute names:

```c
int x __attribute__((aligned(16));  // error[E0031]: missing `)`
```

The parser reports malformed wrappers locally, then recovers so the
surrounding declaration can still be checked.

## E0032 — malformed inline assembly syntax

GNU inline assembly syntax was recognized but its template, operands,
constraints, clobbers, parentheses, or terminating semicolon were
malformed:

```c
asm("nop" : "r"x);  // error[E0032]: missing operand expression parentheses
```

The parser reports malformed inline asm locally, then recovers at the
statement boundary so the enclosing block can continue parsing.

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

## E0076 — illegal declarator form

C99 §6.7.5 constrains the shapes of declarators in several ways. `rcc`
rejects these illegal forms at lowering time:

- **`void x;` for an object.** Only pointers-to-void (`void *p`) and
  functions returning void (`void f(void)`) are legal; declaring a
  variable of type `void` is a constraint violation (§6.2.5p19).
- **Function returning an array** (`int f()[10]`). §6.7.5.3p1
  requires that a function's return type shall not be an array type.
- **Function returning a function** (`int f()(int)`). §6.7.5.3p1
  also forbids functions returning function types — use a pointer.

```c
void x;              // error[E0076]: cannot declare variable of type `void`
int f()[10];         // error[E0076]: function cannot return array type
int g()(int);        // error[E0076]: function cannot return function type
```

---

## E0077 — invalid bit-field width

C99 §6.7.2.1p4 constrains bit-field widths inside a `struct` or
`union`: the width shall be a non-negative integer constant expression
whose value does not exceed the width of the underlying integer
type. A width of zero is legal only for an **anonymous** bit-field
(used as an alignment separator that forces the next field onto a new
storage unit); a named bit-field must have a positive width.

`rcc` rejects the following at lowering time:

- **Non-constant width** — the width expression did not reduce to an
  integer literal (most constant folds beyond literals will land in a
  later typeck task).
- **Negative width** — e.g. `int x : -1;`.
- **Width exceeding the underlying type** — e.g. `int x : 64;` against
  a 32-bit `int`.
- **Named zero-width bit-field** — e.g. `int x : 0;`. Only the
  anonymous form `int : 0;` is permitted as a separator.

```c
struct bad {
    int a : -1;      // error[E0077]: bit-field width cannot be negative
    int b : 64;      // error[E0077]: bit-field width 64 exceeds width of underlying type
    int c : 0;       // error[E0077]: named bit-field must have a non-zero width
};

struct ok {
    int : 0;         // legal: anonymous separator — forces alignment
    int flag : 1;    // legal: 1 ≤ 32 (int width)
};
```

---

## E0078 — duplicate enumerator name

C99 §6.7.2.2p3 requires the enumerators inside a single enumeration
specifier to have pairwise distinct names, and §6.4.4.3 places every
enumeration constant in the ordinary identifier namespace. Repeating
an enumerator name — either within the same `enum { ... }` body or
against an earlier ordinary-namespace binding at the same scope — is
therefore a constraint violation. `rcc` rejects the repeat and keeps
the first binding so that subsequent references still resolve.

```c
enum { A, A };            // error[E0078]: duplicate enumerator name `A`

typedef int B;
enum { B };               // error[E0078]: duplicate enumerator name `B`
```

---

## E0079 — invalid initializer designator

C99 §6.7.8 requires initializer designators to match the aggregate
currently being initialized: `[N]` is only valid for arrays, `.field`
is only valid for structs/unions, and a selected index or field must
exist. `rcc` reports E0079 during HIR lowering instead of silently
dropping the initializer entry.

```c
int a[2] = { [4] = 1 };   // error[E0079]: index past the array bound
int b[2] = { .x = 1 };    // error[E0079]: field designator on an array
struct S { int x; };
struct S s = { .y = 1 };  // error[E0079]: no member named `y`
```

---

## E0080 — assignment to rvalue

C99 §6.5.16p2 requires the left operand of a simple or compound
assignment to be a *modifiable lvalue*. The narrower constraint that
the LHS be an lvalue at all is checked first — writing to the result
of a cast, an arithmetic expression, a literal, a function call, or
any other expression that does not designate an object is a
constraint violation. The broader modifiable-lvalue check also uses
this code for writes through const-qualified objects, array objects,
and increment/decrement of const lvalues.

```c
void f(int x) {
    (int)x = 1;     // error[E0080]: assignment to rvalue
    1 = x;          // error[E0080]: assignment to rvalue
    x + 0 = 1;      // error[E0080]: assignment to rvalue
    const int c = 0;
    c = 1;          // error[E0080]: const object is not modifiable
}
```

`x = 1;` on a plain `int` local is well-formed: an identifier
referring to an object is an lvalue.

---

## E0081 — incompatible types in assignment

C99 §6.5.16.1p1 enumerates the only legal RHS shapes for a simple
assignment, function-call argument, return statement, or initializer:

- both operands have arithmetic type (the RHS may be implicitly
  converted; lossy conversions are flagged with W0008, not E0081);
- both operands are compatible struct or union types;
- both operands are pointers to compatible types, with the LHS's
  pointee qualifier set including every qualifier on the RHS's
  pointee;
- one operand is a pointer to an object/incomplete type and the
  other is a pointer to (qualified or unqualified) `void`;
- the LHS is a pointer and the RHS is a *null pointer constant* —
  an integer constant expression with value 0, optionally cast to
  `void *` (§6.3.2.3p3);
- the LHS is `_Bool` and the RHS is any pointer.

Anything else is a constraint violation:

```c
struct A { int x; };
struct B { int y; };
void f(struct A a) {
    struct B *p = &a;   // error[E0081]: incompatible types in assignment
    int *q = 1;         // error[E0081]: only the *integer constant 0*
                        //               is a null pointer constant
    int n = &a;         // error[E0081]: pointer cannot initialise an int
}
```

Pointer assignment that drops a qualifier on the pointee — `int *p
= &c_i;` where `c_i` is `const int` — also lands here; the LHS's
pointee qualifier set must be a superset of the RHS's so writing
through the LHS cannot violate the source's `const` / `volatile` /
`restrict` promise.

Null-pointer-constant detection unwraps `Cast` and the
type-checker's own `Convert` wrappers, so `(void *)0`, `(int *)0`,
and `0` itself all match.

---

## E0082 — incompatible pointer conversion

C99 §6.3.2.3 enumerates the only implicit conversions between
pointer types that the type-checker may insert without an explicit
cast:

- any pointer to (qualified or unqualified) `void` may be converted
  to/from a pointer to any object/incomplete type, with qualifier
  *additions* on the destination side allowed but qualifier
  *removals* requiring an explicit cast;
- a *null pointer constant* (the integer constant `0`, optionally
  cast to `void *`) converts to any pointer type;
- two pointers to compatible types (in the §6.7.5 sense) are
  interchangeable when the destination's pointee qualifier set
  includes every qualifier of the source's pointee;
- two pointers to function types are interchangeable iff the
  function types are compatible (matching return type, parameter
  list, and variadicity).

All other pointer-shaped conversions land here:

```c
void f(void) {
    int   *p   = 0;        // OK: null pointer constant
    void  *q   = p;        // OK: object pointer to void *
    int    x   = 0;
    int   *r   = &x;
    char  *s   = r;        // error[E0082]: int * is not compatible
                           //               with char *
    const int *cp = r;     // OK: qualifier addition
    int   *rw   = cp;      // error[E0082]: drops `const` qualifier
                           //               (cast required)
    int (*fp1)(int);
    int (*fp2)(double) = fp1;  // error[E0082]: incompatible function
                               //               signatures
    int   *ip = 1;         // error[E0082]: only the integer constant
                           //               0 may be assigned to a
                           //               pointer without a cast
    int    n  = ip;        // error[E0082]: pointer to integer needs
                           //               an explicit cast
}
```

Function pointers are *not* object pointers (§6.3.2.3p8), so
`void *p = &f;` and `int (*fp)() = malloc(n);` are both
constraint violations regardless of the matching pointee shapes.

Null-pointer-constant detection unwraps `Cast` and the
type-checker's own `Convert` wrappers, so `(void *)0`, `(int *)0`,
and `0` itself all qualify as null pointer constants and may be
assigned to any pointer type.

---

## E0083 — invalid operands, controlling expression, or function call

Raised by the type-checker when operand types do not match any rule
the operator allows, or when a statement / conditional controlling
expression is not scalar. It also covers call-expression constraints:
the callee must have function or pointer-to-function type, and a
prototyped call must pass the required number of fixed arguments
(C99 §6.5.2.2, §6.5.5–§6.5.15, §6.8.4).

```c
struct S { int x; } s1, s2;
void f(int *p, int i, double d) {
    s1 / s2;          // error[E0083]: arithmetic ops need arithmetic
                      //               (or pointer + integer for +/-)
    p & i;            // error[E0083]: bitwise ops need integer
    p % i;            // error[E0083]: % is integer-only
    p && d;           // error[E0083]: logical operands must be scalar
                      //               with a common type
    if (s1) {}         // error[E0083]: condition must be scalar
    p ? p : d;         // error[E0083]: conditional arms incompatible
    i();               // error[E0083]: called expression is not a function
    printf();          // error[E0083]: too few prototype arguments
}
```

The diagnostic is emitted at the operator, controlling expression, or
call span. Operand types or expected / actual argument counts are
included when they help explain the failed rule.

---

## E0084 — non-constant expression in static initializer

C99 §6.7.8p4 requires every expression in the initializer of an object
with static or thread storage duration to be a constant expression
(§6.6) or a string literal. After type checking, the constant-expression
evaluator must be able to fold each scalar leaf into one of:

- an integer constant expression (§6.6p6),
- an arithmetic constant expression (§6.6p7), or
- an address constant (§6.6p8) — `&obj`, `&arr[ice]`, function
  designator, or `(T*)0 + ice`.

```c
static int x = 2 + 3;     // OK — integer constant expression
static int y = foo();     // error[E0084]: non-constant expression in
                          //               static initializer
static int *p = &x;       // OK — address constant
```

The label points at the offending sub-expression so users can see
which leaf of an aggregate initializer is at fault. The check has no
effect on local-variable initializers — those may be arbitrary
expressions per §6.7.8p3.

---

## E0085 — sizeof operand has no complete object layout

C99 §6.5.3.4 requires `sizeof` to have a complete object layout. For
ordinary types this is a compile-time size. For VLA operands the bound
is runtime-dependent, but the element layout still must be known so the
CFG can lower `sizeof a` to `len(a) * sizeof(element)`.

```c
struct Incomplete;

unsigned long a = sizeof(struct Incomplete);  // error[E0085]
```

The CFG pass emits E0085 before LLVM codegen instead of materialising a
silent `0` byte size. Supported layout answers come from the shared
`rcc_hir::LayoutCx` service used by both CFG and LLVM codegen.

---

## E0086 — invalid switch label

C99 §6.8.4.2 requires every `case` and `default` label to appear inside
an enclosing `switch` statement. Within one switch, each `case`
constant value must be unique and there may be only one `default`.

```c
void f(int x) {
    case 1: ;              // error[E0086]: case outside switch
    switch (x) {
        case 1:
        case 1: ;          // error[E0086]: duplicate case value
        default:
        default: ;         // error[E0086]: duplicate default label
    }
}
```

---

## E0087 — invalid member access

C99 §6.5.2.3 requires `.` to select a member from a struct or union
object, and `->` to select a member through a pointer to struct or
union. The selected member name must exist in that record type.

```c
int x;
x.y;                    // error[E0087]: not a struct or union

struct S { int a; };
struct S s;
s.b;                    // error[E0087]: no member named `b`

int *p;
p->a;                   // error[E0087]: not a pointer to struct/union
```

After this check succeeds, HIR member accesses are rewritten from the
source member name to a numeric field index, so CFG and codegen never
guess which field was intended.

---

## E0088 — typed HIR invariant violation

Raised by the typeck-to-CFG boundary verifier when a supposedly clean
type-checking pass still leaves `Ty::Error`, an unresolved placeholder,
or an untyped initializer leaf in HIR.

This is an internal compiler invariant diagnostic, not the first error
users should normally see. It means an earlier phase parsed and lowered
a construct but failed to either:

- emit the real source-language diagnostic,
- assign a concrete C99 type, or
- reject / feature-gate an unsupported extension before CFG/codegen.

The diagnostic points at the HIR node's source span so the missing
semantic check can be routed back to the responsible phase.

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

## W0007 — enumerator value outside the range of `int`

C99 §6.7.2.2p2 requires each enumerator's value to be representable as
`int`; §6.7.2.2p4 then lets the implementation pick any integer type
wide enough to hold every enumerator of the enumeration. In M4 `rcc`
simplifies the rule to "the underlying type is always `int`", so an
explicit value outside `[INT_MIN, INT_MAX]` — or a defaulted value
that would overflow `int` via the implicit `prev + 1` step — is
flagged and the enumerator is still recorded so downstream passes see
a stable binding. M6 will promote the rule to the full §6.7.2.2p4
selection algorithm, at which point this warning goes away.

```c
enum huge { A = 0xFFFFFFFFFF };  // warning[W0007]: value 1099511627775 of enumerator `A` is outside the range of `int`
```

Like every warning, W0007 does not count toward `Handler::has_errors`.

## W0008 — implicit conversion narrows value

The C99 §6.5.16.1 assignment-compatibility rules accept any
arithmetic-to-arithmetic conversion on the RHS of `=` (and on
function-call arguments, return statements, and initializers — all
follow the same rule). Many of those conversions silently lose
information at run time:

```c
int    x = 1.5;            // warning[W0008]: 1.5 cannot be represented as `int`
unsigned char b = 300;     // warning[W0008]: 300 wraps to 44
int    n = 1ULL << 40;     // warning[W0008]: high bits of `unsigned long long` lost
unsigned u = -1;           // warning[W0008]: -1 reinterpreted as 0xFFFFFFFF
```

`rcc` follows every other modern C compiler and warns whenever the
destination type cannot represent the full range or precision of the
source type. The conversion is still performed; the warning gives
the user a chance to add an explicit cast or fix the type.

Task 07-05 introduces the warning; task 07-07 wires it to the
implicit `Convert` insertion pass.

Like every warning, W0008 does not count toward `Handler::has_errors`.


---

## W0009 — integer overflow in constant expression

C99 §6.6 specifies integer constant expressions and §6.5p5 declares
that signed-integer overflow is undefined behaviour. The constant
evaluator (`rcc_typeck::const_eval`) detects overflow on `+ - * /
% <<` while folding and reports it instead of silently wrapping:

```c
int  a = INT_MAX + 1;          // warning[W0009]: overflow folds to None
long b = (1LL << 62) * 4;      // warning[W0009]: high bits lost
int  c = -INT_MIN;             // warning[W0009]: |INT_MIN| has no positive
                               //                 counterpart in `int`
```

When `eval_int` reports `W0009` it returns `None`; the surrounding
expression therefore decays to a runtime computation rather than an
integer constant. Initializers and `case` labels that needed a
constant value will follow up with their own diagnostic.

---

## W0010 — division by zero in constant expression

The constant-expression evaluator also flags `n / 0` and `n % 0`:

```c
int q = 10 / 0;                // warning[W0010]
int r = 10 % 0;                // warning[W0010]
```

C99 §6.5.5p5 makes division by zero undefined behaviour. The
evaluator returns `None` rather than panicking, so the rest of the
translation unit still type-checks; the runtime behaviour is left
to whatever LLVM emits.

---

## W0011 — shift count out of range in constant expression

C99 §6.5.7p3 makes left- or right-shifting by a value `>= width` (or
negative) undefined behaviour. The evaluator detects this when both
operands are constants:

```c
int x = 1 << 32;               // warning[W0011]: 32 bits is the int width
int y = 1 << -1;               // warning[W0011]: negative shift count
```

As with W0009 / W0010 the fold returns `None`; the operator stays
in HIR for codegen to emit (where LLVM in turn picks a target-
specific behaviour).

---

## W0013 — GNU statement expression extension

`({ ... })` is a GNU C extension that evaluates a compound statement
as an expression:

```c
int x = ({ int y = 1; y; });   // warning[W0013] in strict C99 mode
```

The parser preserves the statement-expression AST so HIR/CFG work can
diagnose labels, gotos, lifetimes, and result type rules later. Enable
`Options::gnu_statement_expressions` to accept the syntax without this
compatibility warning.

---

## W0014 — GNU initializer range designator extension

`[lo ... hi] = value` is a GNU designated-initializer extension that
initializes a contiguous range of array elements:

```c
int a[8] = { [1 ... 5] = 9 };  // warning[W0014] in strict C99 mode
```

The parser preserves the range as a distinct AST designator so later
initializer lowering can expand it or diagnose overlap and ordering
rules. Enable `Options::gnu_range_designators` to accept the syntax
without this compatibility warning.

## W0015 — GNU attribute syntax extension

`__attribute__((...))` is a GNU C extension, not C99 syntax:

```c
int x __attribute__((aligned(16)));  // warning[W0015] in strict C99 mode
```

The parser preserves the attribute name and raw argument tokens for
phase-14 semantic checks. Enable `Options::gnu_attributes` to accept
the syntax without this compatibility warning.

## W0016 — GNU inline assembly syntax extension

GNU inline assembly is a GNU C extension, not C99 syntax:

```c
asm volatile ("nop");  // warning[W0016] in strict C99 mode
```

The parser preserves the template, qualifiers, operands, constraints,
clobbers, and spans for phase-14 validation and LLVM lowering. Enable
`Options::gnu_inline_asm` to accept the syntax without this
compatibility warning.

## W0017 — GNU omitted conditional operand extension

GNU C permits the middle operand of `?:` to be omitted:

```c
int x = y ?: 42;  // warning[W0017] in strict C99 mode
```

This means `y ? y : 42`, except `y` is evaluated exactly once. The
parser preserves the construct as its own expression node so HIR and CFG
lowering can maintain that single-evaluation guarantee. Enable
`Options::gnu_omitted_conditional_operand` to accept the syntax without
this compatibility warning.

## W0018 — GNU conditional expression with one void operand

C99 requires both result operands of `?:` to have type `void` when
either one is void. GNU C accepts a single void arm and gives the whole
conditional expression type `void`:

```c
1 ? value : (void)side_effect();  // warning[W0018] in strict C99 mode
```

The type checker preserves this GNU-compatible void result so
statement-position uses can continue through CFG/codegen. Enable
`Options::gnu_conditional_void_operand` to accept the construct without
this compatibility warning.

## W0019 — GNU case range extension

`case lo ... hi:` is a GNU C extension that matches every integer case
value in the inclusive range:

```c
switch (x) {
case 0 ... 5: return 1;  // warning[W0019] in strict C99 mode
}
```

The parser preserves the range as an explicit switch label so HIR can
expand and validate it before CFG lowering. Enable
`Options::gnu_case_ranges` to accept the construct without this
compatibility warning.

## W0020 — GNU labels-as-values extension

GNU C lets a program take the address of a local label and jump through
that value:

```c
void *p = &&target;  // warning[W0020] in strict C99 mode
goto *p;             // warning[W0020] in strict C99 mode
target: ;
```

The extension lowers to LLVM `blockaddress` and `indirectbr` inside the
owning function. Enable `Options::gnu_labels_as_values` to accept
`&&label` and `goto *expr` without this compatibility warning.

## W0021 — GNU lvalue comma extension

GNU C treats a comma expression as an lvalue when its right operand is
an lvalue:

```c
int i, j;
(i = 5, j) = 6;  // warning[W0021] in strict C99 mode
```

C99 makes every comma expression an rvalue, so `rcc` emits W0021 while
preserving GNU semantics for recovery and compatibility tests. Enable
`Options::gnu_lvalue_comma` to accept the construct without this
compatibility warning.

## W0022 — GNU function name alias

C99 defines `__func__` as an implicit function-scope identifier. GNU C
also accepts `__FUNCTION__` as an alias for the same string payload:

```c
char *f(void) { return __FUNCTION__; }  // warning[W0022] in strict C99 mode
```

HIR lowering preserves `__FUNCTION__` as a function-name string so GNU
compatibility tests can continue. Enable `Options::gnu_function_names`
to accept the alias without this compatibility warning.

## W0023 — GNU __va_area__ compatibility builtin

C99 exposes variadic arguments through `<stdarg.h>`, not through an
identifier that names the ABI varargs save area. Some chibicc fixtures
use `__va_area__` inside variadic functions:

```c
void f(int n, ...) {
    void *p = __va_area__;  // warning[W0023] in strict C99 mode
}
```

HIR lowering accepts `__va_area__` only inside variadic functions so the
compatibility suite can exercise runtime varargs behaviour. Enable
`Options::gnu_va_area` to accept it without this compatibility warning.

## W0024 — GNU typeof type specifier extension

`typeof (expr)` and `typeof (type-name)` are GNU C declaration
specifiers, not C99 syntax:

```c
int f(void);
extern typeof(f) f;  // warning[W0024] in strict C99 mode
```

The parser preserves the specifier so compatibility declarations can
reach HIR lowering. Enable `Options::gnu_typeof` or pass
`-fgnu-typeof` to accept it without this compatibility warning.
