//! eframe application: layout, event pump and modal routing.

use crate::models::SessionStatus;
use crate::session::{AppEvent, ToastKind};
use crate::state::{AppState, SystemInfoKind, CONNECTION_CHECK_INTERVAL_MS};
use crate::theme;
use crate::ui;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};

pub struct TinyTermApp {
    pub state: AppState,
    pub fonts_installed: bool,
    pub last_zoom: f32,
    pub last_font_key: (String, u32),
    /// Logo texture for the empty states (loaded on the first frame).
    pub logo: Option<egui::TextureHandle>,
}

impl TinyTermApp {
    pub fn new(state: AppState) -> Self {
        let font_key = (state.settings.font_family.clone(), state.settings.font_size);
        let zoom = state.app_zoom;
        Self {
            state,
            fonts_installed: false,
            last_zoom: zoom,
            last_font_key: font_key,
            logo: None,
        }
    }

    // ── Event pump ───────────────────────────────────────────────────────────

    fn pump_events(&mut self) {
        let events = self.state.mgr.bus.drain();
        for event in events {
            self.apply_event(event);
        }
    }

    fn apply_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Output { .. } => {}
            AppEvent::Closed { session_id, reason } => {
                self.state
                    .mark_backend_status(&session_id, SessionStatus::Disconnected, Some(reason));
            }
            AppEvent::Ready {
                session_id,
                home,
                cwd,
                ..
            } => {
                // A side terminal has no `SessionTab` of its own — without this
                // branch it stayed stuck on "connecting" forever.
                if self
                    .state
                    .mark_backend_status(&session_id, SessionStatus::Connected, None)
                {
                    return;
                }
                if let Some(s) = self.state.session_tab_mut(&session_id) {
                    s.status = SessionStatus::Connected;
                    s.error = None;
                    s.local_path = crate::local_fs::home_dir().to_string_lossy().to_string();
                    if let Some(cwd) = cwd.clone() {
                        s.terminal_path = Some(cwd.clone());
                        s.remote_path = cwd;
                    }
                }
                let _ = home;
                // If the file manager was already open, load its panels now.
                if self
                    .state
                    .session_tab(&session_id)
                    .map(|(_, s)| s.fm_open)
                    .unwrap_or(false)
                {
                    let (local, remote) = self
                        .state
                        .fm
                        .get(&session_id)
                        .map(|fm| (fm.local.path.clone(), fm.remote.path.clone()))
                        .unwrap_or_default();
                    self.state.load_local_dir(&session_id, local);
                    self.state.load_remote_dir(&session_id, remote);
                }
                self.state.toast("连接成功", ToastKind::Success);
            }
            AppEvent::Failed {
                session_id,
                error,
                host_key,
            } => {
                if let Some(prompt) = host_key {
                    self.state.pending_trust =
                        Some((session_id.clone(), (*prompt).clone()));
                    ui::dialogs::host_key_confirm(&mut self.state, *prompt);
                    self.state.mark_backend_status(
                        &session_id,
                        SessionStatus::Error,
                        Some(error),
                    );
                    return;
                }
                if self
                    .state
                    .mark_backend_status(&session_id, SessionStatus::Error, Some(error.clone()))
                {
                    // Side terminal: the pane closes, so surface the reason.
                    self.state.toast(
                        format!("辅助终端打开失败: {error}"),
                        ToastKind::Error,
                    );
                }
            }
            AppEvent::RemoteDir {
                request,
                session_id,
                path,
                result,
            } => {
                if let Err(e) = &result {
                    if e == "__refresh__" {
                        self.state.load_remote_dir(&session_id, path);
                        return;
                    }
                }
                self.state
                    .apply_remote_dir(request, session_id, path, result);
            }
            AppEvent::RemoteCwd { session_id, path } => {
                self.state.update_session_path(&session_id, &path);
                // Two-phase open, phase 2: navigate to the *real* pwd unless the
                // user has since browsed somewhere by hand.
                let follow = self
                    .state
                    .fm
                    .get(&session_id)
                    .map(|fm| {
                        fm.remote.auto_follow
                            && fm.remote.path != path
                            && fm.remote.last_follow_path.as_deref() != Some(path.as_str())
                    })
                    .unwrap_or(false);
                if follow {
                    if let Some(fm) = self.state.fm.get_mut(&session_id) {
                        fm.remote.last_follow_path = Some(path.clone());
                    }
                    self.state.load_remote_dir(&session_id, path);
                }
            }
            AppEvent::Transfer(progress) => {
                let session_id = progress.session_id.clone();
                self.state.upsert_transfer(*progress);
                // Recompute the button state from the queue instead of toggling
                // a flag: a batch keeps `group_id` set for its whole lifetime,
                // so the old "clear on done" logic never fired for it and the
                // upload button stayed in its loading state forever.
                if let Some(sid) = session_id {
                    self.state.refresh_transfer_busy(&sid);
                }
            }
            AppEvent::RemoteDelete {
                path,
                success,
                error,
                ..
            } => {
                if success {
                    self.state
                        .toast("已删除", ToastKind::Success);
                } else {
                    self.state.toast(
                        format!(
                            "删除失败 {}: {}",
                            crate::remote_fs::basename(&path),
                            error.unwrap_or_default()
                        ),
                        ToastKind::Error,
                    );
                }
            }
            AppEvent::HostProbe { host_id, reachable } => {
                self.state.apply_host_probe(&host_id, reachable);
            }
            AppEvent::Toast { message, kind } => self.state.toast(message, kind),
            AppEvent::Query { request, result } => {
                self.apply_query(request, result);
            }
        }
    }

    fn apply_query(&mut self, request: u64, result: Result<String, String>) {
        // History popup?
        let history_target = self
            .state
            .quick
            .iter()
            .find(|(_, q)| q.history_request == Some(request))
            .map(|(k, _)| k.clone());
        if let Some(session_id) = history_target {
            let q = self.state.quick_mut(&session_id);
            q.history_loading = false;
            match result {
                Ok(output) => {
                    q.history = ui::system_info::parse_history(&output);
                    q.history_error = None;
                }
                Err(e) => q.history_error = Some(e),
            }
            return;
        }

        // System info modal?
        if let Some(info) = self.state.system_info.as_mut() {
            if info.loading {
                info.loading = false;
                match result {
                    Ok(output) => {
                        info.rows = if info.kind == SystemInfoKind::Disk {
                            ui::system_info::parse_disk_output(&output)
                        } else {
                            ui::system_info::parse_process_output(&output)
                        };
                        info.page = 0;
                        info.error = None;
                    }
                    Err(e) => info.error = Some(e),
                }
                return;
            }
        }
    }

    // ── Layout ───────────────────────────────────────────────────────────────

    fn paint_background(&mut self, ui: &mut egui::Ui, rect: Rect) {
        theme::paint_cosmic_background(ui.painter(), rect);
        let time = ui.input(|i| i.time) as f32;
        theme::paint_grid_background(ui.painter(), rect, time);
        // ~30 fps is plenty for a slow drift and keeps the CPU cool.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }

    fn main_layout(&mut self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        self.paint_background(ui, full);

        let body = Rect::from_min_max(
            Pos2::new(
                full.left() + theme::APP_BODY_PADDING,
                full.top() + theme::APP_BODY_TOP_PADDING,
            ),
            Pos2::new(
                full.right() - theme::APP_BODY_PADDING,
                full.bottom() - theme::APP_BODY_PADDING,
            ),
        );

        let sidebar_width = if self.state.sidebar_collapsed {
            theme::SIDEBAR_COLLAPSED_WIDTH
        } else {
            theme::SIDEBAR_WIDTH
        };
        let sidebar_rect = Rect::from_min_size(
            body.min,
            Vec2::new(sidebar_width, body.height()),
        );
        let content_rect = Rect::from_min_max(
            Pos2::new(sidebar_rect.right() + theme::APP_BODY_GAP, body.top()),
            body.right_bottom(),
        );

        ui::sidebar::show(ui, &mut self.state, sidebar_rect);
        self.content(ui, content_rect);
    }

    fn content(&mut self, ui: &mut egui::Ui, rect: Rect) {
        if self.state.host_tabs.is_empty() {
            self.empty_state(ui, rect);
            return;
        }
        let Some(tab_id) = self.state.active_host_tab.clone() else {
            self.empty_state(ui, rect);
            return;
        };
        let Some(tab) = self.state.host_tab(&tab_id).cloned() else {
            self.empty_state(ui, rect);
            return;
        };

        // ── Session tab strip ────────────────────────────────────────────────
        let tabstrip = Rect::from_min_size(
            rect.min,
            Vec2::new(rect.width(), theme::TABSTRIP_HEIGHT),
        );
        ui::session_tabs::show(ui, &mut self.state, tabstrip, &tab);

        // ── Workspace ────────────────────────────────────────────────────────
        let workspace = Rect::from_min_max(
            Pos2::new(rect.left(), tabstrip.bottom()),
            rect.right_bottom(),
        );
        if tab.sessions.is_empty() {
            let painter = ui.painter().clone();
            theme::glass_panel(&painter, workspace, theme::RADIUS_MD);
            let inner = workspace.shrink(4.0);
            painter.rect_filled(
                inner,
                CornerRadius::same(theme::RADIUS_XS),
                theme::TERMINAL_BG,
            );
            // Logo circle
            let circle = Rect::from_center_size(
                inner.center() - Vec2::new(0.0, 20.0),
                Vec2::splat(72.0),
            );
            painter.circle_filled(
                circle.center(),
                36.0,
                Color32::from_rgba_premultiplied(15, 34, 66, 36),
            );
            painter.circle_stroke(circle.center(), 35.0, Stroke::new(2.0, Color32::from_rgba_premultiplied(15, 34, 66, 82)));
            draw_logo(&painter, self.logo.as_ref(), circle, 0.62);
            painter.text(
                Pos2::new(inner.center().x, circle.bottom() + 22.0),
                Align2::CENTER_CENTER,
                "点击 + 新建终端连接",
                theme::f_sm(),
                theme::TEXT_MUTED,
            );
            return;
        }

        let active_session = tab
            .active_session
            .clone()
            .or_else(|| tab.sessions.last().map(|s| s.id.clone()));
        let Some(active_id) = active_session else {
            return;
        };
        let Some(session) = tab.session(&active_id).cloned() else {
            return;
        };

        // File manager reserves the bottom of the workspace when open.
        let fm_height = if session.fm_open {
            (crate::state::FM_CONTENT_HEIGHT + crate::state::FM_BAR_HEIGHT)
                .min((workspace.height() - 80.0).max(crate::state::FM_BAR_HEIGHT))
        } else {
            crate::state::FM_BAR_HEIGHT
        };
        // Always keep a gap so the file manager reads as its own rounded panel,
        // collapsed or not.
        let gap = 6.0;
        let terminal_area = Rect::from_min_max(
            workspace.min,
            Pos2::new(
                workspace.right(),
                workspace.bottom() - fm_height - gap,
            ),
        );
        let fm_rect = Rect::from_min_max(
            Pos2::new(workspace.left(), terminal_area.bottom() + gap),
            workspace.right_bottom(),
        );

        // Terminal area frame
        let painter = ui.painter().clone();
        let cr = CornerRadius {
            nw: 0,
            ne: 0,
            sw: theme::RADIUS_MD,
            se: theme::RADIUS_MD,
        };
        painter.rect_filled(terminal_area, cr, theme::BG_PANEL);
        crate::widgets::stroke_open(
            &painter,
            terminal_area,
            theme::RADIUS_MD,
            Stroke::new(1.0, theme::BORDER),
            crate::widgets::OpenSide::Top,
        );
        let inner = terminal_area.shrink(4.0);
        painter.rect_filled(
            inner,
            CornerRadius::same(theme::RADIUS_XS),
            theme::TERMINAL_BG,
        );

        // Side terminal split
        if session.side_terminal_open {
            let half = (inner.width() - 6.0) * 0.5;
            let main_rect = Rect::from_min_size(inner.min, Vec2::new(half, inner.height()));
            let side_rect = Rect::from_min_size(
                Pos2::new(inner.left() + half + 6.0, inner.top()),
                Vec2::new(half, inner.height()),
            );
            painter.line_segment(
                [
                    Pos2::new(side_rect.left() - 3.0, side_rect.top()),
                    Pos2::new(side_rect.left() - 3.0, side_rect.bottom()),
                ],
                Stroke::new(1.0, Color32::from_rgba_premultiplied(38, 61, 97, 46)),
            );
            ui::terminal_view::show(
                ui,
                &mut self.state,
                main_rect,
                ui::terminal_view::TerminalPane {
                    backend_id: &session.id,
                    status: session.status,
                    error: session.error.as_deref(),
                },
            );
            match session.side_terminal_status {
                SessionStatus::Connected => {
                    let side_id = session.side_terminal_id.clone().unwrap_or_default();
                    ui::terminal_view::show(
                        ui,
                        &mut self.state,
                        side_rect,
                        ui::terminal_view::TerminalPane {
                            backend_id: &side_id,
                            status: session.side_terminal_status,
                            error: session.side_terminal_error.as_deref(),
                        },
                    );
                }
                SessionStatus::Connecting => {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(side_rect));
                    child.vertical_centered(|ui| {
                        ui.add_space((side_rect.height() * 0.5 - 16.0).max(0.0));
                        crate::widgets::loading_blocks(ui, ui.input(|i| i.time) as f32, 1.4);
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("正在打开辅助终端...")
                                .size(theme::TEXT_XS)
                                .color(Color32::from_rgba_premultiplied(200, 210, 255, 184)),
                        );
                    });
                }
                _ => {
                    ui.painter().text(
                        side_rect.center(),
                        Align2::CENTER_CENTER,
                        session
                            .side_terminal_error
                            .clone()
                            .unwrap_or_else(|| "辅助终端打开失败".into()),
                        theme::f_xs(),
                        Color32::from_rgb(0xff, 0x8f, 0x8f),
                    );
                }
            }
        } else {
            ui::terminal_view::show(
                ui,
                &mut self.state,
                inner,
                ui::terminal_view::TerminalPane {
                    backend_id: &session.id,
                    status: session.status,
                    error: session.error.as_deref(),
                },
            );
        }

        // ── File manager ─────────────────────────────────────────────────────
        ui::file_manager::show(ui, &mut self.state, &active_id, fm_rect);
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter().clone();
        theme::glass_panel(&painter, rect, theme::RADIUS_MD);
        let center = rect.center();
        let circle = Rect::from_center_size(center - Vec2::new(0.0, 30.0), Vec2::splat(120.0));
        painter.circle_filled(
            circle.center(),
            60.0,
            Color32::from_rgba_premultiplied(15, 34, 66, 36),
        );
        painter.circle_stroke(
            circle.center(),
            59.0,
            Stroke::new(2.0, Color32::from_rgba_premultiplied(15, 34, 66, 82)),
        );
        painter.circle_stroke(
            circle.center(),
            62.0,
            Stroke::new(1.0, Color32::from_rgba_premultiplied(15, 34, 66, 20)),
        );
        draw_logo(&painter, self.logo.as_ref(), circle, 0.72);
        painter.text(
            Pos2::new(center.x, circle.bottom() + 26.0),
            Align2::CENTER_CENTER,
            "TinyTerm",
            theme::f_xl(),
            theme::TEXT_SECONDARY,
        );
        painter.text(
            Pos2::new(center.x, circle.bottom() + 52.0),
            Align2::CENTER_CENTER,
            "点击左侧 主机管理 添加主机并开始连接",
            theme::f_sm(),
            theme::TEXT_MUTED,
        );
    }

    // ── Global shortcuts ─────────────────────────────────────────────────────

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (zoom_in, zoom_out, zoom_reset) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)),
                cmd && i.key_pressed(egui::Key::Minus),
                cmd && i.key_pressed(egui::Key::Num0),
            )
        });
        if zoom_in {
            self.state.app_zoom =
                (self.state.app_zoom + crate::state::APP_ZOOM_STEP).min(crate::state::APP_ZOOM_MAX);
        }
        if zoom_out {
            self.state.app_zoom =
                (self.state.app_zoom - crate::state::APP_ZOOM_STEP).max(crate::state::APP_ZOOM_MIN);
        }
        if zoom_reset {
            self.state.app_zoom = crate::state::APP_ZOOM_MIN;
        }

        // Cmd+, opens settings.
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma)) {
            self.state.modal = crate::state::ModalKind::Settings;
        }
    }

    fn ensure_logo(&mut self, ctx: &egui::Context) {
        if self.logo.is_none() {
            self.logo = crate::assets::logo_texture(ctx);
        }
    }

    fn ensure_fonts(&mut self, ctx: &egui::Context) {
        let key = (
            self.state.settings.font_family.clone(),
            self.state.settings.font_size,
        );
        if !self.fonts_installed || key != self.last_font_key {
            theme::install_fonts(ctx, &key.0, key.1 as f32);
            self.fonts_installed = true;
            self.last_font_key = key;
        }
    }

    /// Called once per frame after the UI has been drawn.
    fn tick(&mut self, ctx: &egui::Context) {
        self.state.tick_animations();
        self.state.probe_open_hosts();
        self.state.poll_terminal_cwd();

        // Expire the "adding session" spinner.
        let now = crate::state::now_ms();
        self.state.adding_session.retain(|_, until| *until > now - 5000.0);

        if !self.state.local_load_queue.is_empty() || !self.state.pending_cancel_done.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }

        // Repaint while any transfer is active.
        if self
            .state
            .transfers
            .iter()
            .any(|t| t.status.is_active())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        // Keep the cursor blink and the drifting grid alive.
        ctx.request_repaint_after(std::time::Duration::from_millis(66));

        let _ = CONNECTION_CHECK_INTERVAL_MS;
    }
}

/// Draw the TinyTerm logo centred in `circle`, scaled to `fraction` of its
/// diameter. Falls back to a text glyph if the texture failed to load.
fn draw_logo(
    painter: &egui::Painter,
    logo: Option<&egui::TextureHandle>,
    circle: Rect,
    fraction: f32,
) {
    match logo {
        Some(texture) => {
            let size = circle.width() * fraction;
            let rect = Rect::from_center_size(circle.center(), Vec2::splat(size));
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            painter.text(
                circle.center(),
                Align2::CENTER_CENTER,
                ">_",
                theme::font_mono(circle.width() * 0.5),
                theme::ACCENT_LIGHT,
            );
        }
    }
}

impl eframe::App for TinyTermApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ensure_fonts(&ctx);
        self.ensure_logo(&ctx);

        if (ctx.zoom_factor() - self.state.app_zoom).abs() > 0.001 {
            ctx.set_zoom_factor(self.state.app_zoom);
            self.last_zoom = self.state.app_zoom;
        }

        self.handle_shortcuts(&ctx);
        self.pump_events();

        ui.set_clip_rect(ui.max_rect());
        self.main_layout(ui);

        // ── Modals & overlays ────────────────────────────────────────────────
        egui::Area::new(egui::Id::new("modals"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::Pos2::ZERO)
            .show(&ctx, |ui| {
                ui.set_clip_rect(ctx.input(|i| i.viewport_rect()));
                ui::hosts_modal::show(ui, &mut self.state);
                ui::credentials_modal::show(ui, &mut self.state);
                ui::settings_modal::show(ui, &mut self.state);
                ui::system_info::show(ui, &mut self.state);
                ui::dialogs::conflict_dialog(ui, &mut self.state);
                ui::dialogs::confirm_dialog(ui, &mut self.state);
                ui::dialogs::login_dialog(ui, &mut self.state);
                ui::dialogs::paste_dialog(ui, &mut self.state);
                ui::toast::show(ui, &mut self.state);
            });

        self.tick(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.state.shutdown();
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
}
