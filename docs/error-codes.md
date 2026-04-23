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

## E0006 — empty character constant

A character constant must contain at least one character.

```c
char c = '';  // error[E0006]: empty character constant
```

## E0007 — multi-character character constant

More than one character inside a character constant. C99 allows this but
the value is implementation-defined. `rcc` emits a warning.

```c
int x = 'ab';  // warning[E0007]: multi-character character constant
```

## E0008 — invalid numeric suffix

An integer or floating-point literal has a suffix that is not a valid
C99 integer suffix (`u`, `l`, `ul`, `ull`, etc.) or float suffix
(`f`, `l`).

```c
int x = 42q;  // error[E0008]: invalid numeric suffix 'q'
```

## E0009 — integer literal overflow

The value of an integer literal exceeds the range of the widest
representable type (`unsigned long long`).

```c
int x = 99999999999999999999;  // error[E0009]: integer literal overflow
```

## E0010 — floating-point literal overflow

A floating-point literal cannot be represented even as `long double`.

```c
double d = 1e99999;  // error[E0010]: floating-point literal overflow
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
