//! Terminal pane: grid rendering, keyboard input, selection, scroll,
//! context menu, quick-actions toolbar and status overlays.

use crate::models::SessionStatus;
use crate::state::{AppState, PasteConfirm};
use crate::term::{self, CellMetrics};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

pub const TERMINAL_PADDING: f32 = 8.0;

/// Everything the terminal pane needs to know about the session it renders.
///
/// `backend_id` is the SSH session id (the tab's own id for the main terminal,
/// the auxiliary session id for the side terminal), while `status`/`error`
/// describe *that* backend session.
pub struct TerminalPane<'a> {
    pub backend_id: &'a str,
    pub status: SessionStatus,
    pub error: Option<&'a str>,
}

pub fn show(ui: &mut Ui, app: &mut AppState, rect: Rect, pane: TerminalPane<'_>) {
    let session_id = pane.backend_id.to_string();
    let status = pane.status;

    let inner = rect.shrink(TERMINAL_PADDING);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_XS), theme::TERMINAL_BG);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_XS),
        Stroke::new(1.0, Color32::from_rgba_premultiplied(15, 26, 34, 36)),
        StrokeKind::Inside,
    );

    match status {
        SessionStatus::Connected => {
            render_connected(ui, app, &session_id, inner);
        }
        SessionStatus::Connecting => {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
            child.vertical_centered(|ui| {
                ui.add_space((inner.height() * 0.5 - 20.0).max(0.0));
                widgets::loading_blocks(ui, ui.input(|i| i.time) as f32, 1.6);
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("正在连接...")
                        .size(theme::TEXT_SM)
                        .color(theme::WARNING),
                );
            });
        }
        SessionStatus::Error | SessionStatus::Disconnected => {
            render_status_overlay(ui, app, &session_id, inner, status, pane.error);
        }
    }

    // Quick-actions toolbar (only while connected).
    if status == SessionStatus::Connected {
        crate::ui::quick_actions::show(ui, app, &session_id, rect);
    }
}

fn render_connected(ui: &mut Ui, app: &mut AppState, session_id: &str, rect: Rect) {
    let Some(live) = app.mgr.get(session_id) else {
        // The backend session vanished — show a hint rather than a blank pane.
        let painter = ui.painter();
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "会话已结束",
            theme::f_sm(),
            theme::TEXT_MUTED,
        );
        return;
    };

    let font_size = app.settings.font_size.max(6) as f32;
    let metrics = CellMetrics::measure(ui.ctx(), font_size);
    let (cols, rows) = term::Terminal::fit(rect, metrics);

    // ── Resize the emulator to match the pane ────────────────────────────────
    {
        let mut t = live.terminal.lock();
        if t.size() != (rows, cols) {
            t.resize(rows, cols);
        }
        t.apply_scroll();
    }

    let needs_resize = {
        let ui_state = app.terminal_ui_mut(session_id);
        if ui_state.last_size != (rows, cols) {
            ui_state.last_size = (rows, cols);
            true
        } else {
            false
        }
    };
    if needs_resize {
        app.mgr.resize(session_id, cols, rows);
        if let Some(s) = app.session_tab_mut(session_id) {
            s.cols = cols;
            s.rows = rows;
        }
    }

    // ── Painting ─────────────────────────────────────────────────────────────
    let blink = app.settings.cursor_blink;
    let cursor_style = term::CursorStyle::from_setting(&app.settings.cursor_style);
    let t_secs = ui.input(|i| i.time);
    let cursor_on = !blink || (t_secs * 1.6).fract() < 0.6;
    let selection = app.terminal_ui_mut(session_id).selection;
    {
        let t = live.terminal.lock();
        term::paint(
            ui.painter(),
            rect,
            &t,
            font_size,
            metrics,
            cursor_style,
            cursor_on,
            selection,
        );
    }

    // ── Interaction ──────────────────────────────────────────────────────────
    let response = ui.interact(rect, ui.id().with(("term", session_id)), Sense::click_and_drag());
    let ctx = ui.ctx().clone();

    // Focus handling: the terminal takes focus whenever nothing else owns it,
    // so typing works immediately after switching tabs — but never steals it
    // from a modal's text field.
    if response.clicked() {
        ctx.memory_mut(|m| m.request_focus(response.id));
        // A plain click (not a drag) drops the previous selection.
        app.terminal_ui_mut(session_id).selection = term::Selection::default();
    }
    let mut has_focus = ctx.memory(|m| m.has_focus(response.id));
    if !has_focus
        && ctx.memory(|m| m.focused().is_none())
        && app.modal == crate::state::ModalKind::None
        && app.paste_confirm.is_none()
        && app.login_prompt.is_none()
        && app.confirm.is_none()
        && app.conflict.is_none()
    {
        ctx.memory_mut(|m| m.request_focus(response.id));
        has_focus = true;
    }

    // While the terminal owns the keyboard it must keep every keystroke.
    // egui's focus navigation otherwise treats Tab / arrow keys / Escape as
    // "move focus to the next widget": focus jumps out of the terminal into
    // the surrounding UI and typing stops reaching the shell. The lock filter
    // keeps those keys inside the terminal (they are still delivered as normal
    // events, so `encode_key` sends \t, \x1b[A, \x1b, … to the session).
    if has_focus {
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }

    // ── Mouse reporting (TUI apps such as htop / vim) ────────────────────────
    let (mouse_mode, mouse_sgr, _bracketed_paste) = {
        let t = live.terminal.lock();
        let screen = t.screen();
        (
            screen.mouse_protocol_mode(),
            matches!(
                screen.mouse_protocol_encoding(),
                vt100::MouseProtocolEncoding::Sgr
            ),
            screen.bracketed_paste(),
        )
    };
    let mouse_reporting = mouse_mode != vt100::MouseProtocolMode::None
        && !ctx.input(|i| i.modifiers.shift);
    if mouse_reporting {
        let mut reports: Vec<String> = Vec::new();
        let mods = ctx.input(|i| i.modifiers);
        let mod_bits = (if mods.shift { 4 } else { 0 })
            + (if mods.alt { 8 } else { 0 })
            + (if mods.command || mods.ctrl { 16 } else { 0 });
        let cell_of = |pos: Pos2| term::Terminal::cell_at(rect, metrics, pos, cols, rows);
        if let Some(pos) = response.interact_pointer_pos() {
            let (c, r) = cell_of(pos);
            let motion = response.dragged();
            if response.clicked() || response.drag_started() {
                reports.push(mouse_seq(0 + mod_bits, c, r, false, motion, mouse_sgr));
            }
            if response.drag_stopped() || response.secondary_clicked() {
                reports.push(mouse_seq(0 + mod_bits, c, r, true, motion, mouse_sgr));
            } else if response.dragged() {
                reports.push(mouse_seq(0 + mod_bits, c, r, false, true, mouse_sgr));
            }
        }
        if response.hovered() {
            let wheel = ctx.input(|i| i.smooth_scroll_delta.y);
            if wheel.abs() > 0.5 {
                let (c, r) = ctx
                    .input(|i| i.pointer.hover_pos())
                    .map(cell_of)
                    .unwrap_or((0, 0));
                let button = if wheel > 0.0 { 64 } else { 65 };
                let times = (wheel.abs() / metrics.height).ceil().clamp(1.0, 5.0) as usize;
                for _ in 0..times {
                    reports.push(mouse_seq(button + mod_bits, c, r, false, false, mouse_sgr));
                }
            }
        }
        if !reports.is_empty() {
            app.mgr.write(session_id, reports.concat().into_bytes());
        }
    }

    // Selection drag (disabled while the app owns the mouse, unless Shift)
    if response.drag_started() && !mouse_reporting {
        if let Some(pos) = response.interact_pointer_pos() {
            let (c, r) = term::Terminal::cell_at(rect, metrics, pos, cols, rows);
            let ui_state = app.terminal_ui_mut(session_id);
            ui_state.selection = term::Selection {
                start: (r, c),
                end: (r, c),
                active: true,
            };
            ui_state.dragging = true;
        }
    }
    if response.dragged() && !mouse_reporting && app.terminal_ui_mut(session_id).dragging {
        if let Some(pos) = response.interact_pointer_pos() {
            let (c, r) = term::Terminal::cell_at(rect, metrics, pos, cols, rows);
            let ui_state = app.terminal_ui_mut(session_id);
            ui_state.selection.end = (r, c);
        }
    }
    if response.drag_stopped() {
        app.terminal_ui_mut(session_id).dragging = false;
    }

    // Right click → context menu
    if response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            app.terminal_ui_mut(session_id).context_menu = Some(pos);
        }
    }

    // Scroll wheel → scrollback
    if response.hovered() {
        let delta = ctx.input(|i| i.smooth_scroll_delta.y);
        if delta.abs() > 0.5 {
            let lines = (delta / metrics.height).round() as i32;
            if lines != 0 {
                live.terminal.lock().scroll_by(lines);
            }
        }
    }

    // ── Keyboard input ───────────────────────────────────────────────────────
    if has_focus {
        let app_cursor = live.terminal.lock().screen().application_cursor();
        let events = ctx.input(|i| i.events.clone());
        let mut to_send: Vec<String> = Vec::new();
        let mut paste: Option<String> = None;

        for event in events {
            match event {
                egui::Event::Text(text) => {
                    let mods = ctx.input(|i| i.modifiers);
                    if mods.command || mods.alt {
                        continue;
                    }
                    if !text.is_empty() {
                        to_send.push(text);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: _,
                    modifiers,
                    ..
                } => {
                    if modifiers.command {
                        // Cmd+C / Cmd+V are handled below.
                        continue;
                    }
                    if let Some(bytes) = term::encode_key(
                        Some(key),
                        "",
                        modifiers.ctrl,
                        modifiers.alt,
                        modifiers.shift,
                        app_cursor,
                    ) {
                        // Plain printable keys are delivered through Event::Text.
                        let is_plain = !modifiers.ctrl
                            && !modifiers.alt
                            && is_plain_key(key);
                        if !is_plain {
                            to_send.push(bytes);
                        }
                    } else if modifiers.alt {
                        if let Some(ch) = alt_char(key) {
                            to_send.push(format!("\x1b{ch}"));
                        }
                    }
                }
                egui::Event::Paste(text) => {
                    paste = Some(text);
                }
                egui::Event::Copy => {
                    let selection = app.terminal_ui_mut(session_id).selection;
                    let text = live.terminal.lock().selected_text(selection);
                    if !text.is_empty() {
                        ctx.copy_text(text);
                        app.toast("复制成功", crate::session::ToastKind::Success);
                    }
                }
                _ => {}
            }
        }

        // Cmd+C / Cmd+V
        let (cmd_c, cmd_v) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::C),
                i.modifiers.command && i.key_pressed(egui::Key::V),
            )
        });
        if cmd_c {
            let selection = app.terminal_ui_mut(session_id).selection;
            let text = live.terminal.lock().selected_text(selection);
            if !text.is_empty() {
                ctx.copy_text(text);
                app.toast("复制成功", crate::session::ToastKind::Success);
            }
        }
        if cmd_v {
            if let Ok(text) = read_clipboard(&ctx) {
                if !text.is_empty() {
                    paste = Some(text);
                }
            }
        }

        if !to_send.is_empty() {
            let data = to_send.concat();
            app.mgr.write(session_id, data.into_bytes());
            live.terminal.lock().scroll_to_bottom();
        }
        if let Some(text) = paste {
            app.paste_confirm = Some(PasteConfirm {
                text,
                session_id: session_id.to_string(),
            });
        }
    }

    // ── Context menu ─────────────────────────────────────────────────────────
    if let Some(pos) = app.terminal_ui_mut(session_id).context_menu {
        let has_selection = {
            let selection = app.terminal_ui_mut(session_id).selection;
            !live.terminal.lock().selected_text(selection).is_empty()
        };
        let menu_rect = Rect::from_min_size(pos, Vec2::new(140.0, 74.0));
        let screen = ctx.input(|i| i.viewport_rect());
        let mut menu_rect = menu_rect;
        if menu_rect.right() > screen.right() {
            menu_rect = menu_rect.translate(Vec2::new(screen.right() - menu_rect.right() - 4.0, 0.0));
        }
        if menu_rect.bottom() > screen.bottom() {
            menu_rect = menu_rect.translate(Vec2::new(0.0, screen.bottom() - menu_rect.bottom() - 4.0));
        }

        let mut close = false;
        let painter = ui.painter().clone();
        painter.rect_filled(
            menu_rect,
            CornerRadius::same(theme::RADIUS_SM),
            Color32::from_rgba_premultiplied(11, 26, 49, 245),
        );
        painter.rect_stroke(
            menu_rect,
            CornerRadius::same(theme::RADIUS_SM),
            Stroke::new(1.0, Color32::from_rgba_premultiplied(38, 61, 97, 89)),
            StrokeKind::Inside,
        );

        let items = [("复制", has_selection), ("粘贴", true)];
        for (i, (label, enabled)) in items.iter().enumerate() {
            let item_rect = Rect::from_min_size(
                Pos2::new(menu_rect.left() + 6.0, menu_rect.top() + 6.0 + i as f32 * 31.0),
                Vec2::new(menu_rect.width() - 12.0, 28.0),
            );
            let r = ui.interact(
                item_rect,
                ui.id().with(("term-menu", session_id, i)),
                if *enabled { Sense::click() } else { Sense::hover() },
            );
            if r.hovered() && *enabled {
                ui.painter().rect_filled(
                    item_rect,
                    CornerRadius::same(theme::RADIUS_XS),
                    Color32::from_rgba_premultiplied(28, 49, 84, 160),
                );
            }
            ui.painter().text(
                Pos2::new(item_rect.left() + 10.0, item_rect.center().y),
                Align2::LEFT_CENTER,
                *label,
                theme::f_xs(),
                if *enabled {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_MUTED
                },
            );
            if r.clicked() && *enabled {
                close = true;
                if i == 0 {
                    let selection = app.terminal_ui_mut(session_id).selection;
                    let text = live.terminal.lock().selected_text(selection);
                    if !text.is_empty() {
                        ctx.copy_text(text);
                        app.toast("复制成功", crate::session::ToastKind::Success);
                    }
                } else {
                    match read_clipboard(&ctx) {
                        Ok(text) if !text.is_empty() => {
                            app.paste_confirm = Some(PasteConfirm {
                                text,
                                session_id: session_id.to_string(),
                            });
                        }
                        _ => app.toast("剪贴板为空", crate::session::ToastKind::Info),
                    }
                }
            }
        }

        // Click anywhere else closes the menu.
        if ctx.input(|i| i.pointer.any_click()) && !menu_rect.contains(
            ctx.input(|i| i.pointer.interact_pos().unwrap_or(Pos2::ZERO)),
        ) {
            close = true;
        }
        if close {
            app.terminal_ui_mut(session_id).context_menu = None;
        }
    }
}

fn alt_char(key: egui::Key) -> Option<char> {
    use egui::Key::*;
    Some(match key {
        A => 'a',
        B => 'b',
        C => 'c',
        D => 'd',
        E => 'e',
        F => 'f',
        G => 'g',
        H => 'h',
        I => 'i',
        J => 'j',
        K => 'k',
        L => 'l',
        M => 'm',
        N => 'n',
        O => 'o',
        P => 'p',
        Q => 'q',
        R => 'r',
        S => 's',
        T => 't',
        U => 'u',
        V => 'v',
        W => 'w',
        X => 'x',
        Y => 'y',
        Z => 'z',
        Num0 => '0',
        Num1 => '1',
        Num2 => '2',
        Num3 => '3',
        Num4 => '4',
        Num5 => '5',
        Num6 => '6',
        Num7 => '7',
        Num8 => '8',
        Num9 => '9',
        _ => return None,
    })
}

/// Encode one mouse event: SGR (`\x1b[<b;x;yM/m`) or legacy X10.
fn mouse_seq(button: u8, col: u16, row: u16, release: bool, motion: bool, sgr: bool) -> String {
    let mut b = button;
    if motion {
        b += 32;
    }
    let x = col as u32 + 1;
    let y = row as u32 + 1;
    if sgr {
        let final_byte = if release { 'm' } else { 'M' };
        format!("\x1b[<{b};{x};{y}{final_byte}")
    } else {
        let cb = if release { 3 } else { b } + 32;
        format!("\x1b[M{}{}{}", (cb as char), (32 + x) as u8 as char, (32 + y) as u8 as char)
    }
}

/// True for keys that produce printable text via `Event::Text`.
fn is_plain_key(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(
        key,
        A | B | C | D | E | F | G | H | I | J | K | L | M | N | O | P | Q | R | S | T | U | V
            | W | X | Y | Z
            | Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9
            | Space | Minus | Equals | Plus | Period | Comma | Semicolon | Quote | Slash
            | Backslash | OpenBracket | CloseBracket | Backtick
    )
}

/// Read the clipboard.
///
/// Prefers the `Paste` event egui already delivered this frame (keyboard
/// paste), and otherwise reads the OS clipboard directly so the right-click
/// menu's "粘贴" works too.
fn read_clipboard(ctx: &egui::Context) -> Result<String, ()> {
    if let Some(text) = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Paste(t) => Some(t.clone()),
            _ => None,
        })
    }) {
        return Ok(text);
    }
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .map_err(|_| ())
}

fn render_status_overlay(
    ui: &mut Ui,
    app: &mut AppState,
    session_id: &str,
    rect: Rect,
    status: SessionStatus,
    error: Option<&str>,
) {
    let is_error = status == SessionStatus::Error;
    let error_text = error
        .map(|s| s.to_string())
        .or_else(|| app.session_tab(session_id).and_then(|(_, s)| s.error.clone()))
        .unwrap_or_default();
    let is_auth_error = is_error
        && {
            let lower = error_text.to_lowercase();
            lower.contains("auth")
                || lower.contains("password")
                || lower.contains("credential")
                || lower.contains("permission denied")
        };

    let bg = if is_error {
        Color32::from_rgba_premultiplied(7, 5, 16, 232)
    } else {
        Color32::from_rgba_premultiplied(9, 11, 16, 200)
    };
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_MD), bg);

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(24.0))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.add_space((rect.height() * 0.5 - 70.0).max(0.0));
    child.label(
        egui::RichText::new(if is_error { "⚠ 连接失败" } else { "⚠ 连接已断开" })
            .size(theme::TEXT_LG)
            .strong()
            .color(if is_error {
                theme::ERROR
            } else {
                theme::TEXT_PRIMARY
            }),
    );
    child.add_space(6.0);
    child.label(
        egui::RichText::new(theme::truncate(
            &child.painter(),
            &error_text,
            &theme::font_mono(theme::TEXT_XS),
            rect.width() - 60.0,
        ))
        .size(theme::TEXT_XS)
        .monospace()
        .color(theme::TEXT_MUTED),
    );

    if is_auth_error {
        child.add_space(10.0);
        let mut password = app
            .reconnect_passwords
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        let response = widgets::text_input(&mut child, &mut password, "输入密码重试...", 200.0, true);
        if response.changed() {
            app.reconnect_passwords
                .insert(session_id.to_string(), password.clone());
        }
    }

    child.add_space(12.0);
    if widgets::ghost_button(&mut child, "↺ 重新连接", true).clicked() {
        let password = app.reconnect_passwords.get(session_id).cloned();
        app.reconnect_session(session_id, password.as_deref());
    }
}

