//! Character cursor used by the lexer. Byte-oriented; handles UTF-8 lazily.

use std::str::Chars;

/// A peekable char iterator that tracks byte offset.
pub struct Cursor<'a> {
    initial_len: usize,
    chars: Chars<'a>,
}

impl<'a> Cursor<'a> {
    /// Build a cursor at offset 0 of `src`.
    pub fn new(src: &'a str) -> Self {
        Self { initial_len: src.len(), chars: src.chars() }
    }

    /// Byte offset from the start of the original `src`.
    pub fn offset(&self) -> usize {
        self.initial_len - self.chars.as_str().len()
    }

    /// Remaining, un-consumed slice.
    pub fn rest(&self) -> &'a str {
        self.chars.as_str()
    }

    /// Whether the cursor is past the end of input.
    pub fn is_eof(&self) -> bool {
        self.chars.as_str().is_empty()
    }

    /// Peek the next char without consuming.
    pub fn first(&self) -> Option<char> {
        self.chars.clone().next()
    }

    /// Peek the char after `first`, if any.
    pub fn second(&self) -> Option<char> {
        let mut c = self.chars.clone();
        c.next()?;
        c.next()
    }

    /// Consume and return the next char, or `None` at EOF.
    pub fn bump(&mut self) -> Option<char> {
        self.chars.next()
    }

    /// Consume while `pred` holds.
    pub fn eat_while<F: FnMut(char) -> bool>(&mut self, mut pred: F) {
        while let Some(c) = self.first() {
            if !pred(c) {
                break;
            }
            self.bump();
        }
    }

    /// Peek the `n`-th character ahead (0-indexed: `peek_at(0)` == `first()`).
    pub fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.clone().nth(n)
    }

    /// Consume the next character if `pred` returns true for it.
    /// Returns `true` if a character was consumed.
    pub fn bump_if<F: FnOnce(char) -> bool>(&mut self, pred: F) -> bool {
        match self.first() {
            Some(c) if pred(c) => {
                self.bump();
                true
            }
            _ => false,
        }
    }

    /// Consume characters while `pred` holds, returning how many were consumed.
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
}
