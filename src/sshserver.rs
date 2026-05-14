use std::sync::Arc;

use russh::keys::ssh_key;
use russh::server::{self, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use tokio::net::TcpListener;

use crate::routing_table::RoutingTable;
use crate::rib::MrtRibEntry;

// ─── Server factory ───────────────────────────────────────────────────────────

struct SshServer {
    table: Arc<RoutingTable<MrtRibEntry>>,
    juniper: bool,
    terse: bool,
}

impl server::Server for SshServer {
    type Handler = ClientSession;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> ClientSession {
        ClientSession {
            table: Arc::clone(&self.table),
            juniper: self.juniper,
            terse: self.terse,
            line_buf: Vec::new(),
            channel_id: None,
        }
    }

    fn handle_session_error(&mut self, error: <ClientSession as server::Handler>::Error) {
        eprintln!("SSH session error: {error:#?}");
    }
}

// ─── Per-connection handler ───────────────────────────────────────────────────

struct ClientSession {
    table: Arc<RoutingTable<MrtRibEntry>>,
    juniper: bool,
    terse: bool,
    line_buf: Vec<u8>,
    channel_id: Option<ChannelId>,
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
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        self.channel_id = Some(channel.id());
        Ok(true)
    }

    async fn pty_request(
        &mut self,
        _channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.data(channel, b"router> ".to_vec())?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        for &byte in data {
            match byte {
                3 | 4 => {
                    // Ctrl+C / Ctrl+D
                    return Err(russh::Error::Disconnect);
                }
                0x7f | 0x08 => {
                    // Backspace / DEL
                    if !self.line_buf.is_empty() {
                        self.line_buf.pop();
                        session.data(channel, b"\x08 \x08".to_vec())?;
                    }
                }
                b'\r' | b'\n' => {
                    session.data(channel, b"\r\n".to_vec())?;
                    let line = std::str::from_utf8(&self.line_buf)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    self.line_buf.clear();

                    if !line.is_empty() {
                        let mut out: Vec<u8> = Vec::new();
                        crate::execute_query(&line, &self.table, &mut out,
                                             self.juniper, self.terse).ok();
                        let ssh_out = lf_to_crlf(&out);
                        if !ssh_out.is_empty() {
                            session.data(channel, ssh_out)?;
                        }
                    }
                    session.data(channel, b"router> ".to_vec())?;
                }
                _ => {
                    self.line_buf.push(byte);
                    session.data(channel, vec![byte])?;
                }
            }
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.close(channel)?;
        Ok(())
    }
}

fn lf_to_crlf(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + 16);
    for &b in input {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

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

    let mut server = SshServer { table, juniper, terse };
    let socket = TcpListener::bind(("0.0.0.0", port)).await?;
    eprintln!("SSH server listening on 0.0.0.0:{}", port);
    server.run_on_socket(config, &socket).await?;
    Ok(())
}
