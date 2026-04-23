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
