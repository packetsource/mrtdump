//! `--More--` pagination state, in the style of Cisco IOS `terminal length`.
//!
//! This only tracks line counts and classifies keypresses; the shell does the
//! actual waiting for a key so that the pager never blocks the SSH session.

pub const MORE_PROMPT: &[u8] = b"--More--";
/// Carriage return and erase-to-end-of-line, wiping the `--More--` prompt.
pub const MORE_ERASE: &[u8] = b"\r\x1b[K";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Space: another full page.
    Page,
    /// Enter: one more line.
    Line,
    /// `q`, `Q` or Ctrl-C: discard the rest of the output.
    Quit,
}

pub fn classify(b: u8) -> Option<Key> {
    match b {
        b' ' => Some(Key::Page),
        b'\r' | b'\n' => Some(Key::Line),
        b'q' | b'Q' | 0x03 => Some(Key::Quit),
        _ => None,
    }
}

pub struct Pager {
    /// Lines per page; zero disables paging.
    limit: usize,
    lines: usize,
}

impl Pager {
    pub fn new(limit: usize) -> Self {
        Pager { limit, lines: 0 }
    }

    /// A full page has been shown and the user must be asked to continue.
    pub fn full(&self) -> bool {
        self.limit > 0 && self.lines >= self.limit
    }

    pub fn line(&mut self) {
        self.lines += 1;
    }

    pub fn page(&mut self) {
        self.lines = 0;
    }

    pub fn one_line(&mut self) {
        self.lines = self.limit.saturating_sub(1);
    }
}
