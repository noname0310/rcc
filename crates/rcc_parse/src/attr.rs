//! GNU `__attribute__((...))` parser surface.

use rcc_ast::{Attribute, AttributeArg, AttributeToken, AttributeTokenKind};
use rcc_lexer::Punct;
use rcc_span::Span;

use crate::keywords::Keyword;
use crate::token::TokenKind;
use crate::Parser;

const SUPPORTED_ATTRS: &[&str] = &[
    "access",
    "aligned",
    "alloc_align",
    "alloc_size",
    "always_inline",
    "artificial",
    "assume_aligned",
    "cold",
    "const",
    "copy",
    "constructor",
    "deprecated",
    "destructor",
    "fallthrough",
    "flatten",
    "format",
    "format_arg",
    "hot",
    "leaf",
    "malloc",
    "may_alias",
    "mode",
    "ms_struct",
    "no_instrument_function",
    "noinline",
    "nonnull",
    "nothrow",
    "noreturn",
    "nonstring",
    "packed",
    "pure",
    "returns_nonnull",
    "returns_twice",
    "scalar_storage_order",
    "section",
    "sentinel",
    "transparent_union",
    "unused",
    "used",
    "vector_size",
    "visibility",
    "warn_unused_result",
    "weak",
];

pub(crate) fn peek_attribute(p: &Parser<'_>) -> bool {
    is_attribute_at(p, p.cursor)
}

pub(crate) fn is_attribute_at(p: &Parser<'_>, at: usize) -> bool {
    matches!(
        p.tokens.get(at).map(|t| &t.kind),
        Some(TokenKind::Ident(sym)) if matches!(
            p.session.interner.get(*sym),
            "__attribute__" | "__attribute"
        )
    )
}

pub(crate) fn skip_attribute_groups_at(p: &Parser<'_>, mut at: usize) -> usize {
    while is_attribute_at(p, at) {
        let Some(TokenKind::Punct(Punct::LParen)) = p.tokens.get(at + 1).map(|t| &t.kind) else {
            return at;
        };
        let Some(TokenKind::Punct(Punct::LParen)) = p.tokens.get(at + 2).map(|t| &t.kind) else {
            return at;
        };

        let mut depth = 2usize;
        at += 3;
        while let Some(tok) = p.tokens.get(at) {
            match tok.kind {
                TokenKind::Punct(Punct::LParen) => depth += 1,
                TokenKind::Punct(Punct::RParen) => {
                    depth -= 1;
                    if depth == 0 {
                        at += 1;
                        break;
                    }
                }
                _ => {}
            }
            at += 1;
        }

        if depth != 0 {
            return at;
        }
    }
    at
}

pub(crate) fn parse_attributes(p: &mut Parser<'_>) -> Vec<Attribute> {
    let mut attrs = Vec::new();
    while peek_attribute(p) {
        attrs.extend(parse_attribute_group(p));
    }
    attrs
}

fn parse_attribute_group(p: &mut Parser<'_>) -> Vec<Attribute> {
    let start = p.cur_span();
    p.bump(); // `__attribute__`
    if !p.session.opts.gnu_attributes {
        p.session
            .handler
            .struct_warn(start, "GNU `__attribute__` is not part of C99")
            .code(rcc_errors::codes::W0015)
            .note("parsing it as an extension so phase 14 can validate semantics")
            .emit();
    }

    let Some(open_outer) = expect_punct(p, Punct::LParen, "expected `(` after `__attribute__`")
    else {
        return Vec::new();
    };
    let Some(open_inner) = expect_punct(p, Punct::LParen, "expected second `(` in attribute list")
    else {
        return Vec::new();
    };

    let mut attrs = Vec::new();
    loop {
        if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::RParen))) {
            break;
        }
        match parse_attribute_item(p) {
            Some(attr) => attrs.push(attr),
            None => {
                recover_to_attr_separator(p);
            }
        }
        match p.peek().map(|t| &t.kind) {
            Some(TokenKind::Punct(Punct::Comma)) => {
                p.bump();
            }
            _ => break,
        }
    }

    let _ = expect_punct(p, Punct::RParen, "expected `)` to close attribute list")
        .unwrap_or(open_inner);
    let _ = expect_punct(p, Punct::RParen, "expected final `)` after attribute list")
        .unwrap_or(open_outer);
    attrs
}

fn parse_attribute_item(p: &mut Parser<'_>) -> Option<Attribute> {
    let (name, name_span) = match p.peek() {
        Some(tok) => match tok.kind {
            TokenKind::Ident(sym) => {
                let span = tok.span;
                p.bump();
                (sym, span)
            }
            TokenKind::Keyword(kw) => {
                let span = tok.span;
                let sym = p.session.interner.intern(keyword_spelling(kw));
                p.bump();
                (sym, span)
            }
            _ => {
                p.session
                    .handler
                    .struct_err(tok.span, "expected attribute name")
                    .code(rcc_errors::codes::E0031)
                    .emit();
                return None;
            }
        },
        None => {
            p.session
                .handler
                .struct_err(p.cur_span(), "expected attribute name before end of input")
                .code(rcc_errors::codes::E0031)
                .emit();
            return None;
        }
    };

    let (args, end) = if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::LParen)))
    {
        parse_attribute_args(p)
    } else {
        (Vec::new(), name_span)
    };
    warn_if_unsupported_attr(p, name, name_span);
    Some(Attribute { name, args, span: name_span.to(end) })
}

fn warn_if_unsupported_attr(p: &mut Parser<'_>, name: rcc_span::Symbol, span: Span) {
    let raw = p.session.interner.get(name).to_owned();
    if supported_attr(&raw) {
        return;
    }
    p.session
        .handler
        .struct_warn(span, format!("unsupported GNU attribute `{raw}` ignored"))
        .code(rcc_errors::codes::W0033)
        .note("the attribute syntax was parsed and preserved, but rcc has no semantics for it")
        .help("add the attribute to rcc's supported attribute table before relying on it")
        .emit();
}

fn supported_attr(raw: &str) -> bool {
    let normalized = normalize_attr_name(raw);
    SUPPORTED_ATTRS.contains(&normalized)
}

fn normalize_attr_name(raw: &str) -> &str {
    raw.trim_matches('_')
}

fn parse_attribute_args(p: &mut Parser<'_>) -> (Vec<AttributeArg>, Span) {
    let open = p.bump().expect("caller checked `(`").span;
    let mut args = Vec::new();
    let mut current: Vec<AttributeToken> = Vec::new();
    let mut arg_start: Option<Span> = None;
    let mut depth_paren = 0u32;
    let mut depth_bracket = 0u32;
    let mut depth_brace = 0u32;
    let mut last = open;

    while let Some(tok) = p.peek().cloned() {
        match tok.kind {
            TokenKind::Punct(Punct::RParen)
                if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 =>
            {
                let close = tok.span;
                p.bump();
                push_arg(&mut args, &mut current, arg_start, last);
                return (args, close);
            }
            TokenKind::Punct(Punct::Comma)
                if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 =>
            {
                p.bump();
                push_arg(&mut args, &mut current, arg_start, last);
                arg_start = None;
                last = tok.span;
                continue;
            }
            TokenKind::Punct(Punct::LParen) => depth_paren += 1,
            TokenKind::Punct(Punct::RParen) => depth_paren = depth_paren.saturating_sub(1),
            TokenKind::Punct(Punct::LBracket) => depth_bracket += 1,
            TokenKind::Punct(Punct::RBracket) => depth_bracket = depth_bracket.saturating_sub(1),
            TokenKind::Punct(Punct::LBrace) => depth_brace += 1,
            TokenKind::Punct(Punct::RBrace) => depth_brace = depth_brace.saturating_sub(1),
            _ => {}
        }
        p.bump();
        if arg_start.is_none() {
            arg_start = Some(tok.span);
        }
        last = tok.span;
        current.push(attribute_token(p, &tok.kind, tok.span));
    }

    p.session
        .handler
        .struct_err(open, "unterminated attribute argument list")
        .code(rcc_errors::codes::E0031)
        .emit();
    push_arg(&mut args, &mut current, arg_start, last);
    (args, last)
}

fn push_arg(
    args: &mut Vec<AttributeArg>,
    current: &mut Vec<AttributeToken>,
    start: Option<Span>,
    end: Span,
) {
    if current.is_empty() {
        return;
    }
    let span = start.unwrap_or(end).to(end);
    args.push(AttributeArg { tokens: std::mem::take(current), span });
}

fn attribute_token(p: &mut Parser<'_>, kind: &TokenKind, span: Span) -> AttributeToken {
    let kind = match kind {
        TokenKind::Ident(sym) => AttributeTokenKind::Symbol(*sym),
        TokenKind::Keyword(kw) => {
            let spelling = keyword_spelling(*kw);
            AttributeTokenKind::Symbol(p.session.interner.intern(spelling))
        }
        TokenKind::IntLit(lit) => AttributeTokenKind::Int(lit.value),
        TokenKind::FloatLit(lit) => AttributeTokenKind::Float(lit.value),
        TokenKind::CharLit(lit) => AttributeTokenKind::Char(lit.value),
        TokenKind::StringLit(lit) => AttributeTokenKind::String(lit.bytes.clone()),
        TokenKind::Punct(punct) => {
            AttributeTokenKind::Punct(p.session.interner.intern(punct_spelling(*punct)))
        }
        TokenKind::Eof => AttributeTokenKind::Symbol(p.session.interner.intern("<eof>")),
    };
    AttributeToken { kind, span }
}

fn expect_punct(p: &mut Parser<'_>, want: Punct, msg: &str) -> Option<Span> {
    match p.peek() {
        Some(tok) if matches!(tok.kind, TokenKind::Punct(pu) if pu == want) => {
            let span = tok.span;
            p.bump();
            Some(span)
        }
        _ => {
            p.session.handler.struct_err(p.cur_span(), msg).code(rcc_errors::codes::E0031).emit();
            None
        }
    }
}

fn recover_to_attr_separator(p: &mut Parser<'_>) {
    while let Some(tok) = p.peek() {
        match tok.kind {
            TokenKind::Punct(Punct::Comma | Punct::RParen) => return,
            _ => {
                p.bump();
            }
        }
    }
}

fn keyword_spelling(kw: Keyword) -> &'static str {
    match kw {
        Keyword::Auto => "auto",
        Keyword::Break => "break",
        Keyword::Case => "case",
        Keyword::Char => "char",
        Keyword::Const => "const",
        Keyword::Continue => "continue",
        Keyword::Default => "default",
        Keyword::Do => "do",
        Keyword::Double => "double",
        Keyword::Else => "else",
        Keyword::Enum => "enum",
        Keyword::Extern => "extern",
        Keyword::Float => "float",
        Keyword::For => "for",
        Keyword::Goto => "goto",
        Keyword::If => "if",
        Keyword::Inline => "inline",
        Keyword::Int => "int",
        Keyword::Long => "long",
        Keyword::Register => "register",
        Keyword::Restrict => "restrict",
        Keyword::Return => "return",
        Keyword::Short => "short",
        Keyword::Signed => "signed",
        Keyword::Sizeof => "sizeof",
        Keyword::Static => "static",
        Keyword::Struct => "struct",
        Keyword::Switch => "switch",
        Keyword::Typedef => "typedef",
        Keyword::Union => "union",
        Keyword::Unsigned => "unsigned",
        Keyword::Void => "void",
        Keyword::Volatile => "volatile",
        Keyword::While => "while",
        Keyword::Bool => "_Bool",
        Keyword::Complex => "_Complex",
        Keyword::Imaginary => "_Imaginary",
    }
}

fn punct_spelling(punct: Punct) -> &'static str {
    match punct {
        Punct::LBracket => "[",
        Punct::RBracket => "]",
        Punct::LParen => "(",
        Punct::RParen => ")",
        Punct::LBrace => "{",
        Punct::RBrace => "}",
        Punct::Dot => ".",
        Punct::Arrow => "->",
        Punct::PlusPlus => "++",
        Punct::MinusMinus => "--",
        Punct::Amp => "&",
        Punct::Star => "*",
        Punct::Plus => "+",
        Punct::Minus => "-",
        Punct::Tilde => "~",
        Punct::Bang => "!",
        Punct::Slash => "/",
        Punct::Percent => "%",
        Punct::ShlShl => "<<",
        Punct::ShrShr => ">>",
        Punct::Lt => "<",
        Punct::Gt => ">",
        Punct::Le => "<=",
        Punct::Ge => ">=",
        Punct::EqEq => "==",
        Punct::BangEq => "!=",
        Punct::Caret => "^",
        Punct::Pipe => "|",
        Punct::AmpAmp => "&&",
        Punct::PipePipe => "||",
        Punct::Question => "?",
        Punct::Colon => ":",
        Punct::Semi => ";",
        Punct::Ellipsis => "...",
        Punct::Eq => "=",
        Punct::StarEq => "*=",
        Punct::SlashEq => "/=",
        Punct::PercentEq => "%=",
        Punct::PlusEq => "+=",
        Punct::MinusEq => "-=",
        Punct::ShlEq => "<<=",
        Punct::ShrEq => ">>=",
        Punct::AmpEq => "&=",
        Punct::CaretEq => "^=",
        Punct::PipeEq => "|=",
        Punct::Comma => ",",
        Punct::Hash => "#",
        Punct::HashHash => "##",
    }
}
