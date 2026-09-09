//! Modal dialogs: unified confirm/alert, transfer-conflict resolution, the
//! connection login prompt and the paste confirmation.

use crate::state::{AppState, ConfirmAction, ConfirmRequest};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

// ── Shared shell ─────────────────────────────────────────────────────────────

/// Geometry of a dialog card.
struct DialogLayout {
    rect: Rect,
    header: Rect,
    body: Rect,
    footer: Rect,
}

/// Dim the screen and lay out a card of `width` × `height`, clamped so it can
/// never spill outside the window.
fn dialog_frame(ui: &mut Ui, width: f32, height: f32) -> DialogLayout {
    let screen = ui.ctx().input(|i| i.viewport_rect());
    ui.painter().rect_filled(
        screen,
        0,
        Color32::from_rgba_premultiplied(2, 6, 14, 184),
    );

    let width = width.min(screen.width() - 48.0).max(280.0);
    let height = height.min(screen.height() - 80.0).max(150.0);
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));

    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_LG), theme::BG_CARD);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_LG),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    // Soft outer glow so the card lifts off the dimmed backdrop.
    theme::glow(&painter, rect, theme::RADIUS_LG, theme::ACCENT, 0.35);

    let header_h = 56.0;
    let footer_h = 60.0;
    let header = Rect::from_min_size(rect.min, Vec2::new(width, header_h));
    let footer = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - footer_h),
        Vec2::new(width, footer_h),
    );
    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 20.0, header.bottom() + 6.0),
        Pos2::new(rect.right() - 20.0, footer.top() - 6.0),
    );

    painter.line_segment(
        [
            Pos2::new(header.left() + 16.0, header.bottom() - 0.5),
            Pos2::new(header.right() - 16.0, header.bottom() - 0.5),
        ],
        Stroke::new(1.0, theme::BORDER),
    );
    painter.line_segment(
        [
            Pos2::new(footer.left() + 16.0, footer.top() + 0.5),
            Pos2::new(footer.right() - 16.0, footer.top() + 0.5),
        ],
        Stroke::new(1.0, theme::BORDER),
    );

    DialogLayout {
        rect,
        header,
        body,
        footer,
    }
}

/// Title row with an icon; returns nothing (the layout already has the rects).
fn dialog_header(ui: &Ui, layout: &DialogLayout, glyph: &str, title: &str) {
    let painter = ui.painter();
    let icon_center = Pos2::new(layout.header.left() + 30.0, layout.header.center().y);
    painter.circle_filled(
        icon_center,
        15.0,
        Color32::from_rgba_premultiplied(29, 57, 102, 110),
    );
    widgets::icon(painter, icon_center, 16.0, glyph, theme::ACCENT_LIGHT);
    painter.text(
        Pos2::new(layout.header.left() + 56.0, layout.header.center().y),
        Align2::LEFT_CENTER,
        theme::truncate(painter, title, &theme::f_md(), layout.rect.width() - 80.0),
        theme::f_md(),
        theme::TEXT_PRIMARY,
    );
}

/// Scrollable, word-wrapped message body. Wrapping (rather than truncating)
/// keeps long file lists readable and guarantees the text stays inside the card.
fn dialog_body(ui: &mut Ui, layout: &DialogLayout, text: &str, salt: &str) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(layout.body));
    egui::ScrollArea::vertical()
        .id_salt(("dialog-body", salt))
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(text)
                    .size(theme::TEXT_SM)
                    .color(theme::TEXT_SECONDARY)
                    .line_height(Some(20.0)),
            );
        });
}

/// Height a dialog needs for `text`, so the card fits its content.
fn measure_dialog_height(ui: &Ui, width: f32, text: &str) -> f32 {
    let galley = ui.painter().layout(
        text.to_owned(),
        theme::f_sm(),
        theme::TEXT_SECONDARY,
        width - 40.0,
    );
    galley.size().y + 56.0 + 60.0 + 24.0
}

// ── Unified confirm / alert ──────────────────────────────────────────────────

pub fn confirm_dialog(ui: &mut Ui, app: &mut AppState) {
    let Some(request) = app.confirm.clone() else {
        return;
    };
    let screen = ui.ctx().input(|i| i.viewport_rect());
    let width = 480.0_f32.min(screen.width() - 48.0);
    let height = measure_dialog_height(ui, width, &request.message);
    let layout = dialog_frame(ui, width, height);

    let glyph = if request.cancel_text.is_empty() {
        crate::icons::INFO
    } else {
        crate::icons::QUESTION
    };
    dialog_header(ui, &layout, glyph, &request.title);
    dialog_body(ui, &layout, &request.message, &request.title);

    let has_cancel = !request.cancel_text.is_empty();
    let mut labels: Vec<(&str, bool)> = Vec::new();
    if has_cancel {
        labels.push((request.cancel_text.as_str(), true));
    }
    labels.push((request.confirm_text.as_str(), true));
    let buttons = Rect::from_min_size(
        Pos2::new(layout.footer.right() - 20.0 - 200.0, layout.footer.center().y - 15.0),
        Vec2::new(200.0, 30.0),
    );
    let clicked = widgets::button_row(ui, buttons, &labels);

    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

    if escape || (has_cancel && clicked == Some(0)) {
        app.confirm = None;
        return;
    }
    if enter || clicked == Some(labels.len() - 1) {
        app.confirm = None;
        apply_confirm(app, request.action, true);
    }
}

// ── Transfer conflict ────────────────────────────────────────────────────────

/// Execute the action a confirm dialog was carrying.
pub fn apply_confirm(app: &mut AppState, action: ConfirmAction, _accepted: bool) {
    match action {
        ConfirmAction::DeleteItems {
            session_id,
            side,
            paths,
            ..
        } => app.fm_delete(&session_id, side, paths),
        ConfirmAction::Upload {
            session_id,
            items,
            target,
            overwrite,
        } => app.start_transfer(
            &session_id,
            crate::models::TransferDirection::Upload,
            items,
            target,
            overwrite,
        ),
        ConfirmAction::Download {
            session_id,
            items,
            target,
            overwrite,
        } => app.start_transfer(
            &session_id,
            crate::models::TransferDirection::Download,
            items,
            target,
            overwrite,
        ),
        ConfirmAction::DeleteHost(id) => app.delete_bookmark(&id),
        ConfirmAction::DeleteCredential(id) => app.delete_profile(&id),
        ConfirmAction::TrustHostKey(prompt) => {
            app.trust_host_key(&prompt);
            let pending = app.pending_trust.take();
            // Retry every main session whose bookmark points at that address…
            let host_ids: Vec<String> = app
                .bookmarks
                .iter()
                .filter(|b| b.host == prompt.host && b.port == prompt.port)
                .map(|b| b.id.clone())
                .collect();
            for id in host_ids {
                app.reconnect_host_sessions(&id, None);
            }
            // …and the auxiliary terminal that triggered the prompt, if any.
            if let Some((backend_id, _)) = pending {
                if app.is_side_terminal(&backend_id) {
                    app.retry_side_terminal(&backend_id);
                }
            }
        }
    }
}

pub fn conflict_dialog(ui: &mut Ui, app: &mut AppState) {
    let Some(request) = app.conflict.clone() else {
        return;
    };
    let screen = ui.ctx().input(|i| i.viewport_rect());
    let width = 540.0_f32.min(screen.width() - 48.0);
    let height = measure_dialog_height(ui, width, &request.message);
    let layout = dialog_frame(ui, width, height);

    dialog_header(ui, &layout, crate::icons::WARNING, &request.title);
    dialog_body(ui, &layout, &request.message, &request.title);

    // 取消 / 跳过现有文件 / 全部覆盖 — right aligned, 8 px apart.
    let buttons_w = 320.0;
    let buttons = Rect::from_min_size(
        Pos2::new(layout.footer.right() - 20.0 - buttons_w, layout.footer.center().y - 15.0),
        Vec2::new(buttons_w, 30.0),
    );
    // A child Ui inherits the parent's (top-down) layout, which would stack the
    // buttons vertically — force a horizontal row.
    // A child Ui inherits the parent's (top-down) layout, which would stack the
    // buttons vertically — force a horizontal, right-aligned row.
    let labels = [("取消", true), ("跳过现有文件", true), ("全部覆盖", true)];
    let clicked = widgets::button_row(ui, buttons, &labels);

    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escape || clicked == Some(0) {
        app.conflict = None;
        return;
    }
    if clicked == Some(1) {
        app.conflict = None;
        start_conflict(app, &request, false);
    } else if clicked == Some(2) {
        app.conflict = None;
        start_conflict(app, &request, true);
    }
}

fn start_conflict(app: &mut AppState, request: &crate::state::ConflictRequest, overwrite: bool) {
    let session_id = app
        .active_tab()
        .and_then(|t| t.active())
        .map(|s| s.id.clone());
    if let Some(session_id) = session_id {
        app.start_transfer(
            &session_id,
            request.direction,
            request.items.clone(),
            request.target.clone(),
            overwrite,
        );
    }
}

// ── Login prompt ─────────────────────────────────────────────────────────────

pub fn login_dialog(ui: &mut Ui, app: &mut AppState) {
    let Some(mut prompt) = app.login_prompt.clone() else {
        return;
    };
    let screen = ui.ctx().input(|i| i.viewport_rect());
    let width = 440.0_f32.min(screen.width() - 48.0);
    let height = 340.0_f32.min(screen.height() - 80.0);
    let layout = dialog_frame(ui, width, height);
    dialog_header(ui, &layout, crate::icons::LOCK_KEY, &prompt.title);

    let body = layout.body;
    let painter = ui.painter().clone();
    painter.text(
        Pos2::new(body.left() + 2.0, body.top() + 10.0),
        Align2::LEFT_CENTER,
        &prompt.host,
        theme::font_mono(theme::TEXT_SM),
        theme::TEXT_MUTED,
    );

    // Username
    let user_label_y = body.top() + 40.0;
    painter.text(
        Pos2::new(body.left() + 2.0, user_label_y),
        Align2::LEFT_CENTER,
        "用户名",
        theme::font_sans(theme::TEXT_XS),
        theme::TEXT_SECONDARY,
    );
    let user_rect = Rect::from_min_size(
        Pos2::new(body.left(), user_label_y + 12.0),
        Vec2::new(body.width(), 32.0),
    );
    let mut user_ui = ui.new_child(egui::UiBuilder::new().max_rect(user_rect));
    let user_resp = widgets::text_input(
        &mut user_ui,
        &mut prompt.username,
        "root",
        user_rect.width(),
        false,
    );

    // Password
    let pw_label_y = user_label_y + 62.0;
    painter.text(
        Pos2::new(body.left() + 2.0, pw_label_y),
        Align2::LEFT_CENTER,
        "密码",
        theme::font_sans(theme::TEXT_XS),
        theme::TEXT_SECONDARY,
    );
    let pw_rect = Rect::from_min_size(
        Pos2::new(body.left(), pw_label_y + 12.0),
        Vec2::new(body.width(), 32.0),
    );
    let mut pw_ui = ui.new_child(egui::UiBuilder::new().max_rect(pw_rect));
    let pw_resp = widgets::text_input(
        &mut pw_ui,
        &mut prompt.password,
        "输入密码",
        pw_rect.width(),
        true,
    );

    if user_resp.changed() || pw_resp.changed() {
        app.login_prompt = Some(prompt.clone());
    }

    let buttons = Rect::from_min_size(
        Pos2::new(
            layout.footer.right() - 20.0 - 160.0,
            layout.footer.center().y - 15.0,
        ),
        Vec2::new(160.0, 30.0),
    );
    let labels = [("取消", true), ("连接", !prompt.username.trim().is_empty())];
    let clicked = widgets::button_row(ui, buttons, &labels);

    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

    if escape || clicked == Some(0) {
        // Cancelling removes the optimistic session tab.
        if let Some((session_id, tab_id)) = prompt.pending_session.clone() {
            if let Some(tab) = app.host_tab_mut(&tab_id) {
                tab.sessions.retain(|s| s.id != session_id);
                if tab.active_session.as_deref() == Some(session_id.as_str()) {
                    tab.active_session = tab.sessions.last().map(|s| s.id.clone());
                }
            }
        }
        app.login_prompt = None;
        return;
    }

    if enter || clicked == Some(1) {
        let username = prompt.username.trim().to_string();
        let password = prompt.password.clone();
        if let Some((session_id, _)) = prompt.pending_session.clone() {
            let bookmark_id = app
                .session_tab(&session_id)
                .map(|(_, s)| s.bookmark_id.clone())
                .unwrap_or_default();
            app.start_connect(&session_id, &bookmark_id, Some(username), Some(password));
        }
        app.login_prompt = None;
    }
}

// ── Paste confirmation ───────────────────────────────────────────────────────

pub fn paste_dialog(ui: &mut Ui, app: &mut AppState) {
    let Some(paste) = app.paste_confirm.clone() else {
        return;
    };
    let screen = ui.ctx().input(|i| i.viewport_rect());
    let width = 560.0_f32.min(screen.width() - 48.0);
    let height = 420.0_f32.min(screen.height() - 80.0);
    let layout = dialog_frame(ui, width, height);

    let line_count = paste.text.lines().count().max(1);
    let char_count = paste.text.chars().count();

    dialog_header(ui, &layout, crate::icons::CLIPBOARD_TEXT, "粘贴确认");

    // Character/line count on the right of the header row.
    ui.painter().text(
        Pos2::new(layout.header.right() - 20.0, layout.header.center().y),
        Align2::RIGHT_CENTER,
        format!("{line_count} 行 · {char_count} 字符"),
        theme::font_sans(theme::TEXT_XS - 1.0),
        theme::TEXT_MUTED,
    );

    // Preview: its own bordered box inside the body area.
    let preview_rect = layout.body;
    let painter = ui.painter().clone();
    painter.rect_filled(
        preview_rect,
        CornerRadius::same(theme::RADIUS_SM),
        Color32::from_rgba_premultiplied(8, 20, 38, 90),
    );
    painter.rect_stroke(
        preview_rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    let preview: String = paste.text.chars().take(2000).collect();
    let preview = if paste.text.chars().count() > 2000 {
        format!("{preview}\n...")
    } else {
        preview
    };
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(preview_rect.shrink(10.0))
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );
    egui::ScrollArea::vertical()
        .id_salt("paste-preview")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.label(
                egui::RichText::new(preview)
                    .size(theme::TEXT_XS)
                    .monospace()
                    .color(theme::TEXT_PRIMARY)
                    .line_height(Some(18.0)),
            );
        });

    let buttons = Rect::from_min_size(
        Pos2::new(
            layout.footer.right() - 20.0 - 160.0,
            layout.footer.center().y - 15.0,
        ),
        Vec2::new(160.0, 30.0),
    );
    let labels = [("取消", true), ("确认", true)];
    let clicked = widgets::button_row(ui, buttons, &labels);

    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));

    if escape || clicked == Some(0) {
        app.paste_confirm = None;
        return;
    }
    if enter || clicked == Some(1) {
        // Respect bracketed-paste mode (DECSET 2004), like xterm.js does.
        let bracketed = app
            .mgr
            .get(&paste.session_id)
            .map(|s| s.terminal.lock().screen().bracketed_paste())
            .unwrap_or(false);
        let data = if bracketed {
            format!("\x1b[200~{}\x1b[201~", paste.text)
        } else {
            paste.text.clone()
        };
        app.mgr.write(&paste.session_id, data.into_bytes());
        app.toast("粘贴成功", crate::session::ToastKind::Success);
        app.paste_confirm = None;
    }
}

/// Host-key confirmation reuses the generic confirm dialog but with the
/// dedicated action payload.
pub fn host_key_confirm(app: &mut AppState, prompt: crate::models::HostKeyVerificationPrompt) {
    app.confirm = Some(ConfirmRequest {
        title: "SSH 主机指纹确认".into(),
        message: prompt.message(),
        confirm_text: "信任并继续".into(),
        cancel_text: "取消".into(),
        action: ConfirmAction::TrustHostKey(Box::new(prompt)),
    });
}
