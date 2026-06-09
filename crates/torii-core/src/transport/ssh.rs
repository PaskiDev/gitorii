//! Smart SSH transport over russh (pure-Rust, no openssl/libssh2).
//!
//! Each Service maps to one SSH exec:
//!   UploadPackLs / UploadPack    → `git-upload-pack 'owner/repo.git'`
//!   ReceivePackLs / ReceivePack  → `git-receive-pack 'owner/repo.git'`
//!
//! Auth order: ssh-agent (SSH_AUTH_SOCK) → ed25519 key on disk → rsa key on disk.
//! Host verification: ~/.ssh/known_hosts. TOFU prompt on unknown hosts (tty only).

use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use git2::{Error, Remote};
use russh::client;
#[cfg(unix)]
use russh::keys::agent::client::AgentClient;
use russh::keys::ssh_key::PublicKey;
use russh::keys::{check_known_hosts, load_secret_key, PrivateKeyWithHashAlg};
use russh::ChannelMsg;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

pub fn factory(remote: &Remote<'_>) -> Result<Transport, Error> {
    Transport::smart(remote, false, SshSubtransport)
}

struct SshSubtransport;

impl SmartSubtransport for SshSubtransport {
    fn action(
        &self,
        url: &str,
        action: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, Error> {
        let (user, host, port, path) = parse_ssh_url(url)?;
        let cmd = match action {
            Service::UploadPackLs | Service::UploadPack => format!("git-upload-pack '{}'", path),
            Service::ReceivePackLs | Service::ReceivePack => {
                format!("git-receive-pack '{}'", path)
            }
        };
        let stream = SshStream::connect(user, host, port, cmd)
            .map_err(|e| Error::from_str(&format!("ssh transport: {}", e)))?;
        Ok(Box::new(stream))
    }

    fn close(&self) -> Result<(), Error> {
        Ok(())
    }
}

/// Parse `ssh://user@host:port/path` and `user@host:path` (scp-like).
fn parse_ssh_url(url: &str) -> Result<(String, String, u16, String), Error> {
    if let Some(rest) = url.strip_prefix("ssh://") {
        let (userhost, path) = rest
            .split_once('/')
            .ok_or_else(|| Error::from_str("ssh url missing path"))?;
        let (user, hostport) = userhost
            .split_once('@')
            .ok_or_else(|| Error::from_str("ssh url missing user"))?;
        let (host, port) = match hostport.split_once(':') {
            Some((h, p)) => (
                h.to_string(),
                p.parse().map_err(|_| Error::from_str("bad port"))?,
            ),
            None => (hostport.to_string(), 22),
        };
        return Ok((user.to_string(), host, port, path.to_string()));
    }
    // scp-like: git@github.com:owner/repo.git
    let (user, rest) = url
        .split_once('@')
        .ok_or_else(|| Error::from_str("ssh url missing user"))?;
    let (host, path) = rest
        .split_once(':')
        .ok_or_else(|| Error::from_str("ssh url missing path"))?;
    Ok((user.to_string(), host.to_string(), 22, path.to_string()))
}

/// Bridges libgit2's blocking Read/Write to russh's async Channel.
struct SshStream {
    runtime: Arc<Runtime>,
    rx: mpsc::Receiver<Vec<u8>>,
    leftover: Vec<u8>,
    tx: mpsc::Sender<Vec<u8>>,
    eof: bool,
}

impl SshStream {
    fn connect(
        user: String,
        host: String,
        port: u16,
        cmd: String,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
        );
        let (read_tx, read_rx) = mpsc::channel::<Vec<u8>>(32);
        let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(32);

        runtime.block_on(async {
            let handler = Handler::new(host.clone(), port);

            let config = Arc::new(client::Config {
                inactivity_timeout: Some(std::time::Duration::from_secs(60)),
                ..Default::default()
            });
            let mut session = client::connect(config, (host.as_str(), port), handler).await?;

            authenticate(&mut session, &user).await?;

            let mut channel = session.channel_open_session().await?;
            channel.exec(true, cmd.as_bytes()).await?;

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        msg = channel.wait() => {
                            match msg {
                                Some(ChannelMsg::Data { data })
                                    if read_tx.send(data.to_vec()).await.is_err() => {
                                        break;
                                    }
                                Some(ChannelMsg::Eof) | Some(ChannelMsg::ExitStatus { .. }) => {}
                                None => break,
                                _ => {}
                            }
                        }
                        out = write_rx.recv() => {
                            match out {
                                Some(buf) => {
                                    if channel.data(&buf[..]).await.is_err() {
                                        break;
                                    }
                                }
                                None => {
                                    let _ = channel.eof().await;
                                }
                            }
                        }
                    }
                }
            });

            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        })?;

        Ok(Self {
            runtime,
            rx: read_rx,
            leftover: Vec::new(),
            tx: write_tx,
            eof: false,
        })
    }
}

/// Try ssh-agent first; fall back to ed25519/rsa keys on disk.
/// Aggregates failures so the user sees what was attempted.
async fn authenticate(
    session: &mut client::Handle<Handler>,
    user: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut tried: Vec<String> = Vec::new();

    if std::env::var_os("SSH_AUTH_SOCK").is_some() {
        match try_agent(session, user).await {
            Ok(true) => return Ok(()),
            Ok(false) => tried.push("ssh-agent (no key accepted)".into()),
            Err(e) => tried.push(format!("ssh-agent ({})", e)),
        }
    } else {
        tried.push("ssh-agent (SSH_AUTH_SOCK not set)".into());
    }

    if let Some(path) = ssh_key_path() {
        match try_disk_key(session, user, &path).await {
            Ok(true) => return Ok(()),
            Ok(false) => tried.push(format!("disk key {:?} (rejected)", path)),
            Err(e) => tried.push(format!("disk key {:?} ({})", path, e)),
        }
    } else {
        tried.push("disk key (no id_ed25519 / id_rsa in ~/.ssh)".into());
    }

    Err(format!("ssh auth failed. tried: {}", tried.join("; ")).into())
}

#[cfg(unix)]
async fn try_agent(
    session: &mut client::Handle<Handler>,
    user: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut agent = AgentClient::connect_env().await?;
    let identities = agent.request_identities().await?;
    if identities.is_empty() {
        return Ok(false);
    }
    let hash = session.best_supported_rsa_hash().await?.flatten();
    for id in identities {
        let pubkey = id.public_key().into_owned();
        let result = session
            .authenticate_publickey_with(user, pubkey, hash, &mut agent)
            .await?;
        if result.success() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// On Windows there's no ssh-agent socket path in russh's
/// `AgentClient::connect_env` (Unix-only API). Skip agent auth
/// cleanly so the caller falls through to the disk-key branch.
#[cfg(not(unix))]
async fn try_agent(
    _session: &mut client::Handle<Handler>,
    _user: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    Ok(false)
}

async fn try_disk_key(
    session: &mut client::Handle<Handler>,
    user: &str,
    path: &PathBuf,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let key = load_secret_key(path, None)?;
    let hash = session.best_supported_rsa_hash().await?.flatten();
    let result = session
        .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
        .await?;
    Ok(result.success())
}

impl Read for SshStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.leftover.is_empty() {
            let n = self.leftover.len().min(buf.len());
            buf[..n].copy_from_slice(&self.leftover[..n]);
            self.leftover.drain(..n);
            return Ok(n);
        }
        if self.eof {
            return Ok(0);
        }
        match self.runtime.block_on(self.rx.recv()) {
            Some(data) => {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                if n < data.len() {
                    self.leftover.extend_from_slice(&data[n..]);
                }
                Ok(n)
            }
            None => {
                self.eof = true;
                Ok(0)
            }
        }
    }
}

impl Write for SshStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let owned = buf.to_vec();
        let n = owned.len();
        self.runtime
            .block_on(self.tx.send(owned))
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn ssh_key_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    for name in ["id_ed25519", "id_rsa"] {
        let p = PathBuf::from(&home).join(".ssh").join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn known_hosts_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ssh").join("known_hosts"))
}

struct Handler {
    host: String,
    port: u16,
    /// Set true once we've prompted/appended for this connection so we don't
    /// re-prompt on reconnects within the same handler instance.
    decided: StdMutex<bool>,
}

impl Handler {
    fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            decided: StdMutex::new(false),
        }
    }
}

impl client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        match check_known_hosts(&self.host, self.port, key) {
            Ok(true) => Ok(true),
            Ok(false) => {
                // Unknown host. TOFU: prompt if tty + STRICT not set, else reject.
                let strict = matches!(
                    std::env::var("TORII_SSH_STRICT").as_deref(),
                    Ok("1") | Ok("true") | Ok("yes")
                );
                if strict {
                    eprintln!(
                        "ssh: host {}:{} not in known_hosts and TORII_SSH_STRICT is set; rejecting",
                        self.host, self.port
                    );
                    return Ok(false);
                }
                if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
                    eprintln!(
                        "ssh: host {}:{} not in known_hosts (no tty to prompt). \
                         Run interactively once to accept, or set TORII_SSH_STRICT=0 explicitly.",
                        self.host, self.port
                    );
                    return Ok(false);
                }
                let mut decided = self.decided.lock().unwrap();
                if *decided {
                    return Ok(true);
                }
                let fp = key.fingerprint(Default::default());
                eprintln!();
                eprintln!(
                    "⚠️  Host {}:{} is not in known_hosts.",
                    self.host, self.port
                );
                eprintln!("    fingerprint: {}", fp);
                eprint!("    Trust and continue? [y/N]: ");
                let _ = std::io::Write::flush(&mut std::io::stderr());
                let mut answer = String::new();
                if std::io::stdin().read_line(&mut answer).is_err() {
                    return Ok(false);
                }
                let yes = matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes");
                if !yes {
                    return Ok(false);
                }
                if let Err(e) = append_known_host(&self.host, self.port, key) {
                    eprintln!("    (warning: failed to write known_hosts: {})", e);
                }
                *decided = true;
                Ok(true)
            }
            Err(e) => {
                eprintln!(
                    "ssh: known_hosts mismatch or parse error for {}:{} — {}",
                    self.host, self.port, e
                );
                Ok(false)
            }
        }
    }
}

fn append_known_host(host: &str, port: u16, key: &PublicKey) -> io::Result<()> {
    let path = known_hosts_path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME not set"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let host_field = if port == 22 {
        host.to_string()
    } else {
        format!("[{}]:{}", host, port)
    };
    let key_line = key
        .to_openssh()
        .map_err(|e| io::Error::other(e.to_string()))?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(f, "{} {}", host_field, key_line)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_ssh_url;

    #[test]
    fn scp_form() {
        let (u, h, p, path) = parse_ssh_url("git@github.com:owner/repo.git").unwrap();
        assert_eq!(u, "git");
        assert_eq!(h, "github.com");
        assert_eq!(p, 22);
        assert_eq!(path, "owner/repo.git");
    }

    #[test]
    fn ssh_url_with_port() {
        let (u, h, p, path) = parse_ssh_url("ssh://git@example.com:2222/group/repo").unwrap();
        assert_eq!(u, "git");
        assert_eq!(h, "example.com");
        assert_eq!(p, 2222);
        assert_eq!(path, "group/repo");
    }

    #[test]
    fn ssh_url_default_port() {
        let (_, _, p, _) = parse_ssh_url("ssh://git@example.com/x/y").unwrap();
        assert_eq!(p, 22);
    }
}
