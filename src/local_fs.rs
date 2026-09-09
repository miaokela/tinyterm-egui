//! Local filesystem helpers — port of `commands/local_fs.rs` and the
//! frontend's local panel operations.

use crate::models::FileInfo;
use anyhow::{anyhow, Result};
use std::path::{Component, Path, PathBuf};

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// `is_filesystem_root`
pub fn is_filesystem_root(path: &Path) -> bool {
    let components: Vec<Component> = path.components().collect();
    components.len() == 1 && components[0] == Component::RootDir
}

/// `guard_local_delete_target`: never delete `/` or the local home directory.
pub fn guard_local_delete(path: &Path) -> Result<()> {
    if is_filesystem_root(path) {
        return Err(anyhow!("拒绝删除文件系统根目录"));
    }
    let home = home_dir();
    let canon_home = home.canonicalize().unwrap_or(home);
    let canon_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if canon_path == canon_home {
        return Err(anyhow!("拒绝删除用户主目录"));
    }
    Ok(())
}

pub fn list_dir(path: &str) -> Result<Vec<FileInfo>> {
    let dir = Path::new(path);
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| anyhow!("{e}"))? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let name = entry.file_name().to_string_lossy().to_string();
        let full = entry.path();
        out.push(FileInfo {
            name,
            path: full.to_string_lossy().to_string(),
            is_dir: meta.is_dir(),
            size: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64),
            permissions: permissions_string(&meta),
            owner: None,
        });
    }
    out.sort_by_key(|f| f.sort_key());
    Ok(out)
}

#[cfg(unix)]
fn permissions_string(meta: &std::fs::Metadata) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let mut s = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if bits & 0o4 != 0 { 'r' } else { '-' });
        s.push(if bits & 0o2 != 0 { 'w' } else { '-' });
        s.push(if bits & 0o1 != 0 { 'x' } else { '-' });
    }
    Some(s)
}

#[cfg(not(unix))]
fn permissions_string(_meta: &std::fs::Metadata) -> Option<String> {
    None
}

pub fn create_dir(path: &str) -> Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}

pub fn delete(path: &str) -> Result<()> {
    let p = Path::new(path);
    guard_local_delete(p)?;
    let meta = std::fs::symlink_metadata(p)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

pub fn rename(old: &str, new: &str) -> Result<()> {
    std::fs::rename(old, new)?;
    Ok(())
}

pub fn join_path(base: &str, name: &str) -> String {
    Path::new(base).join(name).to_string_lossy().to_string()
}
