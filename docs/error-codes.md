# rcc Error Codes

Every user-facing diagnostic emitted by `rcc` carries a stable error code
of the form `EXXXX`. This page is the canonical reference. If a code
appears in compiler output but is missing here, that is a bug — CI will
catch it via `cargo xtask check-error-codes`.

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

An `#else` or `#elif` appeared without an opening `#if`.

```c
#else  // error[E0017]: unmatched #else/#elif
#endif
```

## E0018 — missing #endif at end of file

An `#if` / `#ifdef` / `#ifndef` was opened but never closed.

```c
#ifdef FOO
int x = 1;
// error[E0018]: missing #endif at end of file
```

## E0019 — unknown preprocessor directive

A `#` is followed by a token that is not a recognised C99 directive.

```c
#foobar  // error[E0019]: unknown preprocessor directive
```

## E0020 — #error directive encountered

The user explicitly triggered a compilation error via `#error`.

```c
#error "unsupported platform"  // error[E0020]: #error directive encountered
```

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
