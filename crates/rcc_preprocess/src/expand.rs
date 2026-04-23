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
//! Scope of this task (04-08):
//!
//! - Object-like and function-like expansion with nested-paren-aware
//!   argument collection.
//! - Self-recursion (`#define FOO FOO`) and mutual recursion
//!   (`#define A B` / `#define B A`) both terminate via hide sets.
//! - `#` (stringize), `##` (paste), and variadic `__VA_ARGS__` are
//!   handled by later tasks (04-09 / 04-10 / 04-11). For now `#` and
//!   `##` pass through literally.

use std::collections::VecDeque;

use rcc_data_structures::FxHashSet;
use rcc_lexer::{PpToken, PpTokenKind, Punct};
use rcc_span::{Interner, SourceMap, Symbol};

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
pub fn expand_line(
    source_map: &SourceMap,
    interner: &mut Interner,
    macros: &MacroTable,
    line: Vec<PpToken>,
) -> Vec<PpToken> {
    let input: Vec<ExpToken> =
        line.into_iter().map(|t| ExpToken { tok: t, hide: FxHashSet::default() }).collect();
    let mut exp = Expander { source_map, interner, macros };
    exp.expand(input).into_iter().map(|et| et.tok).collect()
}

struct Expander<'a> {
    source_map: &'a SourceMap,
    interner: &'a mut Interner,
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
                    let replaced = self.subst(&body, &[], &[], &hide);
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

                    let replaced = self.subst(&body, &params, &args, &hide);
                    push_front_all(&mut work, replaced);
                }
            }
        }

        out
    }

    /// Prosser `subst(body, formals, actuals, HS, OS)`:
    ///
    /// Walk the replacement list, emitting each token directly except
    /// for parameter references, which are replaced by the
    /// *fully-expanded* actual argument. At the end, union `hide` into
    /// every output token's hide set (the `HSADD(HS, OS)` step).
    ///
    /// Stringize `#` and paste `##` are not handled here — they are
    /// follow-up tasks 04-09 and 04-10. Whatever `#` / `##` tokens
    /// appear in the body are treated as ordinary punctuators and
    /// passed through.
    fn subst(
        &mut self,
        body: &[PpToken],
        params: &[Symbol],
        args: &[Vec<ExpToken>],
        hide: &HideSet,
    ) -> Vec<ExpToken> {
        let mut out: Vec<ExpToken> = Vec::with_capacity(body.len());

        for tok in body {
            if tok.kind == PpTokenKind::Ident {
                let sym = self.symbol_of(tok);
                if let Some(idx) = params.iter().position(|p| *p == sym) {
                    // Pre-scan (fully expand) the actual argument
                    // before splicing it in. The expanded tokens
                    // inherit HS via HSADD at the end, blocking the
                    // current macro from re-expanding through them.
                    let expanded = self.expand(args[idx].clone());
                    out.extend(expanded);
                    continue;
                }
            }
            out.push(ExpToken { tok: *tok, hide: FxHashSet::default() });
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
        let src = &self.source_map.file(tok.span.file).src;
        let text = &src[tok.span.lo.0 as usize..tok.span.hi.0 as usize];
        self.interner.intern(text)
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

    // ── Acceptance ──────────────────────────────────────────────────

    #[test]
    fn self_recursive_object_macro_terminates() {
        // `#define FOO FOO` + `FOO` → one literal `FOO`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "FOO", "FOO");

        let (_file, line) = tok_line(&mut sess, "call", "FOO\n");
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

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
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

        assert_eq!(pp(&sess, &out), "A", "mutual recursion must emit the original name verbatim");
    }

    #[test]
    fn function_like_max_expands_both_args() {
        // `#define MAX(a,b) ((a)>(b)?(a):(b))` / `MAX(1, 2)`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "MAX", &["a", "b"], "((a)>(b)?(a):(b))");

        let (_file, line) = tok_line(&mut sess, "call", "MAX(1, 2)\n");
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

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
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

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
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

        assert_eq!(pp(&sess, &out), "foo bar 42");
    }

    #[test]
    fn object_macro_expands_to_its_body() {
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "PI", "314");

        let (_file, line) = tok_line(&mut sess, "call", "PI\n");
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

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
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

        assert_eq!(pp(&sess, &out), "2 2");
    }

    #[test]
    fn function_like_not_followed_by_paren_is_not_invoked() {
        // `#define F(x) x` / `F + 1` — no `(`, so `F` passes through.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "F", &["x"], "x");

        let (_file, line) = tok_line(&mut sess, "call", "F + 1\n");
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

        assert_eq!(pp(&sess, &out), "F + 1");
    }

    #[test]
    fn zero_param_function_macro_expands() {
        // `#define ANSWER() 42` / `ANSWER()` → `42`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "ANSWER", &[], "42");

        let (_file, line) = tok_line(&mut sess, "call", "ANSWER()\n");
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

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
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

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
        let sm_arc = Arc::clone(&sess.source_map);
        let sm = sm_arc.read().unwrap();
        let out = expand_line(&sm, &mut sess.interner, &macros, line);
        drop(sm);

        // The exact terminal form is implementation-defined in the
        // details but MUST terminate — i.e., the call returned. We
        // also assert the emitted token stream is non-empty (the
        // original `A ( )` bubbles back out once both hide sets are
        // saturated).
        assert!(!out.is_empty(), "function-like mutual recursion must terminate with something");
    }
}
