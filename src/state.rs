//! Application state: host tabs, session tabs, file-manager state, modals,
//! toasts and the transfer queue.

use crate::models::*;
use crate::session::SessionManager;
use crate::storage::Db;
use egui::Pos2;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

pub const APP_ZOOM_MIN: f32 = 0.8;
pub const APP_ZOOM_MAX: f32 = 1.6;
pub const APP_ZOOM_STEP: f32 = 0.1;
/// Zoom used when the user never changed it. The web client ran at 0.8 because
/// its CSS was designed for that; on the desktop 1.0 is the comfortable size.
pub const APP_ZOOM_DEFAULT: f32 = 1.0;
/// The previous default. Every run used to persist the current zoom, so a stored
/// 0.8 means "never customised" rather than a deliberate choice.
pub const APP_ZOOM_LEGACY_DEFAULT: f32 = 0.8;
pub const ADD_SESSION_MIN_LOADING_MS: f64 = 600.0;
pub const CONNECTION_CHECK_INTERVAL_MS: f64 = 15_000.0;
pub const FM_CONTENT_HEIGHT: f32 = 260.0;
pub const FM_BAR_HEIGHT: f32 = 28.0;

// ── Session / host tabs ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SessionTab {
    pub id: String,
    pub title: String,
    pub bookmark_id: String,
    pub status: SessionStatus,
    pub error: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub local_path: String,
    pub remote_path: String,
    /// Current working directory reported by the terminal.
    pub terminal_path: Option<String>,
    pub fm_open: bool,
    pub side_terminal_open: bool,
    pub side_terminal_id: Option<String>,
    pub side_terminal_status: SessionStatus,
    pub side_terminal_error: Option<String>,
    /// Wall-clock ms when the tab was created (drives the fade-in animation).
    pub created_at_ms: f64,
    /// True while the remote `tar` capability probe has not finished.
    pub tar_support: Option<bool>,
}

impl SessionTab {
    pub fn new(id: String, title: String, bookmark_id: String) -> Self {
        Self {
            id,
            title,
            bookmark_id,
            status: SessionStatus::Connecting,
            error: None,
            cols: 80,
            rows: 24,
            local_path: crate::local_fs::home_dir().to_string_lossy().to_string(),
            remote_path: "/".into(),
            terminal_path: None,
            fm_open: false,
            side_terminal_open: false,
            side_terminal_id: None,
            side_terminal_status: SessionStatus::Disconnected,
            side_terminal_error: None,
            created_at_ms: now_ms(),
            tar_support: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostTab {
    pub id: String,
    pub title: String,
    pub host_id: String,
    pub sessions: Vec<SessionTab>,
    pub active_session: Option<String>,
}

impl HostTab {
    pub fn active(&self) -> Option<&SessionTab> {
        let id = self.active_session.as_deref()?;
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn active_mut(&mut self) -> Option<&mut SessionTab> {
        let id = self.active_session.clone()?;
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    pub fn session(&self, id: &str) -> Option<&SessionTab> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn session_mut(&mut self, id: &str) -> Option<&mut SessionTab> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }
}

pub fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

// ── File manager ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PanelSide {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
pub struct PanelState {
    pub path: String,
    pub entries: Vec<FileInfo>,
    pub selected: BTreeSet<String>,
    pub anchor: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub editing_path: bool,
    pub path_input: String,
    /// Request id of the outstanding listing request.
    pub pending_request: Option<u64>,
    /// Path requested by the outstanding listing request.
    pub pending_path: Option<String>,
    /// Manual scroll offset (rows) — kept so refreshes do not jump.
    pub scroll_to: Option<usize>,
    /// Follow the terminal's cwd automatically. Cleared as soon as the user
    /// navigates the panel by hand, so polling never fights manual browsing.
    pub auto_follow: bool,
    /// Last path we auto-navigated to (success or failure). Prevents the 2 s
    /// cwd poll from re-issuing the same listing — and thus flickering the
    /// spinner — when a directory cannot be listed.
    pub last_follow_path: Option<String>,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            path: String::new(),
            entries: Vec::new(),
            selected: BTreeSet::new(),
            anchor: None,
            loading: false,
            error: None,
            editing_path: false,
            path_input: String::new(),
            pending_request: None,
            pending_path: None,
            scroll_to: None,
            auto_follow: true,
            last_follow_path: None,
        }
    }
}

impl PanelState {
    pub fn visible_entries(&self, show_hidden: bool) -> Vec<&FileInfo> {
        self.entries
            .iter()
            .filter(|f| show_hidden || !f.name.starts_with('.'))
            .collect()
    }

    pub fn selected_items(&self) -> Vec<FileInfo> {
        self.entries
            .iter()
            .filter(|f| self.selected.contains(&f.path))
            .cloned()
            .collect()
    }

    pub fn select_single(&mut self, path: &str) {
        self.selected.clear();
        self.selected.insert(path.to_string());
        self.anchor = Some(path.to_string());
    }

    pub fn select_toggle(&mut self, path: &str) {
        if !self.selected.remove(path) {
            self.selected.insert(path.to_string());
        }
        self.anchor = Some(path.to_string());
    }

    pub fn select_range(&mut self, path: &str, show_hidden: bool) {
        let names: Vec<String> = self
            .visible_entries(show_hidden)
            .iter()
            .map(|f| f.path.clone())
            .collect();
        let anchor = self.anchor.clone().unwrap_or_else(|| path.to_string());
        let (Some(a), Some(b)) = (
            names.iter().position(|p| p == &anchor),
            names.iter().position(|p| p == path),
        ) else {
            self.select_single(path);
            return;
        };
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        self.selected.clear();
        for p in &names[lo..=hi] {
            self.selected.insert(p.clone());
        }
    }
}

/// A local directory listing scheduled a frame later, so the spinner actually
/// renders before the (synchronous, fast) read finishes — same trick as the web
/// client's `setTimeout(..., 50)`.
#[derive(Debug, Clone)]
pub struct LocalLoadRequest {
    pub request: u64,
    pub session_id: String,
    pub path: String,
    pub due_ms: f64,
}

#[derive(Debug, Clone)]
pub struct FileManagerState {
    pub local: PanelState,
    pub remote: PanelState,
    pub show_hidden_local: bool,
    pub show_hidden_remote: bool,
    /// Right-click menu: (side, path, screen position).
    pub context_menu: Option<(PanelSide, Option<String>, Pos2)>,
    /// Inline "new folder" / "rename" prompt.
    pub inline_action: Option<InlineAction>,
    pub last_click: Option<(String, f64)>,
    /// Upload/download in progress flags (disable the divider buttons).
    pub upload_busy: bool,
    pub download_busy: bool,
}

impl Default for FileManagerState {
    fn default() -> Self {
        Self {
            local: PanelState::default(),
            remote: PanelState::default(),
            show_hidden_local: false,
            show_hidden_remote: false,
            context_menu: None,
            inline_action: None,
            last_click: None,
            upload_busy: false,
            download_busy: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InlineAction {
    pub side: PanelSide,
    pub kind: InlineKind,
    pub value: String,
    /// Target path for a rename.
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineKind {
    NewFolder,
    Rename,
}

#[derive(Debug, Clone)]
pub struct ConfirmRequest {
    pub title: String,
    pub message: String,
    pub confirm_text: String,
    pub cancel_text: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteItems {
        session_id: String,
        side: PanelSide,
        paths: Vec<String>,
        is_dir: bool,
    },
    Upload {
        session_id: String,
        items: Vec<FileInfo>,
        target: String,
        overwrite: bool,
    },
    Download {
        session_id: String,
        items: Vec<FileInfo>,
        target: String,
        overwrite: bool,
    },
    DeleteHost(String),
    DeleteCredential(String),
    TrustHostKey(Box<HostKeyVerificationPrompt>),
}

#[derive(Debug, Clone)]
pub struct ConflictRequest {
    pub title: String,
    pub message: String,
    pub items: Vec<FileInfo>,
    pub target: String,
    pub direction: crate::models::TransferDirection,
}

// ── Modals ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    None,
    Hosts,
    HostForm,
    Credentials,
    CredentialForm,
    Settings,
}

#[derive(Debug, Clone, Default)]
pub struct HostFormState {
    pub editing_id: Option<String>,
    pub is_duplicate: bool,
    pub title: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub profile_id: String,
    pub color: String,
    pub description: String,
    pub start_directory_remote: String,
    pub start_directory_local: String,
    pub term: String,
    pub encode: String,
    pub enable_sftp: bool,
    pub keepalive_interval: String,
    pub password: String,
    pub error: Option<String>,
    pub saving: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CredentialFormState {
    pub editing_id: Option<String>,
    pub title: String,
    pub username: String,
    pub auth_type: String,
    pub password: String,
    pub private_key: String,
    pub passphrase: String,
    pub show_password: bool,
    pub show_passphrase: bool,
    pub error: Option<String>,
    pub saving: bool,
    /// When the form was opened from the Hosts form, return there on close.
    pub from_host_form: bool,
}

#[derive(Debug, Clone)]
pub struct LoginPrompt {
    pub title: String,
    pub host: String,
    pub username: String,
    pub password: String,
    /// Session tab waiting for these credentials.
    pub pending_session: Option<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct PasteConfirm {
    pub text: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemInfoKind {
    Cpu,
    Memory,
    Disk,
}

#[derive(Debug, Clone)]
pub struct SystemInfoState {
    pub kind: SystemInfoKind,
    pub session_id: String,
    pub loading: bool,
    pub error: Option<String>,
    pub rows: Vec<Vec<String>>,
    pub page: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickPopup {
    None,
    Commands,
    History,
}

#[derive(Debug, Clone)]
pub struct QuickActionsState {
    pub expanded: bool,
    pub popup: QuickPopup,
    pub history: Vec<String>,
    pub history_loading: bool,
    pub history_error: Option<String>,
    pub history_request: Option<u64>,
}

impl Default for QuickActionsState {
    fn default() -> Self {
        Self {
            expanded: false,
            popup: QuickPopup::None,
            history: Vec::new(),
            history_loading: false,
            history_error: None,
            history_request: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub kind: crate::session::ToastKind,
    pub created_ms: f64,
    pub key: Option<String>,
}

/// Per-terminal interaction state (selection, scroll, context menu).
#[derive(Debug, Clone, Default)]
pub struct TerminalUiState {
    pub selection: crate::term::Selection,
    pub dragging: bool,
    pub context_menu: Option<Pos2>,
    pub last_size: (u16, u16),
}

// ── Top-level app state ──────────────────────────────────────────────────────

pub struct AppState {
    pub db: Arc<Db>,
    pub mgr: Arc<SessionManager>,

    // data
    pub bookmarks: Vec<Bookmark>,
    pub profiles: Vec<Profile>,
    pub settings: Settings,
    pub host_reachability: HashMap<String, HostReachability>,
    pub host_probe_failures: HashMap<String, u32>,
    pub host_probe_flash: HashMap<String, f64>,

    // tabs
    pub host_tabs: Vec<HostTab>,
    pub active_host_tab: Option<String>,

    // ui
    pub sidebar_collapsed: bool,
    pub app_zoom: f32,
    pub modal: ModalKind,
    pub host_form: Option<HostFormState>,
    pub credential_form: Option<CredentialFormState>,
    pub login_prompt: Option<LoginPrompt>,
    pub paste_confirm: Option<PasteConfirm>,
    pub system_info: Option<SystemInfoState>,
    pub quick: HashMap<String, QuickActionsState>,
    pub terminal_ui: HashMap<String, TerminalUiState>,
    pub fm: HashMap<String, FileManagerState>,
    pub transfers: Vec<TransferProgress>,
    pub toasts: Vec<Toast>,
    pub confirm: Option<ConfirmRequest>,
    /// Transfer conflict awaiting the user's decision.
    pub conflict: Option<ConflictRequest>,
    /// Backend session waiting for a host-key decision, so it can be retried
    /// once the user trusts the fingerprint.
    pub pending_trust: Option<(String, HostKeyVerificationPrompt)>,

    // async bookkeeping
    pub adding_session: HashMap<String, f64>,
    /// Passwords typed into the reconnect overlay, keyed by session id.
    pub reconnect_passwords: HashMap<String, String>,
    pub pending_local_requests: HashMap<u64, String>,
    /// Local listings waiting for their 50 ms "show the spinner" delay.
    pub local_load_queue: Vec<LocalLoadRequest>,
    /// Cancelled transfers waiting to flip to `done` (2 s), which is what makes
    /// the queue row disappear.
    pub pending_cancel_done: Vec<(String, f64)>,
    pub host_search: String,

    /// Stars for the cosmic background, regenerated on resize.
    pub last_probe_ms: f64,
    pub last_cwd_poll_ms: f64,
}

impl AppState {
    pub fn new(db: Arc<Db>, mgr: Arc<SessionManager>, settings: Settings, app_zoom: f32) -> Self {
        Self {
            db,
            mgr,
            bookmarks: Vec::new(),
            profiles: Vec::new(),
            settings,
            host_reachability: HashMap::new(),
            host_probe_failures: HashMap::new(),
            host_probe_flash: HashMap::new(),
            host_tabs: Vec::new(),
            active_host_tab: None,
            sidebar_collapsed: false,
            app_zoom,
            modal: ModalKind::None,
            host_form: None,
            credential_form: None,
            login_prompt: None,
            paste_confirm: None,
            system_info: None,
            quick: HashMap::new(),
            terminal_ui: HashMap::new(),
            fm: HashMap::new(),
            transfers: Vec::new(),
            toasts: Vec::new(),
            confirm: None,
            conflict: None,
            pending_trust: None,
            adding_session: HashMap::new(),
            reconnect_passwords: HashMap::new(),
            pending_local_requests: HashMap::new(),
            local_load_queue: Vec::new(),
            pending_cancel_done: Vec::new(),
            host_search: String::new(),
            last_probe_ms: 0.0,
            last_cwd_poll_ms: 0.0,
        }
    }

    // ── Lookups ──────────────────────────────────────────────────────────────

    pub fn bookmark(&self, id: &str) -> Option<&Bookmark> {
        self.bookmarks.iter().find(|b| b.id == id)
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn host_tab(&self, id: &str) -> Option<&HostTab> {
        self.host_tabs.iter().find(|t| t.id == id)
    }

    pub fn host_tab_mut(&mut self, id: &str) -> Option<&mut HostTab> {
        self.host_tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn active_tab(&self) -> Option<&HostTab> {
        let id = self.active_host_tab.as_deref()?;
        self.host_tab(id)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut HostTab> {
        let id = self.active_host_tab.clone()?;
        self.host_tab_mut(&id)
    }

    pub fn fm_state(&self, session_id: &str) -> Option<&FileManagerState> {
        self.fm.get(session_id)
    }

    pub fn fm_state_mut(&mut self, session_id: &str) -> &mut FileManagerState {
        self.fm.entry(session_id.to_string()).or_default()
    }

    pub fn quick_mut(&mut self, session_id: &str) -> &mut QuickActionsState {
        self.quick.entry(session_id.to_string()).or_default()
    }

    pub fn terminal_ui_mut(&mut self, session_id: &str) -> &mut TerminalUiState {
        self.terminal_ui.entry(session_id.to_string()).or_default()
    }

    pub fn session_tab(&self, session_id: &str) -> Option<(&HostTab, &SessionTab)> {
        for tab in &self.host_tabs {
            if let Some(s) = tab.session(session_id) {
                return Some((tab, s));
            }
        }
        None
    }

    pub fn session_tab_mut(&mut self, session_id: &str) -> Option<&mut SessionTab> {
        for tab in &mut self.host_tabs {
            if let Some(idx) = tab.sessions.iter().position(|s| s.id == session_id) {
                return Some(&mut tab.sessions[idx]);
            }
        }
        None
    }

    /// True when `backend_id` belongs to an auxiliary (side) terminal.
    pub fn is_side_terminal(&self, backend_id: &str) -> bool {
        self.host_tabs.iter().any(|t| {
            t.sessions
                .iter()
                .any(|s| s.side_terminal_id.as_deref() == Some(backend_id))
        })
    }

    /// Apply a backend status to whichever terminal owns `backend_id`.
    pub fn mark_backend_status(
        &mut self,
        backend_id: &str,
        status: SessionStatus,
        error: Option<String>,
    ) -> bool {
        if let Some(s) = self.session_tab_mut(backend_id) {
            s.status = status;
            s.error = error;
            return false;
        }
        for tab in &mut self.host_tabs {
            for s in &mut tab.sessions {
                if s.side_terminal_id.as_deref() == Some(backend_id) {
                    s.side_terminal_status = status;
                    s.side_terminal_error = error;
                    if !matches!(status, SessionStatus::Connected | SessionStatus::Connecting) {
                        // The pane cannot show anything useful once the backend
                        // is gone; collapse it (same as the web client).
                        s.side_terminal_open = false;
                    }
                    return true;
                }
            }
        }
        false
    }

    pub fn session_ids(&self) -> Vec<String> {
        self.host_tabs
            .iter()
            .flat_map(|t| t.sessions.iter().map(|s| s.id.clone()))
            .collect()
    }

    // ── Toast helpers ────────────────────────────────────────────────────────

    pub fn toast(&mut self, message: impl Into<String>, kind: crate::session::ToastKind) {
        self.toast_keyed(message, kind, None);
    }

    pub fn toast_keyed(
        &mut self,
        message: impl Into<String>,
        kind: crate::session::ToastKind,
        key: Option<String>,
    ) {
        if let Some(k) = key.as_deref() {
            self.toasts.retain(|t| t.key.as_deref() != Some(k));
        }
        self.toasts.push(Toast {
            message: message.into(),
            kind,
            created_ms: now_ms(),
            key,
        });
        if self.toasts.len() > 6 {
            self.toasts.remove(0);
        }
    }

    pub fn prune_toasts(&mut self) {
        let now = now_ms();
        self.toasts.retain(|t| now - t.created_ms < 2000.0);
    }

    // ── Transfers ────────────────────────────────────────────────────────────

    pub fn upsert_transfer(&mut self, mut progress: TransferProgress) {
        let key = if progress.id.is_empty() {
            format!("{}:{}", progress.direction.as_str(), progress.file_name)
        } else {
            progress.id.clone()
        };
        progress.id = key.clone();
        if let Some(existing) = self.transfers.iter_mut().find(|t| t.id == key) {
            let created = existing.created_at_ms;
            *existing = progress;
            existing.created_at_ms = created;
        } else {
            self.transfers.push(progress);
        }
        // Keep the queue bounded and drop finished rows after a while.
        let now = now_ms();
        self.transfers.retain(|t| {
            t.status.is_active() || (now - t.created_at_ms as f64) < 60_000.0
        });
        if self.transfers.len() > 200 {
            let excess = self.transfers.len() - 200;
            self.transfers.drain(0..excess);
        }
    }

    pub fn session_transfers(&self, session_id: &str) -> Vec<&TransferProgress> {
        self.transfers
            .iter()
            .filter(|t| t.session_id.as_deref() == Some(session_id))
            .collect()
    }

    pub fn active_transfer_count(&self, session_id: &str) -> usize {
        self.transfers
            .iter()
            .filter(|t| {
                t.session_id.as_deref() == Some(session_id)
                    && t.status != TransferStatus::Done
                    && t.group_id.is_none()
            })
            .count()
            .max(
                self.transfers
                    .iter()
                    .filter(|t| {
                        t.session_id.as_deref() == Some(session_id)
                            && t.status != TransferStatus::Done
                    })
                    .count(),
            )
    }
}

