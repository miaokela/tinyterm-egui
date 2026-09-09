//! State transitions — the egui equivalent of the Zustand store actions in
//! `src/store/index.ts` plus the Tauri command wrappers.

use crate::models::*;
use crate::remote_fs;
use crate::session::{AppEvent, ConnectRequest, ToastKind};
use crate::state::*;
use crate::transfer::TransferJob;
use std::time::Duration;

impl AppState {
    // ── Data loading ─────────────────────────────────────────────────────────

    pub fn load_all(&mut self) {
        self.load_bookmarks();
        self.load_profiles();
        self.load_settings();
    }

    pub fn load_bookmarks(&mut self) {
        match self.db.list_bookmarks() {
            Ok(list) => {
                self.bookmarks = list;
                let ids: std::collections::HashSet<String> =
                    self.bookmarks.iter().map(|b| b.id.clone()).collect();
                self.host_reachability.retain(|k, _| ids.contains(k));
            }
            Err(e) => log::error!("load_bookmarks: {e}"),
        }
    }

    pub fn load_profiles(&mut self) {
        match self.db.list_profiles() {
            Ok(list) => self.profiles = list,
            Err(e) => log::error!("load_profiles: {e}"),
        }
    }

    pub fn load_settings(&mut self) {
        match self.db.get_settings() {
            Ok(settings) => {
                if settings.font_size != self.settings.font_size {
                    let _ = self.db.save_settings(&settings);
                }
                self.settings = settings;
            }
            Err(e) => log::error!("load_settings: {e}"),
        }
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = self.db.save_settings(&self.settings) {
            self.toast(format!("保存设置失败: {e}"), ToastKind::Error);
        }
    }

    // ── Host / credential CRUD ───────────────────────────────────────────────

    pub fn create_bookmark(&mut self, bookmark: Bookmark, secret: Option<String>) {
        match self.db.create_bookmark(&bookmark, secret.as_deref()) {
            Ok(created) => {
                self.bookmarks.push(created);
                self.toast("主机已创建", ToastKind::Success);
            }
            Err(e) => self.toast(format!("创建失败: {e}"), ToastKind::Error),
        }
    }

    pub fn update_bookmark(&mut self, bookmark: Bookmark, secret: Option<String>) {
        match self.db.update_bookmark(&bookmark, secret.as_deref()) {
            Ok(updated) => {
                if let Some(slot) = self.bookmarks.iter_mut().find(|b| b.id == updated.id) {
                    *slot = updated;
                }
                self.toast("主机已更新", ToastKind::Success);
            }
            Err(e) => self.toast(format!("更新失败: {e}"), ToastKind::Error),
        }
    }

    pub fn delete_bookmark(&mut self, id: &str) {
        if let Err(e) = self.db.delete_bookmark(id) {
            self.toast(format!("删除失败: {e}"), ToastKind::Error);
            return;
        }
        self.bookmarks.retain(|b| b.id != id);
        self.host_reachability.remove(id);
        // Close any tabs bound to this host.
        let tabs: Vec<String> = self
            .host_tabs
            .iter()
            .filter(|t| t.host_id == id)
            .map(|t| t.id.clone())
            .collect();
        for tab in tabs {
            self.remove_host_tab(&tab);
        }
        self.toast("主机已删除", ToastKind::Success);
    }

    pub fn upsert_profile(
        &mut self,
        profile: Profile,
        secret: Option<String>,
        key: Option<String>,
        passphrase: Option<String>,
    ) {
        match self
            .db
            .upsert_profile(&profile, secret.as_deref(), key.as_deref(), passphrase.as_deref())
        {
            Ok(()) => {
                self.load_profiles();
                self.toast("凭据已保存", ToastKind::Success);
            }
            Err(e) => self.toast(format!("保存失败: {e}"), ToastKind::Error),
        }
    }

    pub fn delete_profile(&mut self, id: &str) {
        if let Err(e) = self.db.delete_profile(id) {
            self.toast(format!("删除失败: {e}"), ToastKind::Error);
            return;
        }
        self.load_profiles();
        self.toast("凭据已删除", ToastKind::Success);
    }

    // ── Host key trust ───────────────────────────────────────────────────────

    pub fn trust_host_key(&mut self, prompt: &HostKeyVerificationPrompt) {
        let now = now_unix();
        let record = TrustedHostKey {
            host: prompt.host.clone(),
            port: prompt.port,
            key_type: prompt.key_type.clone(),
            fingerprint: prompt.fingerprint.clone(),
            created_at: now,
            updated_at: now,
        };
        match self.db.upsert_trusted_host_key(&record) {
            Ok(()) => self.toast("已保存受信任主机指纹", ToastKind::Success),
            Err(e) => self.toast(format!("保存指纹失败: {e}"), ToastKind::Error),
        }
    }

    // ── Host tabs ────────────────────────────────────────────────────────────

    pub fn open_host_tab(&mut self, host_id: &str) {
        if let Some(existing) = self
            .host_tabs
            .iter()
            .find(|t| t.host_id == host_id)
            .map(|t| t.id.clone())
        {
            self.active_host_tab = Some(existing.clone());
            let has_live = self
                .host_tab(&existing)
                .and_then(|t| t.active())
                .map(|s| matches!(s.status, SessionStatus::Connected | SessionStatus::Connecting))
                .unwrap_or(false);
            if !has_live {
                self.open_session(host_id, Some(existing));
            }
            self.modal = ModalKind::None;
            return;
        }

        let Some(host) = self.bookmark(host_id).cloned() else {
            return;
        };
        let title = host.display_name();
        let tab_id = uuid::Uuid::new_v4().to_string();
        self.host_tabs.push(HostTab {
            id: tab_id.clone(),
            title,
            host_id: host_id.to_string(),
            sessions: Vec::new(),
            active_session: None,
        });
        self.active_host_tab = Some(tab_id.clone());
        self.modal = ModalKind::None;
        self.open_session(host_id, Some(tab_id));
    }

    pub fn remove_host_tab(&mut self, tab_id: &str) {
        let Some(tab) = self.host_tab(tab_id).cloned() else {
            return;
        };
        for session in &tab.sessions {
            self.mgr.close(&session.id);
            if let Some(side) = session.side_terminal_id.clone() {
                self.mgr.close(&side);
            }
            self.fm.remove(&session.id);
            self.quick.remove(&session.id);
            self.terminal_ui.remove(&session.id);
        }
        self.transfers
            .retain(|t| tab.sessions.iter().all(|s| t.session_id.as_deref() != Some(&s.id)));
        self.host_tabs.retain(|t| t.id != tab_id);
        if self.active_host_tab.as_deref() == Some(tab_id) {
            self.active_host_tab = self.host_tabs.last().map(|t| t.id.clone());
        }
    }

    pub fn set_active_host_tab(&mut self, tab_id: &str) {
        self.active_host_tab = Some(tab_id.to_string());
    }

    // ── Sessions ─────────────────────────────────────────────────────────────

    pub fn open_session(&mut self, host_id: &str, tab_id: Option<String>) {
        let Some(tab_id) = tab_id.or_else(|| self.active_host_tab.clone()) else {
            return;
        };
        let Some(host) = self.bookmark(host_id).cloned() else {
            return;
        };

        let session_id = uuid::Uuid::new_v4().to_string();
        let tab = SessionTab::new(
            session_id.clone(),
            host.display_name(),
            host_id.to_string(),
        );

        // If no credential is linked, prompt for username / password first.
        let needs_login = host.auth_type == "profile"
            && host
                .profile_id
                .as_deref()
                .map(|s| s.is_empty())
                .unwrap_or(true);

        if let Some(t) = self.host_tab_mut(&tab_id) {
            t.sessions.push(tab.clone());
            t.active_session = Some(session_id.clone());
        } else {
            return;
        }
        self.fm.insert(session_id.clone(), FileManagerState {
            local: PanelState {
                path: tab.local_path.clone(),
                ..Default::default()
            },
            remote: PanelState {
                path: "/".into(),
                ..Default::default()
            },
            show_hidden_local: self.settings.show_hidden_files,
            show_hidden_remote: self.settings.show_hidden_files,
            ..Default::default()
        });
        self.quick_mut(&session_id);

        if needs_login {
            self.login_prompt = Some(LoginPrompt {
                title: format!("连接到 {}", host.display_name()),
                host: format!("{}:{}", host.host, host.port),
                username: host.username.clone(),
                password: String::new(),
                pending_session: Some((session_id.clone(), tab_id.clone())),
            });
            return;
        }

        self.start_connect(&session_id, host_id, None, None);
    }

    /// Spawn the SSH connection for an existing session tab.
    pub fn start_connect(
        &mut self,
        session_id: &str,
        host_id: &str,
        username: Option<String>,
        password: Option<String>,
    ) {
        let (cols, rows) = self
            .session_tab(session_id)
            .map(|(_, s)| (s.cols, s.rows))
            .unwrap_or((80, 24));
        self.mgr.connect(ConnectRequest {
            session_id: session_id.to_string(),
            bookmark_id: host_id.to_string(),
            cols,
            rows,
            username_override: username,
            password_override: password,
            scrollback: self.settings.scrollback as usize,
        });
    }

    pub fn close_session(&mut self, host_tab_id: &str, session_id: &str) {
        if let Some(tab) = self.host_tab(host_tab_id) {
            if let Some(session) = tab.session(session_id) {
                if let Some(side) = session.side_terminal_id.clone() {
                    self.mgr.close(&side);
                }
            }
        }
        self.mgr.close(session_id);
        self.fm.remove(session_id);
        self.quick.remove(session_id);
        self.terminal_ui.remove(session_id);
        self.transfers
            .retain(|t| t.session_id.as_deref() != Some(session_id));

        let mut remove_tab = false;
        if let Some(tab) = self.host_tab_mut(host_tab_id) {
            tab.sessions.retain(|s| s.id != session_id);
            if tab.sessions.is_empty() {
                remove_tab = true;
            } else if tab.active_session.as_deref() == Some(session_id) {
                tab.active_session = tab.sessions.last().map(|s| s.id.clone());
            }
        }
        if remove_tab {
            self.host_tabs.retain(|t| t.id != host_tab_id);
            if self.active_host_tab.as_deref() == Some(host_tab_id) {
                self.active_host_tab = self.host_tabs.last().map(|t| t.id.clone());
            }
        }
    }

    pub fn set_active_session(&mut self, host_tab_id: &str, session_id: &str) {
        if let Some(tab) = self.host_tab_mut(host_tab_id) {
            tab.active_session = Some(session_id.to_string());
        }
    }

    pub fn reconnect_session(&mut self, session_id: &str, password: Option<&str>) {
        let Some(session) = self.session_tab(session_id).map(|(_, s)| s.clone()) else {
            return;
        };
        self.mgr.close(session_id);
        if let Some(s) = self.session_tab_mut(session_id) {
            s.status = SessionStatus::Connecting;
            s.error = None;
            s.terminal_path = None;
            s.side_terminal_open = false;
            s.side_terminal_id = None;
            s.side_terminal_status = SessionStatus::Disconnected;
        }
        self.start_connect(
            session_id,
            &session.bookmark_id,
            None,
            password.map(|s| s.to_string()),
        );
    }

    pub fn reconnect_host_sessions(&mut self, host_id: &str, password: Option<&str>) {
        let targets: Vec<String> = self
            .host_tabs
            .iter()
            .filter(|t| t.host_id == host_id)
            .flat_map(|t| t.sessions.iter())
            .filter(|s| {
                matches!(
                    s.status,
                    SessionStatus::Disconnected | SessionStatus::Error
                )
            })
            .map(|s| s.id.clone())
            .collect();
        for id in targets {
            self.reconnect_session(&id, password);
        }
    }

    pub fn mark_host_sessions_disconnected(&mut self, host_id: &str, reason: &str) {
        let targets: Vec<String> = self
            .host_tabs
            .iter()
            .filter(|t| t.host_id == host_id)
            .flat_map(|t| t.sessions.iter())
            .filter(|s| s.status != SessionStatus::Disconnected)
            .map(|s| s.id.clone())
            .collect();
        for id in targets {
            self.mgr.close(&id);
            if let Some(s) = self.session_tab_mut(&id) {
                s.status = SessionStatus::Disconnected;
                s.error = Some(reason.to_string());
                s.side_terminal_open = false;
                s.side_terminal_id = None;
                s.side_terminal_status = SessionStatus::Disconnected;
            }
        }
    }

    pub fn toggle_side_terminal(&mut self, session_id: &str) {
        let Some(session) = self.session_tab(session_id).map(|(_, s)| s.clone()) else {
            return;
        };
        if session.side_terminal_open {
            if let Some(side) = session.side_terminal_id.clone() {
                self.mgr.close(&side);
            }
            if let Some(s) = self.session_tab_mut(session_id) {
                s.side_terminal_open = false;
                s.side_terminal_id = None;
                s.side_terminal_status = SessionStatus::Disconnected;
                s.side_terminal_error = None;
            }
            return;
        }

        let side_id = uuid::Uuid::new_v4().to_string();
        if let Some(s) = self.session_tab_mut(session_id) {
            s.side_terminal_open = true;
            s.side_terminal_id = Some(side_id.clone());
            s.side_terminal_status = SessionStatus::Connecting;
            s.side_terminal_error = None;
        }
        self.fm.insert(side_id.clone(), FileManagerState::default());
        self.start_connect(&side_id, &session.bookmark_id, None, None);
    }

    /// Record the cwd reported by the terminal. The remote panel decides on its
    /// own whether to follow (see `PanelState::auto_follow`), so this must not
    /// touch `fm.remote.path` — doing so used to make the follow check a no-op.
    /// Re-open an auxiliary terminal after its host key was trusted.
    pub fn retry_side_terminal(&mut self, backend_id: &str) {
        let mut bookmark_id = None;
        for tab in &mut self.host_tabs {
            for s in &mut tab.sessions {
                if s.side_terminal_id.as_deref() == Some(backend_id) {
                    s.side_terminal_open = true;
                    s.side_terminal_status = SessionStatus::Connecting;
                    s.side_terminal_error = None;
                    bookmark_id = Some(s.bookmark_id.clone());
                }
            }
        }
        if let Some(bookmark_id) = bookmark_id {
            self.start_connect(backend_id, &bookmark_id, None, None);
        }
    }

    pub fn update_session_path(&mut self, session_id: &str, path: &str) {
        if let Some(s) = self.session_tab_mut(session_id) {
            s.terminal_path = Some(path.to_string());
            s.remote_path = path.to_string();
        }
    }

    // ── Host probing ─────────────────────────────────────────────────────────

    pub fn probe_open_hosts(&mut self) {
        let now = now_ms();
        if now - self.last_probe_ms < CONNECTION_CHECK_INTERVAL_MS {
            return;
        }
        self.last_probe_ms = now;
        let targets: Vec<(String, String, u16)> = self
            .host_tabs
            .iter()
            .filter_map(|t| {
                self.bookmark(&t.host_id)
                    .map(|b| (b.id.clone(), b.host.clone(), b.port))
            })
            .collect();
        for (id, host, port) in targets {
            self.mgr.probe_host(id, host, port);
        }
    }

    pub fn apply_host_probe(&mut self, host_id: &str, reachable: bool) {
        if reachable {
            self.host_probe_failures.remove(host_id);
            self.host_reachability
                .insert(host_id.to_string(), HostReachability::Reachable);
            self.host_probe_flash
                .insert(host_id.to_string(), now_ms() + 420.0);

            let has_reconnectable = self
                .host_tabs
                .iter()
                .filter(|t| t.host_id == host_id)
                .flat_map(|t| t.sessions.iter())
                .any(|s| {
                    matches!(
                        s.status,
                        SessionStatus::Disconnected | SessionStatus::Error
                    )
                });
            if has_reconnectable {
                self.reconnect_host_sessions(host_id, None);
            }
            return;
        }

        let count = self
            .host_probe_failures
            .entry(host_id.to_string())
            .or_insert(0);
        *count += 1;
        if *count >= 2 {
            let was_unreachable = self
                .host_reachability
                .get(host_id)
                .map(|s| *s == HostReachability::Unreachable)
                .unwrap_or(false);
            self.host_reachability
                .insert(host_id.to_string(), HostReachability::Unreachable);
            if !was_unreachable {
                self.mark_host_sessions_disconnected(
                    host_id,
                    "SSH 端口检测连续失败 2 次，连接已断开。",
                );
            }
        }
    }

    // ── File manager ─────────────────────────────────────────────────────────

    /// Request a local listing. The read itself happens in
    /// [`AppState::pump_local_loads`] ~50 ms later so the spinner is visible.
    pub fn load_local_dir(&mut self, session_id: &str, path: String) {
        let request = self.mgr.request_id();
        if let Some(fm) = self.fm.get_mut(session_id) {
            fm.local.loading = true;
            fm.local.error = None;
            fm.local.pending_request = Some(request);
            fm.local.pending_path = Some(path.clone());
        }
        self.pending_local_requests
            .insert(request, path.clone());
        self.local_load_queue.push(LocalLoadRequest {
            request,
            session_id: session_id.to_string(),
            path,
            due_ms: now_ms() + 50.0,
        });
    }

    /// Run local listings whose 50 ms delay has elapsed.
    pub fn pump_local_loads(&mut self) {
        let now = now_ms();
        let mut index = 0;
        while index < self.local_load_queue.len() {
            if self.local_load_queue[index].due_ms <= now {
                let req = self.local_load_queue.remove(index);
                let result = crate::local_fs::list_dir(&req.path).map_err(|e| e.to_string());
                self.apply_local_dir(req.request, req.path, result);
            } else {
                index += 1;
            }
        }
    }

    pub fn apply_local_dir(
        &mut self,
        request: u64,
        path: String,
        result: Result<Vec<FileInfo>, String>,
    ) {
        self.pending_local_requests.remove(&request);
        let Some(session_id) = self
            .fm
            .iter()
            .find(|(_, fm)| fm.local.pending_request == Some(request))
            .map(|(k, _)| k.clone())
        else {
            return;
        };
        if let Some(fm) = self.fm.get_mut(&session_id) {
            fm.local.loading = false;
            match result {
                Ok(entries) => {
                    fm.local.entries = entries;
                    fm.local.path = path;
                    fm.local.error = None;
                    fm.local.selected.clear();
                }
                Err(e) => fm.local.error = Some(e),
            }
        }
    }

    pub fn load_remote_dir(&mut self, session_id: &str, path: String) {
        let request = self.mgr.request_id();
        if let Some(fm) = self.fm.get_mut(session_id) {
            fm.remote.loading = true;
            fm.remote.error = None;
            fm.remote.pending_request = Some(request);
            fm.remote.pending_path = Some(path.clone());
        }
        let _ = request;
        self.mgr.list_remote_dir(session_id, path);
    }

    /// Apply a remote listing.
    ///
    /// Correlation is by **path**, not by request id: `SessionManager` allocates
    /// its own id when it spawns the listing task, so the id in the event never
    /// matched `pending_request` and the panel stayed in its loading state.
    pub fn apply_remote_dir(
        &mut self,
        request: u64,
        session_id: String,
        path: String,
        result: Result<Vec<FileInfo>, String>,
    ) {
        let _ = request;
        let Some(fm) = self.fm.get_mut(&session_id) else {
            return;
        };
        // Drop stale responses for a directory the user already navigated away
        // from (only the newest requested path clears the spinner).
        if fm.remote.pending_path.as_deref() != Some(path.as_str()) {
            return;
        }
        fm.remote.pending_path = None;
        fm.remote.loading = false;
        match result {
            Ok(entries) => {
                fm.remote.entries = entries;
                fm.remote.path = path;
                fm.remote.error = None;
                fm.remote.selected.clear();
            }
            Err(e) => fm.remote.error = Some(e),
        }
    }

    /// Open the file manager for a session: two-phase remote path sync.
    pub fn open_file_manager(&mut self, session_id: &str) {
        let Some(session) = self.session_tab(session_id).map(|(_, s)| s.clone()) else {
            return;
        };
        let local = if session.local_path.is_empty() {
            crate::local_fs::home_dir().to_string_lossy().to_string()
        } else {
            session.local_path.clone()
        };
        let remote = session
            .terminal_path
            .clone()
            .unwrap_or_else(|| session.remote_path.clone());

        if let Some(fm) = self.fm.get_mut(session_id) {
            fm.local.path = local.clone();
            fm.remote.path = remote.clone();
            fm.remote.auto_follow = true;
            fm.remote.last_follow_path = None;
            fm.remote.error = None;
        } else {
            self.fm.insert(
                session_id.to_string(),
                FileManagerState {
                    local: PanelState {
                        path: local.clone(),
                        ..Default::default()
                    },
                    remote: PanelState {
                        path: remote.clone(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
        }
        if let Some(s) = self.session_tab_mut(session_id) {
            s.fm_open = true;
        }
        self.load_local_dir(session_id, local);
        self.load_remote_dir(session_id, remote);
        self.mgr.refresh_remote_cwd(session_id);
    }

    pub fn close_file_manager(&mut self, session_id: &str) {
        if let Some(s) = self.session_tab_mut(session_id) {
            s.fm_open = false;
        }
    }

    // ── Transfers ────────────────────────────────────────────────────────────

    pub fn start_transfer(
        &mut self,
        session_id: &str,
        direction: TransferDirection,
        items: Vec<FileInfo>,
        target: String,
        overwrite: bool,
    ) {
        if items.is_empty() || self.mgr.get(session_id).is_none() {
            return;
        }
        if let Some(fm) = self.fm.get_mut(session_id) {
            match direction {
                TransferDirection::Upload => fm.upload_busy = true,
                TransferDirection::Download => fm.download_busy = true,
            }
        }
        self.mgr.start_transfer(TransferJob {
            session_id: session_id.to_string(),
            direction,
            items,
            target_dir: target,
            overwrite_all: overwrite,
        });
    }

    /// Cancel one transfer — or, when given a batch parent id, every item in
    /// that batch.
    ///
    /// Mirrors the web client: the row immediately turns into an error row
    /// ("用户取消") and flips to `done` after 2 s, at which point the queue
    /// hides it again.
    /// Derive the upload/download button state from the live queue for one
    /// session. A direction is busy while any of its transfers is pending,
    /// transferring or waiting on a conflict decision.
    pub fn refresh_transfer_busy(&mut self, session_id: &str) {
        let mut upload = false;
        let mut download = false;
        for t in &self.transfers {
            if t.session_id.as_deref() != Some(session_id) || !t.status.is_busy() {
                continue;
            }
            match t.direction {
                TransferDirection::Upload => upload = true,
                TransferDirection::Download => download = true,
            }
        }
        if let Some(fm) = self.fm.get_mut(session_id) {
            fm.upload_busy = upload;
            fm.download_busy = download;
        }
    }

    pub fn cancel_transfer(&mut self, transfer_id: &str) {
        let is_group = self
            .transfers
            .iter()
            .any(|t| t.id == transfer_id && t.group_id.as_deref() == Some(transfer_id));
        let ids: Vec<String> = if is_group {
            self.transfers
                .iter()
                .filter(|t| t.group_id.as_deref() == Some(transfer_id))
                .map(|t| t.id.clone())
                .collect()
        } else {
            vec![transfer_id.to_string()]
        };

        for id in ids {
            // Mark it cancelled for the running worker; `TransferCtx::start`
            // and the terminal `emit` both clear the flag again, so the set
            // never keeps a stale entry for a future transfer of the same path.
            self.mgr.cancel_transfer(&id);
            if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                if t.status != TransferStatus::Done {
                    t.status = TransferStatus::Error;
                    t.error = Some("用户取消".into());
                }
            }
            self.pending_cancel_done.push((id, now_ms() + 2000.0));
        }
        if let Some(sid) = self
            .transfers
            .iter()
            .find(|t| t.id == transfer_id || t.group_id.as_deref() == Some(transfer_id))
            .and_then(|t| t.session_id.clone())
        {
            self.refresh_transfer_busy(&sid);
        }
    }

    /// Flip cancelled rows to `done` once their 2 s grace period elapsed.
    pub fn pump_cancel_done(&mut self) {
        let now = now_ms();
        let mut index = 0;
        while index < self.pending_cancel_done.len() {
            if self.pending_cancel_done[index].1 <= now {
                let (id, _) = self.pending_cancel_done.remove(index);
                let session_id = self
                    .transfers
                    .iter()
                    .find(|t| t.id == id)
                    .and_then(|t| t.session_id.clone());
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    t.status = TransferStatus::Done;
                    t.error = None;
                }
                if let Some(sid) = session_id {
                    self.refresh_transfer_busy(&sid);
                }
            } else {
                index += 1;
            }
        }
    }

    /// Pre-flight conflict detection, ported from `handleTransferToRemote`.
    pub fn request_transfer(
        &mut self,
        session_id: &str,
        direction: TransferDirection,
        target: String,
    ) {
        let Some(fm) = self.fm.get(session_id) else {
            return;
        };
        let (items, existing): (Vec<FileInfo>, Vec<String>) = match direction {
            TransferDirection::Upload => (
                fm.local.selected_items(),
                fm.remote
                    .visible_entries(fm.show_hidden_remote)
                    .iter()
                    .map(|f| f.name.clone())
                    .collect(),
            ),
            TransferDirection::Download => (
                fm.remote.selected_items(),
                fm.local
                    .visible_entries(fm.show_hidden_local)
                    .iter()
                    .map(|f| f.name.clone())
                    .collect(),
            ),
        };

        if items.is_empty() {
            let (title, message) = match direction {
                TransferDirection::Upload => (
                    "上传提示",
                    "请先在本地面板选择要上传的文件或文件夹",
                ),
                TransferDirection::Download => (
                    "下载提示",
                    "请先在远程面板选择要下载的文件或文件夹",
                ),
            };
            self.confirm = Some(ConfirmRequest {
                title: title.into(),
                message: message.into(),
                confirm_text: "知道了".into(),
                cancel_text: String::new(),
                action: ConfirmAction::Upload {
                    session_id: session_id.to_string(),
                    items: Vec::new(),
                    target: String::new(),
                    overwrite: false,
                },
            });
            return;
        }

        let conflicts: Vec<&FileInfo> = items
            .iter()
            .filter(|i| existing.contains(&i.name))
            .collect();
        let has_folder = items.iter().any(|i| i.is_dir);
        let type_label = if has_folder { "个项目" } else { "个文件" };
        let action = match direction {
            TransferDirection::Upload => ConfirmAction::Upload {
                session_id: session_id.to_string(),
                items: items.clone(),
                target: target.clone(),
                overwrite: false,
            },
            TransferDirection::Download => ConfirmAction::Download {
                session_id: session_id.to_string(),
                items: items.clone(),
                target: target.clone(),
                overwrite: false,
            },
        };

        if !conflicts.is_empty() {
            let folder_conflicts = conflicts.iter().any(|c| c.is_dir);
            let names: String = conflicts
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            let names = if names.len() > 50 {
                format!("{}...", &names[..50])
            } else {
                names
            };
            let (verb, noun) = match direction {
                TransferDirection::Upload => ("上传", "上传"),
                TransferDirection::Download => ("下载", "下载"),
            };
            let _ = verb;
            let (title, message) = if folder_conflicts {
                (
                    "文件夹合并/覆盖确认".to_string(),
                    format!(
                        "目标目录中已存在 {} 个同名文件夹（如：{}）。\n继续{}将合并目录。若遇到同名文件，请选择处理方式：",
                        conflicts.len(),
                        names,
                        noun
                    ),
                )
            } else {
                (
                    "文件覆盖确认".to_string(),
                    format!(
                        "目标目录中已存在 {} 个同名文件（如：{}）。\n请选择处理方式：",
                        conflicts.len(),
                        names
                    ),
                )
            };
            self.fm.get_mut(session_id).map(|_| ());
            self.conflict = Some(ConflictRequest {
                title,
                message,
                items,
                target,
                direction,
            });
            return;
        }

        let names: String = items
            .iter()
            .map(|i| i.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let names = if names.len() > 100 {
            format!("{}...", &names[..100])
        } else {
            names
        };
        let (title, verb) = match direction {
            TransferDirection::Upload => ("确认上传", "开始上传"),
            TransferDirection::Download => ("确认下载", "开始下载"),
        };
        let message = format!(
            "确定{} {} {}{}目录？\n{}",
            match direction {
                TransferDirection::Upload => "上传",
                TransferDirection::Download => "下载",
            },
            items.len(),
            type_label,
            match direction {
                TransferDirection::Upload => "到远程",
                TransferDirection::Download => "到本地",
            },
            names
        );
        let _ = action;
        self.confirm = Some(ConfirmRequest {
            title: title.into(),
            message,
            confirm_text: verb.into(),
            cancel_text: "取消".into(),
            action: match direction {
                TransferDirection::Upload => ConfirmAction::Upload {
                    session_id: session_id.to_string(),
                    items,
                    target,
                    overwrite: false,
                },
                TransferDirection::Download => ConfirmAction::Download {
                    session_id: session_id.to_string(),
                    items,
                    target,
                    overwrite: false,
                },
            },
        });
    }

    // ── Context menu actions ─────────────────────────────────────────────────

    pub fn fm_rename(&mut self, session_id: &str, side: PanelSide, path: &str, new_name: &str) {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return;
        }
        let new_path = match side {
            PanelSide::Local => crate::local_fs::join_path(
                &remote_fs::parent_of(path).replace('/', std::path::MAIN_SEPARATOR_STR),
                new_name,
            ),
            PanelSide::Remote => remote_fs::join_path(&remote_fs::parent_of(path), new_name),
        };
        match side {
            PanelSide::Local => {
                if let Err(e) = crate::local_fs::rename(path, &new_path) {
                    self.toast(format!("重命名失败: {e}"), ToastKind::Error);
                } else {
                    self.toast("重命名成功", ToastKind::Success);
                    let path = self
                        .fm
                        .get(session_id)
                        .map(|fm| fm.local.path.clone())
                        .unwrap_or_default();
                    self.load_local_dir(session_id, path);
                }
            }
            PanelSide::Remote => {
                let Some(session) = self.mgr.get(session_id) else {
                    return;
                };
                let (old, new) = (path.to_string(), new_path.clone());
                let mgr = self.mgr.clone();
                let rt = self.mgr.rt.clone();
                let sid = session_id.to_string();
                let path = self
                    .fm
                    .get(session_id)
                    .map(|fm| fm.remote.path.clone())
                    .unwrap_or_default();
                rt.spawn(async move {
                    let result = remote_fs::rename(&session, &old, &new).await;
                    match result {
                        Ok(()) => mgr.bus.send(AppEvent::Toast {
                            message: "重命名成功".into(),
                            kind: ToastKind::Success,
                        }),
                        Err(e) => mgr.bus.send(AppEvent::Toast {
                            message: format!("重命名失败: {e}"),
                            kind: ToastKind::Error,
                        }),
                    }
                    mgr.request_remote_refresh(&sid, path);
                });
            }
        }
    }

    pub fn fm_new_folder(&mut self, session_id: &str, side: PanelSide, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        match side {
            PanelSide::Local => {
                let base = self
                    .fm
                    .get(session_id)
                    .map(|fm| fm.local.path.clone())
                    .unwrap_or_default();
                let path = crate::local_fs::join_path(&base, name);
                if let Err(e) = crate::local_fs::create_dir(&path) {
                    self.toast(format!("创建失败: {e}"), ToastKind::Error);
                } else {
                    self.toast("文件夹已创建", ToastKind::Success);
                    self.load_local_dir(session_id, base);
                }
            }
            PanelSide::Remote => {
                let base = self
                    .fm
                    .get(session_id)
                    .map(|fm| fm.remote.path.clone())
                    .unwrap_or_default();
                let path = remote_fs::join_path(&base, name);
                let Some(session) = self.mgr.get(session_id) else {
                    return;
                };
                let mgr = self.mgr.clone();
                let rt = self.mgr.rt.clone();
                let sid = session_id.to_string();
                rt.spawn(async move {
                    match remote_fs::create_dir(&session, &path).await {
                        Ok(()) => {
                            mgr.bus.send(AppEvent::Toast {
                                message: "文件夹已创建".into(),
                                kind: ToastKind::Success,
                            });
                            mgr.request_remote_refresh(&sid, base);
                        }
                        Err(e) => mgr.bus.send(AppEvent::Toast {
                            message: format!("创建失败: {e}"),
                            kind: ToastKind::Error,
                        }),
                    }
                });
            }
        }
    }

    pub fn fm_delete(&mut self, session_id: &str, side: PanelSide, paths: Vec<String>) {
        match side {
            PanelSide::Local => {
                for path in &paths {
                    if let Err(e) = crate::local_fs::delete(path) {
                        self.toast(format!("删除失败: {e}"), ToastKind::Error);
                    }
                }
                let base = self
                    .fm
                    .get(session_id)
                    .map(|fm| fm.local.path.clone())
                    .unwrap_or_default();
                self.load_local_dir(session_id, base);
                self.toast("已删除", ToastKind::Success);
            }
            PanelSide::Remote => {
                let Some(session) = self.mgr.get(session_id) else {
                    return;
                };
                let base = self
                    .fm
                    .get(session_id)
                    .map(|fm| fm.remote.path.clone())
                    .unwrap_or_default();
                let mgr = self.mgr.clone();
                let rt = self.mgr.rt.clone();
                let sid = session_id.to_string();
                let is_dir_map: Vec<(String, bool)> = paths
                    .iter()
                    .map(|p| {
                        let is_dir = self
                            .fm
                            .get(session_id)
                            .and_then(|fm| fm.remote.entries.iter().find(|f| &f.path == p))
                            .map(|f| f.is_dir)
                            .unwrap_or(false);
                        (p.clone(), is_dir)
                    })
                    .collect();
                rt.spawn(async move {
                    for (path, is_dir) in is_dir_map {
                        match remote_fs::remove(&session, &path, is_dir).await {
                            Ok(()) => mgr.bus.send(AppEvent::RemoteDelete {
                                path,
                                is_dir,
                                success: true,
                                error: None,
                            }),
                            Err(e) => mgr.bus.send(AppEvent::RemoteDelete {
                                path,
                                is_dir,
                                success: false,
                                error: Some(e.to_string()),
                            }),
                        }
                    }
                    mgr.request_remote_refresh(&sid, base);
                });
            }
        }
    }

    /// Open the context-menu confirm dialog for deleting the current selection.
    pub fn request_delete(&mut self, session_id: &str, side: PanelSide, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        let (label, name) = if paths.len() == 1 {
            (
                "删除",
                remote_fs::basename(&paths[0]),
            )
        } else {
            ("删除", format!("{} 项", paths.len()))
        };
        let message = format!("确认{label} {name}？此操作不可撤销。");
        self.confirm = Some(ConfirmRequest {
            title: format!("{label}确认"),
            message,
            confirm_text: label.into(),
            cancel_text: "取消".into(),
            action: ConfirmAction::DeleteItems {
                session_id: session_id.to_string(),
                side,
                is_dir: true,
                paths,
            },
        });
    }

    pub fn clear_context_menu(&mut self, session_id: &str) {
        if let Some(fm) = self.fm.get_mut(session_id) {
            fm.context_menu = None;
        }
    }

    // ── Housekeeping ─────────────────────────────────────────────────────────

    /// Poll remote cwd for every open file manager (throttled).
    pub fn poll_terminal_cwd(&mut self) {
        let now = now_ms();
        if now - self.last_cwd_poll_ms < 2000.0 {
            return;
        }
        self.last_cwd_poll_ms = now;
        let targets: Vec<String> = self
            .host_tabs
            .iter()
            .flat_map(|t| t.sessions.iter())
            .filter(|s| s.fm_open && s.status == SessionStatus::Connected)
            .map(|s| s.id.clone())
            .collect();
        for id in targets {
            self.mgr.refresh_remote_cwd(&id);
        }
    }

    /// Expire the host-probe heartbeat flash and stale toasts.
    pub fn tick_animations(&mut self) {
        self.pump_local_loads();
        self.pump_cancel_done();
        let now = now_ms();
        self.host_probe_flash.retain(|_, until| *until > now);
        self.prune_toasts();
    }

    pub fn shutdown(&mut self) {
        for id in self.session_ids() {
            self.mgr.close(&id);
        }
        // Give the disconnect frames a moment to flush.
        std::thread::sleep(Duration::from_millis(120));
    }
}
