//! Integer constant expression evaluator for `#if` / `#elif`
//! controlling expressions (C99 §6.10.1).
//!
//! The evaluator is deliberately small and self-contained — it shares
//! no code with the eventual C expression parser in `rcc_parse`
//! because `#if` is a distinct language:
//!
//! - **No floats, casts, or pointers.** §6.10.1p4 forbids these
//!   entirely. Only integer constants, identifiers, character
//!   constants, and the listed operators may appear.
//! - **`defined NAME` / `defined(NAME)`** are evaluated *before* macro
//!   expansion (§6.10.1p1): the operator inspects the macro table
//!   using the raw spelling of its operand so a definition of `NAME`
//!   as `0` doesn't cause `defined NAME` to read as `defined 0`.
//! - **Post-expansion identifiers are `0`.** §6.10.1p4: after macro
//!   expansion, every remaining identifier (or keyword) in the
//!   expression is replaced with the pp-number `0`. `true`, `false`,
//!   `bool`, user-defined type names — all of them become `0` in a
//!   `#if` context.
//! - **Arithmetic is 128-bit.** §6.10.1p4 requires the evaluator to
//!   use the widest integer types implementable; we promote to
//!   `i128` / `u128` based on the operand's integer suffix (`u`/`U`
//!   → unsigned). This is wider than any type the target can ever
//!   hold, so wrap-around and overflow semantics match the standard's
//!   "as-if" model for `intmax_t` / `uintmax_t`.
//!
//! Division or remainder by zero in a *live* branch is E0028. Since
//! task 04-14 (conditional stack) has not landed yet the "live"
//! predicate is trivially true — any `#if` expression we parse here
//! is being evaluated for a branch we must materialise. When 04-14
//! arrives the Preprocessor::run call site gates this evaluator
//! behind the live-branch check; the evaluator itself does not need
//! to know.

use std::cell::Cell;
use std::sync::{Arc, RwLock};

use rcc_errors::{codes::E0028, Diagnostic, Handler, Label, Level};
use rcc_lexer::{PpToken, PpTokenKind, Punct};
use rcc_span::{BytePos, Interner, SourceMap, Span};

use crate::expand::expand_line;
use crate::line_map::LineMap;
use crate::macros::MacroTable;

/// Callback used by `__has_include` while evaluating `#if` / `#elif`.
///
/// The evaluator owns no include-search state; callers provide this
/// side-effect-free probe so the expression parser stays independent
/// from filesystem policy.
pub type HasIncludeProbe<'a> = dyn FnMut(&str, bool, Span) -> bool + 'a;

/// Optional extension hooks for `#if` / `#elif` evaluation.
pub struct EvalOptions<'probe, 'counter> {
    /// Enable GNU comma elision for variadic macro expansion while
    /// expanding the controlling expression.
    pub gnu_va_args_elision: bool,
    /// Probe for `__has_include`. `None` makes the operator evaluate to
    /// `0`, useful for direct unit tests of the core evaluator.
    pub has_include: Option<&'probe mut HasIncludeProbe<'probe>>,
    /// Shared expansion state for the `__COUNTER__` predefined macro.
    pub counter: Option<&'counter Cell<u32>>,
}

impl EvalOptions<'_, '_> {
    /// Strict C99 evaluation with no extension probes.
    pub fn strict() -> Self {
        Self { gnu_va_args_elision: false, has_include: None, counter: None }
    }
}

/// Evaluate the controlling expression of a `#if` or `#elif`
/// directive and return its integer value as `i128`.
///
/// The return type is `i128` by convention — callers use only the
/// zero / non-zero bit to decide whether the branch is taken — but
/// internally the evaluator tracks both a full 128-bit bit pattern
/// and a signedness flag so unsigned wrap-around is exact (e.g.
/// `1u - 2u > 0` is true because the difference wraps to `u128::MAX`).
/// When the final result is unsigned, the bits are reinterpreted as
/// `i128` (the `as` cast); the non-zero bit is preserved, which is
/// all the caller observes.
///
/// `tokens` is the raw directive tail as produced by
/// [`crate::directive::Directive::Conditional`] — i.e. no leading
/// `#`, no directive name, no trailing `Newline`. It is inspected
/// twice: once pre-expansion to resolve `defined` operators, then
/// once post-expansion to parse the arithmetic.
pub fn eval_if(
    tokens: &[PpToken],
    source_map: &Arc<RwLock<SourceMap>>,
    interner: &mut Interner,
    handler: &mut Handler,
    macros: &MacroTable,
    line_map: &LineMap,
    mut options: EvalOptions<'_, '_>,
) -> Result<i128, Diagnostic> {
    // Span used for diagnostics pointing at "the whole expression"
    // when no specific token is to blame (e.g. empty condition).
    let whole_span = whole_expression_span(tokens);

    // Synthetic source file holding the literal strings `"0"` and `"1"`.
    // Every "identifier → 0" substitution and every `defined` answer
    // reuses spans into this file so downstream passes (including the
    // integer literal parser below) can read a real source slice.
    let zero_one = register_zero_one_file(source_map);

    // Step 1: fold pre-expansion-only operators before expansion.
    let pre_expansion = resolve_defined(tokens, source_map, macros, interner, zero_one)?;
    let pre_expansion = match options.has_include.as_mut() {
        Some(probe) => {
            resolve_has_include(&pre_expansion, source_map, zero_one, Some(&mut **probe))?
        }
        None => resolve_has_include(&pre_expansion, source_map, zero_one, None)?,
    };

    // Step 2: run the ordinary macro-expansion pipeline.
    let expanded = expand_line(
        source_map,
        interner,
        handler,
        macros,
        line_map,
        pre_expansion,
        options.gnu_va_args_elision,
        false,
        options.counter,
    );

    // Step 3: replace any remaining identifier / keyword with `0`.
    let numeric = identifiers_to_zero(expanded, zero_one);

    // Step 4: parse and evaluate.
    let mut p = Parser::new(&numeric, source_map, whole_span);
    let v = p.parse_expr(true)?;
    p.expect_end()?;
    Ok(v.to_i128())
}

/// Register a tiny synthetic source file containing the strings
/// `0` and `1`, separated by a newline. Returns `(file_id, span_of_0,
/// span_of_1)`.
fn register_zero_one_file(source_map: &Arc<RwLock<SourceMap>>) -> ZeroOne {
    let mut sm = source_map.write().unwrap();
    let id = sm.add_file(std::path::PathBuf::from("<#if-eval:01>"), Arc::from("0 1"));
    ZeroOne {
        zero: Span::new(id, BytePos(0), BytePos(1)),
        one: Span::new(id, BytePos(2), BytePos(3)),
    }
}

#[derive(Copy, Clone)]
struct ZeroOne {
    zero: Span,
    one: Span,
}

impl ZeroOne {
    fn synth(self, value: bool) -> PpToken {
        let span = if value { self.one } else { self.zero };
        PpToken {
            kind: PpTokenKind::PpNumber(rcc_lexer::PpNumberKind::Integer),
            span,
            leading_ws: true,
            at_line_start: false,
        }
    }
}

/// Compute a span covering every token in `tokens`, or a zero-length
/// placeholder if the slice is empty (for diagnostics).
fn whole_expression_span(tokens: &[PpToken]) -> Span {
    match (tokens.first(), tokens.last()) {
        (Some(f), Some(l)) => f.span.to(l.span),
        _ => rcc_span::DUMMY_SP,
    }
}

/// C99 §6.10.1p1: resolve every `defined NAME` / `defined(NAME)` sub-
/// expression to `1` or `0` before the remaining tokens are fed to
/// the macro expander. The operator is recognised by matching an
/// identifier whose spelling is literally `defined`; any other
/// spelling (including a macro-expanded one) is *not* recognised, per
/// the same paragraph.
fn resolve_defined(
    tokens: &[PpToken],
    source_map: &Arc<RwLock<SourceMap>>,
    macros: &MacroTable,
    interner: &mut Interner,
    zero_one: ZeroOne,
) -> Result<Vec<PpToken>, Diagnostic> {
    let mut out: Vec<PpToken> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        if tok.kind == PpTokenKind::Ident && token_text_is(source_map, tok, "defined") {
            // Two legal forms: `defined IDENT` or `defined ( IDENT )`.
            let next = tokens.get(i + 1).ok_or_else(|| malformed_defined(tok.span))?;
            let (name_tok, end_idx) = match next.kind {
                PpTokenKind::Punct(Punct::LParen) => {
                    let ident = tokens.get(i + 2).ok_or_else(|| malformed_defined(tok.span))?;
                    if ident.kind != PpTokenKind::Ident {
                        return Err(malformed_defined(ident.span));
                    }
                    let close = tokens.get(i + 3).ok_or_else(|| malformed_defined(tok.span))?;
                    if close.kind != PpTokenKind::Punct(Punct::RParen) {
                        return Err(malformed_defined(close.span));
                    }
                    (*ident, i + 4)
                }
                PpTokenKind::Ident => (*next, i + 2),
                _ => return Err(malformed_defined(next.span)),
            };
            let sym = {
                let sm = source_map.read().unwrap();
                let txt = token_span_text(&sm, name_tok.span).to_owned();
                interner.intern(&txt)
            };
            let is_def = macros.is_defined(sym);
            out.push(zero_one.synth(is_def));
            i = end_idx;
        } else {
            out.push(tok);
            i += 1;
        }
    }
    Ok(out)
}

/// Resolve `__has_include(<header>)` / `__has_include("header")` before
/// ordinary identifier-to-zero conversion. This is a widely-supported
/// extension rather than C99 proper, but it is deliberately scoped to
/// preprocessor conditionals and has no side effects: the callback probes
/// the include path without loading or recording the header.
fn resolve_has_include(
    tokens: &[PpToken],
    source_map: &Arc<RwLock<SourceMap>>,
    zero_one: ZeroOne,
    mut probe: Option<&mut HasIncludeProbe<'_>>,
) -> Result<Vec<PpToken>, Diagnostic> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        if tok.kind == PpTokenKind::Ident && token_text_is(source_map, tok, "__has_include") {
            let (query, end_idx) = parse_has_include_operand(tokens, i, source_map)?;
            let found = probe
                .as_deref_mut()
                .map(|p| p(&query.name, query.system, tok.span.to(query.end_span)))
                .unwrap_or(false);
            let mut replacement = zero_one.synth(found);
            replacement.leading_ws = tok.leading_ws;
            replacement.at_line_start = tok.at_line_start;
            out.push(replacement);
            i = end_idx;
        } else {
            out.push(tok);
            i += 1;
        }
    }
    Ok(out)
}

struct HasIncludeOperand {
    name: String,
    system: bool,
    end_span: Span,
}

fn parse_has_include_operand(
    tokens: &[PpToken],
    start: usize,
    source_map: &Arc<RwLock<SourceMap>>,
) -> Result<(HasIncludeOperand, usize), Diagnostic> {
    let open = tokens.get(start + 1).ok_or_else(|| malformed_has_include(tokens[start].span))?;
    if open.kind != PpTokenKind::Punct(Punct::LParen) {
        return Err(malformed_has_include(open.span));
    }
    let first = tokens.get(start + 2).ok_or_else(|| malformed_has_include(open.span))?;
    match first.kind {
        PpTokenKind::HeaderName | PpTokenKind::StringLit { .. } => {
            let close = tokens.get(start + 3).ok_or_else(|| malformed_has_include(first.span))?;
            if close.kind != PpTokenKind::Punct(Punct::RParen) {
                return Err(malformed_has_include(close.span));
            }
            let raw = {
                let sm = source_map.read().unwrap();
                token_span_text(&sm, first.span).to_owned()
            };
            let (name, system) = header_name_from_single_token(&raw, first.kind, first.span)?;
            Ok((HasIncludeOperand { name, system, end_span: close.span }, start + 4))
        }
        PpTokenKind::Punct(Punct::Lt) => {
            let mut name = String::new();
            let mut idx = start + 3;
            let end_span;
            loop {
                let tok = tokens.get(idx).ok_or_else(|| malformed_has_include(first.span))?;
                match tok.kind {
                    PpTokenKind::Punct(Punct::Gt) => {
                        end_span = tok.span;
                        break;
                    }
                    PpTokenKind::Punct(Punct::RParen) => {
                        return Err(malformed_has_include(tok.span))
                    }
                    _ => {
                        let sm = source_map.read().unwrap();
                        name.push_str(token_span_text(&sm, tok.span));
                    }
                }
                idx += 1;
            }
            let close = tokens.get(idx + 1).ok_or_else(|| malformed_has_include(end_span))?;
            if close.kind != PpTokenKind::Punct(Punct::RParen) {
                return Err(malformed_has_include(close.span));
            }
            if name.is_empty() {
                return Err(malformed_has_include(first.span.to(end_span)));
            }
            Ok((HasIncludeOperand { name, system: true, end_span: close.span }, idx + 2))
        }
        _ => Err(malformed_has_include(first.span)),
    }
}

fn header_name_from_single_token(
    raw: &str,
    kind: PpTokenKind,
    span: Span,
) -> Result<(String, bool), Diagnostic> {
    match kind {
        PpTokenKind::HeaderName if raw.starts_with('<') && raw.ends_with('>') => {
            Ok((raw[1..raw.len() - 1].to_string(), true))
        }
        PpTokenKind::HeaderName if raw.starts_with('"') && raw.ends_with('"') => {
            Ok((raw[1..raw.len() - 1].to_string(), false))
        }
        PpTokenKind::StringLit { .. } => {
            let Some(open) = raw.find('"') else {
                return Err(malformed_has_include(span));
            };
            let Some(close) = raw.rfind('"') else {
                return Err(malformed_has_include(span));
            };
            if open == close {
                return Err(malformed_has_include(span));
            }
            Ok((raw[open + 1..close].to_string(), false))
        }
        _ => Err(malformed_has_include(span)),
    }
}

fn token_text_is(source_map: &Arc<RwLock<SourceMap>>, tok: PpToken, needle: &str) -> bool {
    let sm = source_map.read().unwrap();
    token_span_text(&sm, tok.span) == needle
}

fn token_span_text(sm: &SourceMap, span: Span) -> &str {
    let f = sm.file(span.file);
    &f.src[span.lo.0 as usize..span.hi.0 as usize]
}

/// Replace every surviving [`PpTokenKind::Ident`] with a synthetic
/// `0` pp-number. Per C99 §6.10.1p4, after macro expansion "each
/// preprocessing token that remains … shall be a preprocessing
/// token that can be one of the following: integer constant,
/// character constant, arithmetic operator, parentheses, etc."
/// — any leftover identifier (whether it names an undefined
/// macro or a C keyword like `sizeof`) is replaced with `0`.
fn identifiers_to_zero(tokens: Vec<PpToken>, zero_one: ZeroOne) -> Vec<PpToken> {
    tokens
        .into_iter()
        .map(|t| {
            if t.kind == PpTokenKind::Ident {
                let mut replacement = zero_one.synth(false);
                replacement.leading_ws = t.leading_ws;
                replacement.at_line_start = t.at_line_start;
                replacement
            } else {
                t
            }
        })
        .collect()
}

/// Internal 128-bit integer value. Tracks signedness so unsigned
/// wrap-around is faithful on subtraction / comparison.
#[derive(Copy, Clone)]
struct Val {
    bits: u128,
    signed: bool,
}

impl Val {
    fn signed_from(n: i128) -> Val {
        Val { bits: n as u128, signed: true }
    }
    fn unsigned_from(n: u128) -> Val {
        Val { bits: n, signed: false }
    }
    fn bool_val(b: bool) -> Val {
        Val::signed_from(if b { 1 } else { 0 })
    }
    fn is_nonzero(self) -> bool {
        self.bits != 0
    }
    fn to_i128(self) -> i128 {
        self.bits as i128
    }
    /// §6.3.1.8 "usual arithmetic conversions" — if either operand is
    /// unsigned the result is unsigned.
    fn promote(a: Val, b: Val) -> bool {
        a.signed && b.signed
    }
}

// ── Parser / evaluator ──────────────────────────────────────────────

struct Parser<'a> {
    tokens: &'a [PpToken],
    pos: usize,
    source_map: &'a Arc<RwLock<SourceMap>>,
    whole_span: Span,
}

impl<'a> Parser<'a> {
    fn new(
        tokens: &'a [PpToken],
        source_map: &'a Arc<RwLock<SourceMap>>,
        whole_span: Span,
    ) -> Self {
        Self { tokens, pos: 0, source_map, whole_span }
    }

    fn peek(&self) -> Option<&PpToken> {
        self.tokens.get(self.pos)
    }

    fn peek_punct(&self) -> Option<Punct> {
        match self.peek()?.kind {
            PpTokenKind::Punct(p) => Some(p),
            _ => None,
        }
    }

    fn bump(&mut self) -> Option<PpToken> {
        let t = self.tokens.get(self.pos).copied();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat_punct(&mut self, want: Punct) -> bool {
        if self.peek_punct() == Some(want) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, want: Punct, label: &str) -> Result<(), Diagnostic> {
        if self.eat_punct(want) {
            Ok(())
        } else {
            let span = self.peek().map(|t| t.span).unwrap_or(self.whole_span);
            Err(expr_error(span, format!("expected `{label}` in #if expression")))
        }
    }

    fn expect_end(&mut self) -> Result<(), Diagnostic> {
        if self.pos == self.tokens.len() {
            Ok(())
        } else {
            let t = self.tokens[self.pos];
            Err(expr_error(t.span, "unexpected trailing token in #if expression".to_string()))
        }
    }

    // ── Precedence climb ───────────────────────────────────────────
    //
    // The `eval` flag threads short-circuit suppression through every
    // level. When false, tokens are still *parsed* (so the position
    // advances correctly) but arithmetic that could raise
    // constraint-violation diagnostics — division / remainder by
    // zero — is skipped, matching §6.5.14/§6.5.15's rule that the
    // unevaluated operand of `&&`, `||`, and `? :` is not evaluated.
    // Dead-branch tokens still have to be syntactically valid (that
    // is C's ordinary constant-expression rule) but must not trigger
    // semantic errors.

    fn parse_expr(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        self.parse_conditional(eval)
    }

    fn parse_conditional(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let cond = self.parse_logical_or(eval)?;
        if self.eat_punct(Punct::Question) {
            let take_then = eval && cond.is_nonzero();
            let take_else = eval && !cond.is_nonzero();
            let then_v = self.parse_expr(take_then)?;
            self.expect_punct(Punct::Colon, ":")?;
            let else_v = self.parse_conditional(take_else)?;
            return Ok(if cond.is_nonzero() { then_v } else { else_v });
        }
        Ok(cond)
    }

    fn parse_logical_or(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_logical_and(eval)?;
        while self.eat_punct(Punct::PipePipe) {
            let rhs_eval = eval && !lhs.is_nonzero();
            let rhs = self.parse_logical_and(rhs_eval)?;
            lhs = Val::bool_val(lhs.is_nonzero() || rhs.is_nonzero());
        }
        Ok(lhs)
    }

    fn parse_logical_and(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_bit_or(eval)?;
        while self.eat_punct(Punct::AmpAmp) {
            let rhs_eval = eval && lhs.is_nonzero();
            let rhs = self.parse_bit_or(rhs_eval)?;
            lhs = Val::bool_val(lhs.is_nonzero() && rhs.is_nonzero());
        }
        Ok(lhs)
    }

    fn parse_bit_or(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_bit_xor(eval)?;
        while self.eat_punct(Punct::Pipe) {
            let rhs = self.parse_bit_xor(eval)?;
            lhs = bitop(lhs, rhs, |a, b| a | b);
        }
        Ok(lhs)
    }

    fn parse_bit_xor(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_bit_and(eval)?;
        while self.eat_punct(Punct::Caret) {
            let rhs = self.parse_bit_and(eval)?;
            lhs = bitop(lhs, rhs, |a, b| a ^ b);
        }
        Ok(lhs)
    }

    fn parse_bit_and(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_equality(eval)?;
        while self.eat_punct(Punct::Amp) {
            let rhs = self.parse_equality(eval)?;
            lhs = bitop(lhs, rhs, |a, b| a & b);
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_relational(eval)?;
        loop {
            if self.eat_punct(Punct::EqEq) {
                let rhs = self.parse_relational(eval)?;
                lhs = Val::bool_val(cmp_eq(lhs, rhs));
            } else if self.eat_punct(Punct::BangEq) {
                let rhs = self.parse_relational(eval)?;
                lhs = Val::bool_val(!cmp_eq(lhs, rhs));
            } else {
                return Ok(lhs);
            }
        }
    }

    fn parse_relational(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_shift(eval)?;
        loop {
            let cmp = if self.eat_punct(Punct::Le) {
                Some(CmpOp::Le)
            } else if self.eat_punct(Punct::Ge) {
                Some(CmpOp::Ge)
            } else if self.eat_punct(Punct::Lt) {
                Some(CmpOp::Lt)
            } else if self.eat_punct(Punct::Gt) {
                Some(CmpOp::Gt)
            } else {
                None
            };
            match cmp {
                Some(op) => {
                    let rhs = self.parse_shift(eval)?;
                    lhs = Val::bool_val(cmp_rel(lhs, rhs, op));
                }
                None => return Ok(lhs),
            }
        }
    }

    fn parse_shift(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_additive(eval)?;
        loop {
            if self.eat_punct(Punct::ShlShl) {
                let rhs = self.parse_additive(eval)?;
                lhs = shl(lhs, rhs);
            } else if self.eat_punct(Punct::ShrShr) {
                let rhs = self.parse_additive(eval)?;
                lhs = shr(lhs, rhs);
            } else {
                return Ok(lhs);
            }
        }
    }

    fn parse_additive(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_multiplicative(eval)?;
        loop {
            if self.eat_punct(Punct::Plus) {
                let rhs = self.parse_multiplicative(eval)?;
                lhs = arith(lhs, rhs, |a, b| a.wrapping_add(b));
            } else if self.eat_punct(Punct::Minus) {
                let rhs = self.parse_multiplicative(eval)?;
                lhs = arith(lhs, rhs, |a, b| a.wrapping_sub(b));
            } else {
                return Ok(lhs);
            }
        }
    }

    fn parse_multiplicative(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        let mut lhs = self.parse_unary(eval)?;
        loop {
            if self.eat_punct(Punct::Star) {
                let rhs = self.parse_unary(eval)?;
                lhs = arith(lhs, rhs, |a, b| a.wrapping_mul(b));
            } else if self.eat_punct(Punct::Slash) {
                let op_span = self.prev_span();
                let rhs = self.parse_unary(eval)?;
                lhs = div_or_rem(lhs, rhs, DivOp::Div, op_span, eval)?;
            } else if self.eat_punct(Punct::Percent) {
                let op_span = self.prev_span();
                let rhs = self.parse_unary(eval)?;
                lhs = div_or_rem(lhs, rhs, DivOp::Rem, op_span, eval)?;
            } else {
                return Ok(lhs);
            }
        }
    }

    /// Span of the most recently consumed token; used so division-by-
    /// zero diagnostics can point at the operator that triggered them.
    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            self.whole_span
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    fn parse_unary(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        if self.eat_punct(Punct::Plus) {
            return self.parse_unary(eval);
        }
        if self.eat_punct(Punct::Minus) {
            let v = self.parse_unary(eval)?;
            // Two's-complement negation works for both signed and
            // unsigned 128-bit bit patterns: `0u - v`.
            let bits = 0u128.wrapping_sub(v.bits);
            return Ok(Val { bits, signed: v.signed });
        }
        if self.eat_punct(Punct::Bang) {
            let v = self.parse_unary(eval)?;
            return Ok(Val::bool_val(!v.is_nonzero()));
        }
        if self.eat_punct(Punct::Tilde) {
            let v = self.parse_unary(eval)?;
            return Ok(Val { bits: !v.bits, signed: v.signed });
        }
        self.parse_primary(eval)
    }

    fn parse_primary(&mut self, eval: bool) -> Result<Val, Diagnostic> {
        if self.eat_punct(Punct::LParen) {
            let v = self.parse_expr(eval)?;
            self.expect_punct(Punct::RParen, ")")?;
            return Ok(v);
        }
        let tok = self
            .bump()
            .ok_or_else(|| expr_error(self.whole_span, "empty #if expression".to_string()))?;
        match tok.kind {
            PpTokenKind::PpNumber(_) => parse_integer_literal(tok, self.source_map),
            PpTokenKind::CharConst { .. } => parse_char_constant(tok, self.source_map),
            _ => {
                Err(expr_error(tok.span, "expected integer constant in #if expression".to_string()))
            }
        }
    }
}

#[derive(Copy, Clone)]
enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Copy, Clone)]
enum DivOp {
    Div,
    Rem,
}

fn bitop(a: Val, b: Val, f: impl Fn(u128, u128) -> u128) -> Val {
    let bits = f(a.bits, b.bits);
    Val { bits, signed: Val::promote(a, b) }
}

fn arith(a: Val, b: Val, f: impl Fn(u128, u128) -> u128) -> Val {
    let bits = f(a.bits, b.bits);
    Val { bits, signed: Val::promote(a, b) }
}

fn div_or_rem(a: Val, b: Val, op: DivOp, op_span: Span, eval: bool) -> Result<Val, Diagnostic> {
    if b.bits == 0 {
        if !eval {
            // Dead branch (short-circuited by `? :`, `&&`, or `||`):
            // §6.5.5p5's "undefined if zero" does not apply because
            // the standard says the unevaluated operand is not
            // evaluated. Yield a dummy zero and carry on.
            return Ok(Val::signed_from(0));
        }
        let msg = match op {
            DivOp::Div => "division by zero in #if expression",
            DivOp::Rem => "remainder by zero in #if expression",
        };
        return Err(Diagnostic {
            level: Level::Error,
            code: Some(E0028),
            message: msg.into(),
            labels: vec![Label { span: op_span, message: "zero divisor".into(), primary: true }],
            notes: vec!["C99 §6.5.5p5: the result of the `/` and `%` operators \
                 is undefined when the right operand is zero"
                .into()],
            help: vec![],
        });
    }
    let signed = Val::promote(a, b);
    let bits = if signed {
        let an = a.bits as i128;
        let bn = b.bits as i128;
        // §6.10.1 demands the widest integer type; wrapping on the
        // INT_MIN / -1 corner is what GCC and chibicc do here.
        let r = match op {
            DivOp::Div => an.wrapping_div(bn),
            DivOp::Rem => an.wrapping_rem(bn),
        };
        r as u128
    } else {
        match op {
            DivOp::Div => a.bits / b.bits,
            DivOp::Rem => a.bits % b.bits,
        }
    };
    Ok(Val { bits, signed })
}

fn shl(a: Val, b: Val) -> Val {
    let shift = (b.bits & 127) as u32;
    let bits = a.bits.wrapping_shl(shift);
    Val { bits, signed: a.signed }
}

fn shr(a: Val, b: Val) -> Val {
    let shift = (b.bits & 127) as u32;
    let bits = if a.signed {
        let n = a.bits as i128;
        n.wrapping_shr(shift) as u128
    } else {
        a.bits.wrapping_shr(shift)
    };
    Val { bits, signed: a.signed }
}

fn cmp_eq(a: Val, b: Val) -> bool {
    // §6.5.9 equality: operand conversion is the same as the usual
    // arithmetic conversions; once both are 128-bit their bit
    // patterns match iff the abstract values match, regardless of
    // signedness (u128::MAX `as i128 == -1` still compares distinct
    // bit patterns).
    a.bits == b.bits
}

fn cmp_rel(a: Val, b: Val, op: CmpOp) -> bool {
    let r = if Val::promote(a, b) {
        (a.bits as i128).cmp(&(b.bits as i128))
    } else {
        a.bits.cmp(&b.bits)
    };
    match op {
        CmpOp::Lt => r.is_lt(),
        CmpOp::Le => r.is_le(),
        CmpOp::Gt => r.is_gt(),
        CmpOp::Ge => r.is_ge(),
    }
}

// ── Integer / character-constant lexing ─────────────────────────────

fn parse_integer_literal(
    tok: PpToken,
    source_map: &Arc<RwLock<SourceMap>>,
) -> Result<Val, Diagnostic> {
    let text = {
        let sm = source_map.read().unwrap();
        token_span_text(&sm, tok.span).to_owned()
    };
    let (digits, suffix, base) = split_integer_text(&text, tok.span)?;
    if digits.is_empty() {
        return Err(expr_error(tok.span, format!("invalid integer literal `{text}`")));
    }
    let mut value: u128 = 0;
    for ch in digits.chars() {
        let digit = match ch {
            '0'..='9' => (ch as u32) - ('0' as u32),
            'a'..='f' => (ch as u32) - ('a' as u32) + 10,
            'A'..='F' => (ch as u32) - ('A' as u32) + 10,
            _ => {
                return Err(expr_error(
                    tok.span,
                    format!("invalid digit `{ch}` in integer literal `{text}`"),
                ))
            }
        };
        if digit >= base {
            return Err(expr_error(
                tok.span,
                format!("digit `{ch}` out of range for base {base} literal `{text}`"),
            ));
        }
        value =
            value.checked_mul(base as u128).and_then(|v| v.checked_add(digit as u128)).ok_or_else(
                || expr_error(tok.span, format!("integer literal `{text}` overflows 128 bits")),
            )?;
    }
    // Suffix classification per C99 §6.4.4.1: `u`/`U` => unsigned,
    // `l`/`L` and `ll`/`LL` width markers are ignored here because
    // the evaluator already uses the widest representable type.
    let mut unsigned = false;
    let mut chars = suffix.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'u' | 'U' => unsigned = true,
            'l' | 'L' => {
                if let Some(&next) = chars.peek() {
                    if next == c {
                        chars.next();
                    }
                }
            }
            _ => return Err(expr_error(tok.span, format!("invalid integer suffix in `{text}`"))),
        }
    }
    Ok(if unsigned { Val::unsigned_from(value) } else { Val::signed_from(value as i128) })
}

/// Split a pp-number into (digits, suffix, base). Only integer forms
/// are supported — a `.` or an exponent marker (`e`/`E`/`p`/`P` on
/// hex floats) makes the literal a float, which §6.10.1 forbids.
///
/// Note on the bare-`0` corner: C99 treats `0` as an octal constant
/// whose digit happens to be zero (§6.4.4.1p1 — the prefix "0" on its
/// own is a legal octal numeric constant). The splitter below strips
/// the leading `0` only when the next byte is itself an octal digit;
/// otherwise the full text (including the `0`) is reparsed as decimal
/// so that `0`, `0u`, `0L` all lex correctly.
fn split_integer_text(text: &str, span: Span) -> Result<(&str, &str, u32), Diagnostic> {
    if text.contains('.') {
        return Err(expr_error(
            span,
            format!("floating-point literal `{text}` is not allowed in #if"),
        ));
    }
    let (body, base): (&str, u32) = if let Some(rest) = text.strip_prefix("0x") {
        (rest, 16)
    } else if let Some(rest) = text.strip_prefix("0X") {
        (rest, 16)
    } else if text.starts_with('0') && text.len() > 1 && matches!(text.as_bytes()[1], b'0'..=b'7') {
        (&text[1..], 8)
    } else {
        (text, 10)
    };
    let mut end = body.len();
    for (i, c) in body.char_indices() {
        let is_digit = match base {
            8 | 10 => c.is_ascii_digit(),
            16 => c.is_ascii_hexdigit(),
            _ => unreachable!(),
        };
        if is_digit {
            continue;
        }
        // Exponent markers make the literal a float; reject them.
        // For decimal / octal, `e`/`E` is an exponent. For hex it's a
        // digit and was already consumed above. Hex floats use
        // `p`/`P`, which are never valid integer digits.
        if base != 16 && (c == 'e' || c == 'E') {
            return Err(expr_error(
                span,
                format!("floating-point literal `{text}` is not allowed in #if"),
            ));
        }
        if c == 'p' || c == 'P' {
            return Err(expr_error(
                span,
                format!("floating-point literal `{text}` is not allowed in #if"),
            ));
        }
        end = i;
        break;
    }
    let digits = &body[..end];
    let suffix = &body[end..];
    Ok((digits, suffix, base))
}

fn parse_char_constant(
    tok: PpToken,
    source_map: &Arc<RwLock<SourceMap>>,
) -> Result<Val, Diagnostic> {
    let text = {
        let sm = source_map.read().unwrap();
        token_span_text(&sm, tok.span).to_owned()
    };
    // Strip encoding prefix and surrounding quotes.
    let inner = text.trim_start_matches('L').trim_start_matches('u').trim_start_matches('U');
    let inner = inner
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .ok_or_else(|| expr_error(tok.span, format!("malformed character constant `{text}`")))?;
    if inner.is_empty() {
        return Err(expr_error(tok.span, "empty character constant".into()));
    }
    let mut it = inner.chars();
    let v: i128 = match it.next().unwrap() {
        '\\' => {
            let next = it.next().ok_or_else(|| {
                expr_error(tok.span, "trailing `\\` in character constant".into())
            })?;
            let code: i128 = match next {
                'n' => 0x0a,
                't' => 0x09,
                'r' => 0x0d,
                '0'..='7' => {
                    let mut value = next.to_digit(8).unwrap() as i128;
                    for _ in 0..2 {
                        let Some(c) = it.clone().next() else { break };
                        let Some(digit) = c.to_digit(8) else { break };
                        value = (value << 3) + digit as i128;
                        it.next();
                    }
                    value
                }
                '\\' => 0x5c,
                '\'' => 0x27,
                '"' => 0x22,
                '?' => 0x3f,
                'a' => 0x07,
                'b' => 0x08,
                'f' => 0x0c,
                'v' => 0x0b,
                _ => {
                    return Err(expr_error(
                        tok.span,
                        format!("unsupported escape `\\{next}` in #if character constant"),
                    ))
                }
            };
            code
        }
        c => c as i128,
    };
    Ok(Val::signed_from(v))
}

// ── Diagnostics ─────────────────────────────────────────────────────

fn malformed_defined(span: Span) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0028),
        message: "malformed `defined` operator".into(),
        labels: vec![Label {
            span,
            message: "expected an identifier or `( identifier )` after `defined`".into(),
            primary: true,
        }],
        notes: vec!["C99 §6.10.1p1: the `defined` operator takes a single macro \
             name, optionally parenthesised"
            .into()],
        help: vec![],
    }
}

fn malformed_has_include(span: Span) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0028),
        message: "malformed `__has_include` operator".into(),
        labels: vec![Label {
            span,
            message: "expected `__has_include(<header>)` or `__has_include(\"header\")`".into(),
            primary: true,
        }],
        notes: vec![
            "`__has_include` is supported as a preprocessor conditional extension in C99 mode"
                .into(),
        ],
        help: vec![],
    }
}

fn expr_error(span: Span, msg: String) -> Diagnostic {
    Diagnostic {
        level: Level::Error,
        code: Some(E0028),
        message: msg.clone(),
        labels: vec![Label { span, message: msg, primary: true }],
        notes: vec![],
        help: vec![],
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! Unit tests for the `#if` expression evaluator. These exercise
    //! `eval_if` directly so the state-machine integration (task
    //! 04-14) is not in the loop. Input tokens are produced by the
    //! real directive parser so that the test bed matches the
    //! production call site shape.

    use super::*;
    use crate::directive::{parse_directive, ConditionalKind, Directive};
    use crate::line_stream::LineStream;
    use crate::macros::{MacroDef, MacroKind, MacroTable};
    use rcc_errors::{CaptureEmitter, Handler};
    use rcc_lexer::tokenize;
    use rcc_session::{Options, Session};
    use std::path::PathBuf;

    /// Seed a session around a single `#if ...\n` line, extract the
    /// condition token vector, and return everything the evaluator
    /// needs. The macro table is empty; callers that need definitions
    /// insert them afterwards via the returned mutable reference.
    fn seed_if(src_line: &str) -> (Session, Vec<PpToken>, MacroTable, CaptureEmitter) {
        let cap = CaptureEmitter::new();
        let handler = Handler::with_emitter(Box::new(cap.clone()));
        let mut sess = Session::with_handler(Options::default(), handler);
        let id = sess
            .source_map
            .write()
            .unwrap()
            .add_file(PathBuf::from("<if-test>"), Arc::from(src_line));
        let src = sess.source_map.read().unwrap().file(id).src.clone();
        let mut ls = LineStream::new(tokenize(id, &src));
        let line = ls.next_line().expect("one directive line");
        let d = parse_directive(&line, &src, &mut sess.interner).expect("well-formed directive");
        let condition = match d {
            Directive::Conditional { kind: ConditionalKind::If, condition, .. } => condition,
            other => panic!("expected Conditional::If, got {other:?}"),
        };
        (sess, condition, MacroTable::default(), cap)
    }

    fn run_eval(
        sess: &mut Session,
        tokens: &[PpToken],
        macros: &MacroTable,
    ) -> Result<i128, Diagnostic> {
        let sm_arc = Arc::clone(&sess.source_map);
        let line_map = LineMap::new();
        eval_if(
            tokens,
            &sm_arc,
            &mut sess.interner,
            &mut sess.handler,
            macros,
            &line_map,
            EvalOptions::strict(),
        )
    }

    // ── Literal arithmetic ────────────────────────────────────────

    #[test]
    fn plain_true_literal() {
        let (mut sess, cond, macros, _cap) = seed_if("#if 1\n");
        let v = run_eval(&mut sess, &cond, &macros).expect("eval");
        assert_ne!(v, 0, "got {v}");
    }

    #[test]
    fn plain_false_literal() {
        let (mut sess, cond, macros, _cap) = seed_if("#if 0\n");
        assert_eq!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn addition_and_equality_is_true() {
        // Acceptance: `#if 1+1 == 2` → true.
        let (mut sess, cond, macros, _cap) = seed_if("#if 1+1 == 2\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn shift_left_is_evaluated() {
        // Acceptance: `#if 1 << 2 == 4` → true.
        let (mut sess, cond, macros, _cap) = seed_if("#if 1 << 2 == 4\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn precedence_and_parentheses() {
        let (mut sess, cond, macros, _cap) = seed_if("#if (1+2)*3 == 9\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
        let (mut sess, cond, macros, _cap) = seed_if("#if 1+2*3 == 7\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn conditional_operator_selects_branch() {
        let (mut sess, cond, macros, _cap) = seed_if("#if 1 ? 42 : 0\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
        let (mut sess, cond, macros, _cap) = seed_if("#if 0 ? 1 : 0\n");
        assert_eq!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn logical_and_or_short_circuit() {
        let (mut sess, cond, macros, _cap) = seed_if("#if 0 && 1\n");
        assert_eq!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
        let (mut sess, cond, macros, _cap) = seed_if("#if 1 || 0\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn unary_ops() {
        let (mut sess, cond, macros, _cap) = seed_if("#if !0\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
        let (mut sess, cond, macros, _cap) = seed_if("#if ~0 == -1\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
        let (mut sess, cond, macros, _cap) = seed_if("#if -5 + 5 == 0\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn hex_and_octal_literals() {
        let (mut sess, cond, macros, _cap) = seed_if("#if 0xff == 255\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
        let (mut sess, cond, macros, _cap) = seed_if("#if 010 == 8\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    // ── defined / identifier fallback ─────────────────────────────

    #[test]
    fn defined_unary_form_false_when_absent() {
        // Acceptance: `#if defined FOO` (no FOO) → false.
        let (mut sess, cond, macros, _cap) = seed_if("#if defined FOO\n");
        assert_eq!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn defined_paren_form_true_when_present() {
        let (mut sess, cond, mut macros, _cap) = seed_if("#if defined(FOO)\n");
        let foo = sess.interner.intern("FOO");
        let fake_span = {
            let sm = sess.source_map.read().unwrap();
            let src = &sm.file(cond[0].span.file).src;
            Span::new(cond[0].span.file, BytePos(0), BytePos(src.len() as u32))
        };
        macros.define(MacroDef {
            name: foo,
            kind: MacroKind::ObjectLike,
            body: Vec::new(),
            def_span: fake_span,
            is_predefined: false,
        });
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn not_defined_is_true_when_absent() {
        // Acceptance: `#if !defined BAR` → true.
        let (mut sess, cond, macros, _cap) = seed_if("#if !defined BAR\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn bare_undefined_identifier_is_zero() {
        // §6.10.1p4: a post-expansion identifier is replaced with `0`,
        // so `NO_SUCH_MACRO == 0` must be true.
        let (mut sess, cond, macros, _cap) = seed_if("#if NO_SUCH_MACRO == 0\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn defined_is_not_expanded_across_macros() {
        // `defined` itself is resolved before expansion; even if a
        // macro named `defined` were somehow defined (we don't create
        // one here, but the check is: the spelling is compared
        // textually), the expansion path is not triggered.
        let (mut sess, cond, mut macros, _cap) = seed_if("#if defined(BAR)\n");
        let bar = sess.interner.intern("BAR");
        macros.define(MacroDef {
            name: bar,
            kind: MacroKind::ObjectLike,
            body: Vec::new(),
            def_span: cond[0].span,
            is_predefined: false,
        });
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    // ── Unsigned wrap-around ──────────────────────────────────────

    #[test]
    fn unsigned_subtraction_wraps_positive() {
        // Acceptance: `#if 1u - 2u > 0` → true (unsigned wraparound).
        let (mut sess, cond, macros, _cap) = seed_if("#if 1u - 2u > 0\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn mixed_signed_unsigned_promotes_to_unsigned() {
        // `-1 > 0U` is true under C99 promotion (left converts to unsigned).
        let (mut sess, cond, macros, _cap) = seed_if("#if -1 > 0u\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    // ── Division by zero in a live branch ─────────────────────────

    #[test]
    fn division_by_zero_emits_e0028() {
        let (mut sess, cond, macros, _cap) = seed_if("#if 1/0\n");
        let err =
            run_eval(&mut sess, &cond, &macros).expect_err("div by zero must be a hard error");
        assert_eq!(err.code, Some(E0028));
        assert!(
            err.message.to_lowercase().contains("division"),
            "message should mention division: {:?}",
            err.message
        );
    }

    #[test]
    fn remainder_by_zero_emits_e0028() {
        let (mut sess, cond, macros, _cap) = seed_if("#if 1 % 0\n");
        let err = run_eval(&mut sess, &cond, &macros).expect_err("rem by zero must error");
        assert_eq!(err.code, Some(E0028));
    }

    #[test]
    fn conditional_short_circuits_away_division_by_zero() {
        // §6.5.15p4: the unused branch of `? :` is not evaluated. Our
        // recursive-descent evaluator mirrors that, so `1/0` in the
        // dead arm must not fire E0028.
        // Note: we still PARSE the dead side (both halves must be
        // syntactically valid), but we do not divide.
        let (mut sess, cond, macros, _cap) = seed_if("#if 1 ? 42 : 1/0\n");
        let v = run_eval(&mut sess, &cond, &macros);
        assert!(v.is_ok(), "dead `1/0` must not trigger E0028: {v:?}");
    }

    // ── Operator coverage spot-checks ─────────────────────────────

    #[test]
    fn bitwise_operators() {
        let (mut sess, cond, macros, _cap) = seed_if("#if (0xf0 & 0x0f) == 0\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
        let (mut sess, cond, macros, _cap) = seed_if("#if (0xf0 | 0x0f) == 0xff\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
        let (mut sess, cond, macros, _cap) = seed_if("#if (0xff ^ 0x0f) == 0xf0\n");
        assert_ne!(run_eval(&mut sess, &cond, &macros).unwrap(), 0);
    }

    #[test]
    fn relational_operators() {
        for (src, expected) in [
            ("#if 1 < 2\n", true),
            ("#if 2 < 1\n", false),
            ("#if 2 <= 2\n", true),
            ("#if 3 >= 3\n", true),
            ("#if 4 != 5\n", true),
            ("#if 4 == 4\n", true),
        ] {
            let (mut sess, cond, macros, _cap) = seed_if(src);
            let v = run_eval(&mut sess, &cond, &macros).unwrap();
            assert_eq!(v != 0, expected, "{src} → got {v}, expected {expected}");
        }
    }
}
