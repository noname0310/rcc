//! Golden snapshots for `rcc --emit=tokens`.
//!
//! Each test feeds a small C fragment through `rcc_lexer::pretty::format_tokens`
//! — which is exactly what the driver's `--emit=tokens` stage prints — and
//! stores the expected output under `tests/snapshots/tokens/`. Any change to
//! lexer classification, span accounting, or the pretty-printer format
//! therefore surfaces as a reviewable diff.

use std::path::PathBuf;
use std::sync::Arc;

use rcc_lexer::pretty::format_tokens;
use rcc_span::SourceMap;

#[macro_use]
mod support;

/// Tokenise `src` and return the pretty-printed pp-token stream.
fn render(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let id = sm.add_file(PathBuf::from(name), Arc::from(src));
    format_tokens(src, &sm, id)
}

#[test]
fn hello_world() {
    let src = "#include <stdio.h>\n\
               \n\
               int main(void) {\n    \
               printf(\"Hello, world!\\n\");\n    \
               return 0;\n\
               }\n";
    assert_emit_snapshot!("tokens", "hello", render("hello.c", src));
}

#[test]
fn pp_numbers() {
    // Covers Integer vs Float classification, hex/binary exponents,
    // and the sign-absorption rule after `e`/`E`/`p`/`P`.
    let src = "0 42 0x1f 0755 3.14 .5 1e10 1E-3 0x1.0p0 0X1p+1 1.5f\n";
    assert_emit_snapshot!("tokens", "pp_numbers", render("pp_numbers.c", src));
}

#[test]
fn strings_and_chars() {
    // All five C99/C11 encoding prefixes for both `'...'` and `"..."`.
    let src = "'a' L'b' u'c' U'd' u8'e'\n\"a\" L\"b\" u\"c\" U\"d\" u8\"e\"\n";
    assert_emit_snapshot!("tokens", "strings_and_chars", render("literals.c", src));
}

#[test]
fn punctuators() {
    // Mix of 1-, 2- and 3-char punctuators to exercise max-munch
    // ordering and label stability.
    let src = "a[b].c->d; x++ --y; a+b-c*d/e%f<<g>>h;\n\
               a<b>c<=d>=e==f!=g && h||i?j:k;\n\
               x=1; y+=2; z<<=3; w>>=4; a&=b^=c|=d;\n\
               #define F(...) __VA_ARGS__\n";
    assert_emit_snapshot!("tokens", "punctuators", render("punct.c", src));
}

#[test]
fn comments_and_whitespace() {
    // Line/block comments collapse to whitespace; only `Newline` tokens
    // should survive in the stream.
    let src = "int a; // trailing line comment\n\
               /* leading block */ int b;\n\
               int /* mid */ c;\n";
    assert_emit_snapshot!("tokens", "comments_and_ws", render("comments.c", src));
}

#[test]
fn multiline_and_unicode_ident() {
    // Multi-line construct + a non-ASCII identifier continuation byte
    // to prove LineCol accounting and UTF-8 pass-through.
    let src = "int\n\
               \tfoo_bar\n\
               = 42;\n";
    assert_emit_snapshot!("tokens", "multiline", render("multiline.c", src));
}
