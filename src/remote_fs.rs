//! Remote filesystem operations over SFTP, plus the tar-based directory
//! transfers. Mirrors `src-tauri/src/commands/sftp.rs` + `local_fs.rs`.

use crate::models::{FileInfo, TransferDirection, TransferProgress, TransferStatus};
use crate::session::{AppEvent, LiveSession};
use anyhow::{anyhow, Context, Result};
use russh_sftp::client::SftpSession;
use std::path::Path;
use std::time::SystemTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const CHUNK: usize = 32 * 1024;

// ── Path helpers ─────────────────────────────────────────────────────────────

/// POSIX join (never collapses `..`, matching the frontend's `joinPath`).
pub fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        return name.to_string();
    }
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

pub fn parent_of(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
        None => "/".to_string(),
    }
}

pub fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(i) => trimmed[i + 1..].to_string(),
        None => trimmed.to_string(),
    }
}

/// `normalize_remote_path`: trim, strip trailing slashes (except root).
pub fn normalize_remote_path(path: &str) -> String {
    let mut p = path.trim().to_string();
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

/// `shell_quote` from the frontend: `'` → `'"'"'`.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn system_time_to_unix(t: std::io::Result<SystemTime>) -> Option<i64> {
    t.ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

fn permissions_string(perms: &russh_sftp::protocol::FilePermissions) -> String {
    let mut s = String::with_capacity(9);
    for (r, w, x) in [
        (perms.owner_read, perms.owner_write, perms.owner_exec),
        (perms.group_read, perms.group_write, perms.group_exec),
        (perms.other_read, perms.other_write, perms.other_exec),
    ] {
        s.push(if r { 'r' } else { '-' });
        s.push(if w { 'w' } else { '-' });
        s.push(if x { 'x' } else { '-' });
    }
    s
}

// ── Listing ──────────────────────────────────────────────────────────────────

pub async fn list_dir(session: &LiveSession, path: &str) -> Result<Vec<FileInfo>> {
    let sftp = session.sftp.get().await?;
    let result = read_dir_inner(&sftp, path).await;
    if result.is_err() {
        session.sftp.invalidate().await;
    }
    result
}

async fn read_dir_inner(sftp: &SftpSession, path: &str) -> Result<Vec<FileInfo>> {
    let mut out = Vec::new();
    let entries = sftp
        .read_dir(path)
        .await
        .with_context(|| format!("read_dir {path}"))?;
    for entry in entries {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let meta = entry.metadata();
        let full = join_path(path, &name);
        out.push(FileInfo {
            name,
            path: full,
            is_dir: meta.is_dir(),
            size: meta.len(),
            modified: system_time_to_unix(meta.modified()),
            permissions: Some(permissions_string(&meta.permissions())),
            owner: None,
        });
    }
    out.sort_by_key(|f| f.sort_key());
    Ok(out)
}

pub async fn create_dir(session: &LiveSession, path: &str) -> Result<()> {
    let sftp = session.sftp.get().await?;
    // `create_dir_all`: walk the components so nested creation works.
    let mut current = String::new();
    for part in path.split('/') {
        if part.is_empty() {
            current.push('/');
            continue;
        }
        current = if current == "/" {
            format!("/{part}")
        } else if current.is_empty() {
            part.to_string()
        } else {
            format!("{current}/{part}")
        };
        if sftp.metadata(&current).await.is_ok() {
            continue;
        }
        let _ = sftp.create_dir(&current).await;
    }
    Ok(())
}


pub async fn rename(session: &LiveSession, old: &str, new: &str) -> Result<()> {
    let sftp = session.sftp.get().await?;
    sftp.rename(old, new)
        .await
        .with_context(|| format!("rename {old} -> {new}"))?;
    Ok(())
}

/// `remove_remote_path_fast`: `rm -rf --` / `rm -f --` on the PTY connection.
pub async fn remove(session: &LiveSession, path: &str, is_dir: bool) -> Result<()> {
    let path = normalize_remote_path(path);
    if path.is_empty() || path == "/" {
        return Err(anyhow!("拒绝删除根目录"));
    }
    let home = crate::ssh::remote_home(&session.handle).await.unwrap_or_default();
    if !home.is_empty() && normalize_remote_path(&home) == path {
        return Err(anyhow!("拒绝删除远程用户主目录"));
    }
    let cmd = if is_dir {
        format!("rm -rf -- {}", shell_quote(&path))
    } else {
        format!("rm -f -- {}", shell_quote(&path))
    };
    crate::ssh::exec_string(&session.handle, &cmd).await?;
    Ok(())
}

// ── Transfer ─────────────────────────────────────────────────────────────────

pub struct TransferCtx {
    pub transfer_id: String,
    pub display_name: String,
    pub direction: TransferDirection,
    pub progress_total: u64,
    pub progress_start: u64,
    pub progress_span: u64,
    pub target_path: String,
    pub session_id: String,
    pub group_id: Option<String>,
    pub cancelled: std::sync::Arc<parking_lot::Mutex<std::collections::HashSet<String>>>,
    pub bus: crate::session::EventBus,
}

impl TransferCtx {
    fn emit(&self, status: TransferStatus, transferred: u64, error: Option<String>) {
        // A finished transfer must not leave its id in the cancellation set:
        // transfer ids are derived from the path, so a stale entry would make
        // the *next* transfer of the same file fail instantly with "Cancelled".
        if matches!(status, TransferStatus::Done | TransferStatus::Error) {
            self.cancelled.lock().remove(&self.transfer_id);
        }
        self.bus.send(AppEvent::Transfer(Box::new(TransferProgress {
            id: self.transfer_id.clone(),
            file_name: self.display_name.clone(),
            direction: self.direction,
            total: self.progress_total,
            transferred,
            status,
            error,
            target_path: Some(self.target_path.clone()),
            conflict_path: None,
            conflict_is_dir: false,
            session_id: Some(self.session_id.clone()),
            group_id: self.group_id.clone(),
            created_at_ms: crate::models::now_millis(),
        })));
    }

    pub fn start(&self) {
        // A fresh run clears any leftover cancel flag from a previous attempt
        // at the same destination (ids are path-based and therefore reused).
        self.cancelled.lock().remove(&self.transfer_id);
        self.emit(TransferStatus::Pending, self.progress_start, None);
    }

    fn check_cancelled(&self) -> Result<()> {
        if self.cancelled.lock().contains(&self.transfer_id) {
            return Err(anyhow!("Cancelled"));
        }
        Ok(())
    }

    fn map_progress(&self, raw_transferred: u64, raw_total: u64) -> u64 {
        map_stage_progress(raw_total, raw_transferred, self.progress_start, self.progress_span)
    }
}

pub fn map_stage_progress(raw_total: u64, raw_transferred: u64, start: u64, span: u64) -> u64 {
    if span == 0 || raw_total == 0 {
        return start;
    }
    let scaled = raw_transferred.saturating_mul(span) / raw_total;
    start.saturating_add(scaled).min(start.saturating_add(span))
}

/// `upload_file` — SFTP, 32 KiB chunks, integer-percent progress, cancellable.
pub async fn upload_file(
    session: &LiveSession,
    local_path: &str,
    remote_path: &str,
    overwrite: bool,
    ctx: &TransferCtx,
) -> Result<()> {
    ctx.start();

    let sftp = session.sftp.get().await?;
    if !overwrite && sftp.metadata(remote_path).await.is_ok() {
        return Err(anyhow!("CONFLICT:{remote_path}"));
    }

    let mut file = tokio::fs::File::open(local_path)
        .await
        .with_context(|| format!("open {local_path}"))?;
    let total = file
        .metadata()
        .await
        .map(|m| m.len())
        .unwrap_or(ctx.progress_span);
    let mut remote = sftp
        .create(remote_path)
        .await
        .with_context(|| format!("create {remote_path}"))?;

    let mut buf = vec![0u8; CHUNK];
    let mut transferred = 0u64;
    // `None` so the first chunk always reports; `u64::MAX` as a sentinel made
    // `pct > last_pct` permanently false and suppressed every progress event.
    let mut last_pct: Option<u64> = None;
    loop {
        ctx.check_cancelled()?;
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        remote.write_all(&buf[..n]).await?;
        transferred += n as u64;
        let pct = if total == 0 {
            100
        } else {
            transferred * 100 / total
        };
        if last_pct != Some(pct) {
            last_pct = Some(pct);
            ctx.emit(
                TransferStatus::Transferring,
                ctx.map_progress(transferred, total),
                None,
            );
        }
    }
    remote.flush().await?;
    let _ = remote.sync_all().await;
    let _ = remote.shutdown().await;

    ctx.emit(
        TransferStatus::Done,
        (ctx.progress_start + ctx.progress_span).min(ctx.progress_total.max(ctx.progress_start + ctx.progress_span)),
        None,
    );
    Ok(())
}

/// `download_file` — 32 KiB chunks, cancellable.
pub async fn download_file(
    session: &LiveSession,
    remote_path: &str,
    local_path: &str,
    overwrite: bool,
    ctx: &TransferCtx,
) -> Result<()> {
    ctx.start();

    if !overwrite && Path::new(local_path).exists() {
        return Err(anyhow!("CONFLICT:{local_path}"));
    }

    let sftp = session.sftp.get().await?;
    let meta = sftp
        .metadata(remote_path)
        .await
        .with_context(|| format!("stat {remote_path}"))?;
    let total = meta.len();

    if let Some(parent) = Path::new(local_path).parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    let mut remote = sftp
        .open(remote_path)
        .await
        .with_context(|| format!("open {remote_path}"))?;
    let mut file = tokio::fs::File::create(local_path)
        .await
        .with_context(|| format!("create {local_path}"))?;

    let mut buf = vec![0u8; CHUNK];
    let mut transferred = 0u64;
    let mut last_pct: Option<u64> = None;
    loop {
        ctx.check_cancelled()?;
        let n = remote.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).await?;
        transferred += n as u64;
        let pct = if total == 0 {
            100
        } else {
            transferred * 100 / total
        };
        if last_pct != Some(pct) {
            last_pct = Some(pct);
            ctx.emit(
                TransferStatus::Transferring,
                ctx.map_progress(transferred, total),
                None,
            );
        }
    }
    file.flush().await?;

    ctx.emit(
        TransferStatus::Done,
        (ctx.progress_start + ctx.progress_span).min(ctx.progress_total.max(ctx.progress_start + ctx.progress_span)),
        None,
    );
    Ok(())
}

// ── Directory transfer (tar) ─────────────────────────────────────────────────

/// `ensureRemoteTarSupport` — `command -v tar`.
pub async fn remote_has_tar(session: &LiveSession) -> bool {
    crate::ssh::exec_string(&session.handle, "command -v tar >/dev/null 2>&1 && echo yes")
        .await
        .map(|s| s.trim() == "yes")
        .unwrap_or(false)
}

/// Pack a local directory into a tar file (used for folder upload).
pub fn pack_local_dir(source: &str, target_tar: &str) -> Result<u64> {
    let source_path = Path::new(source);
    let file = std::fs::File::create(target_tar)
        .with_context(|| format!("create {target_tar}"))?;
    let mut builder = tar::Builder::new(file);
    let root = source_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".into());

    let mut entries: Vec<std::path::PathBuf> = Vec::new();
    collect_entries(source_path, &mut entries)?;
    entries.sort();
    for path in &entries {
        let rel = path.strip_prefix(source_path).unwrap_or(path);
        let archive_name = format!("{root}/{}", rel.to_string_lossy().replace('\\', "/"));
        if path.is_dir() {
            builder
                .append_dir(&archive_name, path)
                .with_context(|| format!("append dir {}", path.display()))?;
        } else {
            builder
                .append_path_with_name(path, &archive_name)
                .with_context(|| format!("append {}", path.display()))?;
        }
    }
    builder.finish()?;
    Ok(std::fs::metadata(target_tar).map(|m| m.len()).unwrap_or(0))
}

fn collect_entries(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            out.push(path.clone());
            collect_entries(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Unpack a local tar into `target_dir` (used for folder download).
pub fn unpack_local_dir(tar_path: &str, target_dir: &str, overwrite: bool) -> Result<u64> {
    let file = std::fs::File::open(tar_path).with_context(|| format!("open {tar_path}"))?;
    let mut archive = tar::Archive::new(file);
    std::fs::create_dir_all(target_dir)?;
    let mut count = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        if !overwrite && Path::new(target_dir).join(&path).exists() {
            count += 1;
            continue;
        }
        entry
            .unpack_in(target_dir)
            .with_context(|| format!("unpack {}", path.display()))?;
        count += 1;
    }
    Ok(count)
}
