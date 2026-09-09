# TinyTerm Rust Backend — Exhaustive Implementation Specification

> **Purpose.** This document specifies the complete behaviour of the existing TinyTerm Tauri v2
> Rust backend, in enough detail to reimplement it as a standalone Rust/`egui` application
> (no Tauri, no WebView). It was produced by reading the following files **in full**:
>
> | File | LOC | Role |
> |---|---|---|
> | `tinyterm/src-tauri/src/lib.rs` | 77 | module wiring, Tauri builder, managed state, command registry |
> | `tinyterm/src-tauri/src/main.rs` | 5 | thin binary entry |
> | `tinyterm/src-tauri/src/models.rs` | 234 | all serde data models + legacy password obfuscation |
> | `tinyterm/src-tauri/src/session.rs` | 53 | live-session structs, `SessionManager` |
> | `tinyterm/src-tauri/src/ssh.rs` | 176 | transport, host key, auth, PTY, channel IO primitives |
> | `tinyterm/src-tauri/src/crypto.rs` | 137 | RSA+AES-GCM secret envelope, key file |
> | `tinyterm/src-tauri/src/storage.rs` | 496 | SQLite schema + all CRUD + secret normalisation |
> | `tinyterm/src-tauri/src/commands/mod.rs` | 7 | command module list |
> | `tinyterm/src-tauri/src/commands/app.rs` | 14 | `finish_startup` |
> | `tinyterm/src-tauri/src/commands/settings.rs` | 13 | settings commands |
> | `tinyterm/src-tauri/src/commands/profile.rs` | 189 | credential-profile commands |
> | `tinyterm/src-tauri/src/commands/bookmark.rs` | 216 | bookmark/group commands |
> | `tinyterm/src-tauri/src/commands/ssh.rs` | 682 | session lifecycle, terminal IO, host trust |
> | `tinyterm/src-tauri/src/commands/sftp.rs` | 1025 | SFTP/SCP file ops, transfers, delete guards |
> | `tinyterm/src-tauri/src/commands/local_fs.rs` | 318 | local tar pack/unpack |
>
> Frontend call sites (`src/store/index.ts`, `src/components/TerminalView.tsx`,
> `src/components/FileManager.tsx`, `src/types/index.ts`) were consulted **only** to confirm
> argument casing, event payload shapes and the exact remote `tar` commands that the backend
> primitives are driven with. Every such cross-reference is marked **[frontend]**.

---

## 1. Crate layout & build configuration

### 1.1 `main.rs`

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tinyterm_lib::run()
}
```

### 1.2 `lib.rs` (module root)

```rust
pub mod commands;
pub mod crypto;
pub mod models;
pub mod session;
pub mod ssh;
pub mod storage;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialize storage
            let app_dir = app.path().app_data_dir().expect("failed to get app data dir");
            std::fs::create_dir_all(&app_dir).expect("failed to create app data dir");
            let db_path = app_dir.join("tinyterm.db");
            storage::init_db(&db_path).expect("failed to initialize database");
            storage::normalize_stored_secrets(&storage::DbPath(db_path.clone()))
                .expect("failed to normalize stored secrets");
            app.manage(storage::DbPath(db_path));
            app.manage(session::SessionManager::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ /* 39 commands, see §9 */ ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**Notes for the egui port**

* `env_logger::init()` must run exactly once, before anything else logs.
* Setup order is significant: **create dir → init DB → normalise secrets → register state**.
  Any failure is a hard `panic` (`.expect(...)`), i.e. the app aborts rather than degrading.
* `tauri_plugin_shell` and `tauri_plugin_dialog` are registered but the Rust backend never calls
  them; they exist purely for the frontend (`@tauri-apps/plugin-dialog` file pickers). An egui
  port replaces them with `rfd`-style native dialogs and needs no backend equivalent.

### 1.3 Dependencies (`src-tauri/Cargo.toml`, version `1.0.20`, edition 2021, rust-version 1.77.2)

```toml
tauri = { version = "2", features = ["macos-private-api"] }
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ssh2 = { version = "0.9", features = ["vendored-openssl"] }
rusqlite = { version = "0.31", features = ["bundled"] }
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
dirs = "5"
anyhow = "1"
once_cell = "1"
parking_lot = "0.12"
thiserror = "1"
base64 = "0.22"
openssl = { version = "0.10", features = ["vendored"] }
log = "0.4"
env_logger = "0.11"
tar = "0.4.45"
socket2 = "0.6.3"
```

* `tokio` is declared but **never used** by backend code — there are **zero** `async fn` commands;
  every command is synchronous and Tauri runs it on its own thread pool. All background work uses
  `std::thread::spawn`.
* `once_cell` and `thiserror` are declared but **unused** — there are no `lazy_static`/`once_cell`
  statics and no custom error enums (see §8).
* `dirs` is used **only** for `dirs::home_dir()` in the local-delete guard.

---

## 2. On-disk layout

### 2.1 Application data directory

`app.path().app_data_dir()` with `identifier = "com.tinyterm.app"` (`tauri.conf.json`):

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/com.tinyterm.app/` |
| Linux | `$XDG_DATA_HOME/com.tinyterm.app/` (default `~/.local/share/com.tinyterm.app/`) |
| Windows | `%APPDATA%\com.tinyterm.app\` |

`std::fs::create_dir_all(&app_dir)` is called unconditionally at startup.

### 2.2 Files

| File | Created by | Purpose |
|---|---|---|
| `<app_data_dir>/tinyterm.db` | `storage::init_db` (implicitly, by `Connection::open`) | main SQLite database |
| `<app_data_dir>/tinyterm.db-wal` | SQLite (WAL mode) | write-ahead log |
| `<app_data_dir>/tinyterm.db-shm` | SQLite (WAL mode) | shared-memory index |
| `<app_data_dir>/secret-key.pem` | `crypto::load_or_create_private_key` (first secret write) | RSA-2048 private key, PEM PKCS#1, `chmod 0600` on unix |

The key path is derived as **the parent directory of the DB path**, not from any other config:

```rust
fn private_key_path(db_path: &Path) -> Result<PathBuf> {
    let app_dir = db_path
        .parent()
        .ok_or_else(|| anyhow!("database path has no parent directory"))?;
    Ok(app_dir.join(PRIVATE_KEY_FILE))   // PRIVATE_KEY_FILE = "secret-key.pem"
}
```

Consequence: moving `tinyterm.db` without `secret-key.pem` makes all stored secrets undecryptable.

### 2.3 Temporary paths (created by the frontend, not the backend)

| Path | Producer |
|---|---|
| `<system temp>/tinyterm-pack-<Date.now()>-<rand6>/` | frontend, via `@tauri-apps/api/path` `tempDir()` + `create_local_dir` |
| `<system temp>/tinyterm-pack-<stamp>/.tinyterm-pack.tar` | frontend passes as `target_tar_path` to `pack_local_dir` |
| `<remote_parent>/.tinyterm-pack-<stamp>.tar` | remote tar staging file, uploaded/downloaded with `upload_file`/`download_file` |

---

## 3. Global state

There are **no** `static`/`lazy_static`/`once_cell` values in the backend. All state is Tauri
managed state, i.e. `Arc`-like handles owned by the app and injected into commands.

### 3.1 `DbPath`

```rust
pub struct DbPath(pub PathBuf);   // storage.rs
```

Registered as `app.manage(storage::DbPath(db_path))`. Every storage function takes `&DbPath`.
**egui port:** put `DbPath(PathBuf)` in your `AppState`.

### 3.2 `SessionManager`

```rust
// session.rs
use parking_lot::Mutex;
use ssh2::Session;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::models::Bookmark;

pub struct SshSession {
    /// The main SSH session used for the terminal PTY channel
    pub session: Arc<Mutex<Session>>,
    /// The PTY shell channel
    pub channel: Arc<Mutex<ssh2::Channel>>,
    pub bookmark_id: String,
    /// Dedicated SSH session for SFTP operations (lazily created).
    /// Using a separate TCP connection eliminates contention with terminal I/O —
    /// reads and writes to the PTY channel never block on SFTP and vice-versa.
    pub sftp_session: Arc<Mutex<Option<Session>>>,
    /// Resolved bookmark (with profile credentials already merged) so we can
    /// create the SFTP session on demand without hitting the database again.
    pub resolved_bookmark: Bookmark,
    /// Password override supplied at connection time (if any).
    pub password_override: Option<String>,
    /// Verified host key fingerprint accepted for this live session.
    pub trusted_host_fingerprint: String,
    /// SSH host key algorithm bound to the trusted fingerprint.
    pub trusted_host_key_type: String,
    /// Set to `true` to signal the background reader thread to exit cleanly.
    pub stop_reader: Arc<AtomicBool>,
    /// Sender half of the dedicated writer thread's input queue.
    pub write_tx: Sender<String>,
}

pub struct SessionManager {
    pub sessions: Mutex<HashMap<String, SshSession>>,
    pub cancelled_transfers: Arc<Mutex<HashSet<String>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            cancelled_transfers: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}
```

Registered as `app.manage(session::SessionManager::new())`.

### 3.3 Locking model (critical for correctness)

Lock acquisition order observed everywhere:

1. `SessionManager.sessions` (parking_lot `Mutex<HashMap>`) — **always released as early as
   possible**, except in `execute_remote_command` (see §9.5.10, a known serialization bug).
2. `SshSession.session` (`Arc<Mutex<ssh2::Session>>`).
3. `SshSession.channel` (`Arc<Mutex<ssh2::Channel>>`).

Never the reverse. Notable properties:

* The **reader thread** uses `session_arc.try_lock()` (non-blocking) and only then
  `channel_arc.lock()` (blocking). On `try_lock` failure it sleeps 500 µs and retries.
* The **writer thread** locks `session` (blocking) then `channel` (blocking), toggling
  `set_blocking(true/false)` around the write.
* `get_remote_cwd` locks `session` for the whole command (blocking mode, 5 s timeout), so writes
  queue up in the mpsc channel and are **not lost** — they just wait.
* `sftp_session` is an independent `Arc<Mutex<Option<Session>>>`; it is **never** locked while
  the PTY session mutex is held (each command takes one or the other).
* `cancelled_transfers` is a separate mutex, taken for microseconds per chunk.

---

## 4. SQLite schema

### 4.1 Initialisation (`storage::init_db`) — exact SQL

Executed once at startup through `Connection::execute_batch` on a freshly opened connection:

```sql
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
```

> The `font_family` default is written with **doubled single quotes** inside the SQL string
> literal (`''Courier New''`), i.e. the stored default value is
> `Menlo, Monaco, 'Courier New', monospace`.

### 4.2 Column reference

**`bookmarks`**

| Column | SQL type | Constraints / default | Rust type | Notes |
|---|---|---|---|---|
| `id` | TEXT | PRIMARY KEY | `String` | UUID v4 string |
| `title` | TEXT | NOT NULL | `String` | |
| `host` | TEXT | NOT NULL | `String` | |
| `port` | INTEGER | NOT NULL DEFAULT 22 | `u16` | |
| `username` | TEXT | NOT NULL | `String` | |
| `auth_type` | TEXT | NOT NULL DEFAULT `'password'` | `String` | `password` \| `privateKey` \| `profile` |
| `password` | TEXT | nullable | `Option<String>` | ciphertext envelope or legacy plaintext |
| `password_encrypted` | INTEGER | NOT NULL DEFAULT 0 | `bool` (`i32 != 0`) | legacy electerm flag |
| `private_key` | TEXT | nullable | `Option<String>` | ciphertext envelope, PEM content |
| `passphrase` | TEXT | nullable | `Option<String>` | ciphertext envelope |
| `profile_id` | TEXT | nullable | `Option<String>` | FK-ish → `profiles.id` (no FK constraint) |
| `group_id` | TEXT | nullable | `Option<String>` | FK-ish → `bookmark_groups.id` |
| `term` | TEXT | NOT NULL DEFAULT `'xterm-256color'` | `String` | passed to `request_pty` |
| `encode` | TEXT | NOT NULL DEFAULT `'utf8'` | `String` | stored only, unused by backend |
| `color` | TEXT | nullable | `Option<String>` | UI only |
| `description` | TEXT | nullable | `Option<String>` | UI only |
| `start_directory_remote` | TEXT | nullable | `Option<String>` | stored only; backend never `cd`s |
| `start_directory_local` | TEXT | nullable | `Option<String>` | stored only |
| `enable_sftp` | INTEGER | NOT NULL DEFAULT 1 | `bool` (`i32 != 0`) | stored only |
| `keepalive_interval` | INTEGER | NOT NULL DEFAULT 30000 | `u32` | **stored only** — backend hardcodes 30 s |
| `created_at` | INTEGER | NOT NULL | `i64` | unix seconds |
| `updated_at` | INTEGER | NOT NULL | `i64` | unix seconds |

**`bookmark_groups`**

| Column | SQL type | Constraints | Rust type |
|---|---|---|---|
| `id` | TEXT | PRIMARY KEY | `String` |
| `title` | TEXT | NOT NULL | `String` |
| `parent_id` | TEXT | nullable | `Option<String>` |
| `order_index` | INTEGER | NOT NULL DEFAULT 0 | `i32` |
| `created_at` | INTEGER | NOT NULL | `i64` |

**`profiles`**

| Column | SQL type | Constraints | Rust type |
|---|---|---|---|
| `id` | TEXT | PRIMARY KEY | `String` |
| `title` | TEXT | NOT NULL | `String` |
| `username` | TEXT | NOT NULL | `String` |
| `auth_type` | TEXT | NOT NULL DEFAULT `'password'` | `String` (`password` \| `privateKey`) |
| `password` | TEXT | nullable | `Option<String>` |
| `password_encrypted` | INTEGER | NOT NULL DEFAULT 0 | `bool` |
| `private_key` | TEXT | nullable | `Option<String>` |
| `passphrase` | TEXT | nullable | `Option<String>` |
| `created_at` | INTEGER | NOT NULL | `i64` |

**`settings`** (single row, `id = 1`)

| Column | SQL type | Default | Rust type |
|---|---|---|---|
| `id` | INTEGER | PRIMARY KEY CHECK (id = 1) | — |
| `font_size` | INTEGER | 12 | `u32` |
| `font_family` | TEXT | `Menlo, Monaco, 'Courier New', monospace` | `String` |
| `theme` | TEXT | `dark` | `String` |
| `opacity` | REAL | 1.0 | `f32` |
| `language` | TEXT | `zh` | `String` |
| `scrollback` | INTEGER | 5000 | `u32` |
| `show_hidden_files` | INTEGER | 0 | `bool` |
| `default_protocol` | TEXT | `ssh` | `String` |
| `cursor_style` | TEXT | `block` | `String` |
| `cursor_blink` | INTEGER | 1 | `bool` |
| `bell_style` | TEXT | `none` | `String` |

**`trusted_host_keys`**

| Column | SQL type | Constraints | Rust type |
|---|---|---|---|
| `host` | TEXT | NOT NULL, part of PK | `String` |
| `port` | INTEGER | NOT NULL, part of PK | `u16` |
| `key_type` | TEXT | NOT NULL | `String` |
| `fingerprint` | TEXT | NOT NULL | `String` (`SHA256:<base64url-no-pad>`) |
| `created_at` | INTEGER | NOT NULL | `i64` |
| `updated_at` | INTEGER | NOT NULL | `i64` |

### 4.3 Indices & PRAGMAs

* **No explicit `CREATE INDEX`** statements exist anywhere. Only the implicit indices from the
  PRIMARY KEY definitions (`bookmarks.id`, `bookmark_groups.id`, `profiles.id`, `settings.id`,
  and a unique `(host, port)` index for `trusted_host_keys`). The composite PK table is a normal
  rowid table (no `WITHOUT ROWID`).
* `PRAGMA journal_mode=WAL;` is the only pragma ever set. WAL is persistent in the DB file, so
  every subsequent `Connection::open` inherits it.
* **No** `PRAGMA foreign_keys`, **no** `busy_timeout`, **no** `synchronous` override, **no**
  connection pool. `get_conn` opens a brand-new `Connection` for **every** storage call:

```rust
fn get_conn(db_path: &DbPath) -> Result<Connection> {
    Ok(Connection::open(&db_path.0)?)
}
```

  **egui port guidance:** because each call opens/closes a connection and no `busy_timeout` is
  set, concurrent writers can hit `SQLITE_BUSY`. Preserve behaviour by keeping writes short, or
  improve it deliberately with a `busy_timeout` (a behavioural change, document it).

### 4.4 Migration / init sequence

There is **no version table and no migration framework**. The startup sequence is:

1. `Connection::open(db_path)` — creates the file if missing.
2. `execute_batch(§4.1)` — idempotent `CREATE TABLE IF NOT EXISTS` for every table plus
   `INSERT OR IGNORE INTO settings (id) VALUES (1)`.
3. `storage::normalize_stored_secrets(&DbPath(db_path))` — re-encrypts/normalises all secrets
   (see §12.4). This runs on **every** launch and is the only "migration".
4. `.expect("failed to initialize database")` / `.expect("failed to normalize stored secrets")` —
   panic on failure.

Schema changes in the future are expected to be additive `IF NOT EXISTS` + `ALTER TABLE` guarded
by pragma inspection; none exist today.

---

## 5. Data models (`models.rs`)

**No serde rename attributes exist anywhere.** All structs derive
`#[derive(Debug, Clone, Serialize, Deserialize)]` and therefore serialise to JSON with
**`snake_case` keys matching the Rust field names verbatim**. The TypeScript types in
`src/types/index.ts` confirm this (e.g. `password_encrypted`, `start_directory_remote`).
Likewise, **no `#[serde(default)]`** and **no `#[serde(skip_serializing_if)]`**: every field is
required on deserialisation except `Option<T>` fields, which accept `null` (and must be present —
Tauri's deserialiser rejects missing fields unless the type is `Option`, in which case serde
treats a missing field as `None` only when `default` is set; **in practice the frontend always
sends every field explicitly**).

### 5.1 `Bookmark`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String, // "password" | "privateKey" | "profile"
    pub password: Option<String>,
    pub password_encrypted: bool,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
    pub profile_id: Option<String>,
    pub group_id: Option<String>,
    pub term: String,
    pub encode: String,
    pub color: Option<String>,
    pub description: Option<String>,
    pub start_directory_remote: Option<String>,
    pub start_directory_local: Option<String>,
    pub enable_sftp: bool,
    pub keepalive_interval: u32,
    pub created_at: i64,
    pub updated_at: i64,
}
```

Default constructor (used by the frontend when creating a new host; not called by commands):

```rust
impl Bookmark {
    pub fn new_default() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        Self {
            id: Uuid::new_v4().to_string(),
            title: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            auth_type: "password".to_string(),
            password: None,
            password_encrypted: false,
            private_key: None,
            passphrase: None,
            profile_id: None,
            group_id: None,
            term: "xterm-256color".to_string(),
            encode: "utf8".to_string(),
            color: None,
            description: None,
            start_directory_remote: None,
            start_directory_local: None,
            enable_sftp: true,
            keepalive_interval: 30000,
            created_at: now,
            updated_at: now,
        }
    }
}
```

### 5.2 `BookmarkGroup`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkGroup {
    pub id: String,
    pub title: String,
    pub parent_id: Option<String>,
    pub order_index: i32,
    pub created_at: i64,
}

impl BookmarkGroup {
    pub fn new(title: String) -> Self {
        let now = /* unix seconds */;
        Self { id: Uuid::new_v4().to_string(), title, parent_id: None, order_index: 0, created_at: now }
    }
}
```

### 5.3 `Profile`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub title: String,
    pub username: String,
    pub auth_type: String,
    pub password: Option<String>,
    pub password_encrypted: bool,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
    pub created_at: i64,
}

impl Profile {
    pub fn new(title: String, username: String) -> Self { /* id=Uuid v4, auth_type="password",
        password/private_key/passphrase=None, password_encrypted=false, created_at=now */ }
}
```

### 5.4 `Settings`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub font_size: u32,
    pub font_family: String,
    pub theme: String,
    pub opacity: f32,
    pub language: String,
    pub scrollback: u32,
    pub show_hidden_files: bool,
    pub default_protocol: String,
    pub cursor_style: String,
    pub cursor_blink: bool,
    pub bell_style: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: 12,
            font_family: "Menlo, Monaco, 'Courier New', monospace".to_string(),
            theme: "dark".to_string(),
            opacity: 1.0,
            language: "zh".to_string(),
            scrollback: 5000,
            show_hidden_files: false,
            default_protocol: "ssh".to_string(),
            cursor_style: "block".to_string(),
            cursor_blink: true,
            bell_style: "none".to_string(),
        }
    }
}
```

The `Default` impl is **never used by the backend** (the row is always created by
`INSERT OR IGNORE`), but it documents intended values; the SQL defaults in §4.1 are identical.

### 5.5 `FileInfo`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<i64>,
    pub permissions: Option<String>,
    pub owner: Option<String>,
}
```

* Remote listing: `permissions` = `format!("{:o}", stat.perm)` (octal **without** leading zero,
  e.g. `"755"`, `"644"`); `owner` always `None`.
* Local listing: `permissions` and `owner` always `None`.

### 5.6 `TransferProgress`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub id: String,
    pub file_name: String,
    pub direction: String, // "upload" | "download"
    pub total: u64,
    pub transferred: u64,
    pub status: String, // "pending" | "transferring" | "done" | "error" | "conflict"
    pub error: Option<String>,
    pub target_path: Option<String>,
    pub conflict_path: Option<String>,
    pub conflict_is_dir: Option<bool>,
}
```

* `"conflict"` status is produced **only by the frontend** when it catches `CONFLICT:`; the
  backend never emits it (backend emits `error` or returns `Err`).
* `conflict_path` / `conflict_is_dir` are always `None` from the backend.

### 5.7 `RemoteDeleteStatus`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDeleteStatus {
    pub path: String,
    pub is_dir: bool,
    pub success: bool,
    pub error: Option<String>,
}
```

### 5.8 `TrustedHostKey`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedHostKey {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub created_at: i64,
    pub updated_at: i64,
}
```

### 5.9 `HostKeyVerificationPrompt`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostKeyVerificationPrompt {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub reason: String,   // "unknown" | "mismatch"
}
```

### 5.10 Legacy password obfuscation (electerm-compatible)

```rust
pub fn encode_password(s: &str) -> String {
    s.chars()
        .enumerate()
        .map(|(i, c)| char::from_u32(((c as u32 + i as u32 + 1) % 65536) as u32).unwrap_or(c))
        .collect()
}

pub fn decode_password(s: &str) -> String {
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            let code = c as u32;
            let shifted = i as u32 + 1;
            let result = if code >= shifted { code - shifted } else { code + 65536 - shifted };
            char::from_u32(result).unwrap_or(c)
        })
        .collect()
}
```

Per-character shift by `index + 1` modulo 65536. Only used when reading rows written by the
legacy electerm importer (`password_encrypted = 1` and value not carrying the `ttenc:v1:` prefix).

---

## 6. Error model

* **Storage / crypto / ssh layers** return `anyhow::Result<T>`. There are **no** custom error
  enums (`thiserror` is unused).
* **Command layer** returns `Result<T, String>`; the conversion is always
  `.map_err(|e| e.to_string())` (or `map_err(|e| format!("context: {}", e))` for grouped phases).
  The frontend therefore receives a **plain string** in the rejected `invoke()` promise.
* Some commands construct errors with `format!` directly, notably:
  * `"HOST_KEY_PROMPT:<json>"` (§11)
  * `"CONFLICT:<path>"` (§13.1)
  * `"Command failed with exit code {code}: {stdout}, {stderr}"` (§9.5.10, §9.6.13)
  * `"Session not found"` / `"Session disconnected"` / `"Bookmark not found"`
  * `"Credential '{profile_id}' not found"`
  * `"Refusing to delete ..."` guards
  * `"Cancelled"` (emitted as the `error` field of a `transfer-progress` event)
* `log` macros are used for lifecycle/observability: `info!` for create/close/trust/transfer
  success, `warn!` for ignored closes, TCP keepalive failure and transfer failures.
  `env_logger` prints them to stderr (`RUST_LOG` controlled).

**egui port guidance:** keep `Result<T, String>`-style strings if you want byte-identical
user-visible messages; otherwise define a typed error and format it to the same strings.

---

## 7. Command registry (exact names)

From `lib.rs` `tauri::generate_handler![...]` — **39 commands**:

```
bookmark: list_bookmarks, create_bookmark, update_bookmark, delete_bookmark,
          list_bookmark_groups, create_bookmark_group, update_bookmark_group, delete_bookmark_group
profile:  list_profiles, create_profile, update_profile, delete_profile
settings: get_settings, update_settings
ssh:      create_session, close_session, check_session_alive, write_to_session,
          resize_terminal, subscribe_session, get_remote_cwd, execute_remote_command,
          trust_host_key, check_host_port
sftp:     list_remote_dir, list_local_dir, scan_remote_folder, upload_file, download_file,
          cancel_transfer, delete_remote, delete_remote_async, create_remote_dir,
          delete_local, create_local_dir, rename_local, rename_remote
local_fs: pack_local_dir, unpack_local_dir
```

> **`commands::app::finish_startup` is NOT registered.** It exists in the source but is absent
> from `generate_handler!`, so it is dead code (the frontend never calls it; `tauri.conf.json`
> defines no `splash` window). Reproduced here for completeness only (§8.1).

### 7.1 Argument naming convention

Tauri v2 maps JS **camelCase** argument names to Rust **snake_case** parameters
(e.g. JS `sessionId` → Rust `session_id`). Parameters of a struct type (`request`, `bookmark`,
`group`, `profile`, `settings`) are deserialised by serde and therefore use the struct's
**snake_case** field names inside the nested object.

Confirmed examples **[frontend]**:

```ts
invoke('create_session', { request: { bookmark_id, cols, rows, password, username } })
invoke('subscribe_session', { sessionId, dataChannel: channel })
invoke('write_to_session', { sessionId, data })
invoke('resize_terminal', { sessionId, cols, rows })
invoke('upload_file', { sessionId, localPath, remotePath, overwrite, transferId,
                        displayName, progressTotal, progressStart, progressSpan,
                        targetPathOverride })
invoke('pack_local_dir', { sourceDir, targetTarPath, transferId, displayName, direction,
                           progressTotal, progressStart, progressSpan, targetPath })
invoke('delete_remote', { sessionId, path, isDir })
invoke('delete_local', { path, isDir })
```

`State<...>` parameters are **not** part of the JS argument object; `AppHandle` is injected.

---

## 8. Commands by domain

### 8.1 `app` (`commands/app.rs`)

#### `finish_startup(app: AppHandle) -> Result<(), String>` — *unregistered*

1. If `app.get_webview_window("main")` exists: `main_window.show()` (map error with
   `to_string`), then `main_window.set_focus()` (error ignored).
2. If `app.get_webview_window("splash")` exists: `splash_window.close()` (error ignored).
3. Return `Ok(())`.

**egui port:** the equivalent is "mark startup complete and reveal the main window"; there is no
splash window in the egui design unless you add one.

### 8.2 `settings` (`commands/settings.rs`)

```rust
#[tauri::command]
pub fn get_settings(db_path: State<DbPath>) -> Result<Settings, String> {
    storage::get_settings(&db_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_settings(db_path: State<DbPath>, settings: Settings) -> Result<(), String> {
    storage::update_settings(&db_path, &settings).map_err(|e| e.to_string())
}
```

**`get_settings` algorithm**

1. Open a fresh connection.
2. `SELECT font_size, font_family, theme, opacity, language, scrollback, show_hidden_files,
   default_protocol, cursor_style, cursor_blink, bell_style FROM settings WHERE id=1`
   (`query_row`, no params).
3. Map booleans with `row.get::<_, i32>(n)? != 0`.
4. Return `Settings`. If the row is missing (`QueryReturnedNoRows`) the error propagates as a
   string — the row always exists because of `INSERT OR IGNORE` at init.

**`update_settings` algorithm**

1. Open a fresh connection.
2. Single `UPDATE settings SET font_size=?1, font_family=?2, theme=?3, opacity=?4, language=?5,
   scrollback=?6, show_hidden_files=?7, default_protocol=?8, cursor_style=?9, cursor_blink=?10,
   bell_style=?11 WHERE id=1`, with booleans bound as `i32` (`as i32`).
3. Return `Ok(())`. No read-back, no validation, no partial merge — the whole struct is replaced.

### 8.3 `profile` (`commands/profile.rs`)

Shared private helpers (identical in shape to the bookmark ones):

```rust
fn resolve_profile_password(db_path: &DbPath, profile: &Profile,
                            existing_profile: Option<&Profile>) -> Result<Option<String>, String>
fn resolve_profile_plain_secret(db_path: &DbPath, incoming_value: Option<&str>,
                                existing_db_value: Option<&String>) -> Result<Option<String>, String>
fn redact_profile(profile: &mut Profile)   // password=None, password_encrypted=false,
                                           // private_key=None, passphrase=None
```

**`resolve_profile_password` algorithm**

1. If `profile.password` is `Some(p)` and `!p.is_empty()` → return `Some(p)` (incoming plaintext
   wins; the frontend sends `undefined`/omitted when the user did not retype it).
2. Else if the existing DB row has a non-empty `password`:
   * if `crypto::is_encrypted_secret(p)` → `decrypt_secret(db_path, p)` → `Some(plaintext)`
     (error → `Err(e.to_string())`).
   * else if `existing.password_encrypted` → `Some(models::decode_password(p))` (legacy electerm).
   * else → `Some(p)` (legacy plaintext).
3. Else → `Ok(None)`.

**`resolve_profile_plain_secret` algorithm** (for `private_key` / `passphrase`)

1. Incoming non-empty value → `Some(value)`.
2. Else existing non-empty DB value → decrypt if prefixed with `ttenc:v1:`, otherwise use as-is.
3. Else `None`. (Note: no legacy `decode_password` path here.)

#### `list_profiles(db_path: State<DbPath>) -> Result<Vec<Profile>, String>`

1. `storage::list_profiles` → `SELECT id, title, username, auth_type, password,
   password_encrypted, private_key, passphrase, created_at FROM profiles ORDER BY created_at ASC`.
2. Map every row to `Profile`, booleans via `i32 != 0`.
3. **Redact each profile** (`redact_profile`) so the frontend never receives secrets.
4. Return the redacted list.

#### `create_profile(db_path, profile: Profile) -> Result<Profile, String>`

1. Match `profile.auth_type`:
   * `"password"`:
     * `password = profile.password.take().filter(|v| !v.is_empty())`
     * `profile.password = password.map(|v| crypto::encrypt_secret(&db_path.0, &v))` (error →
       string)
     * `profile.password_encrypted = profile.password.is_some()`
     * `private_key = None`, `passphrase = None`
   * `"privateKey"`:
     * `password = None`, `password_encrypted = false`
     * `private_key = encrypt_secret(non-empty private_key or None)`
     * `passphrase = encrypt_secret(non-empty passphrase or None)`
   * `_` (anything else, including `"profile"`): all four secret fields cleared.
2. `storage::create_profile` → `INSERT INTO profiles (id, title, username, auth_type, password,
   password_encrypted, private_key, passphrase, created_at) VALUES (?1..?9)` with
   `password_encrypted as i32`.
3. Return the profile **redacted** (secrets stripped) so the caller cannot read back the envelope.

No uniqueness check on `id`; a duplicate id fails with a SQLite constraint error string.

#### `update_profile(db_path, profile: Profile) -> Result<Profile, String>`

1. Load **all** profiles (`storage::list_profiles`) and `find(|item| item.id == profile.id)` to
   get `existing_profile` (note: full table scan, not a `WHERE id` query).
2. Same `auth_type` match as `create_profile`, but secret values are resolved with the
   `resolve_*` helpers so that an unchanged/omitted secret is preserved:
   * `"password"` → `resolve_profile_password` → re-encrypt → `password_encrypted = is_some()`.
   * `"privateKey"` → `resolve_profile_plain_secret` for key and passphrase → re-encrypt both.
   * `_` → clear all.
3. `storage::update_profile` → `UPDATE profiles SET title=?2, username=?3, auth_type=?4,
   password=?5, password_encrypted=?6, private_key=?7, passphrase=?8 WHERE id=?1`.
   `created_at` is never modified.
4. Return the profile redacted.

#### `delete_profile(db_path, id: String) -> Result<(), String>`

1. `DELETE FROM profiles WHERE id=?1`.
2. Return `Ok(())`. **No cascade**: `bookmarks.profile_id` values referencing the deleted profile
   become dangling. A later connect on such a bookmark fails with
   `Credential '<profile_id>' not found` (§9.5.1).

### 8.4 `bookmark` (`commands/bookmark.rs`)

Private helpers mirror the profile ones, with the extra legacy boolean argument:

```rust
fn resolve_bookmark_password(db_path, bookmark: &Bookmark, existing: Option<&Bookmark>) -> Result<Option<String>, String>
fn resolve_bookmark_plain_secret(db_path, incoming: Option<&str>, existing_db: Option<&String>) -> Result<Option<String>, String>
fn redact_bookmark(b: &mut Bookmark)  // password=None, password_encrypted=false,
                                      // private_key=None, passphrase=None
```

`resolve_bookmark_password` is byte-for-byte the same algorithm as
`resolve_profile_password` (incoming non-empty wins → existing decrypted/decoded → `None`).

#### `list_bookmarks(db_path) -> Result<Vec<Bookmark>, String>`

1. `storage::list_bookmarks` → `SELECT id, title, host, port, username, auth_type, password,
   password_encrypted, private_key, passphrase, profile_id, group_id, term, encode, color,
   description, start_directory_remote, start_directory_local, enable_sftp, keepalive_interval,
   created_at, updated_at FROM bookmarks ORDER BY created_at ASC`.
2. Map rows to `Bookmark` (`enable_sftp`/`password_encrypted` via `i32 != 0`).
3. Redact every bookmark.
4. Return.

#### `create_bookmark(db_path, bookmark: Bookmark) -> Result<Bookmark, String>`

1. Same three-way `auth_type` match as `create_profile` (encrypt `password` for `"password"`,
   encrypt `private_key` + `passphrase` for `"privateKey"`, clear everything otherwise).
2. `INSERT INTO bookmarks (id, title, host, port, username, auth_type, password,
   password_encrypted, private_key, passphrase, profile_id, group_id, term, encode, color,
   description, start_directory_remote, start_directory_local, enable_sftp, keepalive_interval,
   created_at, updated_at) VALUES (?1..?22)`.
   `created_at`/`updated_at` come **from the frontend payload unchanged** (the command does not
   stamp them).
3. Return the bookmark redacted.

#### `update_bookmark(db_path, bookmark: Bookmark) -> Result<Bookmark, String>`

1. Load all bookmarks, find by `id` → `existing_bookmark`.
2. Same `auth_type` match using `resolve_bookmark_password` / `resolve_bookmark_plain_secret`,
   re-encrypting the resolved plaintexts.
3. Stamp `bookmark.updated_at = now_unix()` (system clock, `as_secs() as i64`).
4. `UPDATE bookmarks SET title=?2 ... keepalive_interval=?20, updated_at=?21 WHERE id=?1`
   (`created_at` untouched).
5. Return the bookmark redacted.

#### `delete_bookmark(db_path, id: String) -> Result<(), String>`

`DELETE FROM bookmarks WHERE id=?1`; `Ok(())`.

#### `list_bookmark_groups(db_path) -> Result<Vec<BookmarkGroup>, String>`

`SELECT id, title, parent_id, order_index, created_at FROM bookmark_groups ORDER BY order_index ASC`
→ `Vec<BookmarkGroup>` (no redaction needed).

#### `create_bookmark_group(db_path, group: BookmarkGroup) -> Result<BookmarkGroup, String>`

`INSERT INTO bookmark_groups (id, title, parent_id, order_index, created_at) VALUES (?1..?5)`;
returns the group **as supplied** (echo, not re-read).

#### `update_bookmark_group(db_path, group: BookmarkGroup) -> Result<BookmarkGroup, String>`

`UPDATE bookmark_groups SET title=?2, parent_id=?3, order_index=?4 WHERE id=?1`; returns the echo.

#### `delete_bookmark_group(db_path, id: String) -> Result<(), String>`

1. `DELETE FROM bookmark_groups WHERE id=?1`.
2. `UPDATE bookmarks SET group_id=NULL WHERE group_id=?1` (bookmarks are re-parented to "no group",
   never deleted).
3. `Ok(())`. **Child groups are not re-parented** — their `parent_id` becomes dangling.

### 8.5 `ssh` / terminal (`commands/ssh.rs`)

#### Types

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub bookmark_id: String,
    pub cols: u32,
    pub rows: u32,
    pub password: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustHostKeyRequest {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
}
```

Private helpers:

```rust
fn now_unix() -> i64                                    // system time, seconds
fn host_key_prompt_error(prompt: &HostKeyVerificationPrompt) -> String
fn resolve_password_secret(db_path: &DbPath, db_value: Option<String>, password_encrypted: bool) -> Result<Option<String>, String>
fn resolve_plain_secret(db_path: &DbPath, db_value: Option<String>) -> Result<Option<String>, String>
fn hydrate_bookmark_auth(db_path: &DbPath, bookmark: &mut Bookmark) -> Result<(), String>
fn resolve_bookmark_for_connection(db_path: &DbPath, bookmark_id: &str) -> Result<(Bookmark, Option<String>), String>
fn ensure_host_key_trusted(db_path: &DbPath, bookmark: &Bookmark, sess: &ssh2::Session) -> Result<(String, String), String>
```

`resolve_password_secret`: empty/`None` → `None`; `ttenc:v1:` prefix → `decrypt_secret`;
else `password_encrypted == true` → `models::decode_password`; else the raw value.

`resolve_plain_secret`: empty/`None` → `None`; `ttenc:v1:` prefix → `decrypt_secret`;
else raw value.

`hydrate_bookmark_auth`: per `auth_type`:
* `"password"` → `password = resolve_password_secret(...)`, `password_encrypted = false`,
  `private_key = None`, `passphrase = None`.
* `"privateKey"` → `private_key`/`passphrase` through `resolve_plain_secret`, `password = None`,
  `password_encrypted = false`.
* `_` → clear all four.

#### 8.5.1 `create_session(db_path, session_manager, request) -> Result<CreateSessionResponse, String>`

Logs `create_session request bookmark_id={} cols={} rows={}`.

Algorithm, in exact order:

1. `let (bookmark, _) = resolve_bookmark_for_connection(&db_path, &request.bookmark_id)?;`
   * Load **all** bookmarks (`list_bookmarks`), `find(|b| b.id == bookmark_id).cloned()`; not
     found → `Err("Bookmark not found")`.
   * If `bookmark.auth_type == "profile"`:
     * `profile_id` must be `Some(id)` with `!id.is_empty()`, otherwise **return the bookmark
       as-is with `None`** (frontend supplies username/password in the request).
     * Load all profiles, find by id; missing → `Err("Credential '{profile_id}' not found")`.
     * Merge: `bookmark.username = profile.username`, `bookmark.auth_type = profile.auth_type`,
       `bookmark.password = resolve_password_secret(profile.password, profile.password_encrypted)`,
       `password_encrypted = false`, `private_key`/`passphrase` via `resolve_plain_secret`.
     * Return `(bookmark, Some(profile_id))`.
   * Else `hydrate_bookmark_auth(&mut bookmark)` and return `(bookmark, None)`.
2. Apply the username override: if `request.username` is `Some(u)` and `!u.is_empty()`:
   `bookmark.username = u`; and if `bookmark.auth_type.is_empty() || == "profile"` then
   `bookmark.auth_type = "password"`.
3. `let password = request.password.as_deref();`
4. `ssh::connect_ssh_transport(&bookmark)` — TCP + SSH handshake only (no auth). Error →
   `Err(e.to_string())`, e.g. `TCP connect to host:22 failed: ...`, `SSH session init failed: ...`,
   `SSH handshake failed: ...`.
5. `ensure_host_key_trusted(&db_path, &bookmark, &sess)` → `(fingerprint, key_type)` or
   `HOST_KEY_PROMPT:<json>` error (§11).
6. `ssh::authenticate_ssh(&sess, &bookmark, password)` — `request.password` takes precedence over
   the stored/decrypted `bookmark.password`; private-key auth ignores the password entirely.
7. `ssh::open_shell_channel(&sess, &bookmark.term, request.cols, request.rows)` — `request_pty(term,
   None, Some((cols, rows, 0, 0)))` then `shell()`.
8. `sess.set_keepalive(true, 30)` — SSH-level keepalive every **30 seconds** of idle
   (`want_reply = true`). `bookmark.keepalive_interval` is **ignored**.
9. `sess.set_blocking(false)` — non-blocking libssh2 mode for the PTY session.
10. Wrap: `session_arc = Arc::new(Mutex::new(sess))`, `channel_arc = Arc::new(Mutex::new(channel))`.
11. Create `mpsc::channel::<String>()` (unbounded) and spawn the **writer thread**:

    ```rust
    std::thread::spawn(move || {
        while let Ok(data) = write_rx.recv() {
            let sess = w_session.lock();
            let mut ch = w_channel.lock();
            sess.set_blocking(true);
            let _ = ch.write_all(data.as_bytes());
            sess.set_blocking(false);
        }
    });
    ```

    * The thread owns the receiver for the session's lifetime.
    * Data is written as UTF-8 bytes of the `String`; write errors are swallowed (`let _`).
    * Dropping the `Sender` (when `SshSession` is removed from the map) ends `recv()` with `Err`
      and terminates the thread.
12. Build `SshSession` (see §3.2) with `session_id = Uuid::new_v4().to_string()`,
    `password_override: request.password`, `trusted_host_fingerprint`,
    `trusted_host_key_type`, `stop_reader: Arc::new(AtomicBool::new(false))`, `write_tx`.
13. `session_manager.sessions.lock().insert(session_id.clone(), ssh_session)`.
14. Log `create_session success session_id=... bookmark_id=... host=...:port=...`.
15. Return `CreateSessionResponse { session_id }`.

**Important:** `create_session` deliberately does **not** start the reader thread. The frontend
creates its `Channel` and calls `subscribe_session` immediately after, so no output is lost
between shell-open and subscription.

#### 8.5.2 `subscribe_session(session_manager, session_id, data_channel: tauri::ipc::Channel<String>) -> Result<(), String>`

1. Lock `sessions`; `get(&session_id)` or `Err("Session not found")`; clone `session` Arc,
   `channel` Arc, `stop_reader` Arc. **Release the map lock** before returning.
2. `stop.store(false, Ordering::Relaxed)` (allows re-subscribe after reconnect).
3. Spawn the reader thread:

```rust
let mut buf = vec![0u8; 16384];
let mut batch = String::new();
let mut last_flush = std::time::Instant::now();
const FLUSH_MS: u128 = 5;

loop {
    if stop.load(Ordering::Relaxed) {
        if !batch.is_empty() { let _ = data_channel.send(batch); }
        break;
    }
    let data_opt = {
        let _sess_guard = match session_arc.try_lock() {
            Some(g) => g,
            None => { std::thread::sleep(Duration::from_micros(500)); continue; }
        };
        let mut ch = channel_arc.lock();
        match ch.read(&mut buf) {
            Ok(0) => None,
            Ok(n) => Some(String::from_utf8_lossy(&buf[..n]).to_string()),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
            Err(_) => if ch.eof() { Some("\r\n[Session closed]\r\n".to_string()) } else { None },
        }
    };  // both locks released here
    match data_opt {
        Some(text) => {
            batch.push_str(&text);
            if last_flush.elapsed().as_millis() >= FLUSH_MS || batch.len() >= 4096 {
                if data_channel.send(std::mem::take(&mut batch)).is_err() { break; }
                last_flush = std::time::Instant::now();
            }
            // no sleep: keep filling the batch
        }
        None => {
            if !batch.is_empty() && last_flush.elapsed().as_millis() >= FLUSH_MS {
                if data_channel.send(std::mem::take(&mut batch)).is_err() { break; }
                last_flush = std::time::Instant::now();
            }
            std::thread::sleep(Duration::from_micros(1000));
        }
    }
}
```

**Aggregation window: exactly 5 ms** (`FLUSH_MS = 5`), with an additional hard flush when the
pending batch reaches **4096 bytes**. Read buffer: **16384 bytes**. Idle poll interval: **1000 µs**.
`try_lock` contention backoff: **500 µs**. Output is decoded with `String::from_utf8_lossy`
(invalid UTF-8 becomes U+FFFD; raw bytes are never forwarded). `Channel::send` failure (JS side
gone) breaks the loop. EOF sentinel `"\r\n[Session closed]\r\n"` is sent whenever a read error
occurs and `ch.eof()` is true — it can repeat on subsequent iterations until the thread stops.

#### 8.5.3 `close_session(session_manager, session_id: String) -> Result<(), String>`

1. Log request.
2. `sessions.lock().remove(&session_id)` → `Option<SshSession>` (lock released immediately).
3. If present:
   * `stop_reader.store(true, Ordering::Relaxed)`.
   * Lock `sftp_session`; if `Some(sftp)`, `sftp.disconnect(None, "closing", None).ok()` (reason
     code `None`, description `"closing"`, language `None`); the `Option` is taken (set to `None`).
   * `let _sess = ssh_sess.session.lock();` then `ssh_sess.channel.lock().send_eof()` (result
     ignored). Holding the session guard ensures any in-flight writer write finishes first.
   * Dropping `ssh_sess` drops `write_tx` → writer thread exits.
   * Log success.
4. If absent: `warn!("close_session ignored missing session_id={}")` and return `Ok(())` —
   **never an error** (idempotent close).

#### 8.5.4 `check_session_alive(session_manager, session_id: String) -> Result<bool, String>`

1. Lock map; missing session → `Ok(false)`.
2. Lock session; `!sess.authenticated()` → `Ok(false)`.
3. `sess.keepalive_send().is_err()` → `Ok(false)` (actively probes the connection).
4. Lock channel; `channel.eof()` → `Ok(false)`.
5. `Ok(true)`.

Not called by the current frontend **[frontend]** but must be preserved.

#### 8.5.5 `write_to_session(session_manager, session_id: String, data: String) -> Result<(), String>`

1. Lock map, get session or `Err("Session not found")`, clone `write_tx`, release lock.
2. `tx.send(data).map_err(|e| e.to_string())` — returns immediately (fire-and-forget); only fails
   if the writer thread has exited.

#### 8.5.6 `resize_terminal(session_manager, session_id: String, cols: u32, rows: u32) -> Result<(), String>`

1. Lock map; **missing session → `Ok(())`** (silent no-op).
2. Clone `session` and `channel` Arcs, release map lock.
3. Lock session, then `channel.lock().request_pty_size(cols, rows, None, None)`
   (pixel width/height `None`), error → `Err(e.to_string())`.

#### 8.5.7 `get_remote_cwd(session_manager, session_id: String) -> Result<String, String>`

1. Lock map, get `session` Arc or `Err("Session not found")`, release lock.
2. Lock the session (held for the whole operation).
3. `sess.set_blocking(true); sess.set_timeout(5000);` — 5-second socket/operation timeout.
4. Open a **new exec channel** and run this exact POSIX shell one-liner:

```sh
p=$(cat /proc/$$/status 2>/dev/null|grep -m1 '^PPid:'|awk '{print $2}');if [ -n "$p" ];then for f in /proc/[0-9]*/status;do pid="${f#/proc/}";pid="${pid%/status}";[ "$pid" = "$$" ]&&continue;pp=$(grep -m1 '^PPid:' "$f" 2>/dev/null|awk '{print $2}');if [ "$pp" = "$p" ];then cwd=$(readlink "/proc/$pid/cwd" 2>/dev/null)&&[ -n "$cwd" ]&&echo "$cwd"&&exit 0;fi;done;fi;if command -v lsof >/dev/null 2>&1&&[ -n "$p" ];then cwd=$(lsof -a -p "$p" -d cwd -F n 2>/dev/null|grep '^n/'|cut -c2-);[ -n "$cwd" ]&&echo "$cwd"&&exit 0;fi;pwd
```

   Strategy, in order:
   1. **`/proc` sibling scan** — read `PPid:` of `$$` (the exec shell), then scan every
      `/proc/<pid>/status` for a process whose `PPid:` equals that, and `readlink /proc/<pid>/cwd`.
      This finds the PTY shell (the parent of the exec channel's shell) even though the exec
      channel itself runs in a different process.
   2. **`lsof`** — `lsof -a -p "$p" -d cwd -F n` on the parent pid, stripping the leading `n`.
   3. **`pwd`** — universal fallback (prints the exec shell's cwd, not the terminal's).
5. `exec_channel.exec(cmd)`; `read_to_string(&mut output)`; `wait_close().ok()` (ignored).
6. `cwd = output.trim()`; empty → `Err("empty cwd output")`, else `Ok(cwd)`.
7. Always (after the closure): `sess.set_timeout(0); sess.set_blocking(false);` then return the
   captured result. Note the timeout reset happens even on error, but **not** if the thread panics.

Error strings: `"channel_session: {e}"`, `"exec cwd command: {e}"`, `"read cwd output: {e}"`,
`"empty cwd output"`.

#### 8.5.8 `trust_host_key(db_path, request: TrustHostKeyRequest) -> Result<(), String>`

1. Log `trust_host_key host={}:{} key_type={} fingerprint={}`.
2. `now = now_unix()`.
3. `storage::upsert_trusted_host_key(&db_path, &TrustedHostKey { host, port, key_type,
   fingerprint, created_at: now, updated_at: now })`:

```sql
INSERT INTO trusted_host_keys (host, port, key_type, fingerprint, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT(host, port) DO UPDATE SET
  key_type = excluded.key_type,
  fingerprint = excluded.fingerprint,
  updated_at = excluded.updated_at
```

   `created_at` is **not** updated on conflict (original creation time is retained).

#### 8.5.9 `check_host_port(host: String, port: u16) -> Result<bool, String>`

* **No state parameters at all.**
1. `host = host.trim()`; if `host.is_empty() || port == 0` → `Ok(false)`.
2. `(host, port).to_socket_addrs()`; error → `Err(format!("resolve {}:{} failed: {}", host, port, e))`.
3. Empty address list → `Ok(false)`.
4. `timeout = 1500 ms`; try `TcpStream::connect_timeout(&addr, timeout)` for each resolved address
   **in order**; first success → `Ok(true)`.
5. All failed → `Ok(false)`. Never returns `Err` for connection refusal (only for DNS failure).

#### 8.5.10 `execute_remote_command(session_id: String, command: String, session_manager: State<SessionManager>) -> Result<String, String>`

> Parameter order differs from other commands (`session_manager` is last).

1. `let sessions = session_manager.sessions.lock();` — **the map lock is held for the entire
   command** (see caveat below).
2. `sessions.get(&session_id).ok_or("Session disconnected")?`.
3. `let sess = session.session.lock();`
4. `sess.set_blocking(true);` (**no timeout is set** — an unresponsive host can block indefinitely).
5. Open a new channel; `exec(&command)`; `read_to_string(&mut s)`; `wait_close()?`.
6. `exit_status = channel.exit_status().unwrap_or(-1)`; if `!= 0`: read `channel.stderr()` into a
   string (errors ignored) and return
   `Err(format!("Command failed with exit code {}: {}, {}", exit_status, s, stderr))`.
7. On success return `Ok(s)` (stdout only, **not trimmed**).
8. Always: `sess.set_timeout(0); sess.set_blocking(false);`

Error strings: `"Session disconnected"`, `"channel_session: {e}"`, `"exec: {e}"`, `"read: {e}"`,
`"wait_close: {e}"`, `"Command failed with exit code {code}: {stdout}, {stderr}"`.

**Caveat (must be preserved or deliberately fixed in the egui port):** because the
`sessions` guard is alive for the whole call, any concurrent command that needs the map
(`write_to_session`, `close_session`, `subscribe_session`, SFTP helpers) blocks until the remote
command finishes. The reader thread is unaffected (it only touches the session/channel Arcs).

**`execute_remote_command` is the only "run arbitrary command" primitive**; directory transfers,
`tar` unpacking, temp-file cleanup and shell introspection all go through it (§14).

### 8.6 `sftp` (`commands/sftp.rs`)

#### Shared helpers

```rust
fn get_sftp_info(session_manager: &State<SessionManager>, session_id: &str)
    -> Result<(Arc<parking_lot::Mutex<Option<ssh2::Session>>>, Bookmark, Option<String>, String, String), String>
fn ensure_sftp_session<'a>(guard: &'a mut Option<ssh2::Session>, bookmark: &Bookmark,
    password: Option<&str>, expected_host_fingerprint: &str, expected_host_key_type: &str)
    -> Result<&'a ssh2::Session, String>
fn with_sftp<T>(session_manager: &State<SessionManager>, session_id: &str,
    body: impl FnOnce(&ssh2::Sftp) -> Result<T, String>) -> Result<T, String>
fn get_ssh_session(session_manager: &State<SessionManager>, session_id: &str)
    -> Result<Arc<parking_lot::Mutex<ssh2::Session>>, String>
fn shell_quote(value: &str) -> String          // '...' with ' → '"'"'
fn normalize_remote_path(path: &str) -> String // trim + strip trailing '/' (keep single "/")
fn is_filesystem_root(path: &Path) -> bool
fn guard_local_delete_target(path: &Path) -> Result<(), String>
fn guard_remote_delete_target(session_manager, session_id, path) -> Result<String, String>
fn get_remote_home_dir(ssh_session: &Arc<Mutex<Session>>) -> Result<String, String>
fn map_stage_progress(raw_total: u64, raw_transferred: u64, stage_start: u64, stage_span: u64) -> u64
fn emit_transfer_progress(app, transfer_id, file_name, direction, total, transferred, status,
                          error, target_path, conflict_path, conflict_is_dir)
fn emit_remote_delete_status(app, path, is_dir, success, error)
fn scan_folder_recursive(sftp: &ssh2::Sftp, base_path: &str) -> Result<Vec<FileInfo>, String>
fn remove_remote_dir_recursive(sftp: &ssh2::Sftp, dir_path: &Path) -> Result<(), String>
fn remove_remote_path(sftp: &ssh2::Sftp, path: &Path, is_dir: bool) -> Result<(), String>
fn remove_remote_path_fast(ssh_session: &Arc<Mutex<Session>>, path: &str, is_dir: bool) -> Result<(), String>
```

**`get_sftp_info`** — locks the map, clones the `sftp_session` Arc plus `resolved_bookmark`,
`password_override`, `trusted_host_fingerprint`, `trusted_host_key_type`; releases the map lock
immediately (returns an owned tuple). Missing session → `Err("Session not found")`.

**`ensure_sftp_session`** — if the guard is `None`:

1. `ssh::connect_ssh_transport(bookmark)` → error `"SFTP connection failed: {e}"`.
2. `ssh::verify_host_key(&sess, expected_host_key_type, expected_host_fingerprint)` → error
   `"SFTP host key verification failed: {e}"` (strict match against the fingerprint pinned at
   `create_session` time; **no trust prompt** on the SFTP connection).
3. `ssh::authenticate_ssh(&sess, bookmark, password)` → error `"SFTP authentication failed: {e}"`.
4. `sess.set_blocking(true)` — permanently blocking (never toggled; no PTY contention).
5. Store in the guard.

**`with_sftp`** — full algorithm:

1. `get_sftp_info`.
2. `let mut guard = sftp_arc.lock();` (held for the whole body).
3. `ensure_sftp_session(...)`.
4. `sess.sftp()`; on error: `*guard = None;` (drop cached connection) and return
   `Err("SFTP init failed: {e}")`.
5. `let result = body(&sftp);`
6. If `result` is `Err` and the **lowercased** message contains any of
   `"channel"`, `"transport"`, `"eof"`, `"broken pipe"`, `"connection reset"` → `*guard = None;`
   so the next call reconnects.
7. Return `result`.

#### 8.6.1 `list_remote_dir(session_manager, session_id: String, path: String) -> Result<Vec<FileInfo>, String>`

1. `with_sftp` → `sftp.readdir(Path::new(&path))`, error `"readdir failed: {e}"`.
2. For each `(pathbuf, stat)`:
   * `name = pathbuf.file_name().and_then(to_str).unwrap_or("")`
   * `full_path = format!("{}/{}", path.trim_end_matches('/'), name)` (note: for `path == "/"`
     this yields `/name`).
   * `is_dir = stat.is_dir()`, `size = stat.size.unwrap_or(0)`,
     `modified = stat.mtime.map(|t| t as i64)`,
     `permissions = stat.perm.map(|p| format!("{:o}", p))`, `owner = None`.
3. Sort: directories first, then **byte-wise** `name` ascending:
   `if a.is_dir == b.is_dir { a.name.cmp(&b.name) } else { b.is_dir.cmp(&a.is_dir) }`.
4. Return. `.`/`..` are **not** filtered here (OpenSSH's readdir does not return them).

#### 8.6.2 `list_local_dir(path: String) -> Result<Vec<FileInfo>, String>`

* **No session/state parameters.**
1. `fs::read_dir(&path)` (error → string).
2. For each entry (`.flatten()` — individual entry errors are skipped):
   * `meta = entry.metadata()?`
   * `name = entry.file_name().to_string_lossy()`
   * `path = entry.path().to_string_lossy()`
   * `modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64)`
   * `is_dir = meta.is_dir()`, `size = meta.len()`, `permissions = None`, `owner = None`.
3. Same sort as remote. Hidden files are **not** filtered by the backend (the frontend applies
   `show_hidden_files`).

#### 8.6.3 `scan_remote_folder(session_manager, session_id: String, path: String) -> Result<Vec<FileInfo>, String>`

`with_sftp` → `scan_folder_recursive(sftp, &path)`:

* `readdir(base_path)`; error `"readdir failed: {e}"`.
* For each entry: `full_path = format!("{}/{}", base_path.trim_end_matches('/'), name)`.
  * If `stat.is_dir()`: skip names `"."` and `".."`, otherwise recurse and extend.
  * Else push `FileInfo { is_dir: false, size: stat.size.unwrap_or(0), modified: stat.mtime.map(i64),
    permissions: stat.perm.map(|p| format!("{:o}", p)), owner: None }`.
* Returns a **flat list of files only** (no directory entries, no sort order guarantee beyond
  readdir order). Used by the frontend to pre-compute a batch download size.

#### 8.6.4 `cancel_transfer(session_manager, transfer_id: String) -> Result<(), String>`

`session_manager.cancelled_transfers.lock().insert(transfer_id); Ok(())`.
Cooperative cancellation: the transfer loop polls the set once per chunk and aborts with
`Err("Cancelled")`. Entries are removed by the transfer thread at start, on success and on error.

#### 8.6.5 `upload_file(...) -> Result<(), String>`

```rust
pub fn upload_file(
    session_manager: State<SessionManager>,
    session_id: String,
    local_path: String,
    remote_path: String,
    overwrite: bool,
    transfer_id: Option<String>,
    display_name: Option<String>,
    progress_total: Option<u64>,
    progress_start: Option<u64>,
    progress_span: Option<u64>,
    target_path_override: Option<String>,
    app: AppHandle,
) -> Result<(), String>
```

Algorithm (all before spawning):

1. Log request (`transfer_id` defaults in the log line to `upload:{remote_path}`).
2. `file_name = display_name` or `Path::new(local_path).file_name()` or `"file"`.
3. `raw_total = fs::metadata(&local_path)?.len()` (error → string; the file must exist).
4. `transfer_id = transfer_id.unwrap_or_else(|| format!("upload:{}", remote_path))`.
5. `stage_total = progress_total.unwrap_or(raw_total)`,
   `stage_start = progress_start.unwrap_or(0)`,
   `stage_span = progress_span.unwrap_or(raw_total)`,
   `target_path = target_path_override.unwrap_or(remote_path.clone())`.
6. **Conflict check** (skipped when `overwrite == true`): `with_sftp` → `sftp.stat(remote_path).is_ok()`;
   if true → `Err(format!("CONFLICT:{}", remote_path))` **before** any event.
7. `get_sftp_info(...)` (owned tuple) and `Arc::clone(&session_manager.cancelled_transfers)`;
   `cancelled_transfers.lock().remove(&transfer_id)`.
8. Emit `transfer-progress` with `status = "pending"`, `total = stage_total`,
   `transferred = stage_start`, `target_path = Some(target_path)`, all conflict fields `None`.
9. `thread::spawn`:

   1. `File::open(&local_path)` + `BufReader`.
   2. Lock the SFTP guard; `ensure_sftp_session(...)`.
   3. `sess.sftp()`; on error invalidate the guard and return `Err("SFTP init failed: {e}")`.
   4. `sftp.create(remote_path)` → error `"Create remote file failed: {e}"`
      (**truncates/creates; no temp file, no parent-directory creation, no chmod**).
   5. Emit `"transferring"` (total `stage_total`, transferred `stage_start`).
   6. `const CHUNK: usize = 32768;` loop:
      * cancellation check → `Err("Cancelled")`;
      * `local_reader.read(&mut buf)`; `0` → break;
      * `remote_file.write_all(&buf[..n])`;
      * `transferred += n`;
      * `pct = if raw_total > 0 { transferred * 100 / raw_total } else { 0 }`;
        if `pct > last_pct` → emit `"transferring"` with
        `map_stage_progress(raw_total, transferred, stage_start, stage_span)` (throttle = one event
        per 1 % of the raw file size).
   7. `remote_file.flush()`.
   8. On success: remove from cancelled set; emit `"done"` with
      `transferred = (stage_start + stage_span).min(stage_total)`.
   9. On error: remove from cancelled set; emit `"error"` with `transferred = stage_start` and
      `error = Some(err)`.
10. Command returns `Ok(())` immediately after spawning.

#### 8.6.6 `download_file(...) -> Result<(), String>`

```rust
pub fn download_file(
    session_manager: State<SessionManager>,
    session_id: String,
    remote_path: String,
    local_path: String,
    overwrite: bool,
    transfer_id: Option<String>,
    display_name: Option<String>,
    progress_total: Option<u64>,
    progress_start: Option<u64>,
    progress_span: Option<u64>,
    target_path_override: Option<String>,
    app: AppHandle,
) -> Result<(), String>
```

Algorithm:

1. Log request (default log id `download:{local_path}`).
2. `file_name = display_name` or `Path::new(remote_path).file_name()` or `"file"`.
3. `transfer_id = transfer_id.unwrap_or_else(|| format!("download:{}", local_path))`.
4. `configured_stage_total = progress_total` (**kept as `Option`** — the real size is only known
   after `scp_recv`), `stage_start = progress_start.unwrap_or(0)`,
   `target_path = target_path_override.unwrap_or(local_path.clone())`.
5. Conflict check: if `!overwrite && Path::new(&local_path).exists()` →
   `Err(format!("CONFLICT:{}", local_path))`.
6. `parent = PathBuf::from(&local_path).parent()` or `Err("Invalid local path")`;
   `fs::create_dir_all(&parent)` — **local parent directories are created automatically**.
7. Emit `"pending"` with `total = configured_stage_total.unwrap_or(0)`,
   `transferred = stage_start`.
8. `get_sftp_info`, clone cancelled set, `remove(&transfer_id)`.
9. `thread::spawn`:
   1. Lock SFTP guard; `ensure_sftp_session(...)`.
   2. **`sess.scp_recv(Path::new(&remote_path))`** → `(remote_file, stat)`; error
      `"SCP recv failed: {e}"`. *(Download uses the SCP subsystem over the dedicated SFTP SSH
      session, not SFTP `open`.)*
   3. `total = stat.size()`; `stage_total = configured_stage_total.unwrap_or(total)`;
      `stage_span = progress_span.unwrap_or(total)`.
   4. Emit `"transferring"`.
   5. `File::create(&local_path)` + `BufWriter` (**truncates/creates; no temp file, no chmod**).
   6. `buf = [0u8; 32768]` loop: cancellation → `Err("Cancelled")`; `read` → `0` breaks;
      `write_all`; `transferred += n`; `pct = transferred*100/total` (0 when `total == 0`);
      on `pct > last_pct` emit `"transferring"` with
      `map_stage_progress(total, transferred, stage_start, stage_span)`.
   7. `local_writer.flush()`; return `Ok(total)`.
   8. On success: emit `"done"` with
      `transferred = (stage_start + progress_span.unwrap_or(total)).min(stage_total)`.
   9. On error: emit `"error"` with `total = configured_stage_total.unwrap_or(0)`,
      `transferred = stage_start`, `error = Some(err)`.
10. Return `Ok(())` immediately.

#### 8.6.7 `delete_remote(session_manager, session_id: String, path: String, is_dir: bool) -> Result<(), String>`

1. `guarded_path = guard_remote_delete_target(&session_manager, &session_id, &path)?` (§8.6.15).
2. `ssh_session = get_ssh_session(...)` (the **PTY** session, not the SFTP one).
3. Try `remove_remote_path_fast(&ssh_session, &guarded_path, is_dir)` (`rm -rf --` / `rm -f --`);
   on **any** error fall back to `with_sftp` → `remove_remote_path` (recursive SFTP delete).
4. Return the result of whichever path ran last.

#### 8.6.8 `delete_remote_async(session_manager, session_id: String, path: String, is_dir: bool, app: AppHandle) -> Result<(), String>`

1. Same guard (`?` propagates guard failures synchronously).
2. Capture `sftp_info` and `ssh_session`, `target_path = path.clone()` (the **original**,
   non-canonicalised path is reported).
3. Spawn a thread: `remove_remote_path_fast(...).or_else(|| ensure_sftp_session(...) + sftp() +
   remove_remote_path(...))`.
4. Emit `remote-delete-status` with `RemoteDeleteStatus { path: target_path, is_dir, success,
   error }` — `success = true` on `Ok`, `false` with the error string otherwise.
5. Command returns `Ok(())` immediately.

#### 8.6.9 `create_remote_dir(session_manager, session_id: String, path: String) -> Result<(), String>`

`with_sftp`: `sftp.mkdir(Path::new(&path), 0o755)`; on error, `sftp.stat(path)`; if it succeeds
and `is_dir()` → `Ok(())` (idempotent), else `Err(e.to_string())`.

#### 8.6.10 `rename_remote(session_manager, session_id: String, old_path: String, new_path: String) -> Result<(), String>`

`with_sftp`: `sftp.rename(Path::new(&old_path), Path::new(&new_path), None)` — no overwrite flags
passed (libssh2 default behaviour).

#### 8.6.11 `delete_local(path: String, _is_dir: bool) -> Result<(), String>`

* `_is_dir` is **unused**; the file type is determined from the filesystem.
1. `metadata = fs::symlink_metadata(&raw_path)?` (does **not** follow symlinks).
2. If `metadata.file_type().is_symlink()` → `fs::remove_file(&raw_path)` (symlink itself is
   removed; the target is untouched, and no guard applies).
3. `resolved_path = fs::canonicalize(&raw_path)?`.
4. `guard_local_delete_target(&resolved_path)?` (root/home protection).
5. If `metadata.is_dir()` → `fs::remove_dir_all(&resolved_path)` else `fs::remove_file(&resolved_path)`.

#### 8.6.12 `create_local_dir(path: String) -> Result<(), String>`

`fs::create_dir_all(&path)` (idempotent, recursive).

#### 8.6.13 `rename_local(old_path: String, new_path: String) -> Result<(), String>`

`fs::rename(&old_path, &new_path)` (fails across filesystems; no copy fallback).

#### 8.6.14 `remove_remote_path_fast` (used by both delete commands)

```rust
let command = if is_dir { format!("rm -rf -- {}", shell_quote(path)) }
              else       { format!("rm -f -- {}", shell_quote(path)) };
let sess = ssh_session.lock();
sess.set_blocking(true);
// new channel → exec → read stdout → wait_close → exit_status
// exit_status != 0 → Err("Command failed with exit code {code}: {stdout}, {stderr}")
sess.set_timeout(0);
sess.set_blocking(false);
```

Note: it sets blocking mode on the **shared PTY session** (the terminal keeps working because the
reader uses `try_lock` and the writer waits for the mutex).

#### 8.6.15 Delete guards

```rust
fn guard_remote_delete_target(session_manager, session_id, path) -> Result<String, String> {
    let normalized_input = normalize_remote_path(path);          // trim + strip trailing '/'
    if normalized_input.is_empty() || normalized_input == "/" {
        return Err("Refusing to delete remote root directory".to_string());
    }
    let canonical_path = with_sftp(session_manager, session_id, |sftp| {
        sftp.realpath(Path::new(&normalized_input))
            .map(|p| normalize_remote_path(&p.to_string_lossy()))
            .map_err(|_| normalized_input.clone())
    }).unwrap_or_else(|_| normalized_input.clone());             // fall back to raw input
    if canonical_path == "/" {
        return Err("Refusing to delete remote root directory".to_string());
    }
    let ssh_session = get_ssh_session(session_manager, session_id)?;
    let home_dir = get_remote_home_dir(&ssh_session)?;            // exec `printf '%s' "$HOME"`
    if !home_dir.is_empty() && canonical_path == home_dir {
        return Err("Refusing to delete remote home directory".to_string());
    }
    Ok(canonical_path)
}
```

`get_remote_home_dir` runs on the PTY session with `set_blocking(true)`, `set_timeout(5000)`,
`exec("printf '%s' \"$HOME\"")`, `read_to_string`, `wait_close`, `normalize_remote_path`, then
restores `set_timeout(0)` and `set_blocking(false)`.

```rust
fn is_filesystem_root(path: &Path) -> bool {
    let mut components = path.components();
    match (components.next(), components.next(), components.next()) {
        (Some(Component::RootDir), None, None) => true,
        (Some(Component::Prefix(_)), Some(Component::RootDir), None) => true,   // Windows C:\
        _ => false,
    }
}

fn guard_local_delete_target(path: &Path) -> Result<(), String> {
    if is_filesystem_root(path) { return Err("Refusing to delete filesystem root".to_string()); }
    if let Some(home_dir) = dirs::home_dir() {
        let home_dir = fs::canonicalize(home_dir).map_err(|e| e.to_string())?;
        if path == home_dir { return Err("Refusing to delete local home directory".to_string()); }
    }
    Ok(())
}
```

### 8.7 `local_fs` (`commands/local_fs.rs`)

Event helpers emit `transfer-progress` with `error = None` (progress) or `status = "error"` plus
the message (error); `conflict_path`/`conflict_is_dir` are always `None`.

#### 8.7.1 `pack_local_dir(...) -> Result<(), String>`

```rust
pub fn pack_local_dir(
    source_dir: String,
    target_tar_path: String,
    transfer_id: Option<String>,
    display_name: Option<String>,
    direction: Option<String>,
    progress_total: Option<u64>,
    progress_start: Option<u64>,
    progress_span: Option<u64>,
    target_path: Option<String>,
    app: AppHandle,
) -> Result<(), String>
```

*No session/state parameters.* Returns `Ok(())` immediately; all work happens on a spawned thread
and all failures are reported **only** through `transfer-progress` events.

Thread body:

1. `File::create(&target_tar_path)` (error → event).
2. `let mut builder = Builder::new(tar_file);` (`tar` crate, GNU headers via `Header::new_gnu`).
3. `path = Path::new(&source_dir)`; `folder_name = path.file_name().ok_or("Invalid directory name")?`.
4. Defaults: `display_name = folder_name` (or `"folder"` if not valid UTF-8),
   `direction = "upload"`, `total = 100`, `start = 0`, `span = 20`.
5. `collect_pack_entries(path, Path::new(folder_name), &mut entries)`:

```rust
fn collect_pack_entries(source_path: &Path, archive_path: &Path, entries: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), String> {
    entries.push((source_path.to_path_buf(), archive_path.to_path_buf()));
    if source_path.is_dir() {
        let mut children = fs::read_dir(source_path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.path());          // deterministic order
        for child in children {
            collect_pack_entries(&child.path(), &archive_path.join(child.file_name()), entries)?;
        }
    }
    Ok(())
}
```

   * The archive root entry is the **folder name itself** (`folder/…`), so unpacking recreates the
     folder inside the destination.
   * Directories are included as explicit entries; `is_dir()` follows symlinks (a symlink to a
     directory is recursed).
6. If `transfer_id` is `Some`, emit `"transferring"` with `transferred = start`.
7. For each entry (index): `builder.append_dir(archive_entry, source_entry)` for directories, else
   `File::open` + `builder.append_file(archive_entry, &mut file)`; then emit
   `map_stage_progress(index + 1, total_entries, start, span)`.
   * `append_dir`/`append_file` copy mode/uid/gid/mtime/size from the local metadata
     (`HeaderMode::Complete`), so the archive records the local permissions.
8. `builder.finish()`.
9. Final emit with `transferred = (start + span).min(total)`.
10. On any error: `emit_stage_error` with `status = "error"` and the error string,
    `total = progress_total.unwrap_or(100)`, `transferred = progress_start.unwrap_or(0)`.

**Progress mapping** (`local_fs`):

```rust
fn map_stage_progress(index: usize, total_entries: usize, start: u64, span: u64) -> u64 {
    if total_entries == 0 || span == 0 { return start; }
    if span == 1 { return start; }
    start + ((index as u64) * (span - 1) / total_entries as u64).min(span - 1)
}
```

#### 8.7.2 `unpack_local_dir(...) -> Result<(), String>`

```rust
pub fn unpack_local_dir(
    tar_path: String,
    target_dir: String,
    overwrite: bool,
    transfer_id: Option<String>,
    display_name: Option<String>,
    direction: Option<String>,
    progress_total: Option<u64>,
    progress_start: Option<u64>,
    progress_span: Option<u64>,
    target_path: Option<String>,
    app: AppHandle,
) -> Result<(), String>
```

Thread body:

1. Defaults: `display_name = "folder"`, `direction = "download"`, `total = 100`, `start = 0`,
   `span = 20`.
2. **First pass** (counting): `File::open(&tar_path)` → `Archive::new` → `entries()?.count()` to get
   `total_entries` (the archive is read twice).
3. Emit `"transferring"` at `start`.
4. **Second pass**: `File::open(&tar_path)` → `Archive::new`; for each entry:
   * `entry_path = entry.path()?.into_owned()`
   * `destination = Path::new(&target_dir).join(&entry_path)`
   * if `!overwrite && destination.exists()` → emit progress and `continue` (**skip, do not fail**)
   * else `entry.unpack_in(&target_dir)?` (tar's `unpack_in` rejects `..` traversal and strips
     leading `/`)
   * emit `map_stage_progress(index + 1, total_entries, start, span)`.
5. Final emit with `transferred = (start + span).min(total)`.
6. On error: emit `status = "error"`.

**tar crate defaults relevant here:** `preserve_permissions = false` (mode is applied as
`mode & 0o777`, i.e. setuid/setgid/sticky bits are stripped), `preserve_mtime = true`,
`overwrite = true` (but the backend pre-checks `destination.exists()` when `overwrite == false`),
`mask = 0`, ownerships not preserved.

**Permission handling summary for the port:** the backend never calls `chmod`/`set_permissions`
itself. Local pack records source modes; local unpack applies them masked to `0o777`; remote
unpack is delegated to the remote `tar` (subject to the remote umask); remote `create_remote_dir`
uses mode `0o755`; SFTP uploads/downloads do not set any mode.

---

## 9. SSH session lifecycle (end-to-end)

```
Frontend                     Rust backend                                     Remote
─────────────────────────────────────────────────────────────────────────────────────────
new Channel<string>()  ──┐
invoke('create_session') │
   ↓                     │
                         resolve_bookmark_for_connection()  ← SQLite
                         TcpStream::connect(host:port)                       ── TCP ──▶
                         set_read_timeout(30s); set_tcp_keepalive(60s/15s)
                         Session::new(); set_tcp_stream(); handshake()       ── SSH  ──▶
                         get_host_key_fingerprint/type                       ◀─ key ──
                         ensure_host_key_trusted() ← SQLite trusted_host_keys
                            └─ unknown/mismatch → Err("HOST_KEY_PROMPT:{json}")
                         authenticate_ssh()  (password | private key)
                         open_shell_channel(term, cols, rows)                ── pty-req ─▶
                                                                            ── shell  ──▶
                         set_keepalive(true, 30); set_blocking(false)
                         spawn writer thread (mpsc<String> → channel writes)
                         insert into SessionManager.sessions[session_id]
   ◀── session_id ───────┘
invoke('subscribe_session', {sessionId, dataChannel})
                         spawn reader thread (try_lock + 5 ms batching)
                         reader ── Channel<String> ──▶ term.write(data)
invoke('write_to_session', {sessionId, data}) → write_tx.send(data) → writer thread
invoke('resize_terminal') → request_pty_size
invoke('get_remote_cwd')  → exec channel (/proc → lsof → pwd)
invoke('close_session')   → stop_reader=true; sftp disconnect; channel.send_eof(); drop tx
```

### 9.1 Transport setup (`ssh::connect_ssh_transport`)

```rust
pub fn connect_ssh_transport(bookmark: &Bookmark) -> Result<Session> {
    let addr = format!("{}:{}", bookmark.host, bookmark.port);
    let tcp = TcpStream::connect(&addr)
        .map_err(|e| anyhow!("TCP connect to {} failed: {}", addr, e))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30)))?;

    set_tcp_keepalive(&tcp).unwrap_or_else(|e| {
        log::warn!("Failed to set TCP keepalive: {}", e);
    });

    let mut sess = Session::new().map_err(|e| anyhow!("SSH session init failed: {}", e))?;
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| anyhow!("SSH handshake failed: {}", e))?;

    Ok(sess)
}
```

* **No connect timeout** — a black-holed host blocks for the OS default (`~75 s` on Linux/macOS).
  The 30 s value is a **read** timeout, not a connect timeout, and it remains in effect for the
  life of the socket (it is what turns a dead peer into an `EAGAIN`/`WouldBlock` in blocking mode).
* TCP keepalive (`socket2`): first probe after **60 s** idle, retry every **15 s**; failure to set
  it is logged and ignored. Implemented by wrapping the raw fd/socket with
  `socket2::Socket::from_raw_fd` and `std::mem::forget` so socket2 does not close it.

### 9.2 Host key fingerprint & type (`ssh.rs`)

```rust
pub fn get_host_key_fingerprint(sess: &Session) -> Result<String> {
    let hash = sess.host_key_hash(ssh2::HashType::Sha256)
        .ok_or_else(|| anyhow!("Failed to compute host key fingerprint"))?;
    Ok(format!("SHA256:{}", STANDARD_NO_PAD.encode(hash)))
}

pub fn get_host_key_type(sess: &Session) -> Result<String> {
    let (_, key_type) = sess.host_key()
        .ok_or_else(|| anyhow!("Failed to read SSH host key"))?;
    let label = match key_type {
        ssh2::HostKeyType::Rsa       => "ssh-rsa",
        ssh2::HostKeyType::Dss       => "ssh-dss",
        ssh2::HostKeyType::Ecdsa256  => "ecdsa-sha2-nistp256",
        ssh2::HostKeyType::Ecdsa384  => "ecdsa-sha2-nistp384",
        ssh2::HostKeyType::Ecdsa521  => "ecdsa-sha2-nistp521",
        ssh2::HostKeyType::Ed25519   => "ssh-ed25519",
        ssh2::HostKeyType::Unknown   => "unknown",
    };
    Ok(label.to_string())
}

pub fn verify_host_key(sess: &Session, expected_key_type: &str, expected_fingerprint: &str) -> Result<()> {
    let actual_key_type = get_host_key_type(sess)?;
    let actual_fingerprint = get_host_key_fingerprint(sess)?;
    if actual_key_type != expected_key_type || actual_fingerprint != expected_fingerprint {
        return Err(anyhow!(
            "Host key mismatch: expected {} {}, got {} {}",
            expected_key_type, expected_fingerprint, actual_key_type, actual_fingerprint,
        ));
    }
    Ok(())
}
```

**Fingerprint string format:** `SHA256:` followed by **base64 standard alphabet without padding**
(`base64::engine::general_purpose::STANDARD_NO_PAD`) of the raw SHA-256 digest of the host key
blob. Example shape: `SHA256:AbCdEf...` (no trailing `=`). This matches `ssh-keygen -lf` output
except OpenSSH prints unpadded base64 too, so the strings are directly comparable.

### 9.3 Authentication (`ssh::authenticate_ssh`)

```rust
match bookmark.auth_type.as_str() {
    "privateKey" => {
        let key_content = bookmark.private_key.as_deref()
            .ok_or_else(|| anyhow!("Private key content is empty"))?;
        let passphrase = bookmark.passphrase.as_deref();
        sess.userauth_pubkey_memory(
            &bookmark.username,
            None,            // public key derived from the private key
            key_content,
            passphrase,      // Option<&str>: None = unencrypted key
        ).map_err(|e| anyhow!("Private key auth failed: {}", e))?;
    }
    _ => {
        let pwd = password
            .or_else(|| bookmark.password.as_deref())
            .ok_or_else(|| anyhow!("No password provided"))?;
        sess.userauth_password(&bookmark.username, pwd)
            .map_err(|e| anyhow!("Password auth failed: {}", e))?;
    }
}
if !sess.authenticated() { return Err(anyhow!("Authentication failed")); }
```

* Auth-method selection is driven **only** by `bookmark.auth_type` (`"privateKey"` → key, anything
  else → password).
* The `password` parameter (the connection-time override) wins over the stored password.
* There is **no** keyboard-interactive / agent / password-then-key fallback chain.
* `bookmark.encode` and `start_directory_*` are not used during connect.

### 9.4 PTY / shell channel (`ssh::open_shell_channel`)

```rust
let mut channel = sess.channel_session()
    .map_err(|e| anyhow!("Channel open failed: {}", e))?;
channel.request_pty(term, None, Some((cols, rows, 0, 0)))
    .map_err(|e| anyhow!("PTY request failed: {}", e))?;
channel.shell()
    .map_err(|e| anyhow!("Shell request failed: {}", e))?;
```

* `term` = `bookmark.term` (default `xterm-256color`), terminal modes `None`,
  size `(cols, rows, width_px = 0, height_px = 0)`.
* Resize: `channel.request_pty_size(cols, rows, None, None)`.

### 9.5 Channel IO primitives (`ssh.rs`)

```rust
pub fn read_channel_data(channel: &mut ssh2::Channel) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; 4096];
    match channel.read(&mut buf) {
        Ok(n) if n > 0 => Ok(buf[..n].to_vec()),
        Ok(_) => Ok(vec![]),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(vec![]),
        Err(e) => Err(anyhow!("Read channel error: {}", e)),
    }
}

pub fn write_channel_data(channel: &mut ssh2::Channel, data: &[u8]) -> Result<()> {
    channel.write_all(data).map_err(|e| anyhow!("Write channel error: {}", e))?;
    Ok(())
}

pub fn resize_channel(channel: &mut ssh2::Channel, cols: u32, rows: u32) -> Result<()> {
    channel.request_pty_size(cols, rows, None, None)
        .map_err(|e| anyhow!("Resize PTY failed: {}", e))?;
    Ok(())
}
```

* `read_channel_data` / `write_channel_data` / `resize_channel` are **defined but unused** by the
  command layer (which inlines equivalent logic). They document the intended primitives.
* `connect_ssh(bookmark, password)` (transport + authenticate) is likewise a convenience wrapper;
  `create_session` calls the two halves separately because the host-key check must sit between them.

### 9.6 Keepalive handling

| Mechanism | Setting | Where |
|---|---|---|
| TCP keepalive | idle **60 s**, interval **15 s**, enabled best-effort | `ssh.rs::set_tcp_keepalive`, at transport creation |
| TCP read timeout | **30 s** | `ssh.rs::connect_ssh_transport` |
| SSH keepalive | `set_keepalive(true, 30)` → probe every **30 s** of idle, want-reply | `create_session` step 8 |
| Liveness probe | `sess.keepalive_send()` on demand | `check_session_alive` |
| Bookmark `keepalive_interval` | **ignored by the backend** (stored/edited by the UI only) | — |

---

## 10. Host key trust flow

### 10.1 Verification inside `create_session`

```rust
fn ensure_host_key_trusted(db_path: &DbPath, bookmark: &Bookmark, sess: &ssh2::Session)
    -> Result<(String, String), String>
{
    let fingerprint = ssh::get_host_key_fingerprint(sess).map_err(|e| e.to_string())?;
    let key_type = ssh::get_host_key_type(sess).map_err(|e| e.to_string())?;
    let trusted = storage::get_trusted_host_key(db_path, &bookmark.host, bookmark.port)
        .map_err(|e| e.to_string())?;

    match trusted {
        Some(record) if record.fingerprint == fingerprint && record.key_type == key_type => {
            Ok((fingerprint, key_type))
        }
        Some(_) => Err(host_key_prompt_error(&HostKeyVerificationPrompt {
            host: bookmark.host.clone(),
            port: bookmark.port,
            key_type,
            fingerprint,
            reason: "mismatch".to_string(),
        })),
        None => Err(host_key_prompt_error(&HostKeyVerificationPrompt {
            host: bookmark.host.clone(),
            port: bookmark.port,
            key_type,
            fingerprint,
            reason: "unknown".to_string(),
        })),
    }
}
```

* Lookup key is `(bookmark.host, bookmark.port)` **exactly as entered** (no DNS normalisation, no
  port defaulting beyond the stored value).
* Both `key_type` **and** `fingerprint` must match; a changed key *type* with the same fingerprint
  (impossible in practice) still counts as a mismatch.
* `reason` values: `"unknown"` (no row) and `"mismatch"` (row exists, differs).
* The SSH handshake is **already complete** when the prompt is produced; the TCP connection is then
  dropped (the `Session` is never stored), so the frontend must re-invoke `create_session` after
  trusting.

### 10.2 Error string format

```rust
fn host_key_prompt_error(prompt: &HostKeyVerificationPrompt) -> String {
    format!(
        "HOST_KEY_PROMPT:{}",
        serde_json::to_string(prompt).unwrap_or_else(|_| "{}".to_string())
    )
}
```

Exact wire format (no spaces added by serde_json):

```
HOST_KEY_PROMPT:{"host":"example.com","port":22,"key_type":"ssh-ed25519","fingerprint":"SHA256:AbCd...","reason":"unknown"}
```

The frontend locates the marker with `indexOf('HOST_KEY_PROMPT:')` and `JSON.parse`s the remainder
**[frontend]**, so any prefix text (e.g. Tauri's `"Error: "`) is tolerated. On `"mismatch"` the UI
shows a MITM warning; on `"unknown"` a first-connect confirmation.

### 10.3 `trust_host_key` and re-connect

1. UI confirms → `invoke('trust_host_key', { request: { host, port, key_type, fingerprint } })`.
2. Backend upserts the row with `created_at = updated_at = now_unix()`.
3. UI re-invokes `create_session` with the same parameters; the second handshake now finds a
   matching row and proceeds to authentication.

**Trust is keyed by host+port only** — there is no per-bookmark pinning, no `known_hosts` file, and
no way to revoke from the backend (no delete/list command for `trusted_host_keys`).

### 10.4 SFTP host key policy

The lazily-created SFTP connection uses `ssh::verify_host_key` against the fingerprint pinned in
the live `SshSession`. A changed key on the SFTP path yields
`"SFTP host key verification failed: Host key mismatch: expected {t} {f}, got {t2} {f2}"` — the
session must be recreated (and re-trusted) by the user.

---

## 11. Crypto subsystem (`crypto.rs`)

### 11.1 Constants

```rust
const ENCRYPTED_PREFIX: &str = "ttenc:v1:";
const PRIVATE_KEY_FILE: &str = "secret-key.pem";
```

### 11.2 Envelope format

```
ttenc:v1:<b64(rsa_oaep_sha1(aes_key))>:<b64(nonce_12)>:<b64(tag_16)>:<b64(ciphertext)>
```

* Four `:`-separated fields after the prefix; **all base64 uses `STANDARD` (padded, `+`/`/`)**.
* Exactly four fields are required — a fifth field makes decryption fail with
  `"invalid encrypted secret payload"`.
* RSA: 2048-bit, `Padding::PKCS1_OAEP` (OpenSSL default OAEP digest = **SHA-1**).
* AES: `aes_256_gcm()` (256-bit key, 96-bit nonce, 128-bit tag), **AAD empty**.

### 11.3 Encryption

```rust
pub fn encrypt_secret(db_path: &Path, plaintext: &str) -> Result<String> {
    if plaintext.is_empty() { return Ok(String::new()); }

    let rsa = load_or_create_private_key(db_path)?;
    let mut aes_key = [0u8; 32];
    let mut nonce   = [0u8; 12];
    let mut tag     = [0u8; 16];
    rand_bytes(&mut aes_key)?;
    rand_bytes(&mut nonce)?;

    let ciphertext = encrypt_aead(
        Cipher::aes_256_gcm(), &aes_key, Some(&nonce), &[], plaintext.as_bytes(), &mut tag,
    )?;

    let public_pem = rsa.public_key_to_pem_pkcs1()?;
    let public_rsa = Rsa::public_key_from_pem_pkcs1(&public_pem)?;
    let mut encrypted_key = vec![0u8; public_rsa.size() as usize];
    let encrypted_key_len = public_rsa.public_encrypt(&aes_key, &mut encrypted_key, Padding::PKCS1_OAEP)?;
    encrypted_key.truncate(encrypted_key_len);

    Ok(format!("{}{}:{}:{}:{}", ENCRYPTED_PREFIX,
        STANDARD.encode(encrypted_key), STANDARD.encode(nonce),
        STANDARD.encode(tag), STANDARD.encode(ciphertext)))
}
```

* Empty plaintext ⇒ **empty string** (no envelope), so an empty secret is indistinguishable from
  "no secret".
* A fresh random AES key and nonce per call (no key reuse, no deterministic encryption).

### 11.4 Decryption

```rust
pub fn decrypt_secret(db_path: &Path, stored_value: &str) -> Result<String> {
    if stored_value.is_empty() { return Ok(String::new()); }
    if !is_encrypted_secret(stored_value) { return Ok(stored_value.to_string()); }   // legacy passthrough

    let payload = stored_value.strip_prefix(ENCRYPTED_PREFIX)
        .ok_or_else(|| anyhow!("invalid encrypted secret prefix"))?;
    let mut parts = payload.split(':');
    let encrypted_key = parts.next().ok_or_else(|| anyhow!("missing encrypted key"))?;
    let nonce         = parts.next().ok_or_else(|| anyhow!("missing nonce"))?;
    let tag           = parts.next().ok_or_else(|| anyhow!("missing tag"))?;
    let ciphertext    = parts.next().ok_or_else(|| anyhow!("missing ciphertext"))?;
    if parts.next().is_some() { return Err(anyhow!("invalid encrypted secret payload")); }

    let rsa = load_or_create_private_key(db_path)?;
    let encrypted_key = STANDARD.decode(encrypted_key)?;
    let nonce = STANDARD.decode(nonce)?;
    let tag = STANDARD.decode(tag)?;
    let ciphertext = STANDARD.decode(ciphertext)?;

    let mut aes_key = vec![0u8; rsa.size() as usize];
    let aes_key_len = rsa.private_decrypt(&encrypted_key, &mut aes_key, Padding::PKCS1_OAEP)?;
    aes_key.truncate(aes_key_len);

    let plaintext = decrypt_aead(Cipher::aes_256_gcm(), &aes_key, Some(&nonce), &[], &ciphertext, &tag)?;
    String::from_utf8(plaintext).context("decrypted secret is not valid UTF-8")
}
```

* **Legacy compatibility path:** any value **not** starting with `ttenc:v1:` is returned verbatim.
  Combined with `models::decode_password` at the call sites, this covers
  (a) electerm-obfuscated values when `password_encrypted = 1`, and (b) plaintext values from
  older TinyTerm versions.
* `is_encrypted_secret(v) == v.starts_with("ttenc:v1:")`.
* Decryption failure error strings: `invalid encrypted secret prefix`, `missing encrypted key`,
  `missing nonce`, `missing tag`, `missing ciphertext`, `invalid encrypted secret payload`,
  plus OpenSSL errors from `private_decrypt`/`decrypt_aead` and
  `"decrypted secret is not valid UTF-8"`.

### 11.5 Key file management

```rust
fn load_or_create_private_key(db_path: &Path) -> Result<Rsa<Private>> {
    let key_path = private_key_path(db_path)?;          // <db_dir>/secret-key.pem
    if key_path.exists() {
        let pem = fs::read(&key_path)
            .with_context(|| format!("failed to read encryption key at {}", key_path.display()))?;
        return Rsa::private_key_from_pem(&pem).context("failed to parse encryption key");
    }
    let rsa = Rsa::generate(2048).context("failed to generate RSA keypair")?;
    let pem = rsa.private_key_to_pem().context("failed to encode RSA private key")?;
    fs::write(&key_path, pem)
        .with_context(|| format!("failed to write encryption key at {}", key_path.display()))?;
    restrict_private_key_permissions(&key_path)?;        // unix: 0o600
    Ok(rsa)
}
```

* Key is generated **lazily on first encryption/decryption**, not at startup.
* **Hazard to preserve/document:** `decrypt_secret` also calls `load_or_create_private_key`, so if
  the key file is missing, a *new* key is silently generated and the subsequent OAEP decrypt fails
  with a padding error (the freshly generated key cannot read old envelopes). No backup/recovery
  path exists.
* `restrict_private_key_permissions` is a no-op on non-unix targets.

### 11.6 `normalize_stored_secrets` — the secret migration pass

Runs at every startup, immediately after `init_db`. Two identical passes (bookmarks, then profiles).

**Bookmarks pass**

1. `SELECT id, auth_type, password, password_encrypted, private_key, passphrase FROM bookmarks`.
2. Skip rows whose `auth_type` is not one of `password`, `privateKey`, `profile`.
3. `auth_type == "password"`:
   * `synced = password.filter(|v| !v.is_empty()).map(|password| {`
     * `plaintext = if is_encrypted_secret(&password) { decrypt_secret(...)? }`
       `else if password_encrypted { models::decode_password(&password) }`
       `else { password };`
     * `encrypt_secret(&db_path.0, &plaintext) })` (transposed to `Option`)
   * `UPDATE bookmarks SET password=?2, password_encrypted=?3, private_key=NULL, passphrase=NULL WHERE id=?1`
     with `password_encrypted = synced.is_some() as i32`.
4. `auth_type == "privateKey"`:
   * `private_key`: keep if already prefixed with `ttenc:v1:`, else `encrypt_secret` (empty → `None`).
   * `passphrase`: same treatment.
   * `UPDATE bookmarks SET password=NULL, password_encrypted=0, private_key=?2, passphrase=?3 WHERE id=?1`.
5. `auth_type == "profile"` (the `else` branch):
   * `UPDATE bookmarks SET password=NULL, password_encrypted=0, private_key=NULL, passphrase=NULL WHERE id=?1`.
6. Global sweep: `UPDATE bookmarks SET private_key=NULL, passphrase=NULL WHERE auth_type != 'privateKey'`.

**Profiles pass**

* Same three-way branch, but **no `auth_type` allow-list check** (any unknown auth type falls into
  the clearing `else` branch).
* Global sweep: `UPDATE profiles SET private_key=NULL, passphrase=NULL WHERE auth_type != 'privateKey'`.

**Idempotence:** values already carrying the `ttenc:v1:` prefix are decrypted and re-encrypted
(password branch — new envelope every launch, i.e. rows are rewritten each start) or kept verbatim
(private-key branch).

---

## 12. File transfer internals

### 12.1 SFTP connection reuse

* Each `SshSession` owns `sftp_session: Arc<Mutex<Option<ssh2::Session>>>`.
* The first SFTP command lazily creates a **second TCP+SSH connection** to the same host, verifies
  the pinned host key, authenticates with `resolved_bookmark` + `password_override`, and sets
  blocking mode permanently.
* The connection is cached for the life of the session and shared by all SFTP/SCP operations of
  that session (serialised by the `Mutex`).
* It is invalidated (`*guard = None`) when: `sess.sftp()` fails, or the body error message
  (lowercased) contains `channel`, `transport`, `eof`, `broken pipe`, or `connection reset`.
* It is explicitly disconnected on `close_session` (`disconnect(None, "closing", None)`).
* The terminal PTY session is **never** used for SFTP (except `rm`/`printf`/`tar` shell commands
  via `execute_remote_command` and `remove_remote_path_fast`).

### 12.2 Chunk sizes and buffers

| Path | Buffer / chunk | I/O type |
|---|---|---|
| `upload_file` | `const CHUNK: usize = 32768` (`[0u8; 32768]`), `BufReader` over the local file | SFTP `sftp.create` + `write_all` |
| `download_file` | `[0u8; 32768]`, `BufWriter` over the local file | **SCP** `sess.scp_recv` + `read` |
| `subscribe_session` | `16384` read buffer, 4096-byte flush threshold | SSH channel |
| `get_remote_cwd` / `execute_remote_command` | `read_to_string` (unbounded) | exec channel |

### 12.3 Progress event cadence and payload

* Event name: **`transfer-progress`**, emitted through `AppHandle::emit` (broadcast; errors ignored
  with `let _ =`).
* Payload: the full `TransferProgress` struct in snake_case JSON.
* Cadence:
  * exactly one `"pending"` event emitted **synchronously** before the worker thread starts;
  * `"transferring"` events emitted **only when the integer percentage of the raw byte count
    changes** (`pct = transferred * 100 / raw_total` strictly greater than the last emitted pct) —
    i.e. ≤ 101 events per transfer, and **zero** events for a 0-byte file;
  * one terminal `"done"` or `"error"` event.
* Progress mapping for multi-stage (tar) transfers uses
  `map_stage_progress(raw_total, raw_transferred, stage_start, stage_span)`:

```rust
fn map_stage_progress(raw_total: u64, raw_transferred: u64, stage_start: u64, stage_span: u64) -> u64 {
    if stage_span == 0 { return stage_start; }
    if raw_total == 0 { return stage_start; }
    let scaled = raw_transferred.saturating_mul(stage_span) / raw_total;
    stage_start.saturating_add(scaled).min(stage_start.saturating_add(stage_span))
}
```

* `done` reports `(stage_start + stage_span).min(stage_total)` so a UI waiting for an exact
  percentage can unblock.
* `target_path` always carries the *display* path (`target_path_override` when supplied, otherwise
  the real destination) so directory transfers report the folder rather than the temp tar file.
* `remote-delete-status` is the only other backend event: payload `RemoteDeleteStatus`.

### 12.4 Cancellation

* Mechanism: `SessionManager.cancelled_transfers: Arc<Mutex<HashSet<String>>>` (a cooperative
  **flag set**, not an `AtomicBool`).
* `cancel_transfer(transfer_id)` inserts the id.
* The worker removes its id **before** starting (`remove`), then checks
  `cancelled_transfers.lock().contains(&transfer_id)` once per 32 KiB chunk; on hit it returns
  `Err("Cancelled")`, which surfaces as a `"error"` progress event with `error = "Cancelled"`.
* The id is removed again on both success and error, so the set stays small.
* Cancellation is **not** immediate: it takes effect after the current chunk (and, for uploads, after
  any blocked SFTP write returns).
* `pack_local_dir` / `unpack_local_dir` do **not** support cancellation (no id check).

### 12.5 Conflict handling

| Operation | Check | Behaviour |
|---|---|---|
| `upload_file` | `with_sftp` → `sftp.stat(remote_path).is_ok()`, skipped if `overwrite == true` | returns `Err("CONFLICT:<remote_path>")` **synchronously**, before any event or thread |
| `download_file` | `Path::new(&local_path).exists()`, skipped if `overwrite == true` | returns `Err("CONFLICT:<local_path>")` |
| `unpack_local_dir` | `destination.exists()` per entry when `overwrite == false` | silently **skips** the existing entry (progress still advances); no error |

**[frontend]** `startTransferTask` inspects the rejection message for `CONFLICT:` and marks the
transfer row `status: "conflict"` so the UI can offer "overwrite / skip / rename".

### 12.6 Temp files

* The backend itself creates **no** temporary files. Upload writes directly to `remote_path`;
  download writes directly to `local_path`; both create/truncate first, so an interrupted transfer
  leaves a partial file at the final destination.
* Temp tar staging names are chosen by the frontend:
  * local: `<tempDir>/tinyterm-pack-<Date.now()>-<rand6>/.tinyterm-pack.tar`
  * remote: `<remote_dir>/.tinyterm-pack-<Date.now()>-<rand6>.tar`
  * cleanup: `rm -f '<tmpTarRemote>'` via `execute_remote_command`, `delete_local` for the local
    files (both in a `finally` block, errors swallowed).

### 12.7 Permissions

* SFTP upload (`sftp.create`) and download (`File::create`) apply the default mode only
  (remote umask / local `0o666 & ~umask`). No `chmod`, no mode copying from the source.
* `create_remote_dir` uses `0o755`.
* Remote `tar` unpack applies the archive's modes subject to the remote umask; local
  `unpack_local_dir` applies `mode & 0o777` (setuid/setgid/sticky stripped).

---

## 13. Directory transfer (tar) — full flow

The backend supplies three primitives; the **orchestration lives in the frontend** and must be
reproduced identically by the egui app.

### 13.1 Upload a local folder → remote

```text
stamp        = `${Date.now()}-${rand6}`
localSubTmp  = <tempDir>/tinyterm-pack-<stamp>
tmpTarLocal  = <localSubTmp>/.tinyterm-pack.tar
tmpTarRemote = <targetRemoteDir>/.tinyterm-pack-<stamp>.tar
remoteTarget = <targetRemoteDir>/<folder.name>
transferId   = `upload:${remoteTarget}`

1. create_local_dir({ path: localSubTmp })
2. pack_local_dir({ sourceDir: folder.path, targetTarPath: tmpTarLocal, transferId,
                    displayName: folder.name, direction: "upload",
                    progressTotal: 100, progressStart: 0, progressSpan: 20,
                    targetPath: remoteTarget })                 // 0 → 20 %
3. upload_file({ sessionId, localPath: tmpTarLocal, remotePath: tmpTarRemote,
                 overwrite: true, transferId, displayName: folder.name,
                 progressTotal: 100, progressStart: 20, progressSpan: 60,
                 targetPathOverride: remoteTarget })            // 20 → 80 %
4. if (overwriteAll) delete_remote({ sessionId, path: remoteTarget, isDir: true }).catch(ignore)
5. execute_remote_command({ sessionId, command:
      overwriteAll
        ? `mkdir -p ${q(targetRemoteDir)} && tar -xf ${q(tmpTarRemote)} -C ${q(targetRemoteDir)}`
        : `mkdir -p ${q(targetRemoteDir)} && tar -k -xf ${q(tmpTarRemote)} -C ${q(targetRemoteDir)}` })
6. mark done (100 %)
finally:
7. execute_remote_command({ sessionId, command: `rm -f ${q(tmpTarRemote)}` }).catch(ignore)
8. delete_local({ path: tmpTarLocal, isDir: false }).catch(ignore)
9. delete_local({ path: localSubTmp, isDir: true }).catch(ignore)
```

`q(v) = "'" + v.replace(/'/g, "'\"'\"'") + "'"` — identical to the backend's `shell_quote`
**[frontend]**.

### 13.2 Download a remote folder → local

```text
remoteParent = dirname(folder.path) || "/"
tmpTarRemote = <remoteParent>/.tinyterm-pack-<stamp>.tar
tmpTarLocal  = <tempDir>/tinyterm-pack-<stamp>/.tinyterm-pack.tar
localTarget  = <targetLocalDir>/<folder.name>
transferId   = `download:${localTarget}`

1. create_local_dir({ path: localSubTmp })
2. execute_remote_command({ sessionId, command:
      `tar -cf ${q(tmpTarRemote)} -C ${q(remoteParent)} ${q(folder.name)}` })   // 0 → 20 %
3. download_file({ sessionId, remotePath: tmpTarRemote, localPath: tmpTarLocal,
                   overwrite: true, transferId, displayName: folder.name,
                   progressTotal: 100, progressStart: 20, progressSpan: 60,
                   targetPathOverride: localTarget })                             // 20 → 80 %
4. if (overwriteAll) delete_local({ path: localTarget, isDir: true }).catch(ignore)
5. unpack_local_dir({ tarPath: tmpTarLocal, targetDir: targetLocalDir,
                      overwrite: overwriteAll, transferId, displayName: folder.name,
                      direction: "download", progressTotal: 100,
                      progressStart: 80, progressSpan: 20, targetPath: localTarget })  // 80 → 100 %
6. mark done
finally:
7. execute_remote_command({ sessionId, command: `rm -f ${q(tmpTarRemote)}` }).catch(ignore)
8. delete_local({ path: tmpTarLocal, isDir: false }).catch(ignore)
9. delete_local({ path: localSubTmp, isDir: true }).catch(ignore)
```

### 13.3 Exact remote commands (summary)

| Purpose | Command (after shell-quoting) |
|---|---|
| Pack remote folder | `tar -cf '<tmpTarRemote>' -C '<remoteParent>' '<folderName>'` |
| Unpack remote (overwrite) | `mkdir -p '<targetRemoteDir>' && tar -xf '<tmpTarRemote>' -C '<targetRemoteDir>'` |
| Unpack remote (no overwrite) | `mkdir -p '<targetRemoteDir>' && tar -k -xf '<tmpTarRemote>' -C '<targetRemoteDir>'` |
| Cleanup | `rm -f '<tmpTarRemote>'` |
| Fast delete dir | `rm -rf -- '<path>'` |
| Fast delete file | `rm -f -- '<path>'` |
| Remote home | `printf '%s' "$HOME"` |
| Remote cwd | §8.5.7 one-liner |

### 13.4 Pack/unpack implementation details

* **Local pack** walks the tree **twice**: once to collect `(source, archive)` pairs with a
  deterministic `sort_by_key(entry.path())` order, once to append them. The archive root entry is
  the folder's basename.
* **Local unpack** reads the archive **twice**: once to count entries (progress denominator), once
  to extract. Extraction uses `entry.unpack_in(target_dir)`, which:
  * strips leading `/`,
  * refuses `..` components (entries are skipped),
  * creates intermediate directories,
  * overwrites by default.
* **Stage progress** for these commands is entry-count based
  (`local_fs::map_stage_progress`), not byte based.

---

## 14. Local FS helpers & path guards

| Helper | Rule |
|---|---|
| `is_filesystem_root` | path components are exactly `[RootDir]` (unix) or `[Prefix, RootDir]` (windows) |
| `guard_local_delete_target` | reject filesystem root; reject `canonicalize(home_dir())` |
| `normalize_remote_path` | trim whitespace; strip all trailing `/` while length > 1; `""` stays `""` |
| `guard_remote_delete_target` | reject `""` and `"/"`; `realpath` (fallback = raw input); reject `/`; reject remote `$HOME`; return the canonical path |
| `shell_quote` | `'` + value with `'` → `'"'"'` + `'` |
| `list_local_dir` | no filtering; hidden files returned |
| `delete_local` | symlinks removed as links (no guard); directories `remove_dir_all` |
| `create_local_dir` | `create_dir_all` (recursive, idempotent) |
| `rename_local` | `fs::rename` only |

---

## 15. Events & IPC channels (complete list)

| Channel/Event | Direction | Payload | Emitted by |
|---|---|---|---|
| `Channel<String>` (`data_channel`) | Rust → JS callback | UTF-8-lossy terminal output batches (5 ms / 4096 B) | `subscribe_session` reader thread |
| `transfer-progress` | Rust → all windows | `TransferProgress` (snake_case) | `upload_file`, `download_file`, `pack_local_dir`, `unpack_local_dir` |
| `remote-delete-status` | Rust → all windows | `RemoteDeleteStatus` | `delete_remote_async` |
| command return values | request/response | see §8 | all commands |

**egui port mapping**

| Tauri concept | egui equivalent |
|---|---|
| `State<DbPath>` / `State<SessionManager>` | fields of your `AppState`, passed by reference |
| `AppHandle::emit("event", payload)` | an `mpsc::Sender<AppEvent>` polled in `update()` (or a `Vec` queue guarded by a mutex) |
| `tauri::ipc::Channel<String>` | `std::sync::mpsc::Sender<String>` handed to the reader thread; drain it each frame and feed the terminal emulator |
| `#[tauri::command]` | plain `pub fn` on `AppState` |
| command `Result<T, String>` | same, or a typed error converted to the same strings |

---

## 16. Behavioural gaps, quirks and hazards to preserve (or fix deliberately)

1. **`finish_startup` is dead code** (not in the handler list).
2. **`execute_remote_command` holds the sessions map lock** for the whole remote command; the
   frontend's own `clear`/`rm`/`tar` calls can therefore block terminal writes briefly.
3. **`bookmark.keepalive_interval` is ignored** — keepalive is hardcoded to 30 s (+ TCP 60/15 s).
4. **`download_file` uses SCP, `upload_file` uses SFTP** — asymmetric; SCP does not support
   resuming and ignores `permissions`.
5. **No temp file / atomic rename** for uploads and downloads — an aborted transfer leaves a
   truncated file at the final path.
6. **`password_encrypted` is reused as a "password present" flag** after normalisation
   (`synced_password.is_some()`), not as "legacy-encrypted".
7. **Profile deletion does not clear `bookmarks.profile_id`** → later connects fail with
   `Credential '<id>' not found`.
8. **Group deletion does not re-parent child groups** → dangling `parent_id`.
9. **`get_remote_cwd` and `get_remote_home_dir` run on the PTY session** in blocking mode, stalling
   terminal I/O for up to 5 s on an unresponsive host.
10. **`execute_remote_command` sets no timeout** — it can block indefinitely.
11. **Reader thread decodes with `from_utf8_lossy`** — binary output (e.g. `cat` of a binary file)
    is mangled into U+FFFD.
12. **`[Session closed]` sentinel can repeat** every loop iteration after EOF until the thread is
    stopped.
13. **Cancellation ids are never garbage-collected** if `cancel_transfer` is called for a transfer
    that never starts (only worker threads remove their own ids).
14. **No `busy_timeout` / connection pooling** — concurrent writers can raise `SQLITE_BUSY`
    (surfaced as an error string).
15. **Missing `secret-key.pem` silently generates a new key**, making existing secrets
    undecryptable with an opaque OAEP error.
16. **`normalize_stored_secrets` re-encrypts password rows on every launch**, rewriting the DB each
    start.
17. **`list_bookmarks` returns `password_encrypted = false` always** because of redaction — the
    frontend cannot tell whether a password exists except by the presence of a non-null
    `password`… which is also redacted to `None`. (The UI infers "password saved" from its own
    state.) Preserve this redaction contract if the egui UI relies on it, or add an explicit
    `has_password` flag.
18. **`check_session_alive` and `resize_channel`/`read_channel_data`/`write_channel_data` are
    unused** by the current frontend/command layer.
19. **`update_bookmark` stamps `updated_at` but `create_bookmark` does not** (it trusts the payload).
20. **`delete_local` ignores its `is_dir` argument** and always re-stats the path.

---

## 17. Exact error string catalogue

Command-level strings (what the UI sees), grouped by origin:

```
# create_session / connect
Bookmark not found
Credential '{profile_id}' not found
TCP connect to {host}:{port} failed: {io_error}
SSH session init failed: {e}
SSH handshake failed: {e}
HOST_KEY_PROMPT:{json}
Private key content is empty
Private key auth failed: {e}
No password provided
Password auth failed: {e}
Authentication failed
Channel open failed: {e}
PTY request failed: {e}
Shell request failed: {e}
Session not found
Session disconnected

# host key (crypto/ssh layer)
Failed to compute host key fingerprint
Failed to read SSH host key
Host key mismatch: expected {key_type} {fingerprint}, got {key_type} {fingerprint}

# remote exec / cwd
channel_session: {e}
exec: {e}
exec cwd command: {e}
read: {e}
read cwd output: {e}
wait_close: {e}
empty cwd output
Command failed with exit code {code}: {stdout}, {stderr}
resolve {host}:{port} failed: {e}

# sftp
SFTP connection failed: {e}
SFTP host key verification failed: {e}
SFTP authentication failed: {e}
SFTP init failed: {e}
readdir failed: {e}
unlink failed for {path:?}: {e}
rmdir failed for {path:?}: {e}
SCP recv failed: {e}
Create remote file failed: {e}
Invalid local path
CONFLICT:{path}
Cancelled                       (as TransferProgress.error)
Refusing to delete remote root directory
Refusing to delete remote home directory
Refusing to delete filesystem root
Refusing to delete local home directory
resolve {}:{} failed: {}        (check_host_port)

# local tar
Invalid directory name

# crypto
invalid encrypted secret prefix
missing encrypted key
missing nonce
missing tag
missing ciphertext
invalid encrypted secret payload
decrypted secret is not valid UTF-8
failed to read encryption key at {path}
failed to parse encryption key
failed to generate RSA keypair
failed to encode RSA private key
failed to write encryption key at {path}
database path has no parent directory
```

---

## 18. Constant reference

| Constant | Value | Location |
|---|---|---|
| `ENCRYPTED_PREFIX` | `"ttenc:v1:"` | crypto.rs |
| `PRIVATE_KEY_FILE` | `"secret-key.pem"` | crypto.rs |
| RSA key size | `2048` bits | crypto.rs |
| RSA padding | `PKCS1_OAEP` (SHA-1 default) | crypto.rs |
| AES | `aes_256_gcm`, key 32 B, nonce 12 B, tag 16 B, empty AAD | crypto.rs |
| Key file mode | `0o600` (unix) | crypto.rs |
| DB file name | `"tinyterm.db"` | lib.rs |
| Reader read buffer | `16384` bytes | commands/ssh.rs |
| Reader flush window | `5` ms (`FLUSH_MS`) | commands/ssh.rs |
| Reader flush threshold | `4096` bytes | commands/ssh.rs |
| Reader idle sleep | `1000` µs | commands/ssh.rs |
| Reader try_lock backoff | `500` µs | commands/ssh.rs |
| Writer queue | unbounded `mpsc::channel::<String>` | commands/ssh.rs |
| SSH keepalive | `set_keepalive(true, 30)` → 30 s | commands/ssh.rs |
| TCP keepalive | idle 60 s, interval 15 s | ssh.rs |
| TCP read timeout | 30 s | ssh.rs |
| `get_remote_cwd` timeout | 5000 ms | commands/ssh.rs |
| `get_remote_home_dir` timeout | 5000 ms | commands/sftp.rs |
| `check_host_port` timeout | 1500 ms | commands/ssh.rs |
| Upload/download chunk | `32768` bytes | commands/sftp.rs |
| Progress throttle | 1 % of raw bytes | commands/sftp.rs |
| `create_remote_dir` mode | `0o755` | commands/sftp.rs |
| tar pack progress default | total 100, start 0, span 20 | commands/local_fs.rs |
| tar unpack progress default | total 100, start 0, span 20 | commands/local_fs.rs |
| Delete guard: remote root | `""` or `"/"` (after normalise, and after realpath) | commands/sftp.rs |
| Delete guard: local home | `canonicalize(dirs::home_dir())` | commands/sftp.rs |
| Timestamps | unix **seconds** (`as_secs() as i64`) | everywhere |

---

## 19. Suggested egui module layout (1:1 with the spec)

```
src/
  main.rs            // env_logger, AppState construction, eframe::run_native
  models.rs          // §5 — verbatim structs (drop serde if unused, keep field names)
  storage.rs         // §4 — schema + CRUD + normalize_stored_secrets
  crypto.rs          // §11 — verbatim
  ssh.rs             // §9.1–9.5 primitives — verbatim
  session.rs         // §3 — SshSession / SessionManager — verbatim
  commands/
    bookmark.rs      // §8.4
    profile.rs       // §8.3
    settings.rs      // §8.2
    ssh.rs           // §8.5 + §10
    sftp.rs          // §8.6 + §12
    local_fs.rs      // §8.7 + §13.4
  events.rs          // AppEvent enum replacing AppHandle::emit + Channel
```

Replace `tauri::State<T>` with `&AppState`, `tauri::ipc::Channel<String>` with
`mpsc::Sender<String>`, and `AppHandle::emit(name, payload)` with
`events_tx.send(AppEvent::TransferProgress(payload))`. Everything else — algorithms, error
strings, constants, lock ordering, SQL — can be transcribed literally from this document.
