//! SSH front end: serves the shared `crate::cli` shell over russh.
//!
//! This module is only transport glue: authentication, pty / shell / exec /
//! window-change requests, the host key, and an `SshTransport` that carries
//! the shell's output back over the channel.

use std::sync::Arc;

use russh::keys::ssh_key;
use russh::server::{self, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::rib::MrtRibEntry;
use crate::routing_table::RoutingTable;

use crate::cli::commands::OutputFormat;
use crate::cli::handlers::TableSummary;
use crate::cli::shell::{self, Input, Params, Transport};

// ─── Transport ────────────────────────────────────────────────────────────────

/// Carries a shell session's output over an SSH channel.
struct SshTransport {
    handle: server::Handle,
    channel: ChannelId,
}

impl Transport for SshTransport {
    async fn write(&mut self, data: Vec<u8>) -> bool {
        self.handle.data(self.channel, data).await.is_ok()
    }

    async fn write_err(&mut self, data: Vec<u8>) -> bool {
        self.handle.extended_data(self.channel, 1, data).await.is_ok()
    }

    async fn finish(self, code: u32) {
        let _ = self.handle.exit_status_request(self.channel, code).await;
        let _ = self.handle.eof(self.channel).await;
        let _ = self.handle.close(self.channel).await;
    }
}

// ─── Server factory ───────────────────────────────────────────────────────────

struct SshServer {
    table: Arc<RoutingTable<MrtRibEntry>>,
    summary: Arc<TableSummary>,
    format: OutputFormat,
}

impl server::Server for SshServer {
    type Handler = ClientSession;

    fn new_client(&mut self, peer: Option<std::net::SocketAddr>) -> ClientSession {
        ClientSession {
            table: Arc::clone(&self.table),
            summary: Arc::clone(&self.summary),
            format: self.format,
            peer,
            pty: None,
            input_tx: None,
        }
    }

    fn handle_session_error(&mut self, error: <ClientSession as server::Handler>::Error) {
        eprintln!("SSH session error: {error:#?}");
    }
}

// ─── Per-connection handler ───────────────────────────────────────────────────

/// Thin russh glue: it records the connection's pty, then hands the channel to
/// a `cli::shell` task and forwards client input to it.
struct ClientSession {
    table: Arc<RoutingTable<MrtRibEntry>>,
    summary: Arc<TableSummary>,
    format: OutputFormat,
    peer: Option<std::net::SocketAddr>,
    pty: Option<(u32, u32)>,
    input_tx: Option<mpsc::UnboundedSender<Input>>,
}

impl ClientSession {
    fn params(&self) -> Params {
        Params {
            table: Arc::clone(&self.table),
            summary: Arc::clone(&self.summary),
            peer: self.peer,
            format: self.format,
            pty: self.pty,
        }
    }

    fn forward(&self, input: Input) {
        if let Some(tx) = &self.input_tx {
            let _ = tx.send(input);
        }
    }
}

impl server::Handler for ClientSession {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        _: &str,
        _key: &ssh_key::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        Ok(server::Auth::Accept)
    }

    async fn auth_openssh_certificate(
        &mut self,
        _user: &str,
        _cert: &russh::keys::Certificate,
    ) -> Result<server::Auth, Self::Error> {
        Ok(server::Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.pty = Some((col_width, row_height));
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.pty = Some((col_width, row_height));
        self.forward(Input::Resize { cols: col_width, rows: row_height });
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let (tx, rx) = mpsc::unbounded_channel();
        self.input_tx = Some(tx);
        let transport = SshTransport { handle: session.handle(), channel };
        tokio::spawn(shell::run_shell(self.params(), transport, rx));
        Ok(())
    }

    /// `ssh host "show route 192.0.2.1"`: one command, no pty session.
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let command = String::from_utf8_lossy(data).into_owned();
        let (tx, rx) = mpsc::unbounded_channel();
        self.input_tx = Some(tx);
        let transport = SshTransport { handle: session.handle(), channel };
        tokio::spawn(shell::run_exec(self.params(), transport, rx, command));
        Ok(())
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.forward(Input::Data(data.to_vec()));
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.forward(Input::Eof);
        Ok(())
    }

    async fn channel_close(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Dropping the sender ends the shell task.
        self.input_tx = None;
        Ok(())
    }
}

// ─── Host key persistence ─────────────────────────────────────────────────────

// ─── Host key persistence ─────────────────────────────────────────────────────

fn load_or_generate_key() -> anyhow::Result<russh::keys::PrivateKey> {
    use std::os::unix::fs::PermissionsExt;

    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME not set"))?;
    let priv_path = std::path::PathBuf::from(&home).join(".mrtdump_ssh_id_rsa");
    let pub_path  = std::path::PathBuf::from(&home).join(".mrtdump_ssh_id_rsa.pub");

    if priv_path.exists() {
        let pem = std::fs::read_to_string(&priv_path)?;
        let key = russh::keys::PrivateKey::from_openssh(pem.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to load {}: {e}", priv_path.display()))?;
        eprintln!("Loaded SSH host key from {}", priv_path.display());
        return Ok(key);
    }

    let key = russh::keys::PrivateKey::random(
        &mut rand::rng(),
        russh::keys::Algorithm::Ed25519,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Private key — OpenSSH PEM format, mode 600
    let pem = key.to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("Failed to serialise host key: {e}"))?;
    std::fs::write(&priv_path, pem.as_bytes())?;
    std::fs::set_permissions(&priv_path, std::fs::Permissions::from_mode(0o600))?;

    // Public key — authorized_keys format
    let pub_str = key.public_key().to_openssh()
        .map_err(|e| anyhow::anyhow!("Failed to serialise public key: {e}"))?;
    std::fs::write(&pub_path, format!("{}\n", pub_str))?;

    eprintln!("Generated SSH host key, saved to {}", priv_path.display());
    Ok(key)
}


// ─── Entry point ─────────────────────────────────────────────────────────────

pub async fn run(
    table: Arc<RoutingTable<MrtRibEntry>>,
    port: u16,
    juniper: bool,
    terse: bool,
) -> anyhow::Result<()> {
    let config = Arc::new(russh::server::Config {
        inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
        auth_rejection_time: std::time::Duration::from_secs(1),
        auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
        keys: vec![load_or_generate_key()?],
        ..Default::default()
    });

    let summary = Arc::new(TableSummary::from_table(&table));
    let mut server = SshServer {
        table,
        summary,
        format: OutputFormat::from_flags(juniper, terse),
    };
    let socket = TcpListener::bind(("0.0.0.0", port)).await?;
    eprintln!("SSH server listening on 0.0.0.0:{}", port);
    server.run_on_socket(config, &socket).await?;
    Ok(())
}
