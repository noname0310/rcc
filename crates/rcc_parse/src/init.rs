//! Initializer parsing (C99 §6.7.8).
//!
//! A C99 *initializer* is either a single assignment-expression or a
//! brace-enclosed *initializer-list* whose elements may be prefixed
//! by a *designation* — a chain of `.ident` / `[expr]` designators
//! followed by `=` naming which sub-object the value initialises:
//!
//! ```text
//! initializer:
//!     assignment-expression
//!     { initializer-list }
//!     { initializer-list , }
//!
//! initializer-list:
//!     designation? initializer
//!     initializer-list , designation? initializer
//!
//! designation:
//!     designator-list =
//!
//! designator-list:
//!     designator
//!     designator-list designator
//!
//! designator:
//!     [ constant-expression ]
//!     . identifier
//! ```
//!
//! We mirror the grammar directly: [`parse_initializer`] dispatches on
//! `{` versus "anything else"; the list form delegates to
//! [`parse_init_list`], which repeatedly reads one `designation?
//! initializer` pair, tolerates a trailing `,`, and stops at the
//! closing `}`.
//!
//! ## Designator expressions and the constant-expression rule
//!
//! C99 §6.7.8 lists the array-designator expression as a
//! *constant-expression*. "Constant-expression" is not a separate
//! grammar level — it is exactly the *conditional-expression*
//! non-terminal plus a semantic constraint (§6.6p2). Enforcing the
//! constant-ness is a type-check-time job; the parser accepts the
//! whole assignment-expression superset here, the same shape the
//! other "constant expression in disguise" slots (bitfield width,
//! case label, enumerator value) already take. A later pass rejects
//! non-constant forms with a precise diagnostic.
//!
//! ## Empty `{}`
//!
//! §6.7.8p1 lists `{ initializer-list }` and `{ initializer-list , }`
//! — **not** `{}`. GCC accepts the empty form as an extension; we
//! match the spec and reject it with a diagnostic, matching what the
//! task file specifies. Recovery keeps parsing so a spurious `{}`
//! does not poison the remainder of the declaration.

use rcc_ast::{Designator, Initializer};
use rcc_lexer::Punct;

use crate::expr::parse_assignment_expression;
use crate::token::TokenKind;
use crate::Parser;

/// Parse a C99 §6.7.8 *initializer*. Dispatches on the current
/// token: `{` starts a brace-enclosed list (possibly with
/// designators); anything else falls through to
/// [`parse_assignment_expression`] for the bare-expression form.
///
/// Returns `None` when the expression form fails to produce an
/// operand; the inner parser has already emitted a diagnostic at
/// the offending cursor position. The list form always returns
/// `Some(_)` so callers can keep making progress even when some
/// element was malformed.
pub fn parse_initializer(p: &mut Parser<'_>) -> Option<Initializer> {
    if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::LBrace))) {
        Some(Initializer::List(parse_init_list(p)))
    } else {
        let expr = parse_assignment_expression(p)?;
        Some(Initializer::Expr(expr))
    }
}

/// Parse a brace-enclosed *initializer-list*. Caller has confirmed
/// the current token is `{`.
///
/// Shape: `{ (designation? initializer) (, designation? initializer)* ,? }`.
///
/// Recovery strategy:
///
/// - Unexpected end-of-input inside the list: emit a diagnostic
///   anchored at the opening `{` and return what we have so far.
/// - Missing `,`/`}` after an element: emit a diagnostic but return
///   the elements gathered so far.
/// - Empty `{}`: emit a diagnostic (C99 §6.7.8 grammar requires at
///   least one element) and return an empty list.
fn parse_init_list(p: &mut Parser<'_>) -> Vec<(Vec<Designator>, Initializer)> {
    let open = p.bump().expect("caller peeked `{`").span;
    let mut elements: Vec<(Vec<Designator>, Initializer)> = Vec::new();

    loop {
        // Check for `}` (end of list) first so that a trailing `,`
        // followed by `}` terminates cleanly without trying to
        // parse a phantom element after the comma.
        match p.peek() {
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RBrace)) => {
                p.bump();
                if elements.is_empty() {
                    p.session
                        .handler
                        .struct_err(open, "empty initializer list is invalid in C99")
                        .label(open, "expected at least one initializer inside `{`")
                        .emit();
                }
                return elements;
            }
            Some(_) => {}
            None => {
                p.session
                    .handler
                    .struct_err(p.cur_span(), "unexpected end of input inside initializer list")
                    .label(open, "unclosed `{` here")
                    .emit();
                return elements;
            }
        }

        let designators = parse_designator_chain(p);
        let Some(init) = parse_initializer(p) else {
            // The inner parser has already diagnosed; skip to the
            // next `,` / `}` so the remaining elements still parse.
            skip_to_comma_or_rbrace(p);
            if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Comma))) {
                p.bump();
            }
            continue;
        };
        elements.push((designators, init));

        match p.peek() {
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Comma)) => {
                p.bump();
                // Loop continues; next iteration will notice `}` if
                // this was a trailing comma.
            }
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RBrace)) => {
                p.bump();
                return elements;
            }
            Some(_) => {
                let at = p.cur_span();
                p.session
                    .handler
                    .struct_err(at, "expected `,` or `}` in initializer list")
                    .label(open, "initializer list starts here")
                    .emit();
                skip_to_comma_or_rbrace(p);
                if matches!(p.peek().map(|t| &t.kind), Some(TokenKind::Punct(Punct::Comma))) {
                    p.bump();
                }
            }
            None => {
                p.session
                    .handler
                    .struct_err(p.cur_span(), "unexpected end of input inside initializer list")
                    .label(open, "unclosed `{` here")
                    .emit();
                return elements;
            }
        }
    }
}

/// Parse the optional designator chain that may prefix an
/// initializer-list element. The chain is any run of `.ident` /
/// `[expr]` designators terminated by `=`. If the current token is
/// neither `.` nor `[`, the chain is empty and no `=` is consumed.
///
/// Returns an empty `Vec` for the positional case. For the designated
/// case the vector contains one entry per designator in source order
/// — so `[2].sub` yields `[Index(2), Field(sub)]`.
///
/// Error recovery: a `.` not followed by an identifier, a `[` with no
/// closing `]`, or a designator chain missing its terminating `=`
/// are all diagnosed in place; the parser still returns whatever
/// designators it collected so the surrounding initializer can keep
/// making progress.
fn parse_designator_chain(p: &mut Parser<'_>) -> Vec<Designator> {
    let mut chain: Vec<Designator> = Vec::new();

    loop {
        // Extract just the punctuator + span so we can bump and emit
        // diagnostics without fighting the borrow checker over the
        // peeked token's kind field (which is not `Copy`).
        let Some((punct, op_span)) = p.peek().and_then(|t| match t.kind {
            TokenKind::Punct(pu) => Some((pu, t.span)),
            _ => None,
        }) else {
            break;
        };

        match punct {
            Punct::Dot => {
                p.bump();
                match p.peek() {
                    Some(t) => {
                        if let TokenKind::Ident(sym) = t.kind {
                            p.bump();
                            chain.push(Designator::Field(sym));
                        } else {
                            let at = t.span;
                            p.session
                                .handler
                                .struct_err(at, "expected identifier after `.` in designator")
                                .label(op_span, "designator begins here")
                                .emit();
                            break;
                        }
                    }
                    None => {
                        p.session
                            .handler
                            .struct_err(op_span, "expected identifier after `.` in designator")
                            .emit();
                        break;
                    }
                }
            }
            Punct::LBracket => {
                let lb_span = op_span;
                p.bump();
                let Some(expr) = parse_assignment_expression(p) else {
                    break;
                };
                match p.peek() {
                    Some(t) if matches!(t.kind, TokenKind::Punct(Punct::RBracket)) => {
                        p.bump();
                    }
                    _ => {
                        let at = p.cur_span();
                        p.session
                            .handler
                            .struct_err(at, "expected `]` to close array designator")
                            .label(lb_span, "unmatched `[` here")
                            .emit();
                    }
                }
                chain.push(Designator::Index(expr));
            }
            _ => break,
        }
    }

    if !chain.is_empty() {
        match p.peek() {
            Some(t) if matches!(t.kind, TokenKind::Punct(Punct::Eq)) => {
                p.bump();
            }
            _ => {
                let at = p.cur_span();
                p.session.handler.struct_err(at, "expected `=` after designator chain").emit();
            }
        }
    }

    chain
}

/// Advance the cursor to the next `,` or `}` at the current brace
/// depth, or to end-of-input. Inner `{`/`(`/`[` are tracked so a
/// comma inside a nested initializer / subscript / call argument
/// list does not end the enclosing element prematurely.
fn skip_to_comma_or_rbrace(p: &mut Parser<'_>) {
    let mut depth_brace: u32 = 0;
    let mut depth_paren: u32 = 0;
    let mut depth_bracket: u32 = 0;
    while let Some(tok) = p.peek() {
        match tok.kind {
            TokenKind::Punct(Punct::LBrace) => {
                depth_brace += 1;
                p.bump();
            }
            TokenKind::Punct(Punct::RBrace) => {
                if depth_brace == 0 {
                    return;
                }
                depth_brace -= 1;
                p.bump();
            }
            TokenKind::Punct(Punct::LParen) => {
                depth_paren += 1;
                p.bump();
            }
            TokenKind::Punct(Punct::RParen) => {
                depth_paren = depth_paren.saturating_sub(1);
                p.bump();
            }
            TokenKind::Punct(Punct::LBracket) => {
                depth_bracket += 1;
                p.bump();
            }
            TokenKind::Punct(Punct::RBracket) => {
                depth_bracket = depth_bracket.saturating_sub(1);
                p.bump();
            }
            TokenKind::Punct(Punct::Comma)
                if depth_brace == 0 && depth_paren == 0 && depth_bracket == 0 =>
            {
                return;
            }
            _ => {
                p.bump();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase7::convert;
    use rcc_ast::{Expr, ExprKind};
    use rcc_lexer::{PpToken, Tokenizer};
    use rcc_session::Session;
    use rcc_span::FileId;
    use std::sync::Arc;

    fn mk_session(src: &str) -> (Session, FileId, rcc_errors::CaptureEmitter) {
        let (sess, cap) = Session::for_test();
        let fid =
            sess.source_map.write().unwrap().add_file("t.c".into(), Arc::from(src.to_owned()));
        (sess, fid, cap)
    }

    fn parse_init_str(src: &str) -> (Initializer, Vec<rcc_errors::Diagnostic>, Session) {
        let (mut sess, fid, cap) = mk_session(src);
        let pps: Vec<PpToken> = Tokenizer::new(fid, src).collect();
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let init = parse_initializer(&mut parser).expect("initializer parses");
        assert_eq!(
            parser.cursor,
            parser.tokens.len(),
            "initializer parser must consume every token of {src:?}",
        );
        (init, cap.diagnostics(), sess)
    }

    fn intlit_text(e: &Expr, sess: &Session) -> String {
        match &e.kind {
            ExprKind::IntLit { text } => sess.interner.get(*text).to_string(),
            other => panic!("expected IntLit, got {other:?}"),
        }
    }

    // ── Bare expression form ────────────────────────────────────────

    #[test]
    fn bare_expression_wraps_in_expr_variant() {
        let (init, diags, sess) = parse_init_str("42");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::Expr(e) => assert_eq!(intlit_text(&e, &sess), "42"),
            other => panic!("expected Expr(42), got {other:?}"),
        }
    }

    // ── Positional brace lists ──────────────────────────────────────

    #[test]
    fn three_positional_ints() {
        let (init, diags, sess) = parse_init_str("{ 1, 2, 3 }");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::List(items) => {
                assert_eq!(items.len(), 3);
                for (designators, _) in &items {
                    assert!(designators.is_empty(), "positional element: no designators");
                }
                let expected = ["1", "2", "3"];
                for (i, (_, sub)) in items.iter().enumerate() {
                    match sub {
                        Initializer::Expr(e) => assert_eq!(intlit_text(e, &sess), expected[i]),
                        other => panic!("item {i}: expected Expr, got {other:?}"),
                    }
                }
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn trailing_comma_is_accepted() {
        // §6.7.8p1 grammar explicitly allows `{ initializer-list , }`.
        let (init, diags, _sess) = parse_init_str("{ 1, 2, }");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::List(items) => assert_eq!(items.len(), 2),
            other => panic!("expected List, got {other:?}"),
        }
    }

    // ── Designators ─────────────────────────────────────────────────

    #[test]
    fn single_field_designator() {
        let (init, diags, sess) = parse_init_str("{ .x = 1 }");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::List(items) => {
                assert_eq!(items.len(), 1);
                let (chain, sub) = &items[0];
                assert_eq!(chain.len(), 1);
                match &chain[0] {
                    Designator::Field(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                    other => panic!("expected Field(x), got {other:?}"),
                }
                match sub {
                    Initializer::Expr(e) => assert_eq!(intlit_text(e, &sess), "1"),
                    other => panic!("expected Expr(1), got {other:?}"),
                }
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn single_index_designator() {
        let (init, diags, sess) = parse_init_str("{ [2] = 3 }");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::List(items) => {
                assert_eq!(items.len(), 1);
                let (chain, sub) = &items[0];
                assert_eq!(chain.len(), 1);
                match &chain[0] {
                    Designator::Index(e) => assert_eq!(intlit_text(e, &sess), "2"),
                    other => panic!("expected Index(2), got {other:?}"),
                }
                match sub {
                    Initializer::Expr(e) => assert_eq!(intlit_text(e, &sess), "3"),
                    other => panic!("expected Expr(3), got {other:?}"),
                }
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn mixed_field_and_index_chain() {
        // `{ .x[2] = 5 }` — `x` is a field, then the `[2]` designator
        // indexes into it. Chain order: [Field(x), Index(2)].
        let (init, diags, sess) = parse_init_str("{ .x[2] = 5 }");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::List(items) => {
                assert_eq!(items.len(), 1);
                let (chain, sub) = &items[0];
                assert_eq!(chain.len(), 2, "chain is [.x, [2]]");
                match &chain[0] {
                    Designator::Field(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                    other => panic!("chain[0]: expected Field(x), got {other:?}"),
                }
                match &chain[1] {
                    Designator::Index(e) => assert_eq!(intlit_text(e, &sess), "2"),
                    other => panic!("chain[1]: expected Index(2), got {other:?}"),
                }
                match sub {
                    Initializer::Expr(e) => assert_eq!(intlit_text(e, &sess), "5"),
                    other => panic!("expected Expr(5), got {other:?}"),
                }
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn nested_designator_in_array() {
        // C99 §6.7.8 example: `{ [0].x = 1 }` — the index selects an
        // aggregate element, then `.x` selects a field within it.
        let (init, diags, sess) = parse_init_str("{ [0].x = 1 }");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::List(items) => {
                assert_eq!(items.len(), 1);
                let (chain, _) = &items[0];
                assert_eq!(chain.len(), 2);
                assert!(matches!(chain[0], Designator::Index(_)));
                match &chain[1] {
                    Designator::Field(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                    other => panic!("chain[1] must be Field, got {other:?}"),
                }
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    // ── Nested initializer lists ────────────────────────────────────

    #[test]
    fn nested_brace_lists() {
        let (init, diags, _sess) = parse_init_str("{ { 1, 2 }, { 3, 4 } }");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::List(outer) => {
                assert_eq!(outer.len(), 2);
                for (_, sub) in &outer {
                    match sub {
                        Initializer::List(inner) => assert_eq!(inner.len(), 2),
                        other => panic!("expected nested List, got {other:?}"),
                    }
                }
            }
            other => panic!("expected outer List, got {other:?}"),
        }
    }

    // ── Mixed positional and designated elements ────────────────────

    #[test]
    fn mixing_positional_and_designated() {
        // C99 §6.7.8 permits mixing: `{ 1, .x = 2, 3 }` — the
        // positional and designated forms coexist inside one list.
        let (init, diags, sess) = parse_init_str("{ 1, .x = 2, 3 }");
        assert!(diags.is_empty(), "clean: {diags:?}");
        match init {
            Initializer::List(items) => {
                assert_eq!(items.len(), 3);
                assert!(items[0].0.is_empty(), "first element is positional");
                assert_eq!(items[1].0.len(), 1, "second element has one designator");
                match &items[1].0[0] {
                    Designator::Field(sym) => assert_eq!(sess.interner.get(*sym), "x"),
                    other => panic!("expected Field(x), got {other:?}"),
                }
                assert!(items[2].0.is_empty(), "third element is positional again");
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    // ── Error paths ─────────────────────────────────────────────────

    #[test]
    fn empty_brace_list_is_diagnosed() {
        let (mut sess, fid, cap) = mk_session("{}");
        let pps: Vec<PpToken> = Tokenizer::new(fid, "{}").collect();
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let init = parse_initializer(&mut parser).expect("still builds empty List on recovery");
        match init {
            Initializer::List(items) => assert!(items.is_empty()),
            other => panic!("expected List, got {other:?}"),
        }
        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("empty initializer list"),
            "message: {:?}",
            diags[0].message,
        );
    }

    #[test]
    fn dot_without_identifier_is_diagnosed() {
        let (mut sess, fid, cap) = mk_session("{ . = 1 }");
        let pps: Vec<PpToken> = Tokenizer::new(fid, "{ . = 1 }").collect();
        let tokens = convert(&mut sess, &pps);
        let mut parser = Parser::new(&mut sess, tokens);
        let _ = parse_initializer(&mut parser);
        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.message.contains("identifier after `.`")),
            "expected ident-after-dot diagnostic, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
