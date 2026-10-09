//! The per-session shell task.
//!
//! A front end forwards everything the user types (`Input`) over a channel to
//! one task per session (this module), which owns the line editor and writes
//! back through a [`Transport`]. That suits russh, whose `Handler::data` must
//! return promptly and so cannot wait for a `--More--` keypress, and it lets
//! the SSH server and the local terminal share all of the shell. Commands run
//! on the blocking pool and stream their output back through
//! `sink::ChannelWriter`.
//!
//! Two entry points:
//! * [`run_shell`]: an interactive shell. With a pty it gets the line editor,
//!   MOTD, prompt and paging; without one (`ssh -T host < script`, or stdin
//!   redirected locally) it reads plain lines, with no echo, prompt or paging.
//! * [`run_exec`]: a single command, e.g. from an SSH exec request. No paging, no
//!   prompt, errors on stderr, and the command's success as the exit status.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::rib::MrtRibEntry;
use crate::routing_table::RoutingTable;

use super::commands::{self, Command, OutputFormat};
use super::editor::{Editor, Event};
use super::handlers::{self, TableSummary};
use super::pager::{self, Key, Pager};
use super::sink::{lf_to_crlf, ChannelWriter};

pub const PROMPT: &str = "router> ";

/// Lines per page when the client reports no usable window size.
const DEFAULT_PAGE_LENGTH: usize = 24;

/// Where a session's output goes. Each write returns false once the other
/// end has gone away.
pub trait Transport: Send + 'static {
    fn write(&mut self, data: Vec<u8>) -> impl Future<Output = bool> + Send;
    /// Diagnostics for a session that has no pty (stderr).
    fn write_err(&mut self, data: Vec<u8>) -> impl Future<Output = bool> + Send;
    /// End the session, reporting `code` as its exit status.
    fn finish(self, code: u32) -> impl Future<Output = ()> + Send;
}

/// What the user sent, as seen by the shell task.
pub enum Input {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    Eof,
}

/// Everything a session needs to know about its connection.
pub struct Params {
    pub table: Arc<RoutingTable<MrtRibEntry>>,
    pub summary: Arc<TableSummary>,
    pub peer: Option<SocketAddr>,
    pub format: OutputFormat,
    /// Window size (cols, rows) if the session has a terminal.
    pub pty: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Done,
    Failed,
    Exit,
}

enum Ev {
    Input(Option<Input>),
    Chunk(Option<Vec<u8>>),
}

struct Shell<T: Transport> {
    transport: T,
    input: mpsc::UnboundedReceiver<Input>,
    params: Params,
    format: OutputFormat,
    /// A pty is attached: output needs CRLF line endings.
    tty: bool,
    /// Line editor, paging and Ctrl-C handling; shell sessions with a pty only.
    interactive: bool,
    cols: u32,
    rows: u32,
    /// Set by `terminal length`; otherwise derived from the window height.
    term_length: Option<usize>,
    /// The client sent EOF.
    eof: bool,
    /// The channel is gone; stop talking.
    closed: bool,
    editor: Editor,
    /// Partial line for sessions without a pty.
    linebuf: Vec<u8>,
}

pub async fn run_shell<T: Transport>(
    params: Params,
    transport: T,
    input: mpsc::UnboundedReceiver<Input>,
) {
    let interactive = params.pty.is_some();
    let mut sh = Shell::new(params, transport, input, interactive);
    if sh.interactive {
        let mut out = Vec::new();
        handlers::motd(&mut out, &sh.params.summary, sh.params.peer).ok();
        out.extend_from_slice(PROMPT.as_bytes());
        sh.send(&out).await;
    }
    sh.shell_loop().await;
    sh.finish(0).await;
}

pub async fn run_exec<T: Transport>(
    params: Params,
    transport: T,
    input: mpsc::UnboundedReceiver<Input>,
    command: String,
) {
    let mut sh = Shell::new(params, transport, input, false);
    let outcome = sh.exec_line(&command).await;
    sh.finish(if outcome == Outcome::Failed { 1 } else { 0 }).await;
}

impl<T: Transport> Shell<T> {
    fn new(
        params: Params,
        transport: T,
        input: mpsc::UnboundedReceiver<Input>,
        interactive: bool,
    ) -> Self {
        let (cols, rows) = params.pty.unwrap_or((0, 0));
        Shell {
            transport,
            input,
            format: params.format,
            tty: params.pty.is_some(),
            interactive,
            cols,
            rows,
            term_length: None,
            eof: false,
            closed: false,
            editor: Editor::new(PROMPT),
            linebuf: Vec::new(),
            params,
        }
    }

    // ── Output ────────────────────────────────────────────────────────────

    async fn send(&mut self, bytes: &[u8]) {
        if bytes.is_empty() || self.closed {
            return;
        }
        let data = if self.tty { lf_to_crlf(bytes) } else { bytes.to_vec() };
        if !self.transport.write(data).await {
            self.closed = true;
        }
    }

    /// Diagnostics go to stderr unless a pty merges the streams anyway.
    async fn send_err(&mut self, text: &str) {
        if self.tty {
            self.send(text.as_bytes()).await;
        } else if !self.closed {
            if !self.transport.write_err(text.as_bytes().to_vec()).await {
                self.closed = true;
            }
        }
    }

    async fn finish(self, code: u32) {
        self.transport.finish(code).await;
    }

    fn resize(&mut self, cols: u32, rows: u32) {
        self.cols = cols;
        self.rows = rows;
    }

    /// Lines per page; zero means no paging.
    fn page_length(&self) -> usize {
        if !self.interactive {
            return 0;
        }
        self.term_length.unwrap_or(if self.rows > 1 {
            self.rows as usize - 1
        } else {
            DEFAULT_PAGE_LENGTH
        })
    }

    // ── Input loops ───────────────────────────────────────────────────────

    async fn shell_loop(&mut self) {
        while let Some(input) = self.input.recv().await {
            let keep_going = match input {
                Input::Resize { cols, rows } => {
                    self.resize(cols, rows);
                    true
                }
                Input::Eof => {
                    self.eof = true;
                    false
                }
                Input::Data(data) if self.interactive => self.on_data_tty(data).await,
                Input::Data(data) => self.on_data_lines(&data).await,
            };
            if !keep_going || self.eof || self.closed {
                break;
            }
        }
        // A final line without a newline, from a piped script.
        if !self.interactive && !self.closed && !self.linebuf.is_empty() {
            let line = String::from_utf8_lossy(&std::mem::take(&mut self.linebuf)).into_owned();
            self.exec_line(&line).await;
        }
    }

    /// Interactive input through the line editor. Returns false to end the session.
    async fn on_data_tty(&mut self, data: Vec<u8>) -> bool {
        let mut rest = &data[..];
        let mut out = Vec::new();
        while let Some(ev) = self.editor.next(&mut rest, &mut out) {
            match ev {
                Event::Line(line) => {
                    self.send(&out).await;
                    out.clear();
                    if self.exec_line(&line).await == Outcome::Exit || self.closed || self.eof {
                        return false;
                    }
                    out.extend_from_slice(PROMPT.as_bytes());
                }
                Event::Tab => self.complete(&mut out),
                Event::Help => self.help(&mut out),
                Event::Interrupt => out.extend_from_slice(PROMPT.as_bytes()),
                Event::Eof => {
                    out.extend_from_slice(b"\r\n");
                    self.send(&out).await;
                    return false;
                }
            }
        }
        self.send(&out).await;
        true
    }

    /// Plain line input with no echo, for shells without a pty.
    async fn on_data_lines(&mut self, data: &[u8]) -> bool {
        for &b in data {
            if b != b'\n' {
                self.linebuf.push(b);
                continue;
            }
            let raw = std::mem::take(&mut self.linebuf);
            let line = String::from_utf8_lossy(&raw);
            let outcome = self.exec_line(line.trim_end_matches('\r')).await;
            if outcome == Outcome::Exit || self.closed {
                return false;
            }
        }
        true
    }

    // ── TAB and ? ─────────────────────────────────────────────────────────

    fn complete(&mut self, out: &mut Vec<u8>) {
        let before = self.editor.before_cursor().to_string();
        let Ok(s) = commands::suggest(&before) else {
            out.push(0x07);
            return;
        };
        if s.keywords.is_empty() {
            out.push(0x07);
            return;
        }
        let names: Vec<&str> = s.keywords.iter().map(|k| k.0).collect();
        let common = common_prefix(&names);
        if names.len() == 1 {
            self.editor.replace_before_cursor(s.start, &format!("{} ", names[0]), out);
        } else if common.len() > s.partial.len() {
            self.editor.replace_before_cursor(s.start, common, out);
        } else {
            out.extend_from_slice(format!("\r\n{}\r\n", names.join("  ")).as_bytes());
            self.editor.redraw(out);
        }
    }

    fn help(&mut self, out: &mut Vec<u8>) {
        let before = self.editor.before_cursor().to_string();
        out.extend_from_slice(b"\r\n");
        match commands::suggest(&before) {
            Ok(s) => out.extend_from_slice(s.render_help().as_bytes()),
            Err(e) => out.extend_from_slice(e.render(PROMPT.len(), true).as_bytes()),
        }
        self.editor.redraw(out);
    }

    // ── Command execution ─────────────────────────────────────────────────

    async fn exec_line(&mut self, line: &str) -> Outcome {
        if line.trim().is_empty() {
            return Outcome::Done;
        }
        match commands::parse(line) {
            Err(e) => {
                let msg = e.render(PROMPT.len(), self.interactive);
                self.send_err(&msg).await;
                Outcome::Failed
            }
            Ok(Command::Exit) => Outcome::Exit,
            Ok(Command::Output(f)) => {
                self.format = f;
                Outcome::Done
            }
            Ok(Command::TerminalLength(n)) => {
                self.term_length = Some(n);
                Outcome::Done
            }
            Ok(cmd) => self.run_command(cmd).await,
        }
    }

    /// Run a query handler on the blocking pool and page its output to the
    /// client. Ctrl-C, quitting the pager or a dropped connection abort it.
    async fn run_command(&mut self, cmd: Command) -> Outcome {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(4);
        let table = Arc::clone(&self.params.table);
        let format = self.format;
        let job = tokio::task::spawn_blocking(move || {
            let mut w = ChannelWriter::new(tx);
            let r = handlers::dispatch(&table, format, cmd, &mut w);
            r.and_then(|()| io::Write::flush(&mut w))
        });

        let mut pager = Pager::new(self.page_length());
        let mut aborted = false;
        while !aborted {
            let ev = tokio::select! {
                i = self.input.recv() => Ev::Input(i),
                c = rx.recv() => Ev::Chunk(c),
            };
            match ev {
                Ev::Chunk(Some(chunk)) => aborted = !self.page_out(&mut pager, &chunk).await,
                Ev::Chunk(None) => break,
                Ev::Input(Some(Input::Data(d))) => {
                    if self.interactive && d.contains(&0x03) {
                        self.send(b"^C\n").await;
                        aborted = true;
                    }
                }
                Ev::Input(Some(Input::Resize { cols, rows })) => self.resize(cols, rows),
                Ev::Input(Some(Input::Eof)) => {
                    self.eof = true;
                    aborted = self.interactive;
                }
                Ev::Input(None) => {
                    self.closed = true;
                    aborted = true;
                }
            }
        }
        drop(rx);

        match job.await {
            Ok(Ok(())) => Outcome::Done,
            Ok(Err(e)) if e.kind() == io::ErrorKind::BrokenPipe => Outcome::Done,
            Ok(Err(e)) => {
                self.send_err(&format!("% Error: {e}\n")).await;
                Outcome::Failed
            }
            Err(_) => {
                self.send_err("% Internal error while running command\n").await;
                Outcome::Failed
            }
        }
    }

    /// Send a chunk of handler output, pausing at `--More--` whenever a page
    /// is full. Returns false if the output should be abandoned.
    async fn page_out(&mut self, pager: &mut Pager, chunk: &[u8]) -> bool {
        let mut out: Vec<u8> = Vec::new();
        for seg in chunk.split_inclusive(|&b| b == b'\n') {
            if pager.full() {
                self.send(&out).await;
                out.clear();
                self.send(pager::MORE_PROMPT).await;
                let key = self.wait_key().await;
                self.send(pager::MORE_ERASE).await;
                match key {
                    Some(Key::Page) => pager.page(),
                    Some(Key::Line) => pager.one_line(),
                    Some(Key::Quit) | None => return false,
                }
            }
            out.extend_from_slice(seg);
            if seg.ends_with(b"\n") {
                pager.line();
            }
        }
        self.send(&out).await;
        !self.closed
    }

    async fn wait_key(&mut self) -> Option<Key> {
        loop {
            match self.input.recv().await {
                Some(Input::Data(d)) => {
                    if let Some(key) = d.iter().find_map(|&b| pager::classify(b)) {
                        return Some(key);
                    }
                }
                Some(Input::Resize { cols, rows }) => self.resize(cols, rows),
                Some(Input::Eof) => {
                    self.eof = true;
                    return None;
                }
                None => {
                    self.closed = true;
                    return None;
                }
            }
        }
    }
}

fn common_prefix<'a>(names: &[&'a str]) -> &'a str {
    let first = names[0];
    let mut len = first.len();
    for n in &names[1..] {
        len = len.min(first.bytes().zip(n.bytes()).take_while(|(a, b)| a == b).count());
    }
    &first[..len]
}
