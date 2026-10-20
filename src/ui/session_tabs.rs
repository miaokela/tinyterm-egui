//! Session tab strip (`.session-tabstrip` + `.session-chrome-tab`).

use crate::models::SessionStatus;
use crate::state::{AppState, HostTab, ADD_SESSION_MIN_LOADING_MS};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};

const TAB_MIN_WIDTH: f32 = 96.0;
const TAB_MAX_WIDTH: f32 = 192.0;

pub fn show(ui: &mut Ui, app: &mut AppState, rect: Rect, tab: &HostTab) {
    let painter = ui.painter().clone();
    let cr = CornerRadius {
        nw: theme::RADIUS_PANEL,
        ne: theme::RADIUS_PANEL,
        sw: 0,
        se: 0,
    };
    painter.rect_filled(
        rect,
        cr,
        Color32::from_rgba_premultiplied(6, 17, 32, 163),
    );
    // No bottom edge: the terminal area below removes its top edge too, so the
    // two panels share one seamless seam instead of a double line.
    widgets::stroke_open(
        &painter,
        rect,
        theme::RADIUS_PANEL,
        Stroke::new(1.0, theme::BORDER),
        widgets::OpenSide::Bottom,
    );

    let side_btn_size = 28.0;
    let tabs_area = Rect::from_min_max(
        Pos2::new(rect.left() + 4.0, rect.top()),
        Pos2::new(rect.right() - 4.0 - side_btn_size - 6.0, rect.bottom()),
    );

    // ── Tabs ─────────────────────────────────────────────────────────────────
    let session_ids: Vec<String> = tab.sessions.iter().map(|s| s.id.clone()).collect();
    let has_host = !tab.host_id.is_empty();
    let adding = app
        .adding_session
        .get(&tab.id)
        .map(|until| *until > crate::state::now_ms())
        .unwrap_or(false);

    let x = tabs_area.left();
    let mut scroll = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(tabs_area)
            .layout(egui::Layout::left_to_right(egui::Align::Max)),
    );
    egui::ScrollArea::horizontal()
        .id_salt(("session-tabs", &tab.id))
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(6.0);
            for (index, sid) in session_ids.iter().enumerate() {
                let Some(session) = tab.session(sid) else {
                    continue;
                };
                let title = if session.title.is_empty() {
                    format!("终端 {}", index + 1)
                } else {
                    session.title.clone()
                };
                let active = tab.active_session.as_deref() == Some(sid.as_str());
                let is_new = crate::state::now_ms() - session.created_at_ms < 1600.0;
                if chrome_tab(ui, app, &tab.id, sid, &title, active, session.status, is_new) {
                    // handled inside
                }
            }
            if has_host {
                let (btn_rect, r) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                let painter = ui.painter();
                if adding {
                    painter.rect_filled(
                        btn_rect,
                        CornerRadius::same(theme::RADIUS_SM),
                        Color32::from_rgba_premultiplied(28, 49, 84, 150),
                    );
                    widgets::spinner(
                        painter,
                        btn_rect.center(),
                        7.0,
                        ui.input(|i| i.time) as f32,
                        theme::TEXT_PRIMARY,
                    );
                } else {
                    if r.hovered() {
                        painter.rect_filled(
                            btn_rect,
                            CornerRadius::same(theme::RADIUS_SM),
                            Color32::from_rgba_premultiplied(22, 49, 96, 120),
                        );
                    }
                    widgets::plus(
                        painter,
                        btn_rect.center(),
                        12.0,
                        if r.hovered() {
                            theme::TEXT_PRIMARY
                        } else {
                            Color32::from_rgba_premultiplied(105, 130, 156, 184)
                        },
                    );
                }
                if r.clicked() && !adding {
                    add_session(app, &tab.id);
                }
            }
            ui.add_space(4.0);
        });

    // ── Side-terminal toggle ─────────────────────────────────────────────────
    if !tab.sessions.is_empty() {
        let active_session = tab.active();
        let side_open = active_session.map(|s| s.side_terminal_open).unwrap_or(false);
        let busy = active_session
            .map(|s| s.side_terminal_status == SessionStatus::Connecting)
            .unwrap_or(false);
        let btn_rect = Rect::from_center_size(
            Pos2::new(rect.right() - 4.0 - side_btn_size * 0.5, rect.center().y),
            Vec2::splat(side_btn_size),
        );
        let r = ui.interact(btn_rect, ui.id().with(("side-term", &tab.id)), Sense::click());
        let painter = ui.painter();
        if r.hovered() {
            painter.rect_filled(
                btn_rect,
                CornerRadius::same(theme::RADIUS_SM),
                Color32::from_rgba_premultiplied(22, 49, 96, 120),
            );
        }
        let color = if busy {
            theme::TEXT_PRIMARY
        } else if side_open {
            Color32::from_rgba_premultiplied(85, 170, 119, 217)
        } else if r.hovered() {
            theme::TEXT_PRIMARY
        } else {
            Color32::from_rgba_premultiplied(107, 137, 168, 179)
        };
        if busy {
            widgets::spinner(
                painter,
                btn_rect.center(),
                7.0,
                ui.input(|i| i.time) as f32,
                color,
            );
        } else {
            widgets::icon(painter, btn_rect.center(), 14.0, crate::icons::COLUMNS, color);
        }
        if r.clicked() {
            if let Some(s) = tab.active() {
                let sid = s.id.clone();
                app.toggle_side_terminal(&sid);
            }
        }
    }
    let _ = x;
}

/// Returns `true` when the tab was closed.
fn chrome_tab(
    ui: &mut Ui,
    app: &mut AppState,
    host_tab_id: &str,
    session_id: &str,
    title: &str,
    active: bool,
    status: SessionStatus,
    is_new: bool,
) -> bool {
    let font = theme::f_sm();
    let title_w = ui
        .painter()
        .layout_no_wrap(title.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let width = (title_w + 48.0).clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, theme::CHROME_TAB_HEIGHT + 5.0),
        Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return false;
    }
    // Lift the tab 1 px off the strip's bottom edge so it never sits exactly on
    // the seam with the terminal area below.
    let tab_rect = Rect::from_min_size(
        Pos2::new(
            rect.left(),
            rect.bottom() - theme::CHROME_TAB_HEIGHT - 1.0,
        ),
        Vec2::new(rect.width(), theme::CHROME_TAB_HEIGHT),
    );
    let cr = CornerRadius {
        nw: theme::RADIUS_SM,
        ne: theme::RADIUS_SM,
        sw: 0,
        se: 0,
    };

    let painter = ui.painter();
    let hovered = response.hovered();
    let disconnected = status == SessionStatus::Disconnected;
    let bg = if active {
        if disconnected {
            Color32::from_rgba_premultiplied(59, 59, 71, 235)
        } else {
            // A blue-tinted glass fill: distinct from the strip and from every
            // hovered/inactive tab, but still quiet enough for the cosmic theme.
            Color32::from_rgba_premultiplied(17, 49, 96, 240)
        }
    } else if disconnected {
        Color32::from_rgba_premultiplied(76, 76, 88, 40)
    } else if hovered {
        Color32::from_rgba_premultiplied(22, 49, 96, 120)
    } else {
        Color32::from_rgba_premultiplied(10, 18, 30, 26)
    };
    let alpha = if is_new {
        let age = crate::state::now_ms()
            - app
                .session_tab(session_id)
                .map(|(_, s)| s.created_at_ms)
                .unwrap_or(0.0);
        ((age / 1600.0).clamp(0.0, 1.0) * 0.6 + 0.4) as f32
    } else {
        1.0
    };
    painter.rect_filled(tab_rect, cr, bg.gamma_multiply(alpha));

    if active {
        // A single 1 px accent contour: across the rounded top and straight
        // down both sides. The top bar and the side borders are now one
        // continuous line, so the corners have no thick cap or visible seam.
        widgets::stroke_open(
            painter,
            tab_rect,
            theme::RADIUS_SM,
            Stroke::new(1.0, theme::ACCENT_HOVER.gamma_multiply(alpha)),
            widgets::OpenSide::Bottom,
        );
    }

    // Status dot
    let dot_center = Pos2::new(tab_rect.left() + 12.0, tab_rect.center().y);
    let dot_color = if disconnected {
        Color32::from_rgba_premultiplied(112, 112, 121, 191)
    } else {
        status.dot_color()
    };
    let pulse = if status == SessionStatus::Connecting {
        0.35 + 0.65 * ((ui.input(|i| i.time) * 6.2831).sin() * 0.5 + 0.5) as f32
    } else {
        1.0
    };
    // Inactive tabs keep a readable but recessed status dot; the active one is
    // larger, brighter and gets the connected halo.
    let dot_radius = if active { 3.5 } else { 2.5 };
    let dot_alpha = if active { 1.0 } else { 0.6 };
    painter.circle_filled(
        dot_center,
        dot_radius,
        dot_color.gamma_multiply(pulse * dot_alpha),
    );
    if status == SessionStatus::Connected && active {
        painter.circle_stroke(
            dot_center,
            5.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(87, 227, 165, 90)),
        );
    }

    // Title
    let text_left = dot_center.x + 10.0;
    let text_right = tab_rect.right() - 22.0;
    let label = theme::truncate(painter, title, &font, (text_right - text_left).max(10.0));
    let text_color = if active {
        if disconnected {
            Color32::from_rgba_premultiplied(220, 220, 228, 245)
        } else {
            Color32::WHITE
        }
    } else if disconnected {
        Color32::from_rgba_premultiplied(126, 126, 138, 168)
    } else if hovered {
        Color32::from_rgba_premultiplied(201, 216, 233, 220)
    } else {
        Color32::from_rgba_premultiplied(124, 151, 178, 155)
    };
    painter.text(
        Pos2::new(text_left, tab_rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        text_color.gamma_multiply(alpha),
    );

    // Close button
    let close_rect = Rect::from_center_size(
        Pos2::new(tab_rect.right() - 11.0, tab_rect.center().y),
        Vec2::splat(16.0),
    );
    let close = ui.interact(
        close_rect,
        ui.id().with(("session-close", session_id)),
        Sense::click(),
    );
    let opacity = if close.hovered() {
        1.0
    } else if active {
        0.9
    } else if hovered {
        0.65
    } else {
        0.22
    };
    widgets::cross(
        painter,
        close_rect.center(),
        9.0,
        Color32::from_rgba_unmultiplied(255, 255, 255, (opacity * 255.0) as u8),
    );
    if close.clicked() {
        app.close_session(host_tab_id, session_id);
        return true;
    }

    if response.clicked() {
        app.set_active_session(host_tab_id, session_id);
    }
    false
}

/// `handleAddSession` — shows a spinner for at least `ADD_SESSION_MIN_LOADING_MS`.
pub fn add_session(app: &mut AppState, host_tab_id: &str) {
    if app
        .adding_session
        .get(host_tab_id)
        .map(|until| *until > crate::state::now_ms())
        .unwrap_or(false)
    {
        return;
    }
    let host_id = match app.host_tab(host_tab_id) {
        Some(t) => t.host_id.clone(),
        None => return,
    };
    app.adding_session
        .insert(host_tab_id.to_string(), crate::state::now_ms() + ADD_SESSION_MIN_LOADING_MS);
    app.open_session(&host_id, Some(host_tab_id.to_string()));
}
