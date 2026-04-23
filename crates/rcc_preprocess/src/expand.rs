//! Dave Prosser's standard macro-expansion algorithm (C99 §6.10.3).
//!
//! The preprocessor feeds one logical line at a time into
//! [`expand_line`]. Identifiers that name defined macros are replaced
//! by their expanded replacement lists; the replacement is then
//! *rescanned* so that newly-exposed macro names also expand. The
//! recursion blocker is Prosser's per-token **hide set**: every time a
//! macro `M` is expanded, the name `M` is added to the hide set of
//! every resulting token, so the rescan cannot re-invoke `M` on them.
//!
//! Scope of this task (04-09):
//!
//! - Object-like and function-like expansion with nested-paren-aware
//!   argument collection.
//! - Self-recursion (`#define FOO FOO`) and mutual recursion
//!   (`#define A B` / `#define B A`) both terminate via hide sets.
//! - Stringize `#parameter` (this task, 04-09): inside a function-like
//!   replacement list, `#` followed by one of the macro's parameter
//!   names is replaced at substitution time by a single `StringLit`
//!   whose contents are the actual argument's **raw** token text (i.e.
//!   before hide-set expansion — C99 §6.10.3.2p2), with internal
//!   whitespace collapsed to a single space and embedded `"`/`\`
//!   escaped.
//! - Token pasting `##` and variadic `__VA_ARGS__` are still future
//!   tasks (04-10 / 04-11); `##` continues to pass through here.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use rcc_data_structures::FxHashSet;
use rcc_errors::{codes::E0024, Diagnostic, Handler, Label, Level};
use rcc_lexer::{PpToken, PpTokenKind, Punct, StringEncoding};
use rcc_span::{BytePos, Interner, SourceMap, Span, Symbol};

use crate::macros::{HideSet, MacroKind, MacroTable};

/// Token paired with its Prosser hide set. The hide set travels
/// alongside the raw [`PpToken`] through every expansion step and is
/// dropped once the token reaches the final output stream.
#[derive(Clone, Debug)]
struct ExpToken {
    tok: PpToken,
    hide: HideSet,
}

/// Expand one logical line.
///
/// The returned token vector contains no `Newline` and no `Whitespace`
/// tokens — those are filtered upstream by
/// [`crate::line_stream::LineStream`] — and has every function-like
/// macro invocation, including its argument parentheses, replaced by
/// its rescanned expansion.
///
/// `interner` must be the same interner whose [`Symbol`]s populate
/// [`MacroDef::name`](crate::macros::MacroDef::name); otherwise the
/// identifier-to-definition lookup silently misses.
///
/// `source_map` is taken by shared reference to the session-wide
/// `RwLock<SourceMap>` rather than a read guard so that the stringize
/// operator (task 04-09) can briefly write-lock the map to register a
/// synthetic source file holding the rendered string literal's text.
/// The expander holds no long-lived lock across its own work.
pub fn expand_line(
    source_map: &Arc<RwLock<SourceMap>>,
    interner: &mut Interner,
    handler: &mut Handler,
    macros: &MacroTable,
    line: Vec<PpToken>,
) -> Vec<PpToken> {
    let input: Vec<ExpToken> =
        line.into_iter().map(|t| ExpToken { tok: t, hide: FxHashSet::default() }).collect();
    let mut exp = Expander { source_map, interner, handler, macros };
    exp.expand(input).into_iter().map(|et| et.tok).collect()
}

struct Expander<'a> {
    source_map: &'a Arc<RwLock<SourceMap>>,
    interner: &'a mut Interner,
    handler: &'a mut Handler,
    macros: &'a MacroTable,
}

impl Expander<'_> {
    /// Core Prosser `expand(TS)`.
    ///
    /// Walks the token sequence left-to-right, pushing non-macro
    /// tokens to the output and replacing macro invocations by the
    /// expanded substitution of their replacement list. Replacement
    /// results are pushed back to the *front* of the work queue so
    /// that the rescan happens automatically on the next loop
    /// iteration.
    fn expand(&mut self, input: Vec<ExpToken>) -> Vec<ExpToken> {
        let mut work: VecDeque<ExpToken> = input.into();
        let mut out: Vec<ExpToken> = Vec::with_capacity(work.len());

        while let Some(et) = work.pop_front() {
            // Only identifiers can name macros.
            if et.tok.kind != PpTokenKind::Ident {
                out.push(et);
                continue;
            }
            let name = self.symbol_of(&et.tok);
            // Hide-set blocks self/mutual recursion (Prosser 1986).
            if et.hide.contains(&name) {
                out.push(et);
                continue;
            }
            let Some(def) = self.macros.get(name) else {
                out.push(et);
                continue;
            };

            // Snapshot the definition's kind + body so we can drop the
            // borrow of `self.macros` before recursing.
            let kind = def.kind.clone();
            let body = def.body.clone();

            match kind {
                MacroKind::ObjectLike => {
                    let mut hide = et.hide.clone();
                    hide.insert(name);
                    let replaced = self.subst(&body, &[], &[], &hide, false);
                    push_front_all(&mut work, replaced);
                }
                MacroKind::FunctionLike { params, variadic: _ } => {
                    // Peek for the invocation `(`. Per C99 §6.10.3p10,
                    // a function-like macro name NOT followed by `(`
                    // is NOT a macro invocation — emit the identifier
                    // unchanged.
                    if !matches!(
                        work.front().map(|e| e.tok.kind),
                        Some(PpTokenKind::Punct(Punct::LParen))
                    ) {
                        out.push(et);
                        continue;
                    }
                    // Consume the `(`.
                    let _lparen = work.pop_front().unwrap();

                    let Some((raw_args, close_hide)) = self.collect_args(&mut work) else {
                        // Missing matching `)` — bail out of this
                        // expansion. Diagnostic is deferred to a
                        // follow-up task; for now the macro name and
                        // already-consumed `(` are both lost. We keep
                        // the remainder of the line intact by
                        // restoring nothing: the original tokens are
                        // simply gone. Emit the macro name back out
                        // so callers still see something.
                        out.push(et);
                        continue;
                    };

                    // Reconcile the natural comma-split against the
                    // macro's declared arity. `F()` invoking a
                    // zero-param macro must match; `G(a) G()` is a
                    // single empty argument.
                    let args = reconcile_arity(raw_args, params.len());
                    if args.len() != params.len() {
                        // Arity mismatch — diagnostic deferred. Skip
                        // expansion and emit the bare name (follow-on
                        // tokens still flow normally via the work
                        // queue front, but we've already consumed
                        // `(...)` — those tokens are lost. This is a
                        // known rough edge documented in task 04-08.
                        out.push(et);
                        continue;
                    }

                    // Prosser: HS' = (HS(name) ∩ HS(close-paren)) ∪ {name}.
                    let mut hide: HideSet = et.hide.intersection(&close_hide).copied().collect();
                    hide.insert(name);

                    let replaced = self.subst(&body, &params, &args, &hide, true);
                    push_front_all(&mut work, replaced);
                }
            }
        }

        out
    }

    /// Prosser `subst(body, formals, actuals, HS, OS)`:
    ///
    /// Walk the replacement list, emitting each token directly except
    /// for:
    ///
    /// - **Stringize** (`is_fn_like` + `#` + parameter name): emits a
    ///   single `StringLit` synthesised from the **raw** actual
    ///   argument (C99 §6.10.3.2p2 — stringize runs *before* hide-set
    ///   expansion on the argument).
    /// - **Parameter reference** (identifier is a formal): replaced by
    ///   the *fully-expanded* actual argument (pre-scan).
    ///
    /// At the end, union `hide` into every output token's hide set
    /// (the `HSADD(HS, OS)` step).
    ///
    /// `is_fn_like = false` disables the stringize branch entirely so
    /// that object-like replacement lists preserve `#` and `##` as
    /// ordinary punctuators. Token pasting `##` is still pass-through
    /// pending task 04-10.
    fn subst(
        &mut self,
        body: &[PpToken],
        params: &[Symbol],
        args: &[Vec<ExpToken>],
        hide: &HideSet,
        is_fn_like: bool,
    ) -> Vec<ExpToken> {
        let mut out: Vec<ExpToken> = Vec::with_capacity(body.len());

        let mut i = 0;
        while i < body.len() {
            let tok = body[i];

            // Stringize `#param` — only applies inside function-like
            // replacement lists (C99 §6.10.3.2p1).
            if is_fn_like && tok.kind == PpTokenKind::Punct(Punct::Hash) {
                let next = body.get(i + 1).copied();
                let arg_idx = next.and_then(|nxt| {
                    if nxt.kind != PpTokenKind::Ident {
                        return None;
                    }
                    let sym = self.symbol_of(&nxt);
                    params.iter().position(|p| *p == sym)
                });
                if let Some(idx) = arg_idx {
                    let nxt = next.expect("peek matched");
                    let hash_span = tok.span;
                    // Stringize against the RAW, not-yet-expanded
                    // argument — §6.10.3.2p2.
                    let stringized = self.stringize(&args[idx], hash_span, nxt.span);
                    out.push(stringized);
                    i += 2;
                    continue;
                }
                // `#` not followed by a parameter name — C99
                // §6.10.3.2p1 constraint violation.
                self.emit_e0024(tok.span, next);
                // Pass the `#` through as a raw punctuator so
                // downstream analysis still sees something; the
                // diagnostic has already been recorded.
                out.push(ExpToken { tok, hide: FxHashSet::default() });
                i += 1;
                continue;
            }

            if tok.kind == PpTokenKind::Ident {
                let sym = self.symbol_of(&tok);
                if let Some(idx) = params.iter().position(|p| *p == sym) {
                    // Pre-scan (fully expand) the actual argument
                    // before splicing it in. The expanded tokens
                    // inherit HS via HSADD at the end, blocking the
                    // current macro from re-expanding through them.
                    let expanded = self.expand(args[idx].clone());
                    out.extend(expanded);
                    i += 1;
                    continue;
                }
            }
            out.push(ExpToken { tok, hide: FxHashSet::default() });
            i += 1;
        }

        // HSADD: add the macro's hide set to every output token.
        if !hide.is_empty() {
            for et in &mut out {
                for &h in hide {
                    et.hide.insert(h);
                }
            }
        }

        out
    }

    /// Render a function-like macro argument's raw token sequence as a
    /// C string literal per C99 §6.10.3.2p2:
    ///
    /// - Leading and trailing whitespace of the argument is stripped.
    /// - Whitespace between two tokens collapses to exactly one
    ///   space (signalled by [`PpToken::leading_ws`] on the trailing
    ///   token — prior preprocessor stages dropped the whitespace
    ///   tokens themselves).
    /// - Every `"` and `\` character inside a token's spelling is
    ///   escaped with a preceding backslash; other characters are
    ///   copied verbatim (including already-escaped sequences inside
    ///   string and character literals, whose backslashes each get
    ///   doubled to survive another round of lexing).
    ///
    /// The rendered text is registered as a small synthetic source
    /// file so the produced `StringLit` token's span obeys the usual
    /// invariant that `t.span.file`'s source bytes contain the exact
    /// token spelling. `_hash_span` and `_param_span` are accepted so
    /// the caller can, in the future, weave the original `#param` site
    /// into diagnostics; they are currently unused because `PpToken`
    /// carries no separate diagnostic-origin slot.
    fn stringize(
        &mut self,
        arg_tokens: &[ExpToken],
        _hash_span: Span,
        _param_span: Span,
    ) -> ExpToken {
        let mut body = String::from("\"");
        let mut first = true;
        for et in arg_tokens {
            let tok = &et.tok;
            // `Whitespace`/`Newline` tokens are filtered upstream by
            // `LineStream`, but guard defensively.
            if matches!(tok.kind, PpTokenKind::Whitespace | PpTokenKind::Newline) {
                continue;
            }
            if !first && tok.leading_ws {
                body.push(' ');
            }
            first = false;
            let text = {
                let sm = self.source_map.read().unwrap();
                let src = &sm.file(tok.span.file).src;
                src[tok.span.lo.0 as usize..tok.span.hi.0 as usize].to_string()
            };
            for ch in text.chars() {
                match ch {
                    '\\' => body.push_str("\\\\"),
                    '"' => body.push_str("\\\""),
                    _ => body.push(ch),
                }
            }
        }
        body.push('"');

        let body_len = body.len() as u32;
        let file_id = {
            let mut sm = self.source_map.write().unwrap();
            sm.add_file(PathBuf::from("<stringize>"), Arc::from(body))
        };
        let span = Span::new(file_id, BytePos(0), BytePos(body_len));
        let tok = PpToken {
            kind: PpTokenKind::StringLit { enc: StringEncoding::None },
            span,
            leading_ws: false,
            at_line_start: false,
        };
        ExpToken { tok, hide: FxHashSet::default() }
    }

    /// Emit E0024: `#` in a function-like replacement list not
    /// followed by a parameter name (C99 §6.10.3.2p1).
    fn emit_e0024(&mut self, hash_span: Span, next: Option<PpToken>) {
        let primary_label =
            Label { span: hash_span, message: "`#` here".into(), primary: true };
        let mut labels = vec![primary_label];
        if let Some(nxt) = next {
            labels.push(Label {
                span: nxt.span,
                message: "expected a macro parameter name".into(),
                primary: false,
            });
        }
        let diag = Diagnostic {
            level: Level::Error,
            code: Some(E0024),
            message: "`#` is not followed by a macro parameter".into(),
            labels,
            notes: vec!["C99 §6.10.3.2p1: each `#` in a function-like replacement \
                 list shall be immediately followed by one of the macro's \
                 parameter names"
                .into()],
            help: vec!["did you mean to write a parameter name, or to escape a \
                 literal `#` via `##`?"
                .into()],
        };
        self.handler.emit(&diag);
    }

    /// Collect the argument list of a function-like invocation.
    ///
    /// Called *after* the opening `(` has been popped from `work`.
    /// Returns `(args, close_hide)` where `args` is the natural
    /// comma-split token sequence (length ≥ 1 on success; inner
    /// parentheses protect embedded commas) and `close_hide` is the
    /// hide set of the matching `)` token. Returns `None` if the
    /// closing `)` is never found (unterminated invocation).
    fn collect_args(&self, work: &mut VecDeque<ExpToken>) -> Option<(Vec<Vec<ExpToken>>, HideSet)> {
        let mut args: Vec<Vec<ExpToken>> = Vec::new();
        let mut current: Vec<ExpToken> = Vec::new();
        let mut depth: u32 = 0;

        loop {
            let et = work.pop_front()?;
            match et.tok.kind {
                PpTokenKind::Punct(Punct::LParen) => {
                    depth += 1;
                    current.push(et);
                }
                PpTokenKind::Punct(Punct::RParen) if depth == 0 => {
                    args.push(current);
                    return Some((args, et.hide));
                }
                PpTokenKind::Punct(Punct::RParen) => {
                    depth -= 1;
                    current.push(et);
                }
                PpTokenKind::Punct(Punct::Comma) if depth == 0 => {
                    args.push(current);
                    current = Vec::new();
                }
                _ => current.push(et),
            }
        }
    }

    /// Intern the source text of an identifier token into its
    /// canonical [`Symbol`]. `PpToken` does not carry a symbol; we
    /// recover it on demand from the token's span.
    fn symbol_of(&mut self, tok: &PpToken) -> Symbol {
        let text = {
            let sm = self.source_map.read().unwrap();
            let src = &sm.file(tok.span.file).src;
            src[tok.span.lo.0 as usize..tok.span.hi.0 as usize].to_string()
        };
        self.interner.intern(&text)
    }
}

/// Push `replaced` onto the *front* of `work` in original order. This
/// is the rescan step: the output of `subst` becomes the next input
/// tokens to be scanned for further expansion.
fn push_front_all(work: &mut VecDeque<ExpToken>, replaced: Vec<ExpToken>) {
    for t in replaced.into_iter().rev() {
        work.push_front(t);
    }
}

/// Reconcile the raw comma-split produced by [`Expander::collect_args`]
/// with the macro's declared parameter count.
///
/// `collect_args` always returns at least one entry on success (the
/// final `current` is pushed unconditionally). That means `F()` comes
/// back as `vec![vec![]]`. For a zero-parameter macro we re-interpret
/// that single empty slot as "zero arguments". For any other shape,
/// the natural list is returned unchanged — caller compares it against
/// `params.len()` to decide on arity match.
fn reconcile_arity(mut raw: Vec<Vec<ExpToken>>, param_count: usize) -> Vec<Vec<ExpToken>> {
    if param_count == 0 && raw.len() == 1 && raw[0].is_empty() {
        raw.clear();
    }
    raw
}

#[cfg(test)]
mod tests {
    //! Acceptance tests for task 04-08 (hide-set expansion).

    use super::*;
    use crate::macros::{define_macro, MacroDef};
    use rcc_errors::{CaptureEmitter, Handler};
    use rcc_lexer::tokenize;
    use rcc_session::{Options, Session};
    use rcc_span::{BytePos, FileId, Span};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Tokenise `src` into a fresh file. Strips trailing `Newline`
    /// tokens so callers get a replacement-list or call-site view.
    fn tok_line(session: &mut Session, name: &str, src: &str) -> (FileId, Vec<PpToken>) {
        let file = session
            .source_map
            .write()
            .unwrap()
            .add_file(PathBuf::from(format!("<{name}>")), Arc::from(src));
        let toks: Vec<PpToken> =
            tokenize(file, src).filter(|t| t.kind != PpTokenKind::Newline).collect();
        (file, toks)
    }

    /// Build an object-like macro `NAME body_src` and install it in
    /// `macros` via [`define_macro`].
    fn install_object(session: &mut Session, macros: &mut MacroTable, name: &str, body_src: &str) {
        let full = format!("#define {name} {body_src}\n");
        let name_sym = session.interner.intern(name);
        let (file, mut toks) = tok_line(session, name, &full);
        // Drop `#`, `define`, NAME — keep replacement list.
        toks.drain(..3);
        let def_span = Span::new(file, BytePos(0), BytePos(full.len() as u32));
        let def = MacroDef { name: name_sym, kind: MacroKind::ObjectLike, body: toks, def_span };
        let sm = session.source_map.read().unwrap();
        define_macro(def, macros, &sm, &session.interner).unwrap();
    }

    /// Build a function-like macro and install it.
    fn install_fn(
        session: &mut Session,
        macros: &mut MacroTable,
        name: &str,
        params: &[&str],
        body_src: &str,
    ) {
        // Reconstruct the directive text only for diagnostic spans.
        let params_joined = params.join(",");
        let full = format!("#define {name}({params_joined}) {body_src}\n");
        let name_sym = session.interner.intern(name);
        let param_syms: Vec<Symbol> = params.iter().map(|p| session.interner.intern(p)).collect();
        // Tokenise only the body, not the full directive.
        let (_file, body) = tok_line(session, &format!("{name}-body"), body_src);
        let def_span = Span::new(FileId(0), BytePos(0), BytePos(full.len() as u32));
        let def = MacroDef {
            name: name_sym,
            kind: MacroKind::FunctionLike { params: param_syms, variadic: false },
            body,
            def_span,
        };
        let sm = session.source_map.read().unwrap();
        define_macro(def, macros, &sm, &session.interner).unwrap();
    }

    /// Pretty-print an expanded token stream as a space-separated
    /// source-text string — convenient for assertion.
    fn pp(session: &Session, tokens: &[PpToken]) -> String {
        let sm = session.source_map.read().unwrap();
        tokens
            .iter()
            .map(|t| {
                let src = &sm.file(t.span.file).src;
                src[t.span.lo.0 as usize..t.span.hi.0 as usize].to_string()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Build a session with a capturing emitter.
    fn fresh_session() -> Session {
        let cap = CaptureEmitter::new();
        Session::with_handler(Options::default(), Handler::with_emitter(Box::new(cap)))
    }

    /// Build a session whose `Handler` writes to a returned
    /// [`CaptureEmitter`] so tests can inspect diagnostics.
    fn session_with_capture() -> (Session, CaptureEmitter) {
        let cap = CaptureEmitter::new();
        let sess = Session::with_handler(
            Options::default(),
            Handler::with_emitter(Box::new(cap.clone())),
        );
        (sess, cap)
    }

    /// Convenience wrapper: run [`expand_line`] against a whole
    /// session. Handles the `Arc`-clone / borrow dance so individual
    /// tests stay focused on their input/output pair.
    fn run_expand(sess: &mut Session, macros: &MacroTable, line: Vec<PpToken>) -> Vec<PpToken> {
        let sm_arc = Arc::clone(&sess.source_map);
        expand_line(&sm_arc, &mut sess.interner, &mut sess.handler, macros, line)
    }

    // ── Acceptance ──────────────────────────────────────────────────

    #[test]
    fn self_recursive_object_macro_terminates() {
        // `#define FOO FOO` + `FOO` → one literal `FOO`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "FOO", "FOO");

        let (_file, line) = tok_line(&mut sess, "call", "FOO\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(out.len(), 1, "self-recursion must stop after one round: {:?}", pp(&sess, &out));
        assert_eq!(pp(&sess, &out), "FOO");
    }

    #[test]
    fn mutual_recursion_terminates_with_a_emitted() {
        // `#define A B / #define B A / A` terminates with `A`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "A", "B");
        install_object(&mut sess, &mut macros, "B", "A");

        let (_file, line) = tok_line(&mut sess, "call", "A\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "A", "mutual recursion must emit the original name verbatim");
    }

    #[test]
    fn function_like_max_expands_both_args() {
        // `#define MAX(a,b) ((a)>(b)?(a):(b))` / `MAX(1, 2)`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "MAX", &["a", "b"], "((a)>(b)?(a):(b))");

        let (_file, line) = tok_line(&mut sess, "call", "MAX(1, 2)\n");
        let out = run_expand(&mut sess, &macros, line);

        // Expected: ( ( 1 ) > ( 2 ) ? ( 1 ) : ( 2 ) )
        let joined: String = out
            .iter()
            .map(|t| {
                let sm = sess.source_map.read().unwrap();
                let src = &sm.file(t.span.file).src;
                src[t.span.lo.0 as usize..t.span.hi.0 as usize].to_string()
            })
            .collect();
        assert_eq!(joined, "((1)>(2)?(1):(2))");
    }

    #[test]
    fn nested_parens_in_call_collect_one_arg() {
        // `#define F(x) x` / `F((a, b))` → expands to `(a, b)` (ONE arg).
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "F", &["x"], "x");

        let (_file, line) = tok_line(&mut sess, "call", "F((a, b))\n");
        let out = run_expand(&mut sess, &macros, line);

        let joined: String = out
            .iter()
            .map(|t| {
                let sm = sess.source_map.read().unwrap();
                let src = &sm.file(t.span.file).src;
                src[t.span.lo.0 as usize..t.span.hi.0 as usize].to_string()
            })
            .collect();
        assert_eq!(joined, "(a,b)", "embedded comma inside inner parens must not split arguments");
    }

    // ── Additional coverage ─────────────────────────────────────────

    #[test]
    fn nonmacro_identifiers_pass_through() {
        let mut sess = fresh_session();
        let macros = MacroTable::default();

        let (_file, line) = tok_line(&mut sess, "call", "foo bar 42\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "foo bar 42");
    }

    #[test]
    fn object_macro_expands_to_its_body() {
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "PI", "314");

        let (_file, line) = tok_line(&mut sess, "call", "PI\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "314");
    }

    #[test]
    fn rescan_exposes_nested_macro() {
        // `#define TWO 2` / `#define PAIR TWO TWO` / `PAIR` → `2 2`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "TWO", "2");
        install_object(&mut sess, &mut macros, "PAIR", "TWO TWO");

        let (_file, line) = tok_line(&mut sess, "call", "PAIR\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "2 2");
    }

    #[test]
    fn function_like_not_followed_by_paren_is_not_invoked() {
        // `#define F(x) x` / `F + 1` — no `(`, so `F` passes through.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "F", &["x"], "x");

        let (_file, line) = tok_line(&mut sess, "call", "F + 1\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "F + 1");
    }

    #[test]
    fn zero_param_function_macro_expands() {
        // `#define ANSWER() 42` / `ANSWER()` → `42`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "ANSWER", &[], "42");

        let (_file, line) = tok_line(&mut sess, "call", "ANSWER()\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "42");
    }

    #[test]
    fn argument_is_pre_expanded_before_substitution() {
        // `#define ID(x) x` / `#define ONE 1` / `ID(ONE)` → `1`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "ONE", "1");
        install_fn(&mut sess, &mut macros, "ID", &["x"], "x");

        let (_file, line) = tok_line(&mut sess, "call", "ID(ONE)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "1");
    }

    #[test]
    fn mutual_recursion_through_function_like_terminates() {
        // `#define A() B()` / `#define B() A()` / `A()` terminates.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "A", &[], "B()");
        install_fn(&mut sess, &mut macros, "B", &[], "A()");

        let (_file, line) = tok_line(&mut sess, "call", "A()\n");
        let out = run_expand(&mut sess, &macros, line);

        // The exact terminal form is implementation-defined in the
        // details but MUST terminate — i.e., the call returned. We
        // also assert the emitted token stream is non-empty (the
        // original `A ( )` bubbles back out once both hide sets are
        // saturated).
        assert!(!out.is_empty(), "function-like mutual recursion must terminate with something");
    }

    // ── Stringize `#` (task 04-09) ──────────────────────────────────

    /// Shortcut: expand and return the first token's text if the
    /// output is a single `StringLit`, else panic. Used by the
    /// stringize tests to keep assertions compact.
    fn expect_single_string(sess: &Session, out: &[PpToken]) -> String {
        assert_eq!(out.len(), 1, "expected exactly one output token, got {}: {out:?}", out.len());
        assert!(
            matches!(out[0].kind, PpTokenKind::StringLit { enc: StringEncoding::None }),
            "expected narrow StringLit, got {:?}",
            out[0].kind
        );
        let sm = sess.source_map.read().unwrap();
        let src = &sm.file(out[0].span.file).src;
        src[out[0].span.lo.0 as usize..out[0].span.hi.0 as usize].to_string()
    }

    #[test]
    fn stringize_identifier_argument() {
        // `#define S(x) #x` / `S(hello)` → `"hello"`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "S", &["x"], "#x");

        let (_file, line) = tok_line(&mut sess, "call", "S(hello)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(expect_single_string(&sess, &out), "\"hello\"");
    }

    #[test]
    fn stringize_escapes_quotes_and_backslashes() {
        // `S("a")` → `"\"a\""`. The outer literal keeps its quotes;
        // every `"` and `\` inside the argument tokens is escaped.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "S", &["x"], "#x");

        let (_file, line) = tok_line(&mut sess, "call", "S(\"a\")\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(expect_single_string(&sess, &out), "\"\\\"a\\\"\"");
        assert!(cap.diagnostics().is_empty(), "benign stringize must not diagnose");
    }

    #[test]
    fn stringize_escapes_embedded_backslash_in_string_literal() {
        // Argument spells `"\n"` — the `\` gets doubled and each `"`
        // gets escaped, so the stringized result is `"\"\\n\""`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "S", &["x"], "#x");

        let (_file, line) = tok_line(&mut sess, "call", "S(\"\\n\")\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(expect_single_string(&sess, &out), "\"\\\"\\\\n\\\"\"");
    }

    #[test]
    fn stringize_collapses_whitespace_between_tokens() {
        // `S(1 + 2)` — whitespace between tokens collapses to one
        // space. `S(1+2)` would produce `"1+2"` since no whitespace
        // separates the pp-tokens.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "S", &["x"], "#x");

        let (_file, line) = tok_line(&mut sess, "call", "S(1 +  2)\n");
        let out = run_expand(&mut sess, &macros, line);
        assert_eq!(expect_single_string(&sess, &out), "\"1 + 2\"");

        let (_file, line) = tok_line(&mut sess, "call2", "S(1+2)\n");
        let out = run_expand(&mut sess, &macros, line);
        assert_eq!(expect_single_string(&sess, &out), "\"1+2\"");
    }

    #[test]
    fn stringize_strips_leading_and_trailing_whitespace() {
        // `S(  a  )` → `"a"`; the flanking whitespace is dropped.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "S", &["x"], "#x");

        let (_file, line) = tok_line(&mut sess, "call", "S(  a  )\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(expect_single_string(&sess, &out), "\"a\"");
    }

    #[test]
    fn stringize_empty_argument_is_empty_string() {
        // `#define S(x) #x` / `S()` → `""`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "S", &["x"], "#x");

        let (_file, line) = tok_line(&mut sess, "call", "S()\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(expect_single_string(&sess, &out), "\"\"");
    }

    #[test]
    fn stringize_runs_before_argument_expansion() {
        // C99 §6.10.3.2p2: the argument is stringized BEFORE being
        // fully expanded. `#define ONE 1` / `#define S(x) #x` /
        // `S(ONE)` must render literally `"ONE"`, not `"1"`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "ONE", "1");
        install_fn(&mut sess, &mut macros, "S", &["x"], "#x");

        let (_file, line) = tok_line(&mut sess, "call", "S(ONE)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(expect_single_string(&sess, &out), "\"ONE\"");
    }

    #[test]
    fn stringize_inside_a_larger_body_is_stitched_in() {
        // `#define S(x) "head " #x " tail"` exercises that the
        // stringized token is spliced into a larger replacement list
        // alongside other ordinary tokens.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "S", &["x"], "a #x b");

        let (_file, line) = tok_line(&mut sess, "call", "S(hello)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(out.len(), 3, "expected `a`, `\"hello\"`, `b`, got {out:?}");
        assert!(matches!(out[1].kind, PpTokenKind::StringLit { enc: StringEncoding::None }));
        assert_eq!(pp(&sess, &out), "a \"hello\" b");
    }

    #[test]
    fn hash_not_followed_by_param_emits_e0024() {
        // `#define BAD(x) #y` — `#` is followed by `y`, which is NOT
        // a parameter. Constraint violation C99 §6.10.3.2p1.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "BAD", &["x"], "#y");

        let (_file, line) = tok_line(&mut sess, "call", "BAD(1)\n");
        let _out = run_expand(&mut sess, &macros, line);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "exactly one E0024 expected, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0024));
        assert!(diags[0].labels.iter().any(|l| l.primary), "must have a primary label on `#`");
    }

    #[test]
    fn hash_in_object_macro_body_is_not_a_stringize() {
        // `#define O #` / `O` — in an object-like macro, `#` stays a
        // plain punctuator and no E0024 fires.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "O", "#");

        let (_file, line) = tok_line(&mut sess, "call", "O\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty(), "object-like `#` must not diagnose");
        assert_eq!(out.len(), 1);
        assert!(
            matches!(out[0].kind, PpTokenKind::Punct(Punct::Hash)),
            "object-like body preserves `#` verbatim, got {:?}",
            out[0].kind
        );
    }
}
