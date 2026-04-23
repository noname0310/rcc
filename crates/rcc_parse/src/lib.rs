//! `rcc_parse`: preprocessor-token stream -> C99 AST.
//!
//! Two-stage: first a `PpToken -> Token` conversion (keyword classification,
//! pp-number interpretation, string concatenation), then a recursive-descent
//! parser for declarations/statements with a Pratt expression parser.
//!
//! Resolves the **typedef-name vs identifier** ambiguity by maintaining a
//! scoped symbol table that tracks which names are currently typedef names.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use rcc_ast::{NodeId, TranslationUnit};
use rcc_data_structures::FxHashSet;
use rcc_lexer::PpToken;
use rcc_session::Session;
use rcc_span::{Span, Symbol};

mod expr;
mod keywords;
mod literal;
mod phase7;
mod scope;
mod token;

pub use expr::parse_primary;
pub use keywords::{classify_ident, Keyword, KEYWORDS};
pub use literal::{decode_char, decode_float, decode_integer, decode_string};
pub use phase7::{convert as pp_stream_to_tokens, merge_adjacent_strings, pp_to_token};
pub use scope::{NameKind, Scope, ScopeStack};
pub use token::{
    CharLiteral, FloatLiteral, FloatSuffix, IntLiteral, IntSuffix, StringLiteral, Token, TokenKind,
};

/// Parse a translation unit. Returns `None` if unrecoverable.
///
/// M1 scope: interface only. Implementation lands in M1-follow-up.
pub fn parse(_session: &mut Session, _tokens: Vec<PpToken>) -> Option<TranslationUnit> {
    None
}

/// Parser state. Public so UI tests can instantiate partial parses.
pub struct Parser<'a> {
    /// Compilation session.
    pub session: &'a mut Session,
    /// Tokens converted from pp-tokens (phase 7).
    pub tokens: Vec<Token>,
    /// Cursor into `tokens`.
    pub cursor: usize,
    /// Scope stack for typedef-name resolution.
    pub scopes: ScopeStack,
    /// Set of symbols that have been reported already (dedup).
    pub reported: FxHashSet<Symbol>,
    /// Monotonic `NodeId` counter. Incremented by [`Parser::fresh_id`]
    /// so every AST node minted by the parser has a unique id within
    /// the translation unit.
    pub next_node_id: u32,
}

impl<'a> Parser<'a> {
    /// Build a parser.
    pub fn new(session: &'a mut Session, tokens: Vec<Token>) -> Self {
        Self {
            session,
            tokens,
            cursor: 0,
            scopes: ScopeStack::new(),
            reported: FxHashSet::default(),
            next_node_id: 0,
        }
    }

    /// Span of the current token (or end-of-input span).
    pub fn cur_span(&self) -> Span {
        self.tokens.get(self.cursor).map(|t| t.span).unwrap_or(rcc_span::DUMMY_SP)
    }

    /// Peek the current token without advancing the cursor.
    ///
    /// Returns `None` when the cursor is past the end of the stream.
    /// The EOF pp-token is dropped at phase-7 conversion, so running
    /// off the end is represented by `None` rather than an `Eof` kind.
    pub fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    /// Consume and return the current token, advancing the cursor.
    ///
    /// Returns `None` at end of input. Callers that need the token's
    /// span separately should read [`Parser::cur_span`] before calling.
    pub fn bump(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.cursor).cloned()?;
        self.cursor += 1;
        Some(tok)
    }

    /// Allocate a fresh AST [`NodeId`].
    ///
    /// Ids are dense `u32`s, unique per `Parser` (i.e. per translation
    /// unit). The counter starts at 0 and monotonically increases; it
    /// will only overflow on pathologically large inputs (>4 G nodes)
    /// so debug-mode wrap checks are enough.
    pub fn fresh_id(&mut self) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        id
    }
}
