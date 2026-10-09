//! Bridge between synchronous command handlers and the async SSH session.
//!
//! Handlers write to a plain `&mut dyn Write`. They run on tokio's blocking
//! pool and `ChannelWriter` forwards what they write over a small bounded
//! channel to the session task. That gives:
//!
//! * back-pressure: a slow client or a `--More--` prompt stalls the handler
//!   rather than buffering a whole table dump in memory;
//! * early abort: if the user quits the pager or presses Ctrl-C the receiver
//!   is dropped, the next write fails with `BrokenPipe`, and a handler that
//!   propagates it with `?` stops scanning the table straight away.

use std::io::{self, Write};

use tokio::sync::mpsc;

/// Output is sent in chunks of about this size, or on an explicit `flush()`.
/// A handler that wants progressive output from a slow scan can flush.
const CHUNK: usize = 8192;

pub struct ChannelWriter {
    tx: mpsc::Sender<Vec<u8>>,
    buf: Vec<u8>,
}

impl ChannelWriter {
    pub fn new(tx: mpsc::Sender<Vec<u8>>) -> Self {
        ChannelWriter { tx, buf: Vec::with_capacity(CHUNK) }
    }
}

impl Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= CHUNK {
            self.flush()?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let chunk = std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK));
        self.tx.blocking_send(chunk).map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "output aborted or client went away")
        })
    }
}

/// Convert bare LF to CRLF for a pty. Existing CRLF pairs are left alone.
pub fn lf_to_crlf(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + input.len() / 16 + 16);
    let mut prev = 0u8;
    for &b in input {
        if b == b'\n' && prev != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        prev = b;
    }
    out
}
