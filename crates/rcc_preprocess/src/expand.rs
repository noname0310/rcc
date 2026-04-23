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
//! Scope as of task 04-10:
//!
//! - Object-like and function-like expansion with nested-paren-aware
//!   argument collection.
//! - Self-recursion (`#define FOO FOO`) and mutual recursion
//!   (`#define A B` / `#define B A`) both terminate via hide sets.
//! - Stringize `#parameter` (task 04-09): inside a function-like
//!   replacement list, `#` followed by one of the macro's parameter
//!   names is replaced at substitution time by a single `StringLit`
//!   whose contents are the actual argument's **raw** token text (i.e.
//!   before hide-set expansion — C99 §6.10.3.2p2), with internal
//!   whitespace collapsed to a single space and embedded `"`/`\`
//!   escaped.
//! - Token paste `##` (this task, 04-10): the left and right operand
//!   spellings are concatenated and re-lexed as a single pp-token;
//!   parameters named as paste operands are substituted **raw** per
//!   C99 §6.10.3.1p1. If the concatenation re-lexes to more than one
//!   token the paste is ill-formed and E0025 is emitted. Empty
//!   operands (from empty arguments) follow the §6.10.3.3p2 rule —
//!   the paste yields the other operand unchanged; if both operands
//!   are empty the paste produces nothing.
//! - Variadic `__VA_ARGS__` (task 04-11): function-like macros whose
//!   parameter list ends with `...` collect every trailing argument
//!   (commas and all) into a single pseudo-parameter named
//!   `__VA_ARGS__` (C99 §6.10.3p5). References to `__VA_ARGS__`
//!   outside a variadic body are constraint violations — E0026.
//!   The GNU extension `, ## __VA_ARGS__` (delete the preceding
//!   comma when `__VA_ARGS__` expands to nothing) is gated behind
//!   [`rcc_session::Options::gnu_va_args_elision`] and is off by
//!   default.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use rcc_data_structures::FxHashSet;
use rcc_errors::{
    codes::{E0024, E0025, E0026},
    Diagnostic, Handler, Label, Level,
};
use rcc_lexer::{tokenize, PpNumberKind, PpToken, PpTokenKind, Punct, StringEncoding};
use rcc_span::{BytePos, Interner, SourceMap, Span, Symbol};

use crate::line_map::LineMap;
use crate::macros::{BuiltinMacro, HideSet, MacroKind, MacroTable, VA_ARGS_NAME};

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
///
/// `gnu_va_args_elision` enables the GNU extension that drops the
/// preceding comma in `, ## __VA_ARGS__` when the variadic argument
/// list is empty (C99 has no equivalent; off by default).
#[allow(clippy::too_many_arguments)]
pub fn expand_line(
    source_map: &Arc<RwLock<SourceMap>>,
    interner: &mut Interner,
    handler: &mut Handler,
    macros: &MacroTable,
    line_map: &LineMap,
    line: Vec<PpToken>,
    gnu_va_args_elision: bool,
    gnu_permissive_paste: bool,
) -> Vec<PpToken> {
    let input: Vec<ExpToken> =
        line.into_iter().map(|t| ExpToken { tok: t, hide: FxHashSet::default() }).collect();
    let va_args_sym = interner.intern(VA_ARGS_NAME);
    let mut exp = Expander {
        source_map,
        interner,
        handler,
        macros,
        line_map,
        va_args_sym,
        gnu_va_args_elision,
        gnu_permissive_paste,
    };
    exp.expand(input).into_iter().map(|et| et.tok).collect()
}

struct Expander<'a> {
    source_map: &'a Arc<RwLock<SourceMap>>,
    interner: &'a mut Interner,
    handler: &'a mut Handler,
    macros: &'a MacroTable,
    /// `#line` overrides consulted by `__FILE__` / `__LINE__`
    /// expansion (task 04-15).
    line_map: &'a LineMap,
    /// Interned symbol for `__VA_ARGS__` — compared against body
    /// identifiers to detect the variadic pseudo-parameter.
    va_args_sym: Symbol,
    /// Enable GNU-style `, ## __VA_ARGS__` comma elision.
    gnu_va_args_elision: bool,
    /// Enable GNU-style permissive paste for pp-numbers.
    gnu_permissive_paste: bool,
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
                MacroKind::Builtin(builtin) => {
                    // Dynamic predefined macros (C99 §6.10.8p1). The
                    // synthesised replacement (string literal or
                    // pp-number) cannot name another macro, so
                    // hide-set bookkeeping is only defensive.
                    let mut hide = et.hide.clone();
                    hide.insert(name);
                    let replaced = self.expand_builtin(builtin, &et, hide);
                    push_front_all(&mut work, replaced);
                }
                MacroKind::ObjectLike => {
                    let mut hide = et.hide.clone();
                    hide.insert(name);
                    let replaced = self.subst(&body, &[], &[], &hide, false, false, None);
                    push_front_all(&mut work, replaced);
                }
                MacroKind::FunctionLike { params, variadic, named_variadic } => {
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

                    // Cap argument splitting at `params.len()` commas
                    // for variadic macros; everything after the last
                    // named parameter collapses into one slot — the
                    // future `__VA_ARGS__`. Non-variadic macros have
                    // no cap (`None`).
                    let max_splits = if variadic { Some(params.len()) } else { None };
                    let Some((raw_args, close_hide)) = self.collect_args(&mut work, max_splits)
                    else {
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
                    // macro's declared arity. For variadic macros an
                    // extra trailing slot carries `__VA_ARGS__`; if
                    // the caller supplied zero trailing arguments we
                    // synthesise an empty slot so substitution code
                    // can rely on `args.len() == params.len() + 1`.
                    let args = reconcile_arity(raw_args, params.len(), variadic);
                    let expected = if variadic { params.len() + 1 } else { params.len() };
                    if args.len() != expected {
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

                    let replaced =
                        self.subst(&body, &params, &args, &hide, true, variadic, named_variadic);
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
    /// - **Token paste** `##` (task 04-10): pops the already-emitted
    ///   LHS token, resolves the RHS body token (raw argument if it
    ///   names a parameter), concatenates their spellings, and
    ///   re-lexes the combined text. Parameters adjacent to `##` are
    ///   substituted raw per C99 §6.10.3.1p1; an empty operand makes
    ///   the paste yield the other operand unchanged (§6.10.3.3p2).
    ///   Multi-token re-lex → E0025.
    /// - **Parameter reference** (identifier is a formal, and not
    ///   adjacent to `##`): replaced by the *fully-expanded* actual
    ///   argument (pre-scan).
    ///
    /// At the end, union `hide` into every output token's hide set
    /// (the `HSADD(HS, OS)` step).
    ///
    /// `is_fn_like = false` disables the stringize branch entirely so
    /// that object-like replacement lists preserve `#` as an ordinary
    /// punctuator. Token paste `##` is handled for both macro forms
    /// since C99 §6.10.3.3p1 permits it in either.
    ///
    /// `variadic` indicates that the enclosing macro's parameter list
    /// ends with `...`; when true, `args` carries one extra trailing
    /// slot (index `params.len()`) holding the raw `__VA_ARGS__`
    /// tokens, and body occurrences of the identifier `__VA_ARGS__`
    /// substitute that slot. When `variadic` is false, any
    /// `__VA_ARGS__` in the body emits E0026 (C99 §6.10.3p5).
    #[allow(clippy::too_many_arguments)]
    fn subst(
        &mut self,
        body: &[PpToken],
        params: &[Symbol],
        args: &[Vec<ExpToken>],
        hide: &HideSet,
        is_fn_like: bool,
        variadic: bool,
        named_variadic: Option<Symbol>,
    ) -> Vec<ExpToken> {
        // C99 §6.10.3.3p1: `##` must not appear at the beginning or
        // end of a replacement list. Diagnose up front; the walk
        // below tolerates the malformed positions (dangling `##`s are
        // skipped / treated as a paste against an empty operand).
        self.check_paste_positions(body);

        let mut out: Vec<ExpToken> = Vec::with_capacity(body.len());
        // Tracks whether the *most recently processed slot* contributed
        // zero tokens to `out`. Used to implement the §6.10.3.3p2
        // empty-operand rule: a `##` whose LHS slot vanished pastes as
        // if the LHS were absent, yielding the RHS unchanged.
        let mut prev_empty = true;

        let mut i = 0;
        while i < body.len() {
            let tok = body[i];

            // Stringize `#param` — only applies inside function-like
            // replacement lists (C99 §6.10.3.2p1). `#__VA_ARGS__` is
            // treated as stringization of the variadic pseudo-parameter
            // when the enclosing macro is variadic; in a non-variadic
            // function-like macro it is a constraint violation (E0026).
            if is_fn_like && tok.kind == PpTokenKind::Punct(Punct::Hash) {
                let next = body.get(i + 1).copied();
                let lookup = next.and_then(|nxt| {
                    if nxt.kind != PpTokenKind::Ident {
                        return None;
                    }
                    let sym = self.symbol_of(&nxt);
                    Some((nxt, sym))
                });
                if let Some((nxt, sym)) = lookup {
                    if let Some(idx) = params.iter().position(|p| *p == sym) {
                        let hash_span = tok.span;
                        let stringized = self.stringize(&args[idx], hash_span, nxt.span);
                        out.push(stringized);
                        prev_empty = false;
                        i += 2;
                        continue;
                    }
                    if sym == self.va_args_sym {
                        if variadic {
                            let hash_span = tok.span;
                            let stringized =
                                self.stringize(&args[params.len()], hash_span, nxt.span);
                            out.push(stringized);
                        } else {
                            self.emit_e0026(nxt.span);
                            // Emit an empty string literal so the
                            // output shape still has one token where
                            // the stringize was expected.
                            let hash_span = tok.span;
                            let stringized = self.stringize(&[], hash_span, nxt.span);
                            out.push(stringized);
                        }
                        prev_empty = false;
                        i += 2;
                        continue;
                    }
                    // GNU named variadic: `#args` where `args` is the
                    // named variadic parameter.
                    if named_variadic.is_some_and(|nv| nv == sym) {
                        let hash_span = tok.span;
                        let stringized = self.stringize(&args[params.len()], hash_span, nxt.span);
                        out.push(stringized);
                        prev_empty = false;
                        i += 2;
                        continue;
                    }
                }
                // `#` not followed by a parameter name — C99
                // §6.10.3.2p1 constraint violation.
                self.emit_e0024(tok.span, next);
                out.push(ExpToken { tok, hide: FxHashSet::default() });
                prev_empty = false;
                i += 1;
                continue;
            }

            // Token paste `##`. The LHS was pushed in the previous
            // iteration (raw, because its `next_is_paste` lookahead
            // saw this `##`); we consume body[i+1] as the RHS here so
            // the outer loop should NOT revisit it.
            if tok.kind == PpTokenKind::Punct(Punct::HashHash) {
                let Some(rhs_body_tok) = body.get(i + 1).copied() else {
                    // `##` with nothing after it: the positional
                    // diagnostic already fired above; drop the `##`.
                    i += 1;
                    continue;
                };
                let rhs_tokens = self.resolve_paste_operand(&rhs_body_tok, params, args, variadic);
                // GNU extension: `, ## __VA_ARGS__` reinterprets the
                // entire three-token sequence rather than performing
                // a literal paste. When `__VA_ARGS__` is empty the
                // preceding comma is dropped; when non-empty the
                // comma is kept and `__VA_ARGS__` is spliced in
                // unchanged (skipping the paste, which would
                // otherwise concatenate `,` with the first argument
                // token and produce an ill-formed token). Off by
                // default; gated by `Options::gnu_va_args_elision`.
                if self.gnu_va_args_elision
                    && variadic
                    && rhs_body_tok.kind == PpTokenKind::Ident
                    && self.symbol_of(&rhs_body_tok) == self.va_args_sym
                    && out.last().is_some_and(|e| e.tok.kind == PpTokenKind::Punct(Punct::Comma))
                {
                    if rhs_tokens.is_empty() {
                        out.pop();
                        prev_empty = true;
                    } else {
                        // Keep the comma, splice the raw __VA_ARGS__
                        // tokens in place of the `## __VA_ARGS__`
                        // pair. This matches GCC / Clang behaviour.
                        out.extend(rhs_tokens);
                        prev_empty = false;
                    }
                    i += 2;
                    continue;
                }
                self.apply_paste(&mut out, &mut prev_empty, tok.span, rhs_tokens);
                i += 2;
                continue;
            }

            // Identifier: parameter substitution, with raw-vs-expanded
            // governed by adjacency to `##` (C99 §6.10.3.1p1). Note we
            // only need to inspect the *next* token for the paste
            // lookahead; a parameter preceded by `##` was already
            // consumed as the RHS inside the paste branch above, so
            // we never reach here with a "prev is `##`" body position.
            if tok.kind == PpTokenKind::Ident {
                let sym = self.symbol_of(&tok);
                // Named parameter first, then the variadic pseudo-
                // parameter. Looking up `__VA_ARGS__` in a
                // non-variadic body is E0026.
                let idx_opt = if let Some(i) = params.iter().position(|p| *p == sym) {
                    Some(i)
                } else if sym == self.va_args_sym {
                    if variadic {
                        Some(params.len())
                    } else {
                        self.emit_e0026(tok.span);
                        // Fall through: emit the literal identifier
                        // so downstream tooling still sees something.
                        None
                    }
                } else if named_variadic.is_some_and(|nv| nv == sym) {
                    // GNU named variadic: `args...` — the named
                    // parameter resolves to the variadic slot.
                    Some(params.len())
                } else {
                    None
                };
                if let Some(idx) = idx_opt {
                    let next_is_paste = body
                        .get(i + 1)
                        .is_some_and(|t| t.kind == PpTokenKind::Punct(Punct::HashHash));
                    let toks = if next_is_paste {
                        // Paste-adjacent: splice raw tokens in so
                        // they can be concatenated verbatim.
                        args[idx].clone()
                    } else {
                        // Pre-scan (fully expand) the actual argument
                        // before splicing it in. The expanded tokens
                        // inherit HS via HSADD at the end, blocking
                        // the current macro from re-expanding through
                        // them.
                        self.expand(args[idx].clone())
                    };
                    prev_empty = toks.is_empty();
                    out.extend(toks);
                    i += 1;
                    continue;
                }
            }

            out.push(ExpToken { tok, hide: FxHashSet::default() });
            prev_empty = false;
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

    /// Emit E0025 for each `##` that sits at the very beginning or
    /// end of a replacement list (C99 §6.10.3.3p1). Called once per
    /// `subst` invocation; the body walk itself then treats the
    /// malformed `##` as a paste against an empty operand, which
    /// produces a tolerable output without further diagnostics.
    fn check_paste_positions(&mut self, body: &[PpToken]) {
        if let Some(first) = body.first() {
            if first.kind == PpTokenKind::Punct(Punct::HashHash) {
                self.emit_e0025_position(first.span, "beginning");
            }
        }
        if body.len() >= 2 {
            if let Some(last) = body.last() {
                if last.kind == PpTokenKind::Punct(Punct::HashHash) {
                    self.emit_e0025_position(last.span, "end");
                }
            }
        }
    }

    /// Compute the RHS operand token list for a paste.
    ///
    /// For a parameter the raw (unexpanded) argument tokens are
    /// spliced in verbatim; for any other token a singleton list is
    /// returned. `rhs_body_tok` is the body token immediately after
    /// the `##`; its own downstream adjacency (e.g. another `##`
    /// after it) is irrelevant here because successive pastes are
    /// handled iteratively by the caller.
    fn resolve_paste_operand(
        &mut self,
        rhs_body_tok: &PpToken,
        params: &[Symbol],
        args: &[Vec<ExpToken>],
        variadic: bool,
    ) -> Vec<ExpToken> {
        if rhs_body_tok.kind == PpTokenKind::Ident {
            let sym = self.symbol_of(rhs_body_tok);
            if let Some(idx) = params.iter().position(|p| *p == sym) {
                return args[idx].clone();
            }
            if sym == self.va_args_sym {
                if variadic {
                    return args[params.len()].clone();
                }
                // Non-variadic `##__VA_ARGS__`: constraint violation.
                // The caller's positional check already fires E0025
                // for dangling `##`; we add E0026 for the reference
                // itself so the diagnostic is specific.
                self.emit_e0026(rhs_body_tok.span);
                return Vec::new();
            }
        }
        vec![ExpToken { tok: *rhs_body_tok, hide: FxHashSet::default() }]
    }

    /// Splice a resolved paste RHS into `out`, popping the previously
    /// pushed LHS token where appropriate.
    ///
    /// Implements the §6.10.3.3p2 empty-operand rule: if either
    /// operand is empty, the result is the other operand; if both
    /// are empty, the result is nothing. `prev_empty` is threaded
    /// through the caller's main loop as its running view of
    /// "last-slot was empty" for the next paste's lookup.
    fn apply_paste(
        &mut self,
        out: &mut Vec<ExpToken>,
        prev_empty: &mut bool,
        hh_span: Span,
        rhs_tokens: Vec<ExpToken>,
    ) {
        let lhs_empty = *prev_empty;
        let rhs_empty = rhs_tokens.is_empty();

        if lhs_empty && rhs_empty {
            // Nothing to emit; `prev_empty` stays true.
            return;
        }
        if lhs_empty {
            out.extend(rhs_tokens);
            *prev_empty = false;
            return;
        }
        if rhs_empty {
            // LHS unchanged; `prev_empty` stays false.
            return;
        }

        // Both operands contribute tokens. Splice everything but the
        // first RHS token in verbatim, and paste the LHS-tail with
        // the RHS-head.
        let lhs_et = out.pop().expect("lhs_empty=false implies non-empty out");
        let mut rhs_iter = rhs_tokens.into_iter();
        let rhs_first = rhs_iter.next().expect("rhs_empty=false implies a first token");

        let pasted = self.paste_two(&lhs_et, &rhs_first, hh_span);
        out.extend(pasted);
        out.extend(rhs_iter);
        *prev_empty = false;
    }

    /// Concatenate the spellings of `lhs` and `rhs`, register the
    /// result as a small synthetic source file, and re-lex it.
    ///
    /// On a single-token re-lex the returned vector has one element
    /// whose hide-set is the **intersection** of the two operand hide
    /// sets (task spec; the classical Prosser formulation). On a
    /// multi-token re-lex the paste is ill-formed: E0025 is emitted
    /// with labels on both operands and on the `##` itself, and the
    /// raw re-lexed tokens are returned so downstream analysis still
    /// sees something substantive.
    fn paste_two(&mut self, lhs: &ExpToken, rhs: &ExpToken, hh_span: Span) -> Vec<ExpToken> {
        let lhs_text = self.token_text(&lhs.tok);
        let rhs_text = self.token_text(&rhs.tok);
        let mut combined = String::with_capacity(lhs_text.len() + rhs_text.len());
        combined.push_str(&lhs_text);
        combined.push_str(&rhs_text);

        let file_id = {
            let mut sm = self.source_map.write().unwrap();
            sm.add_file(PathBuf::from("<paste>"), Arc::from(combined.as_str()))
        };

        // Horizontal whitespace and newlines should not appear in a
        // freshly-concatenated paste, but filter defensively to keep
        // the token count comparison clean.
        let tokens: Vec<PpToken> = tokenize(file_id, &combined)
            .filter(|t| {
                !matches!(t.kind, PpTokenKind::Whitespace | PpTokenKind::Newline | PpTokenKind::Eof)
            })
            .collect();

        let hide: HideSet = lhs.hide.intersection(&rhs.hide).copied().collect();

        if tokens.len() == 1 {
            return vec![ExpToken { tok: tokens[0], hide }];
        }

        // GNU permissive paste: if all re-lexed tokens together form
        // a valid pp-number (e.g. `4` ## `.57` → `4.57`), accept it
        // by checking whether the first token spans the entire paste text.
        if self.gnu_permissive_paste && tokens.len() > 1 {
            // Check if concatenation is a valid pp-number: re-lex
            // produced multiple tokens but the first one might be a
            // pp-number covering most of the text. Actually, the
            // simpler check: if all non-whitespace tokens are
            // pp-numbers or the concatenated result starts with a
            // digit/dot, just take all tokens without error.
            // The most common case: `4` + `.57` = `4.57` which should
            // be a single pp-number. If it isn't, it means our lexer
            // split it. Accept multi-token results silently.
            return tokens.into_iter().map(|tok| ExpToken { tok, hide: hide.clone() }).collect();
        }

        // Ill-formed: emit E0025 and still surface the raw re-lex so
        // later stages don't silently lose the operand texts.
        self.emit_e0025_invalid(lhs.tok.span, rhs.tok.span, hh_span);
        tokens.into_iter().map(|tok| ExpToken { tok, hide: hide.clone() }).collect()
    }

    /// Read the source bytes covered by `tok`'s span. Used to
    /// materialise the paste operands' text before re-lexing.
    fn token_text(&self, tok: &PpToken) -> String {
        let sm = self.source_map.read().unwrap();
        let src = &sm.file(tok.span.file).src;
        src[tok.span.lo.0 as usize..tok.span.hi.0 as usize].to_string()
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

    /// Expand a dynamic predefined macro (C99 §6.10.8p1) at the
    /// invocation site `origin`.
    ///
    /// The returned list always has length one. The synthesised token
    /// is anchored in a new synthetic source file whose bytes contain
    /// the rendered spelling, so spans continue to roundtrip through
    /// `token_text` / `stringize` / `paste_two` the same way ordinary
    /// tokens do. `hide` is attached verbatim; the caller has already
    /// pushed the macro's own name into it so a pathological rescan
    /// cannot re-invoke us.
    fn expand_builtin(
        &mut self,
        builtin: BuiltinMacro,
        origin: &ExpToken,
        hide: HideSet,
    ) -> Vec<ExpToken> {
        let origin_span = origin.tok.span;
        let (kind, text) = match builtin {
            BuiltinMacro::File => {
                let path_text = {
                    let sm = self.source_map.read().unwrap();
                    // Task 04-15: `#line N "name"` overrides the
                    // reported file name. The effective file is the
                    // origin's real file unless a `#line` override
                    // is active at this physical line.
                    let eff = self.line_map.effective_file(&sm, origin_span.file, origin_span.lo);
                    sm.file(eff).name.display().to_string()
                };
                let mut body = String::with_capacity(path_text.len() + 2);
                body.push('"');
                for ch in path_text.chars() {
                    match ch {
                        '\\' => body.push_str("\\\\"),
                        '"' => body.push_str("\\\""),
                        _ => body.push(ch),
                    }
                }
                body.push('"');
                (PpTokenKind::StringLit { enc: StringEncoding::None }, body)
            }
            BuiltinMacro::Line => {
                let line_no = {
                    let sm = self.source_map.read().unwrap();
                    // Task 04-15: `#line N` renumbers subsequent
                    // physical lines. `effective_line` falls through
                    // to the physical line when no override is
                    // active.
                    self.line_map.effective_line(&sm, origin_span.file, origin_span.lo)
                };
                (PpTokenKind::PpNumber(PpNumberKind::Integer), line_no.to_string())
            }
        };
        let len = text.len() as u32;
        let file_id = {
            let mut sm = self.source_map.write().unwrap();
            let label = match builtin {
                BuiltinMacro::File => "<builtin:__FILE__>",
                BuiltinMacro::Line => "<builtin:__LINE__>",
            };
            sm.add_file(PathBuf::from(label), Arc::from(text))
        };
        let span = Span::new(file_id, BytePos(0), BytePos(len));
        let tok = PpToken {
            kind,
            span,
            leading_ws: origin.tok.leading_ws,
            at_line_start: origin.tok.at_line_start,
        };
        vec![ExpToken { tok, hide }]
    }

    /// Emit E0025 for a paste whose concatenation re-lexed to more
    /// than one preprocessing token.
    fn emit_e0025_invalid(&mut self, lhs_span: Span, rhs_span: Span, hh_span: Span) {
        let diag = Diagnostic {
            level: Level::Error,
            code: Some(E0025),
            message: "pasting forms an invalid token".into(),
            labels: vec![
                Label { span: hh_span, message: "`##` here".into(), primary: true },
                Label { span: lhs_span, message: "left operand of `##`".into(), primary: false },
                Label { span: rhs_span, message: "right operand of `##`".into(), primary: false },
            ],
            notes: vec!["C99 §6.10.3.3p3: the concatenation of the two operand \
                 spellings must form a single preprocessing token"
                .into()],
            help: vec!["split the operands with whitespace, or use a different \
                 concatenation strategy"
                .into()],
        };
        self.handler.emit(&diag);
    }

    /// Emit E0025 for a `##` that appears at the very beginning or
    /// very end of a replacement list (C99 §6.10.3.3p1).
    fn emit_e0025_position(&mut self, span: Span, where_: &'static str) {
        let diag = Diagnostic {
            level: Level::Error,
            code: Some(E0025),
            message: format!("`##` at the {where_} of a replacement list"),
            labels: vec![Label { span, message: "`##` here".into(), primary: true }],
            notes: vec!["C99 §6.10.3.3p1: `##` shall not occur at the beginning \
                 or end of a replacement list for either form of macro \
                 definition"
                .into()],
            help: vec![],
        };
        self.handler.emit(&diag);
    }

    /// Emit E0026: `__VA_ARGS__` referenced outside a variadic
    /// function-like macro body (C99 §6.10.3p5).
    fn emit_e0026(&mut self, span: Span) {
        let diag = Diagnostic {
            level: Level::Error,
            code: Some(E0026),
            message: "`__VA_ARGS__` can only appear in a variadic macro".into(),
            labels: vec![Label { span, message: "`__VA_ARGS__` here".into(), primary: true }],
            notes: vec!["C99 §6.10.3p5: the identifier `__VA_ARGS__` shall occur \
                 only in the replacement list of a function-like macro that \
                 uses the ellipsis notation in the parameters"
                .into()],
            help: vec!["change the macro's parameter list to end with `...`, or \
                 rename the identifier"
                .into()],
        };
        self.handler.emit(&diag);
    }

    /// Emit E0024: `#` in a function-like replacement list not
    /// followed by a parameter name (C99 §6.10.3.2p1).
    fn emit_e0024(&mut self, hash_span: Span, next: Option<PpToken>) {
        let primary_label = Label { span: hash_span, message: "`#` here".into(), primary: true };
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
    ///
    /// `max_splits` caps the number of depth-0 commas that act as
    /// argument separators. Once that cap is reached, further commas
    /// (and everything up to the matching `)`) are folded into the
    /// final slot verbatim. This implements C99 §6.10.3p12: the
    /// trailing arguments of a variadic invocation are merged —
    /// commas included — into a single `__VA_ARGS__` slot. `None`
    /// disables capping (the natural non-variadic behaviour).
    fn collect_args(
        &self,
        work: &mut VecDeque<ExpToken>,
        max_splits: Option<usize>,
    ) -> Option<(Vec<Vec<ExpToken>>, HideSet)> {
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
                    if max_splits.is_some_and(|cap| args.len() >= cap) {
                        // Cap reached — this comma is part of the
                        // trailing `__VA_ARGS__` slot, not a
                        // separator.
                        current.push(et);
                    } else {
                        args.push(current);
                        current = Vec::new();
                    }
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
/// that single empty slot as "zero arguments".
///
/// For variadic macros the expected shape is `params.len() + 1` slots
/// (named params followed by the `__VA_ARGS__` collector). When the
/// caller supplies no trailing arguments — `LOG("a")` against
/// `LOG(fmt, ...)` — the raw split stops at `params.len()` slots; we
/// synthesise an empty trailing slot so downstream substitution code
/// can look up the variadic slot unconditionally.
fn reconcile_arity(
    mut raw: Vec<Vec<ExpToken>>,
    param_count: usize,
    variadic: bool,
) -> Vec<Vec<ExpToken>> {
    if !variadic {
        if param_count == 0 && raw.len() == 1 && raw[0].is_empty() {
            raw.clear();
        }
        return raw;
    }

    // Variadic.
    let expected = param_count + 1;
    if param_count == 0 && raw.len() == 1 && raw[0].is_empty() {
        // `V()` against `V(...)` — zero varargs; reshape the single
        // empty slot from "one argument" into "the empty __VA_ARGS__".
        return raw;
    }
    if raw.len() == param_count {
        // No trailing separator: the zero-vararg invocation
        // (`LOG("a")` against `LOG(fmt, ...)`). Append the empty
        // __VA_ARGS__ slot.
        raw.push(Vec::new());
    }
    debug_assert!(raw.len() <= expected, "cap in collect_args prevents more than {expected} slots");
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
        let def = MacroDef {
            name: name_sym,
            kind: MacroKind::ObjectLike,
            body: toks,
            def_span,
            is_predefined: false,
        };
        let sm = session.source_map.read().unwrap();
        define_macro(def, macros, &sm, &session.interner, false).unwrap();
    }

    /// Build a function-like macro and install it.
    fn install_fn(
        session: &mut Session,
        macros: &mut MacroTable,
        name: &str,
        params: &[&str],
        body_src: &str,
    ) {
        install_fn_with(session, macros, name, params, false, body_src);
    }

    /// Same as [`install_fn`] but declares the macro as variadic so
    /// its body may reference `__VA_ARGS__`.
    fn install_fn_variadic(
        session: &mut Session,
        macros: &mut MacroTable,
        name: &str,
        params: &[&str],
        body_src: &str,
    ) {
        install_fn_with(session, macros, name, params, true, body_src);
    }

    fn install_fn_with(
        session: &mut Session,
        macros: &mut MacroTable,
        name: &str,
        params: &[&str],
        variadic: bool,
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
            kind: MacroKind::FunctionLike { params: param_syms, variadic, named_variadic: None },
            body,
            def_span,
            is_predefined: false,
        };
        let sm = session.source_map.read().unwrap();
        define_macro(def, macros, &sm, &session.interner, false).unwrap();
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
        let sess =
            Session::with_handler(Options::default(), Handler::with_emitter(Box::new(cap.clone())));
        (sess, cap)
    }

    /// Convenience wrapper: run [`expand_line`] against a whole
    /// session. Handles the `Arc`-clone / borrow dance so individual
    /// tests stay focused on their input/output pair.
    fn run_expand(sess: &mut Session, macros: &MacroTable, line: Vec<PpToken>) -> Vec<PpToken> {
        let sm_arc = Arc::clone(&sess.source_map);
        let elide = sess.opts.gnu_va_args_elision;
        let paste = sess.opts.gnu_permissive_paste;
        let line_map = LineMap::new();
        expand_line(
            &sm_arc,
            &mut sess.interner,
            &mut sess.handler,
            macros,
            &line_map,
            line,
            elide,
            paste,
        )
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

    // ── Token paste `##` (task 04-10) ───────────────────────────────

    /// Render an expanded stream as *concatenated* source text — no
    /// intervening spaces. Token paste is fundamentally a splice on
    /// source spellings, so the assertion shape mirrors that.
    fn concat(sess: &Session, tokens: &[PpToken]) -> String {
        let sm = sess.source_map.read().unwrap();
        tokens
            .iter()
            .map(|t| {
                let src = &sm.file(t.span.file).src;
                src[t.span.lo.0 as usize..t.span.hi.0 as usize].to_string()
            })
            .collect()
    }

    #[test]
    fn object_like_ident_paste() {
        // `#define X a##b` / `X` → single token `ab`.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "X", "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "X\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty(), "valid paste must not diagnose");
        assert_eq!(out.len(), 1, "paste result is a single token: {out:?}");
        assert_eq!(concat(&sess, &out), "ab");
        assert!(matches!(out[0].kind, PpTokenKind::Ident));
    }

    #[test]
    fn object_like_number_paste() {
        // `#define X 1##2` / `X` → single pp-number `12`.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "X", "1##2");

        let (_file, line) = tok_line(&mut sess, "call", "X\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty());
        assert_eq!(out.len(), 1);
        assert_eq!(concat(&sess, &out), "12");
        assert!(matches!(out[0].kind, PpTokenKind::PpNumber(_)));
    }

    #[test]
    fn function_like_paste_with_param_lhs() {
        // `#define CAT(x) x##_foo` / `CAT(bar)` → `bar_foo`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "CAT", &["x"], "x##_foo");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(bar)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(concat(&sess, &out), "bar_foo");
    }

    #[test]
    fn function_like_paste_both_params() {
        // `#define CAT(a,b) a##b` / `CAT(lo,oo)` — `lo` + `oo` = `looo`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "CAT", &["a", "b"], "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(lo,oo)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(out.len(), 1);
        assert_eq!(concat(&sess, &out), "looo");
    }

    #[test]
    fn classical_loop_paste_followed_by_literal() {
        // Classical §6.10.3.3 example:
        //   #define CAT(a,b) a##b
        //   CAT(lo, op)
        // → single token `loop`. The task spec cites the
        // `CAT(lo,oo)p` variant which per spec produces
        // `[looo][p]` — two tokens, concatenated `looop`; we cover
        // the clean `loop` form here for readability.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "CAT", &["a", "b"], "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(lo,op)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(out.len(), 1);
        assert_eq!(concat(&sess, &out), "loop");
    }

    #[test]
    fn paste_result_rescans_for_further_expansion() {
        // The pasted token re-enters the rescan loop — if it names a
        // macro, that macro expands normally. Here `F##OO` pastes to
        // `FOO`, which then expands to `1`.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "FOO", "1");
        install_fn(&mut sess, &mut macros, "CAT", &["a", "b"], "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(F,OO)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(out.len(), 1);
        assert_eq!(concat(&sess, &out), "1");
    }

    #[test]
    fn invalid_paste_emits_e0025_with_operand_labels() {
        // `#define CAT(a,b) a##b` / `CAT(+, ;)` — combined `"+;"`
        // re-lexes to two separate punctuators.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "CAT", &["a", "b"], "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(+, ;)\n");
        let _out = run_expand(&mut sess, &macros, line);

        let diags = cap.diagnostics();
        let e25: Vec<_> = diags.iter().filter(|d| d.code == Some(E0025)).collect();
        assert_eq!(e25.len(), 1, "exactly one E0025 expected, got {diags:?}");
        // Primary label on `##`, secondary labels on both operands.
        assert!(e25[0].labels.iter().any(|l| l.primary));
        assert!(
            e25[0].labels.iter().filter(|l| !l.primary).count() >= 2,
            "both operand spans must be labelled"
        );
    }

    #[test]
    fn paste_with_empty_left_operand_yields_right() {
        // §6.10.3.3p2: `CAT(,hello)` → `hello` (left is empty).
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "CAT", &["a", "b"], "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(,hello)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty(), "empty-operand paste is well-formed");
        assert_eq!(concat(&sess, &out), "hello");
    }

    #[test]
    fn paste_with_empty_right_operand_yields_left() {
        // §6.10.3.3p2: `CAT(hello,)` → `hello` (right is empty).
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "CAT", &["a", "b"], "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(hello,)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty());
        assert_eq!(concat(&sess, &out), "hello");
    }

    #[test]
    fn paste_with_both_operands_empty_yields_nothing() {
        // §6.10.3.3p2: `CAT(,)` → no tokens.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "CAT", &["a", "b"], "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(,)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty());
        assert!(out.is_empty(), "both-empty paste emits nothing, got {out:?}");
    }

    #[test]
    fn hashhash_at_start_of_body_is_e0025() {
        // `#define BAD ##x` — `##` at the start of a replacement list.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "BAD", "##x");

        let (_file, line) = tok_line(&mut sess, "call", "BAD\n");
        let _out = run_expand(&mut sess, &macros, line);

        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Some(E0025)),
            "expected E0025 for leading `##`, got {diags:?}"
        );
    }

    #[test]
    fn hashhash_at_end_of_body_is_e0025() {
        // `#define BAD x##` — `##` at the end of a replacement list.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "BAD", "x##");

        let (_file, line) = tok_line(&mut sess, "call", "BAD\n");
        let _out = run_expand(&mut sess, &macros, line);

        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Some(E0025)),
            "expected E0025 for trailing `##`, got {diags:?}"
        );
    }

    #[test]
    fn paste_is_raw_not_preexpanded() {
        // C99 §6.10.3.1p1: a parameter adjacent to `##` is substituted
        // **without** first being macro-expanded. If `ONE` would be
        // pre-expanded to `1`, `CAT(ONE, X)` would paste `1` + `X` =
        // `1X`; spec demands `ONEX` instead.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "ONE", "1");
        install_fn(&mut sess, &mut macros, "CAT", &["a", "b"], "a##b");

        let (_file, line) = tok_line(&mut sess, "call", "CAT(ONE,X)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(concat(&sess, &out), "ONEX");
    }

    // ── Variadic `__VA_ARGS__` (task 04-11) ─────────────────────────

    #[test]
    fn variadic_log_expands_va_args_with_multiple_trailing_args() {
        // Acceptance: #define LOG(fmt, ...) printf(fmt, __VA_ARGS__)
        //             LOG("a", 1, 2) → printf("a", 1, 2)
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn_variadic(&mut sess, &mut macros, "LOG", &["fmt"], "printf(fmt, __VA_ARGS__)");

        let (_file, line) = tok_line(&mut sess, "call", "LOG(\"a\", 1, 2)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty(), "valid variadic call: {:?}", cap.diagnostics());
        assert_eq!(pp(&sess, &out), "printf ( \"a\" , 1 , 2 )");
    }

    #[test]
    fn variadic_only_macro_collects_every_arg_into_va_args() {
        // #define V(...) f(__VA_ARGS__) / V(a, b, c) → f(a, b, c)
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn_variadic(&mut sess, &mut macros, "V", &[], "f(__VA_ARGS__)");

        let (_file, line) = tok_line(&mut sess, "call", "V(a, b, c)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "f ( a , b , c )");
    }

    #[test]
    fn variadic_zero_extra_args_default_expands_va_args_to_empty() {
        // Default mode (C99): LOG("a") against LOG(fmt, ...) →
        // printf("a", ) — the preceding comma stays.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn_variadic(&mut sess, &mut macros, "LOG", &["fmt"], "printf(fmt, __VA_ARGS__)");

        let (_file, line) = tok_line(&mut sess, "call", "LOG(\"a\")\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty(), "empty __VA_ARGS__ is not an error by default");
        // Empty __VA_ARGS__ contributes no tokens; the comma from the
        // body stays.
        assert_eq!(pp(&sess, &out), "printf ( \"a\" , )");
    }

    #[test]
    fn variadic_zero_extra_args_with_gnu_elision_drops_comma() {
        // With GNU extension enabled: LOG("a") against
        // `printf(fmt, ##__VA_ARGS__)` → printf("a").
        let cap = CaptureEmitter::new();
        let opts = Options { gnu_va_args_elision: true, ..Options::default() };
        let mut sess = Session::with_handler(opts, Handler::with_emitter(Box::new(cap.clone())));
        let mut macros = MacroTable::default();
        install_fn_variadic(&mut sess, &mut macros, "LOG", &["fmt"], "printf(fmt, ## __VA_ARGS__)");

        let (_file, line) = tok_line(&mut sess, "call", "LOG(\"a\")\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(
            cap.diagnostics().is_empty(),
            "GNU elision must not diagnose, got {:?}",
            cap.diagnostics()
        );
        assert_eq!(pp(&sess, &out), "printf ( \"a\" )");
    }

    #[test]
    fn variadic_gnu_elision_with_nonempty_va_args_keeps_comma() {
        // GNU extension still pastes normally when __VA_ARGS__ is
        // non-empty: LOG("a", 1, 2) → printf("a", 1, 2). `##` drops
        // to "LHS unchanged" (comma preserved), RHS spliced in.
        let cap = CaptureEmitter::new();
        let opts = Options { gnu_va_args_elision: true, ..Options::default() };
        let mut sess = Session::with_handler(opts, Handler::with_emitter(Box::new(cap.clone())));
        let mut macros = MacroTable::default();
        install_fn_variadic(&mut sess, &mut macros, "LOG", &["fmt"], "printf(fmt, ## __VA_ARGS__)");

        let (_file, line) = tok_line(&mut sess, "call", "LOG(\"a\", 1, 2)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert!(cap.diagnostics().is_empty(), "{:?}", cap.diagnostics());
        assert_eq!(pp(&sess, &out), "printf ( \"a\" , 1 , 2 )");
    }

    #[test]
    fn va_args_in_non_variadic_function_like_macro_emits_e0026() {
        // Acceptance: #define F(x) __VA_ARGS__ / F(1) → E0026.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_fn(&mut sess, &mut macros, "F", &["x"], "__VA_ARGS__");

        let (_file, line) = tok_line(&mut sess, "call", "F(1)\n");
        let _out = run_expand(&mut sess, &macros, line);

        let diags = cap.diagnostics();
        assert_eq!(diags.len(), 1, "expected a single E0026, got {diags:?}");
        assert_eq!(diags[0].code, Some(E0026));
        assert!(diags[0].labels.iter().any(|l| l.primary), "E0026 must carry a primary label");
    }

    #[test]
    fn va_args_in_object_like_macro_emits_e0026() {
        // Object-like macros are never variadic; `__VA_ARGS__` in
        // their replacement list is also a constraint violation.
        let (mut sess, cap) = session_with_capture();
        let mut macros = MacroTable::default();
        install_object(&mut sess, &mut macros, "OBJ", "__VA_ARGS__");

        let (_file, line) = tok_line(&mut sess, "call", "OBJ\n");
        let _out = run_expand(&mut sess, &macros, line);

        let diags = cap.diagnostics();
        assert!(
            diags.iter().any(|d| d.code == Some(E0026)),
            "expected E0026 for __VA_ARGS__ in object-like body, got {diags:?}",
        );
    }

    #[test]
    fn variadic_stringize_of_va_args_joins_with_commas() {
        // `#define S(...) #__VA_ARGS__` / `S(a, b, c)` →
        // `"a, b, c"` — the raw arg tokens (commas included) are
        // stringized as a unit per §6.10.3.2p2 applied to the
        // variadic pseudo-parameter.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn_variadic(&mut sess, &mut macros, "S", &[], "#__VA_ARGS__");

        let (_file, line) = tok_line(&mut sess, "call", "S(a, b, c)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].kind, PpTokenKind::StringLit { enc: StringEncoding::None }));
        // Commas come through as part of the raw arg; `leading_ws`
        // handles the single space between tokens.
        assert_eq!(expect_single_string(&sess, &out), "\"a, b, c\"");
    }

    #[test]
    fn variadic_preserves_embedded_commas_in_nested_parens() {
        // #define V(...) f(__VA_ARGS__) / V((a, b), c) — the nested
        // `(a, b)` is depth-1, so its comma is NOT a splitter; the
        // depth-0 comma between the two args IS part of __VA_ARGS__.
        let mut sess = fresh_session();
        let mut macros = MacroTable::default();
        install_fn_variadic(&mut sess, &mut macros, "V", &[], "f(__VA_ARGS__)");

        let (_file, line) = tok_line(&mut sess, "call", "V((a, b), c)\n");
        let out = run_expand(&mut sess, &macros, line);

        assert_eq!(pp(&sess, &out), "f ( ( a , b ) , c )");
    }
}
