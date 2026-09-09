//! SQLite persistence — same schema and semantics as the web client's
//! `src-tauri/src/storage.rs` (see `docs/spec-backend.md` §4).

use crate::crypto;
use crate::models::*;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = r#"
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS bookmarks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    username TEXT NOT NULL,
    auth_type TEXT NOT NULL DEFAULT 'password',
    password TEXT,
    password_encrypted INTEGER NOT NULL DEFAULT 0,
    private_key TEXT,
    passphrase TEXT,
    profile_id TEXT,
    group_id TEXT,
    term TEXT NOT NULL DEFAULT 'xterm-256color',
    encode TEXT NOT NULL DEFAULT 'utf8',
    color TEXT,
    description TEXT,
    start_directory_remote TEXT,
    start_directory_local TEXT,
    enable_sftp INTEGER NOT NULL DEFAULT 1,
    keepalive_interval INTEGER NOT NULL DEFAULT 30000,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS bookmark_groups (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    parent_id TEXT,
    order_index INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS profiles (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    username TEXT NOT NULL,
    auth_type TEXT NOT NULL DEFAULT 'password',
    password TEXT,
    password_encrypted INTEGER NOT NULL DEFAULT 0,
    private_key TEXT,
    passphrase TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    font_size INTEGER NOT NULL DEFAULT 12,
    font_family TEXT NOT NULL DEFAULT 'Menlo, Monaco, ''Courier New'', monospace',
    theme TEXT NOT NULL DEFAULT 'dark',
    opacity REAL NOT NULL DEFAULT 1.0,
    language TEXT NOT NULL DEFAULT 'zh',
    scrollback INTEGER NOT NULL DEFAULT 5000,
    show_hidden_files INTEGER NOT NULL DEFAULT 0,
    default_protocol TEXT NOT NULL DEFAULT 'ssh',
    cursor_style TEXT NOT NULL DEFAULT 'block',
    cursor_blink INTEGER NOT NULL DEFAULT 1,
    bell_style TEXT NOT NULL DEFAULT 'none'
);
CREATE TABLE IF NOT EXISTS trusted_host_keys (
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    key_type TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (host, port)
);
INSERT OR IGNORE INTO settings (id) VALUES (1);
"#;

#[derive(Debug, Clone)]
pub struct Db {
    pub path: PathBuf,
}

impl Db {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        let db = Self { path };
        {
            let conn = db.conn()?;
            conn.execute_batch(SCHEMA)
                .context("failed to initialize database")?;
        }
        db.normalize_stored_secrets()
            .context("failed to normalize stored secrets")?;
        Ok(db)
    }

    fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(conn)
    }

    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    // ── Bookmarks ────────────────────────────────────────────────────────────

    pub fn list_bookmarks(&self) -> Result<Vec<Bookmark>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, host, port, username, auth_type, password, password_encrypted,
                    private_key, passphrase, profile_id, group_id, term, encode, color,
                    description, start_directory_remote, start_directory_local, enable_sftp,
                    keepalive_interval, created_at, updated_at
             FROM bookmarks ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Bookmark {
                id: row.get(0)?,
                title: row.get(1)?,
                host: row.get(2)?,
                port: row.get::<_, i64>(3)?.clamp(0, 65535) as u16,
                username: row.get(4)?,
                auth_type: row.get(5)?,
                // Redaction contract: secrets never leave storage through `list`.
                password: None,
                password_encrypted: false,
                private_key: None,
                passphrase: None,
                profile_id: row.get(10)?,
                group_id: row.get(11)?,
                term: row.get(12)?,
                encode: row.get(13)?,
                color: row.get(14)?,
                description: row.get(15)?,
                start_directory_remote: row.get(16)?,
                start_directory_local: row.get(17)?,
                enable_sftp: row.get::<_, i64>(18)? != 0,
                keepalive_interval: row.get::<_, i64>(19)? as u32,
                created_at: row.get(20)?,
                updated_at: row.get(21)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Full row including secrets — used by the SSH layer only.
    pub fn bookmark_with_secrets(&self, id: &str) -> Result<Option<Bookmark>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT id, title, host, port, username, auth_type, password, password_encrypted,
                        private_key, passphrase, profile_id, group_id, term, encode, color,
                        description, start_directory_remote, start_directory_local, enable_sftp,
                        keepalive_interval, created_at, updated_at
                 FROM bookmarks WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Bookmark {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        host: row.get(2)?,
                        port: row.get::<_, i64>(3)?.clamp(0, 65535) as u16,
                        username: row.get(4)?,
                        auth_type: row.get(5)?,
                        password: row.get(6)?,
                        password_encrypted: row.get::<_, i64>(7)? != 0,
                        private_key: row.get(8)?,
                        passphrase: row.get(9)?,
                        profile_id: row.get(10)?,
                        group_id: row.get(11)?,
                        term: row.get(12)?,
                        encode: row.get(13)?,
                        color: row.get(14)?,
                        description: row.get(15)?,
                        start_directory_remote: row.get(16)?,
                        start_directory_local: row.get(17)?,
                        enable_sftp: row.get::<_, i64>(18)? != 0,
                        keepalive_interval: row.get::<_, i64>(19)? as u32,
                        created_at: row.get(20)?,
                        updated_at: row.get(21)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn create_bookmark(&self, b: &Bookmark, secret: Option<&str>) -> Result<Bookmark> {
        let encrypted = self.prepare_secret(secret)?;
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO bookmarks (id, title, host, port, username, auth_type, password,
                password_encrypted, private_key, passphrase, profile_id, group_id, term, encode,
                color, description, start_directory_remote, start_directory_local, enable_sftp,
                keepalive_interval, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,NULL,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
            params![
                b.id, b.title, b.host, b.port as i64, b.username, b.auth_type,
                encrypted.as_ref().map(|(v, _)| v.clone()),
                encrypted.as_ref().map(|(_, e)| *e as i64).unwrap_or(0),
                b.profile_id, b.group_id, b.term, b.encode, b.color, b.description,
                b.start_directory_remote, b.start_directory_local, b.enable_sftp as i64,
                b.keepalive_interval as i64, b.created_at, b.updated_at,
            ],
        )?;
        self.bookmark_with_secrets(&b.id)
            .map(|o| o.map(|mut x| {
                x.password = None;
                x.private_key = None;
                x.passphrase = None;
                x
            }))
            .map(|o| o.unwrap_or_else(|| b.clone()))
    }

    /// `secret`: `None` = keep the stored password, `Some("")` = clear it.
    pub fn update_bookmark(&self, b: &Bookmark, secret: Option<&str>) -> Result<Bookmark> {
        let conn = self.conn()?;
        match secret {
            None => {
                conn.execute(
                    "UPDATE bookmarks SET title=?2, host=?3, port=?4, username=?5, auth_type=?6,
                        profile_id=?7, group_id=?8, term=?9, encode=?10, color=?11, description=?12,
                        start_directory_remote=?13, start_directory_local=?14, enable_sftp=?15,
                        keepalive_interval=?16, updated_at=?17 WHERE id=?1",
                    params![
                        b.id, b.title, b.host, b.port as i64, b.username, b.auth_type,
                        b.profile_id, b.group_id, b.term, b.encode, b.color, b.description,
                        b.start_directory_remote, b.start_directory_local, b.enable_sftp as i64,
                        b.keepalive_interval as i64, now_unix(),
                    ],
                )?;
            }
            Some(secret) => {
                let encrypted = self.prepare_secret(Some(secret))?;
                conn.execute(
                    "UPDATE bookmarks SET title=?2, host=?3, port=?4, username=?5, auth_type=?6,
                        password=?7, password_encrypted=?8, profile_id=?9, group_id=?10, term=?11,
                        encode=?12, color=?13, description=?14, start_directory_remote=?15,
                        start_directory_local=?16, enable_sftp=?17, keepalive_interval=?18,
                        updated_at=?19 WHERE id=?1",
                    params![
                        b.id, b.title, b.host, b.port as i64, b.username, b.auth_type,
                        encrypted.as_ref().map(|(v, _)| v.clone()),
                        encrypted.as_ref().map(|(_, e)| *e as i64).unwrap_or(0),
                        b.profile_id, b.group_id, b.term, b.encode, b.color, b.description,
                        b.start_directory_remote, b.start_directory_local, b.enable_sftp as i64,
                        b.keepalive_interval as i64, now_unix(),
                    ],
                )?;
            }
        }
        Ok(self
            .bookmark_with_secrets(&b.id)?
            .map(|mut x| {
                x.password = None;
                x.private_key = None;
                x.passphrase = None;
                x
            })
            .unwrap_or_else(|| b.clone()))
    }

    pub fn delete_bookmark(&self, id: &str) -> Result<()> {
        self.conn()?
            .execute("DELETE FROM bookmarks WHERE id=?1", params![id])?;
        Ok(())
    }

    // ── Groups ───────────────────────────────────────────────────────────────

    pub fn list_bookmark_groups(&self) -> Result<Vec<BookmarkGroup>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, parent_id, order_index, created_at FROM bookmark_groups
             ORDER BY order_index ASC, created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(BookmarkGroup {
                id: row.get(0)?,
                title: row.get(1)?,
                parent_id: row.get(2)?,
                order_index: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_bookmark_group(&self, g: &BookmarkGroup) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO bookmark_groups (id, title, parent_id, order_index, created_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![g.id, g.title, g.parent_id, g.order_index, g.created_at],
        )?;
        Ok(())
    }

    pub fn delete_bookmark_group(&self, id: &str) -> Result<()> {
        self.conn()?
            .execute("DELETE FROM bookmark_groups WHERE id=?1", params![id])?;
        Ok(())
    }

    // ── Profiles ─────────────────────────────────────────────────────────────

    pub fn list_profiles(&self) -> Result<Vec<Profile>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, username, auth_type, password, password_encrypted, private_key,
                    passphrase, created_at
             FROM profiles ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Profile {
                id: row.get(0)?,
                title: row.get(1)?,
                username: row.get(2)?,
                auth_type: row.get(3)?,
                password: None,
                password_encrypted: false,
                private_key: None,
                passphrase: None,
                created_at: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Full profile including secrets — SSH layer only.
    pub fn profile_with_secrets(&self, id: &str) -> Result<Option<Profile>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT id, title, username, auth_type, password, password_encrypted,
                        private_key, passphrase, created_at
                 FROM profiles WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Profile {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        username: row.get(2)?,
                        auth_type: row.get(3)?,
                        password: row.get(4)?,
                        password_encrypted: row.get::<_, i64>(5)? != 0,
                        private_key: row.get(6)?,
                        passphrase: row.get(7)?,
                        created_at: row.get(8)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// `secret`: `None` = keep, `Some("")` = clear. Same for `key`/`passphrase`.
    pub fn upsert_profile(
        &self,
        p: &Profile,
        secret: Option<&str>,
        key: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM profiles WHERE id=?1",
                params![p.id],
                |_| Ok(true),
            )
            .optional()?
            .unwrap_or(false);

        if !exists {
            let enc = self.prepare_secret(secret)?;
            let enc_key = self.prepare_secret(key)?;
            let enc_pass = self.prepare_secret(passphrase)?;
            conn.execute(
                "INSERT INTO profiles (id, title, username, auth_type, password, password_encrypted,
                    private_key, passphrase, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    p.id, p.title, p.username, p.auth_type,
                    enc.as_ref().map(|(v, _)| v.clone()),
                    enc.as_ref().map(|(_, e)| *e as i64).unwrap_or(0),
                    enc_key.as_ref().map(|(v, _)| v.clone()),
                    enc_pass.as_ref().map(|(v, _)| v.clone()),
                    p.created_at,
                ],
            )?;
            return Ok(());
        }

        conn.execute(
            "UPDATE profiles SET title=?2, username=?3, auth_type=?4 WHERE id=?1",
            params![p.id, p.title, p.username, p.auth_type],
        )?;
        if let Some(secret) = secret {
            let enc = self.prepare_secret(Some(secret))?;
            conn.execute(
                "UPDATE profiles SET password=?2, password_encrypted=?3 WHERE id=?1",
                params![
                    p.id,
                    enc.as_ref().map(|(v, _)| v.clone()),
                    enc.as_ref().map(|(_, e)| *e as i64).unwrap_or(0)
                ],
            )?;
        }
        if let Some(key) = key {
            let enc = self.prepare_secret(Some(key))?;
            conn.execute(
                "UPDATE profiles SET private_key=?2 WHERE id=?1",
                params![p.id, enc.as_ref().map(|(v, _)| v.clone())],
            )?;
        }
        if let Some(passphrase) = passphrase {
            let enc = self.prepare_secret(Some(passphrase))?;
            conn.execute(
                "UPDATE profiles SET passphrase=?2 WHERE id=?1",
                params![p.id, enc.as_ref().map(|(v, _)| v.clone())],
            )?;
        }
        Ok(())
    }

    pub fn delete_profile(&self, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM profiles WHERE id=?1", params![id])?;
        // Quirk preserved from the web client: bookmarks keep a dangling profile_id.
        Ok(())
    }

    // ── Settings ─────────────────────────────────────────────────────────────

    pub fn get_settings(&self) -> Result<Settings> {
        let conn = self.conn()?;
        let s = conn.query_row(
            "SELECT font_size, font_family, theme, opacity, language, scrollback,
                    show_hidden_files, default_protocol, cursor_style, cursor_blink, bell_style
             FROM settings WHERE id = 1",
            [],
            |row| {
                Ok(Settings {
                    font_size: row.get::<_, i64>(0)?.max(1) as u32,
                    font_family: row.get(1)?,
                    theme: row.get(2)?,
                    opacity: row.get(3)?,
                    language: row.get(4)?,
                    scrollback: row.get::<_, i64>(5)?.max(0) as u32,
                    show_hidden_files: row.get::<_, i64>(6)? != 0,
                    default_protocol: row.get(7)?,
                    cursor_style: row.get(8)?,
                    cursor_blink: row.get::<_, i64>(9)? != 0,
                    bell_style: row.get(10)?,
                })
            },
        )?;
        Ok(normalize_settings(s))
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.conn()?.execute(
            "UPDATE settings SET font_size=?1, font_family=?2, theme=?3, opacity=?4, language=?5,
                scrollback=?6, show_hidden_files=?7, default_protocol=?8, cursor_style=?9,
                cursor_blink=?10, bell_style=?11 WHERE id=1",
            params![
                s.font_size as i64,
                s.font_family,
                s.theme,
                s.opacity,
                s.language,
                s.scrollback as i64,
                s.show_hidden_files as i64,
                s.default_protocol,
                s.cursor_style,
                s.cursor_blink as i64,
                s.bell_style,
            ],
        )?;
        Ok(())
    }

    // ── Trusted host keys ────────────────────────────────────────────────────

    pub fn get_trusted_host_key(&self, host: &str, port: u16) -> Result<Option<TrustedHostKey>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT host, port, key_type, fingerprint, created_at, updated_at
                 FROM trusted_host_keys WHERE host=?1 AND port=?2",
                params![host, port as i64],
                |row| {
                    Ok(TrustedHostKey {
                        host: row.get(0)?,
                        port: row.get::<_, i64>(1)?.clamp(0, 65535) as u16,
                        key_type: row.get(2)?,
                        fingerprint: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn upsert_trusted_host_key(&self, k: &TrustedHostKey) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO trusted_host_keys (host, port, key_type, fingerprint, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(host, port) DO UPDATE SET
                key_type=excluded.key_type,
                fingerprint=excluded.fingerprint,
                updated_at=excluded.updated_at",
            params![k.host, k.port as i64, k.key_type, k.fingerprint, k.created_at, k.updated_at],
        )?;
        Ok(())
    }

    pub fn list_trusted_host_keys(&self) -> Result<Vec<TrustedHostKey>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT host, port, key_type, fingerprint, created_at, updated_at
             FROM trusted_host_keys ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TrustedHostKey {
                host: row.get(0)?,
                port: row.get::<_, i64>(1)?.clamp(0, 65535) as u16,
                key_type: row.get(2)?,
                fingerprint: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_trusted_host_key(&self, host: &str, port: u16) -> Result<()> {
        self.conn()?.execute(
            "DELETE FROM trusted_host_keys WHERE host=?1 AND port=?2",
            params![host, port as i64],
        )?;
        Ok(())
    }

    // ── Secrets ──────────────────────────────────────────────────────────────

    /// Returns `(stored_value, is_present)`.
    fn prepare_secret(&self, plaintext: Option<&str>) -> Result<Option<(String, bool)>> {
        match plaintext {
            None => Ok(None),
            Some("") => Ok(Some((String::new(), false))),
            Some(v) => Ok(Some((crypto::encrypt_secret(&self.path, v)?, true))),
        }
    }

    /// Decrypt a stored secret, honouring the legacy `password_encrypted` flag.
    pub fn resolve_password_secret(
        &self,
        value: Option<&str>,
        password_encrypted: bool,
    ) -> Result<Option<String>> {
        match value.filter(|v| !v.is_empty()) {
            None => Ok(None),
            Some(v) => {
                if crypto::is_encrypted_secret(v) {
                    Ok(Some(crypto::decrypt_secret(&self.path, v)?))
                } else if password_encrypted {
                    Ok(Some(decode_password(v)))
                } else {
                    Ok(Some(v.to_string()))
                }
            }
        }
    }

    pub fn resolve_plain_secret(&self, value: Option<&str>) -> Result<Option<String>> {
        match value.filter(|v| !v.is_empty()) {
            None => Ok(None),
            Some(v) => {
                if crypto::is_encrypted_secret(v) {
                    Ok(Some(crypto::decrypt_secret(&self.path, v)?))
                } else {
                    Ok(Some(v.to_string()))
                }
            }
        }
    }

    /// Port of `storage::normalize_stored_secrets` — runs once per launch.
    pub fn normalize_stored_secrets(&self) -> Result<()> {
        let conn = self.conn()?;

        // ── bookmarks ────────────────────────────────────────────────────────
        let rows: Vec<(String, String, Option<String>, bool, Option<String>, Option<String>)> = {
            let mut stmt = conn.prepare(
                "SELECT id, auth_type, password, password_encrypted, private_key, passphrase
                 FROM bookmarks",
            )?;
            let iter = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?;
            iter.collect::<rusqlite::Result<Vec<_>>>()?
        };

        for (id, auth_type, password, password_encrypted, private_key, passphrase) in rows {
            match auth_type.as_str() {
                "password" => {
                    let synced = match password.filter(|v| !v.is_empty()) {
                        None => None,
                        Some(value) => {
                            let plaintext = if crypto::is_encrypted_secret(&value) {
                                crypto::decrypt_secret(&self.path, &value)?
                            } else if password_encrypted {
                                decode_password(&value)
                            } else {
                                value
                            };
                            Some(crypto::encrypt_secret(&self.path, &plaintext)?)
                        }
                    };
                    conn.execute(
                        "UPDATE bookmarks SET password=?2, password_encrypted=?3,
                            private_key=NULL, passphrase=NULL WHERE id=?1",
                        params![id, synced.clone(), synced.is_some() as i64],
                    )?;
                }
                "privateKey" => {
                    let sync_key = match private_key.filter(|v| !v.is_empty()) {
                        None => None,
                        Some(v) if crypto::is_encrypted_secret(&v) => Some(v),
                        Some(v) => Some(crypto::encrypt_secret(&self.path, &v)?),
                    };
                    let sync_pass = match passphrase.filter(|v| !v.is_empty()) {
                        None => None,
                        Some(v) if crypto::is_encrypted_secret(&v) => Some(v),
                        Some(v) => Some(crypto::encrypt_secret(&self.path, &v)?),
                    };
                    conn.execute(
                        "UPDATE bookmarks SET password=NULL, password_encrypted=0,
                            private_key=?2, passphrase=?3 WHERE id=?1",
                        params![id, sync_key, sync_pass],
                    )?;
                }
                "profile" => {
                    conn.execute(
                        "UPDATE bookmarks SET password=NULL, password_encrypted=0,
                            private_key=NULL, passphrase=NULL WHERE id=?1",
                        params![id],
                    )?;
                }
                _ => {}
            }
        }
        conn.execute(
            "UPDATE bookmarks SET private_key=NULL, passphrase=NULL WHERE auth_type != 'privateKey'",
            [],
        )?;

        // ── profiles ─────────────────────────────────────────────────────────
        let rows: Vec<(String, String, Option<String>, bool, Option<String>, Option<String>)> = {
            let mut stmt = conn.prepare(
                "SELECT id, auth_type, password, password_encrypted, private_key, passphrase
                 FROM profiles",
            )?;
            let iter = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?;
            iter.collect::<rusqlite::Result<Vec<_>>>()?
        };

        for (id, auth_type, password, password_encrypted, private_key, passphrase) in rows {
            match auth_type.as_str() {
                "password" => {
                    let synced = match password.filter(|v| !v.is_empty()) {
                        None => None,
                        Some(value) => {
                            let plaintext = if crypto::is_encrypted_secret(&value) {
                                crypto::decrypt_secret(&self.path, &value)?
                            } else if password_encrypted {
                                decode_password(&value)
                            } else {
                                value
                            };
                            Some(crypto::encrypt_secret(&self.path, &plaintext)?)
                        }
                    };
                    conn.execute(
                        "UPDATE profiles SET password=?2, password_encrypted=?3,
                            private_key=NULL, passphrase=NULL WHERE id=?1",
                        params![id, synced.clone(), synced.is_some() as i64],
                    )?;
                }
                "privateKey" => {
                    let sync_key = match private_key.filter(|v| !v.is_empty()) {
                        None => None,
                        Some(v) if crypto::is_encrypted_secret(&v) => Some(v),
                        Some(v) => Some(crypto::encrypt_secret(&self.path, &v)?),
                    };
                    let sync_pass = match passphrase.filter(|v| !v.is_empty()) {
                        None => None,
                        Some(v) if crypto::is_encrypted_secret(&v) => Some(v),
                        Some(v) => Some(crypto::encrypt_secret(&self.path, &v)?),
                    };
                    conn.execute(
                        "UPDATE profiles SET password=NULL, password_encrypted=0,
                            private_key=?2, passphrase=?3 WHERE id=?1",
                        params![id, sync_key, sync_pass],
                    )?;
                }
                _ => {
                    conn.execute(
                        "UPDATE profiles SET password=NULL, password_encrypted=0,
                            private_key=NULL, passphrase=NULL WHERE id=?1",
                        params![id],
                    )?;
                }
            }
        }
        conn.execute(
            "UPDATE profiles SET private_key=NULL, passphrase=NULL WHERE auth_type != 'privateKey'",
            [],
        )?;

        Ok(())
    }
}

/// Port of `normalizeSettings` from the frontend store.
pub fn normalize_settings(mut s: Settings) -> Settings {
    let family = s.font_family.trim().to_string();
    s.font_family = if family.is_empty()
        || family == "Menlo, Monaco, Courier New, monospace"
        || family == "Menlo,Monaco,Courier New,monospace"
    {
        Settings::default().font_family
    } else {
        family
    };

    let d = Settings::default();
    let looks_like_legacy_defaults = (s.font_size == 14 || s.font_size == 13)
        && s.font_family == d.font_family
        && s.theme == d.theme
        && (s.opacity - d.opacity).abs() < f32::EPSILON
        && s.language == d.language
        && s.scrollback == d.scrollback
        && s.show_hidden_files == d.show_hidden_files
        && s.default_protocol == d.default_protocol
        && s.cursor_style == d.cursor_style
        && s.cursor_blink == d.cursor_blink
        && s.bell_style == d.bell_style;
    if looks_like_legacy_defaults {
        s.font_size = d.font_size;
    }
    s
}

/// Default database location, matching the Tauri app's `app_data_dir`.
///
/// `TINYTERM_DB` overrides it (handy for tests and for pointing the egui build
/// at an existing `tinyterm.db`).
pub fn default_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("TINYTERM_DB") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("com.tinyterm.app").join("tinyterm.db")
}

/// Writable fallback used when the platform data directory cannot be created.
pub fn fallback_db_path() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".tinyterm-egui").join("tinyterm.db")
}
