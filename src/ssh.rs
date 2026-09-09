//! SSH transport, host-key verification and authentication (russh).
//!
//! Mirrors `src-tauri/src/ssh.rs` + `commands/ssh.rs::create_session`:
//! transport → host-key check → auth → PTY + shell, in that order, so an
//! untrusted host is rejected *before* any credential is offered.

use crate::models::{Bookmark, HostKeyVerificationPrompt, Profile, TrustedHostKey};
use crate::storage::Db;
use anyhow::{anyhow, Context, Result};
use russh::client::{AuthResult, Config, Handle, Handler};
use russh::keys::{decode_secret_key, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::ChannelMsg;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub type SshHandle = Handle<ClientHandler>;

/// Commands sent to the dedicated PTY writer task.
///
/// Keeping writes behind a channel means the UI thread never touches the SSH
/// channel directly — the same architecture as the web client's writer thread.
#[derive(Debug)]
pub enum WriteCmd {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Close,
}

pub type WriteTx = tokio::sync::mpsc::UnboundedSender<WriteCmd>;

/// Connection + auth material resolved from the bookmark / credential rows.
#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
    pub term: String,
    pub keepalive_interval: u32,
}

impl ResolvedAuth {
    pub fn redacted(&self) -> String {
        format!("{}@{}:{}", self.username, self.host, self.port)
    }
}

/// Errors surfaced by [`connect`]. `HostKey` carries the prompt the UI must
/// show; everything else is a plain message.
#[derive(Debug)]
pub enum ConnectError {
    HostKey(Box<HostKeyVerificationPrompt>),
    Message(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HostKey(p) => write!(
                f,
                "HOST_KEY_PROMPT:{}",
                serde_json::to_string(p).unwrap_or_default()
            ),
            Self::Message(m) => write!(f, "{m}"),
        }
    }
}

impl From<anyhow::Error> for ConnectError {
    fn from(e: anyhow::Error) -> Self {
        Self::Message(e.to_string())
    }
}

// ── Credential resolution ────────────────────────────────────────────────────

/// Port of `resolve_bookmark_for_connection`: merges a linked credential into
/// the bookmark and decrypts the stored secrets.
pub fn resolve_auth(
    db: &Db,
    bookmark: &Bookmark,
    username_override: Option<&str>,
    password_override: Option<&str>,
) -> Result<ResolvedAuth> {
    let mut auth = ResolvedAuth {
        host: bookmark.host.clone(),
        port: bookmark.port,
        username: bookmark.username.clone(),
        auth_type: bookmark.auth_type.clone(),
        password: None,
        private_key: None,
        passphrase: None,
        term: bookmark.term.clone(),
        keepalive_interval: bookmark.keepalive_interval,
    };

    if bookmark.auth_type == "profile" {
        if let Some(profile_id) = bookmark.profile_id.as_deref().filter(|s| !s.is_empty()) {
            let profile: Profile = db
                .profile_with_secrets(profile_id)?
                .ok_or_else(|| anyhow!("Credential '{profile_id}' not found"))?;
            auth.username = profile.username.clone();
            auth.auth_type = profile.auth_type.clone();
            auth.password = db.resolve_password_secret(
                profile.password.as_deref(),
                profile.password_encrypted,
            )?;
            auth.private_key = db.resolve_plain_secret(profile.private_key.as_deref())?;
            auth.passphrase = db.resolve_plain_secret(profile.passphrase.as_deref())?;
        }
    } else {
        let full = db
            .bookmark_with_secrets(&bookmark.id)?
            .unwrap_or_else(|| bookmark.clone());
        match full.auth_type.as_str() {
            "password" => {
                auth.password =
                    db.resolve_password_secret(full.password.as_deref(), full.password_encrypted)?;
            }
            "privateKey" => {
                auth.private_key = db.resolve_plain_secret(full.private_key.as_deref())?;
                auth.passphrase = db.resolve_plain_secret(full.passphrase.as_deref())?;
            }
            _ => {}
        }
    }

    if let Some(username) = username_override.filter(|s| !s.is_empty()) {
        auth.username = username.to_string();
        if auth.auth_type.is_empty() || auth.auth_type == "profile" {
            auth.auth_type = "password".to_string();
        }
    }
    if let Some(password) = password_override {
        auth.password = Some(password.to_string());
    }

    Ok(auth)
}

// ── Handler ──────────────────────────────────────────────────────────────────

/// Records what the server presented and whether it was already trusted.
pub struct ClientHandler {
    pub host: String,
    pub port: u16,
    pub trusted: Option<TrustedHostKey>,
    pub observed: Arc<std::sync::Mutex<Option<HostKeyVerificationPrompt>>>,
}

impl Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = server_public_key.public_key();
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let key_type = key.algorithm().as_str().to_string();

        let ok = match &self.trusted {
            Some(record) => record.fingerprint == fingerprint && record.key_type == key_type,
            None => false,
        };

        if !ok {
            let reason = if self.trusted.is_some() {
                "mismatch"
            } else {
                "unknown"
            };
            *self.observed.lock().unwrap() = Some(HostKeyVerificationPrompt {
                host: self.host.clone(),
                port: self.port,
                key_type,
                fingerprint,
                reason: reason.to_string(),
            });
        }
        Ok(ok)
    }
}

// ── Connect ──────────────────────────────────────────────────────────────────

pub struct Connection {
    pub handle: SshHandle,
    pub observed_host_key: Arc<std::sync::Mutex<Option<HostKeyVerificationPrompt>>>,
}

pub async fn connect(
    auth: &ResolvedAuth,
    trusted: Option<TrustedHostKey>,
    connect_timeout: Duration,
) -> Result<Connection, ConnectError> {
    let mut config = Config::default();
    config.inactivity_timeout = None;
    config.keepalive_interval = Some(Duration::from_secs(30));
    config.keepalive_max = 3;
    config.nodelay = true;

    let observed = Arc::new(std::sync::Mutex::new(None));
    let handler = ClientHandler {
        host: auth.host.clone(),
        port: auth.port,
        trusted,
        observed: observed.clone(),
    };

    let addr = format!("{}:{}", auth.host, auth.port);
    let handle = match tokio::time::timeout(
        connect_timeout,
        russh::client::connect(Arc::new(config), addr.clone(), handler),
    )
    .await
    {
        Err(_) => {
            return Err(ConnectError::Message(format!(
                "TCP connect to {addr} failed: timeout"
            )))
        }
        Ok(Err(e)) => {
            if let Some(prompt) = observed.lock().unwrap().clone() {
                return Err(ConnectError::HostKey(Box::new(prompt)));
            }
            return Err(ConnectError::Message(format!("SSH handshake failed: {e}")));
        }
        Ok(Ok(h)) => h,
    };

    Ok(Connection {
        handle,
        observed_host_key: observed,
    })
}

/// Password / private-key authentication. `password_override` wins over the
/// stored password, exactly like the web client.
pub async fn authenticate(
    handle: &mut SshHandle,
    auth: &ResolvedAuth,
    password_override: Option<&str>,
) -> Result<()> {
    let result: AuthResult = match auth.auth_type.as_str() {
        "privateKey" => {
            let key_content = auth
                .private_key
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("Private key content is empty"))?;
            let key = decode_secret_key(key_content, auth.passphrase.as_deref())
                .map_err(|e| anyhow!("Private key auth failed: {e}"))?;
            let hash_alg = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
            handle
                .authenticate_publickey(auth.username.clone(), key)
                .await
                .map_err(|e| anyhow!("Private key auth failed: {e}"))?
        }
        _ => {
            let password = password_override
                .map(|s| s.to_string())
                .or_else(|| auth.password.clone())
                .ok_or_else(|| anyhow!("No password provided"))?;
            handle
                .authenticate_password(auth.username.clone(), password)
                .await
                .map_err(|e| anyhow!("Password auth failed: {e}"))?
        }
    };

    if !result.success() {
        return Err(anyhow!("Authentication failed"));
    }
    Ok(())
}

/// Open a PTY-backed shell channel.
///
/// Returns the read half (owned by the reader task) and a channel sender that
/// feeds the dedicated writer task, so no russh type needs to be named.
pub async fn open_shell(
    handle: &SshHandle,
    term: &str,
    cols: u32,
    rows: u32,
) -> Result<(
    russh::ChannelReadHalf,
    WriteTx,
)> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| anyhow!("Channel open failed: {e}"))?;
    channel
        .request_pty(true, term, cols, rows, 0, 0, &[])
        .await
        .map_err(|e| anyhow!("PTY request failed: {e}"))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| anyhow!("Shell request failed: {e}"))?;

    let (read_half, write_half) = channel.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WriteCmd>();
    tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            match cmd {
                WriteCmd::Data(bytes) => {
                    if write_half.data_bytes(bytes).await.is_err() {
                        break;
                    }
                }
                WriteCmd::Resize { cols, rows } => {
                    let _ = write_half
                        .window_change(cols as u32, rows as u32, 0, 0)
                        .await;
                }
                WriteCmd::Close => {
                    let _ = write_half.eof().await;
                    let _ = write_half.close().await;
                    break;
                }
            }
        }
    });
    Ok((read_half, tx))
}

// ── Remote exec ──────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
}

pub async fn exec(handle: &SshHandle, command: &str) -> Result<ExecOutput> {
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| anyhow!("channel_session: {e}"))?;
    channel
        .exec(true, command)
        .await
        .map_err(|e| anyhow!("exec: {e}"))?;

    let mut out = ExecOutput::default();
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } => out.stdout.extend_from_slice(&data),
            ChannelMsg::ExtendedData { data, .. } => out.stderr.extend_from_slice(&data),
            ChannelMsg::ExitStatus { exit_status } => out.exit_status = Some(exit_status),
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }
    let _ = channel.close().await;
    Ok(out)
}

pub async fn exec_string(handle: &SshHandle, command: &str) -> Result<String> {
    let out = exec(handle, command).await?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    match out.exit_status {
        Some(0) | None => Ok(stdout),
        Some(code) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(anyhow!(
                "Command failed with exit code {code}: {stdout}, {stderr}"
            ))
        }
    }
}

/// `printf '%s' "$HOME"` — remote home directory.
pub async fn remote_home(handle: &SshHandle) -> Result<String> {
    let home = exec_string(handle, "printf '%s' \"$HOME\"").await?;
    let home = home.trim().to_string();
    if home.is_empty() {
        Err(anyhow!("empty remote home"))
    } else {
        Ok(home)
    }
}

/// Port of `get_remote_cwd`: `/proc` sibling scan → `lsof` → `pwd`.
pub async fn remote_cwd(handle: &SshHandle) -> Result<String> {
    const CMD: &str = concat!(
        r#"p=$(cat /proc/$$/status 2>/dev/null|grep -m1 '^PPid:'|awk '{print $2}');"#,
        r#"if [ -n "$p" ];then "#,
        r#"for f in /proc/[0-9]*/status;do "#,
        r#"pid="${f#/proc/}";pid="${pid%/status}";"#,
        r#"[ "$pid" = "$$" ]&&continue;"#,
        r#"pp=$(grep -m1 '^PPid:' "$f" 2>/dev/null|awk '{print $2}');"#,
        r#"if [ "$pp" = "$p" ];then "#,
        r#"cwd=$(readlink "/proc/$pid/cwd" 2>/dev/null)&&[ -n "$cwd" ]&&echo "$cwd"&&exit 0;"#,
        r#"fi;done;fi;"#,
        r#"if command -v lsof >/dev/null 2>&1&&[ -n "$p" ];then "#,
        r#"cwd=$(lsof -a -p "$p" -d cwd -F n 2>/dev/null|grep '^n/'|cut -c2-);"#,
        r#"[ -n "$cwd" ]&&echo "$cwd"&&exit 0;fi;"#,
        r#"pwd"#,
    );
    let cwd = exec_string(handle, CMD).await?;
    let cwd = cwd.trim().to_string();
    if cwd.is_empty() {
        Err(anyhow!("empty cwd output"))
    } else {
        Ok(cwd)
    }
}

/// Open an SFTP subsystem channel on the shared connection.
pub async fn open_sftp(handle: &SshHandle) -> Result<Arc<russh_sftp::client::SftpSession>> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| anyhow!("channel_session: {e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| anyhow!("request sftp subsystem: {e}"))?;
    let stream = channel.into_stream();
    let sftp = russh_sftp::client::SftpSession::new(stream)
        .await
        .context("sftp init")?;
    Ok(Arc::new(sftp))
}

/// Reusable SFTP slot — created lazily, invalidated on transport errors.
#[derive(Clone)]
pub struct SftpSlot {
    handle: Arc<SshHandle>,
    inner: Arc<Mutex<Option<Arc<russh_sftp::client::SftpSession>>>>,
}

impl SftpSlot {
    pub fn new(handle: Arc<SshHandle>) -> Self {
        Self {
            handle,
            inner: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn get(&self) -> Result<Arc<russh_sftp::client::SftpSession>> {
        let mut guard = self.inner.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.clone());
        }
        let session = open_sftp(&self.handle).await?;
        *guard = Some(session.clone());
        Ok(session)
    }

    pub async fn invalidate(&self) {
        *self.inner.lock().await = None;
    }

    pub async fn close(&self) {
        if let Some(session) = self.inner.lock().await.take() {
            let _ = session.close().await;
        }
    }
}
