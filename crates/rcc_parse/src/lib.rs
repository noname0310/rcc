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

mod decl;
mod expr;
mod init;
mod keywords;
mod literal;
mod phase7;
mod scope;
mod stmt;
mod token;

pub use decl::{
    declare_declarator_name, parse_abstract_declarator, parse_decl_specs, parse_declaration,
    parse_declarator, parse_external_decl, parse_type_name,
};
pub use expr::{
    parse_assignment_expression, parse_expr_bp, parse_expression, parse_postfix,
    parse_prefix_unary, parse_primary,
};
pub use init::parse_initializer;
pub use keywords::{classify_ident, Keyword, KEYWORDS};
pub use literal::{decode_char, decode_float, decode_integer, decode_string};
pub use phase7::{convert as pp_stream_to_tokens, merge_adjacent_strings, pp_to_token};
pub use scope::{NameKind, Scope, ScopeStack};
pub use stmt::{parse_block, parse_block_item, parse_stmt};
pub use token::{
    CharLiteral, FloatLiteral, FloatSuffix, IntLiteral, IntSuffix, StringLiteral, Token, TokenKind,
};

/// Parse a translation unit. Returns `None` if unrecoverable.
pub fn parse(session: &mut Session, tokens: Vec<PpToken>) -> Option<TranslationUnit> {
    let converted = phase7::convert(session, &tokens);
    let mut parser = Parser::new(session, converted);
    let start = parser.cur_span();
    let mut decls = Vec::new();
    while parser.peek().is_some() {
        let before = parser.cursor;
        let err_before = parser.session.handler.error_count();
        match decl::parse_external_decl(&mut parser) {
            Some(d) => decls.push(d),
            None => {
                if parser.cursor == before {
                    // Emit a diagnostic (if none was already emitted
                    // at this position) and skip to the next sync
                    // point so downstream constructs still get parsed.
                    if parser.session.handler.error_count() == err_before {
                        let at = parser.cur_span();
                        parser
                            .session
                            .handler
                            .struct_err(at, "unexpected token at file scope")
                            .code(rcc_errors::codes::E0030)
                            .emit();
                    }
                    parser.recover_to_sync();
                    // If recover_to_sync didn't advance (e.g. stuck on
                    // a stray `}` at file scope), force-skip one token
                    // to guarantee progress and prevent infinite loops.
                    if parser.cursor == before {
                        parser.bump();
                    }
                }
            }
        }
    }
    let end = if parser.cursor > 0 {
        parser.tokens.get(parser.cursor - 1).map(|t| t.span).unwrap_or(start)
    } else {
        start
    };
    Some(TranslationUnit { decls, span: start.to(end) })
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

    /// Skip tokens until a synchronisation point (`;`, `}`, or EOF).
    ///
    /// After an unexpected-token diagnostic, calling this method
    /// advances the cursor past the junk tokens so that the next
    /// iteration of the enclosing parse loop starts from a reasonable
    /// position. `;` is consumed (the statement is over); `}` is
    /// **not** consumed (the caller's block-loop needs to see it).
    pub fn recover_to_sync(&mut self) {
        while let Some(tok) = self.peek() {
            match tok.kind {
                token::TokenKind::Punct(rcc_lexer::Punct::Semi) => {
                    self.bump(); // consume the `;`
                    return;
                }
                token::TokenKind::Punct(rcc_lexer::Punct::RBrace) => {
                    // Don't consume — the block loop needs this `}`.
                    return;
                }
                _ => {
                    self.bump();
                }
            }
        }
    }
}
