//! The local terminal front end, for `mrtdump -i`.
//!
//! Runs the same shell as the SSH server. When stdin and stdout are both
//! terminals the terminal is put into raw mode and the shell gets the full
//! line editor, prompt and paging; otherwise (`echo 'show route ...' |
//! mrtdump -i file`) it reads plain lines with no prompt, echo or paging.

use std::io::{self, Read, Write};
use std::sync::Arc;

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;

use crate::rib::MrtRibEntry;
use crate::routing_table::RoutingTable;

use super::commands::OutputFormat;
use super::handlers::TableSummary;
use super::shell::{self, Input, Params, Transport};

struct LocalTransport;

impl Transport for LocalTransport {
    async fn write(&mut self, data: Vec<u8>) -> bool {
        let mut out = io::stdout().lock();
        out.write_all(&data).and_then(|()| out.flush()).is_ok()
    }

    async fn write_err(&mut self, data: Vec<u8>) -> bool {
        let mut err = io::stderr().lock();
        err.write_all(&data).and_then(|()| err.flush()).is_ok()
    }

    async fn finish(self, _code: u32) {}
}

/// Raw terminal mode on stdin, restored when dropped (including on panic).
struct RawMode {
    saved: libc::termios,
}

impl RawMode {
    fn enable() -> io::Result<Self> {
        // SAFETY: plain termios calls on fd 0 with a zeroed, then filled, struct.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return Err(io::Error::last_os_error());
            }
            let saved = t;
            libc::cfmakeraw(&mut t);
            if libc::tcsetattr(0, libc::TCSANOW, &t) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(RawMode { saved })
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: restores the attributes captured in `enable`.
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.saved);
        }
    }
}

fn is_tty() -> bool {
    // SAFETY: isatty only inspects the descriptor.
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

/// Terminal size as (cols, rows), defaulting to 80x24 if it can't be read.
fn window_size() -> (u32, u32) {
    // SAFETY: TIOCGWINSZ fills the winsize struct we pass.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            (ws.ws_col as u32, ws.ws_row as u32)
        } else {
            (80, 24)
        }
    }
}

/// Run the interactive shell on the local terminal until the user exits or
/// stdin reaches end of file.
pub fn run(table: Arc<RoutingTable<MrtRibEntry>>, format: OutputFormat) -> anyhow::Result<()> {
    let tty = is_tty();
    let _raw = if tty { Some(RawMode::enable()?) } else { None };

    let params = Params {
        summary: Arc::new(TableSummary::from_table(&table)),
        table,
        peer: None,
        format,
        pty: tty.then(window_size),
    };

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    rt.block_on(async move {
        let (tx, rx) = mpsc::unbounded_channel();

        // Blocking stdin reads get a thread of their own.
        let stdin_tx = tx.clone();
        std::thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            let mut buf = [0u8; 4096];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stdin_tx.send(Input::Data(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = stdin_tx.send(Input::Eof);
        });

        if tty {
            let mut winch = signal(SignalKind::window_change())?;
            tokio::spawn(async move {
                while winch.recv().await.is_some() {
                    let (cols, rows) = window_size();
                    if tx.send(Input::Resize { cols, rows }).is_err() {
                        break;
                    }
                }
            });
        }

        shell::run_shell(params, LocalTransport, rx).await;
        Ok(())
    })
}
