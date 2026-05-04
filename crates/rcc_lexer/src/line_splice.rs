//! Line-splicing cursor: wraps [`Cursor`] to transparently skip
//! backslash-newline sequences (C99 §5.1.1.2, translation phase 2).
//!
//! Physical byte offsets are always reported so that [`rcc_span::Span`]
//! values point into the original source and diagnostics underline
//! correctly—even across a splice boundary.

use super::cursor::Cursor;

/// A cursor that transparently removes `\<newline>` (and `\<CR><LF>`)
/// sequences before the tokenizer sees them.
///
/// Every public method mirrors [`Cursor`] in semantics, except that
/// backslash-newline pairs are invisible to the consumer. [`offset`]
/// always returns the **physical** byte position so that spans remain
/// valid.
///
/// [`offset`]: Self::offset
pub struct LineSpliceCursor<'a> {
    inner: Cursor<'a>,
}

/// Remove phase-2 backslash-newline splice sequences from a source slice.
///
/// Token spans deliberately keep physical byte ranges, so a token that begins
/// after a splice can cover bytes such as `\\\r\nIDENT`. Any code recovering a
/// token's logical spelling from its span must run the same phase-2 removal
/// before interning or decoding that spelling.
#[must_use]
pub fn strip_line_splices(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek().copied() {
                Some('\n') => {
                    chars.next();
                    continue;
                }
                Some('\r') => {
                    chars.next();
                    if matches!(chars.peek(), Some('\n')) {
                        chars.next();
                        continue;
                    }
                    out.push('\\');
                    out.push('\r');
                    continue;
                }
                _ => {}
            }
        }
        out.push(ch);
    }
    out
}

impl<'a> LineSpliceCursor<'a> {
    /// Build a line-splicing cursor over `src`.
    pub fn new(src: &'a str) -> Self {
        Self { inner: Cursor::new(src) }
    }

    /// Physical byte offset into the original source (includes any
    /// skipped splice bytes to the left of the current position).
    pub fn offset(&self) -> usize {
        self.inner.offset()
    }

    /// Whether the logical input is exhausted (trailing splices count
    /// as consumed).
    pub fn is_eof(&self) -> bool {
        self.first().is_none()
    }

    /// Peek the next logical character, skipping splices.
    pub fn first(&self) -> Option<char> {
        self.peek_at(0)
    }

    /// Peek the character after [`first`](Self::first), skipping splices.
    pub fn second(&self) -> Option<char> {
        self.peek_at(1)
    }

    /// Peek the `n`-th logical character ahead (0-indexed), skipping
    /// any intervening splice sequences.
    pub fn peek_at(&self, n: usize) -> Option<char> {
        let mut raw = 0;
        let mut logical = 0;
        loop {
            if let Some(skip) = self.splice_len_at(raw) {
                raw += skip;
                continue;
            }
            let c = self.inner.peek_at(raw)?;
            if logical == n {
                return Some(c);
            }
            logical += 1;
            raw += 1;
        }
    }

    /// Consume and return the next logical character, or `None` at EOF.
    ///
    /// Any splice sequences immediately before the character are
    /// consumed first, advancing the physical offset past them.
    pub fn bump(&mut self) -> Option<char> {
        self.skip_splices();
        self.inner.bump()
    }

    /// Consume the next logical character if `pred` returns true.
    pub fn bump_if<F: FnOnce(char) -> bool>(&mut self, pred: F) -> bool {
        match self.first() {
            Some(c) if pred(c) => {
                self.bump();
                true
            }
            _ => false,
        }
    }

    /// Consume logical characters while `pred` holds; return the count.
    pub fn bump_while<F: FnMut(char) -> bool>(&mut self, mut pred: F) -> usize {
        let mut count = 0;
        while let Some(c) = self.first() {
            if !pred(c) {
                break;
            }
            self.bump();
            count += 1;
        }
        count
    }

    /// Consume while `pred` holds (same as [`bump_while`](Self::bump_while)
    /// but discards the count).
    pub fn eat_while<F: FnMut(char) -> bool>(&mut self, mut pred: F) {
        while let Some(c) = self.first() {
            if !pred(c) {
                break;
            }
            self.bump();
        }
    }

    // ── private helpers ─────────────────────────────────────────────

    /// If the raw char at position `raw` starts a splice, return how
    /// many raw chars to skip (2 for `\<LF>`, 3 for `\<CR><LF>`).
    fn splice_len_at(&self, raw: usize) -> Option<usize> {
        if self.inner.peek_at(raw) != Some('\\') {
            return None;
        }
        match self.inner.peek_at(raw + 1) {
            Some('\n') => Some(2),
            Some('\r') if self.inner.peek_at(raw + 2) == Some('\n') => Some(3),
            _ => None,
        }
    }

    /// Advance the inner cursor past all consecutive splice sequences
    /// at the current position.
    fn skip_splices(&mut self) {
        loop {
            match (self.inner.first(), self.inner.second()) {
                (Some('\\'), Some('\n')) => {
                    self.inner.bump();
                    self.inner.bump();
                }
                (Some('\\'), Some('\r')) if self.inner.peek_at(2) == Some('\n') => {
                    self.inner.bump();
                    self.inner.bump();
                    self.inner.bump();
                }
                _ => break,
            }
        }
    }
}
