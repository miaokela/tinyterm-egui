//! Data models — a faithful port of `src-tauri/src/models.rs` plus the
//! frontend-only types from `src/types/index.ts`.

use serde::{Deserialize, Serialize};

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── Host / Bookmark ──────────────────────────────────────────────────────────

/// `auth_type`: `"password" | "privateKey" | "profile"`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String,
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

impl Default for Bookmark {
    fn default() -> Self {
        let now = now_unix();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            auth_type: "password".into(),
            password: None,
            password_encrypted: false,
            private_key: None,
            passphrase: None,
            profile_id: None,
            group_id: None,
            term: "xterm-256color".into(),
            encode: "utf8".into(),
            color: Some("#7c5cbf".into()),
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

impl Bookmark {
    /// Display name used by tabs and lists (`host.title || host.host`).
    pub fn display_name(&self) -> String {
        if self.title.trim().is_empty() {
            self.host.clone()
        } else {
            self.title.clone()
        }
    }

    /// Parsed accent colour (default `#7c5cbf`).
    pub fn accent(&self) -> egui::Color32 {
        self.color
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or(egui::Color32::from_rgb(0x7c, 0x5c, 0xbf))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkGroup {
    pub id: String,
    pub title: String,
    pub parent_id: Option<String>,
    pub order_index: i32,
    pub created_at: i64,
}

// ── Credential / Profile ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub fn is_key(&self) -> bool {
        self.auth_type == "privateKey"
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            font_family: "Menlo, Monaco, 'Courier New', monospace".into(),
            theme: "dark".into(),
            opacity: 1.0,
            language: "zh".into(),
            scrollback: 5000,
            show_hidden_files: false,
            default_protocol: "ssh".into(),
            cursor_style: "block".into(),
            cursor_blink: true,
            bell_style: "none".into(),
        }
    }
}

// ── Files ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<i64>,
    pub permissions: Option<String>,
    pub owner: Option<String>,
}

impl FileInfo {
    /// Sort key: directories first, then case-insensitive name.
    pub fn sort_key(&self) -> (bool, String) {
        (!self.is_dir, self.name.to_lowercase())
    }

    pub fn size_text(&self) -> String {
        if self.is_dir {
            return "—".into();
        }
        human_size(self.size)
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}



// ── Transfers ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

impl TransferDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Transferring,
    Done,
    Error,
    Conflict,
}

impl TransferStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Transferring => "transferring",
            Self::Done => "done",
            Self::Error => "error",
            Self::Conflict => "conflict",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Transferring)
    }

    /// Whether this transfer should keep the upload/download button disabled.
    /// Matches the web client, which treats `conflict` as busy too.
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::Transferring | Self::Conflict
        )
    }
}

#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub id: String,
    pub file_name: String,
    pub direction: TransferDirection,
    pub total: u64,
    pub transferred: u64,
    pub status: TransferStatus,
    pub error: Option<String>,
    pub target_path: Option<String>,
    pub conflict_path: Option<String>,
    pub conflict_is_dir: bool,
    /// Which session tab this transfer belongs to.
    pub session_id: Option<String>,
    /// Batch group id for multi-file transfers.
    pub group_id: Option<String>,
    /// Wall-clock time the row was created (for ordering + auto-dismiss).
    pub created_at_ms: i64,
}

impl TransferProgress {
    pub fn percent(&self) -> f32 {
        if self.total == 0 {
            return if self.status == TransferStatus::Done {
                100.0
            } else {
                0.0
            };
        }
        (self.transferred as f64 / self.total as f64 * 100.0).min(100.0) as f32
    }
}

// ── Host key trust ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedHostKey {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostKeyVerificationPrompt {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    /// `"unknown"` (first contact) or `"mismatch"` (key changed).
    pub reason: String,
}

impl HostKeyVerificationPrompt {
    /// Exact message used by the web client (`buildHostKeyPromptMessage`).
    pub fn message(&self) -> String {
        let summary = format!("{}:{}\n{}\n{}", self.host, self.port, self.key_type, self.fingerprint);
        if self.reason == "mismatch" {
            format!(
                "检测到主机指纹变更。\n\n{summary}\n\n这可能是主机重装，也可能是中间人攻击。仅在你确认这是可信的新指纹时继续。"
            )
        } else {
            format!("首次连接到该主机，需要确认 SSH 指纹。\n\n{summary}\n\n确认后会保存为受信任主机。")
        }
    }
}

// ── Session / tab UI state ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Error => "error",
        }
    }

    pub fn dot_color(self) -> egui::Color32 {
        match self {
            Self::Connected => crate::theme::SUCCESS,
            Self::Connecting => crate::theme::WARNING,
            Self::Error => crate::theme::ERROR,
            Self::Disconnected => egui::Color32::from_rgb(0x96, 0x96, 0xa2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostReachability {
    Unknown,
    Reachable,
    Unreachable,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

pub fn parse_hex_color(s: &str) -> Option<egui::Color32> {
    let h = s.trim().trim_start_matches('#');
    let expand = |c: u8| -> u8 {
        // 0x0f → 0xff
        (c << 4) | c
    };
    match h.len() {
        3 => {
            let v = u16::from_str_radix(h, 16).ok()?;
            let r = ((v >> 8) & 0xf) as u8;
            let g = ((v >> 4) & 0xf) as u8;
            let b = (v & 0xf) as u8;
            Some(egui::Color32::from_rgb(expand(r), expand(g), expand(b)))
        }
        6 => {
            let v = u32::from_str_radix(h, 16).ok()?;
            Some(egui::Color32::from_rgb(
                ((v >> 16) & 0xff) as u8,
                ((v >> 8) & 0xff) as u8,
                (v & 0xff) as u8,
            ))
        }
        _ => None,
    }
}

pub fn color_to_hex(c: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}


/// Legacy electerm-compatible password obfuscation (`encode_password`).
///
/// Kept for byte-compatibility with rows written by the web client when
/// `password_encrypted = 1`.
pub fn encode_password(s: &str) -> String {
    s.chars()
        .enumerate()
        .map(|(i, c)| char::from_u32(((c as u32 + i as u32 + 1) % 65536) as u32).unwrap_or(c))
        .collect()
}


/// Inverse of [`encode_password`] — electerm-compatible de-obfuscation.
pub fn decode_password(s: &str) -> String {
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            let code = c as u32;
            let shifted = i as u32 + 1;
            let result = if code >= shifted {
                code - shifted
            } else {
                code + 65536 - shifted
            };
            char::from_u32(result).unwrap_or(c)
        })
        .collect()
}

/// Convert a unix timestamp to `(year, month, day, hour, minute)` in UTC.
/// Deliberately dependency-free (no chrono).
pub fn time_from_unix(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, minute)
}

/// `YYYY-MM-DD HH:MM` for a file mtime.
pub fn format_mtime(secs: Option<i64>) -> String {
    let Some(secs) = secs else {
        return "—".into();
    };
    let (y, mo, d, h, mi) = time_from_unix(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}")
}
