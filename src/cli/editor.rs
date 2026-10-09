//! A small readline-style line editor driven by raw bytes from an SSH pty.
//!
//! The editor is synchronous and I/O free: the caller feeds it input bytes and
//! it appends the terminal output needed to keep the client's display in step
//! to a buffer. Events that need knowledge of the grammar (TAB, `?`) or the
//! outside world (a finished line) are handed back to the caller one at a
//! time, so output ordering stays correct even when a whole pasted block
//! arrives in a single SSH data packet.
//!
//! Only printable ASCII is accepted into the line, which keeps character and
//! byte offsets identical. Lines longer than the terminal width are not
//! redrawn correctly (the redraw assumes a single physical row).

const HISTORY_LIMIT: usize = 100;

#[derive(Debug, PartialEq)]
pub enum Event {
    /// Enter was pressed; the completed line.
    Line(String),
    Tab,
    Help,
    /// Ctrl-C; the line has already been discarded.
    Interrupt,
    /// Ctrl-D on an empty line.
    Eof,
}

enum Esc {
    None,
    Esc,
    Csi(String),
    Ss3,
}

pub struct Editor {
    prompt: &'static str,
    buf: String,
    cursor: usize,
    history: Vec<String>,
    hist_pos: Option<usize>,
    saved: String,
    esc: Esc,
    last_cr: bool,
}

impl Editor {
    pub fn new(prompt: &'static str) -> Self {
        Editor {
            prompt,
            buf: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_pos: None,
            saved: String::new(),
            esc: Esc::None,
            last_cr: false,
        }
    }

    /// Text to the left of the cursor.
    pub fn before_cursor(&self) -> &str {
        &self.buf[..self.cursor]
    }

    /// Consume input until an event needs the caller's attention, or the
    /// input is exhausted. Display updates are appended to `out`.
    pub fn next(&mut self, input: &mut &[u8], out: &mut Vec<u8>) -> Option<Event> {
        while let Some((&b, rest)) = input.split_first() {
            *input = rest;
            if let Some(ev) = self.byte(b, out) {
                return Some(ev);
            }
        }
        None
    }

    /// Replace the text from `start` up to the cursor and redraw.
    pub fn replace_before_cursor(&mut self, start: usize, text: &str, out: &mut Vec<u8>) {
        self.buf.replace_range(start..self.cursor, text);
        self.cursor = start + text.len();
        self.redraw(out);
    }

    /// Repaint the prompt and line, leaving the cursor in place.
    pub fn redraw(&self, out: &mut Vec<u8>) {
        out.push(b'\r');
        out.extend_from_slice(self.prompt.as_bytes());
        out.extend_from_slice(self.buf.as_bytes());
        out.extend_from_slice(b"\x1b[K");
        let back = self.buf.len() - self.cursor;
        if back > 0 {
            out.extend_from_slice(format!("\x1b[{back}D").as_bytes());
        }
    }

    // ── Input decoding ────────────────────────────────────────────────────

    fn byte(&mut self, b: u8, out: &mut Vec<u8>) -> Option<Event> {
        match std::mem::replace(&mut self.esc, Esc::None) {
            Esc::Esc => {
                self.esc = match b {
                    b'[' => Esc::Csi(String::new()),
                    b'O' => Esc::Ss3,
                    _ => Esc::None,
                };
                return None;
            }
            Esc::Csi(mut params) => {
                if (0x40..=0x7e).contains(&b) {
                    self.csi(&params, b, out);
                } else if params.len() < 16 {
                    params.push(b as char);
                    self.esc = Esc::Csi(params);
                }
                return None;
            }
            Esc::Ss3 => {
                self.csi("", b, out);
                return None;
            }
            Esc::None => {}
        }

        let was_cr = std::mem::replace(&mut self.last_cr, false);
        match b {
            0x1b => self.esc = Esc::Esc,
            b'\r' => {
                self.last_cr = true;
                return Some(self.enter(out));
            }
            b'\n' if !was_cr => return Some(self.enter(out)),
            b'\n' => {}
            0x03 => {
                out.extend_from_slice(b"^C\r\n");
                self.buf.clear();
                self.cursor = 0;
                self.hist_pos = None;
                return Some(Event::Interrupt);
            }
            0x04 if self.buf.is_empty() => return Some(Event::Eof),
            0x04 => self.delete(out),
            0x01 => self.home(out),
            0x05 => self.end(out),
            0x02 => self.left(out),
            0x06 => self.right(out),
            0x0b => {
                self.buf.truncate(self.cursor);
                out.extend_from_slice(b"\x1b[K");
            }
            0x15 => {
                self.buf.drain(..self.cursor);
                self.cursor = 0;
                self.redraw(out);
            }
            0x17 => self.kill_word(out),
            0x0c => {
                out.extend_from_slice(b"\x1b[H\x1b[2J");
                self.redraw(out);
            }
            0x10 => self.history_prev(out),
            0x0e => self.history_next(out),
            0x7f | 0x08 => self.backspace(out),
            0x09 => return Some(Event::Tab),
            b'?' if !self.in_quote() => return Some(Event::Help),
            0x20..=0x7e => self.insert(b as char, out),
            _ => {}
        }
        None
    }

    fn csi(&mut self, params: &str, fin: u8, out: &mut Vec<u8>) {
        match (fin, params) {
            (b'A', _) => self.history_prev(out),
            (b'B', _) => self.history_next(out),
            (b'C', _) => self.right(out),
            (b'D', _) => self.left(out),
            (b'H', _) | (b'~', "1") | (b'~', "7") => self.home(out),
            (b'F', _) | (b'~', "4") | (b'~', "8") => self.end(out),
            (b'~', "3") => self.delete(out),
            _ => {}
        }
    }

    /// Is the cursor inside an open double-quoted string? `?` is literal
    /// there, so regexes can use it.
    fn in_quote(&self) -> bool {
        let mut open = false;
        let mut prev = '\0';
        for c in self.before_cursor().chars() {
            if c == '"' && prev != '\\' {
                open = !open;
            }
            prev = c;
        }
        open
    }

    // ── Editing primitives ────────────────────────────────────────────────

    fn enter(&mut self, out: &mut Vec<u8>) -> Event {
        out.extend_from_slice(b"\r\n");
        let line = std::mem::take(&mut self.buf);
        self.cursor = 0;
        self.hist_pos = None;
        if !line.trim().is_empty() && self.history.last() != Some(&line) {
            self.history.push(line.clone());
            if self.history.len() > HISTORY_LIMIT {
                self.history.remove(0);
            }
        }
        Event::Line(line)
    }

    fn insert(&mut self, c: char, out: &mut Vec<u8>) {
        let at_end = self.cursor == self.buf.len();
        self.buf.insert(self.cursor, c);
        self.cursor += 1;
        if at_end {
            out.push(c as u8);
        } else {
            self.redraw(out);
        }
    }

    fn backspace(&mut self, out: &mut Vec<u8>) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        self.buf.remove(self.cursor);
        if self.cursor == self.buf.len() {
            out.extend_from_slice(b"\x08 \x08");
        } else {
            self.redraw(out);
        }
    }

    fn delete(&mut self, out: &mut Vec<u8>) {
        if self.cursor < self.buf.len() {
            self.buf.remove(self.cursor);
            self.redraw(out);
        }
    }

    fn kill_word(&mut self, out: &mut Vec<u8>) {
        let bytes = self.buf.as_bytes();
        let mut start = self.cursor;
        while start > 0 && bytes[start - 1] == b' ' {
            start -= 1;
        }
        while start > 0 && bytes[start - 1] != b' ' {
            start -= 1;
        }
        self.buf.drain(start..self.cursor);
        self.cursor = start;
        self.redraw(out);
    }

    fn left(&mut self, out: &mut Vec<u8>) {
        if self.cursor > 0 {
            self.cursor -= 1;
            out.extend_from_slice(b"\x1b[D");
        }
    }

    fn right(&mut self, out: &mut Vec<u8>) {
        if self.cursor < self.buf.len() {
            self.cursor += 1;
            out.extend_from_slice(b"\x1b[C");
        }
    }

    fn home(&mut self, out: &mut Vec<u8>) {
        if self.cursor > 0 {
            out.extend_from_slice(format!("\x1b[{}D", self.cursor).as_bytes());
            self.cursor = 0;
        }
    }

    fn end(&mut self, out: &mut Vec<u8>) {
        let n = self.buf.len() - self.cursor;
        if n > 0 {
            out.extend_from_slice(format!("\x1b[{n}C").as_bytes());
            self.cursor = self.buf.len();
        }
    }

    fn history_prev(&mut self, out: &mut Vec<u8>) {
        let pos = match self.hist_pos {
            None if self.history.is_empty() => return,
            None => {
                self.saved = self.buf.clone();
                self.history.len() - 1
            }
            Some(0) => return,
            Some(i) => i - 1,
        };
        self.hist_pos = Some(pos);
        self.set_line(self.history[pos].clone(), out);
    }

    fn history_next(&mut self, out: &mut Vec<u8>) {
        match self.hist_pos {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.hist_pos = Some(i + 1);
                self.set_line(self.history[i + 1].clone(), out);
            }
            Some(_) => {
                self.hist_pos = None;
                let saved = std::mem::take(&mut self.saved);
                self.set_line(saved, out);
            }
        }
    }

    fn set_line(&mut self, line: String, out: &mut Vec<u8>) {
        self.cursor = line.len();
        self.buf = line;
        self.redraw(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(ed: &mut Editor, s: &[u8]) -> (Vec<Event>, Vec<u8>) {
        let (mut rest, mut out, mut evs) = (s, Vec::new(), Vec::new());
        while let Some(ev) = ed.next(&mut rest, &mut out) {
            evs.push(ev);
        }
        (evs, out)
    }

    #[test]
    fn line_editing_and_history() {
        let mut ed = Editor::new("> ");
        let (evs, _) = feed(&mut ed, b"abc\x1b[D\x1b[DX\r");
        assert_eq!(evs, vec![Event::Line("aXbc".into())]);
        let (evs, _) = feed(&mut ed, b"\x1b[A\x15zzz\r");
        assert_eq!(evs, vec![Event::Line("zzz".into())]);
        let (evs, _) = feed(&mut ed, b"\x1b[A\x1b[A\r");
        assert_eq!(evs, vec![Event::Line("aXbc".into())]);
    }

    #[test]
    fn help_is_literal_inside_quotes() {
        let mut ed = Editor::new("> ");
        assert_eq!(feed(&mut ed, b"x ?").0, vec![Event::Help]);
        let (evs, _) = feed(&mut ed, b"\x15x \"a?");
        assert!(evs.is_empty());
        assert_eq!(ed.before_cursor(), "x \"a?");
    }

    #[test]
    fn crlf_is_one_enter_and_split_escapes_survive() {
        let mut ed = Editor::new("> ");
        assert_eq!(feed(&mut ed, b"a\r\n").0, vec![Event::Line("a".into())]);
        feed(&mut ed, b"bc\x1b");
        feed(&mut ed, b"[");
        feed(&mut ed, b"D");
        assert_eq!(feed(&mut ed, b"\r").0, vec![Event::Line("bc".into())]);
        assert_eq!(feed(&mut ed, b"\x04").0, vec![Event::Eof]);
    }
}
