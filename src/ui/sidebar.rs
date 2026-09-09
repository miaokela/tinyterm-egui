//! Left host sidebar — `.host-sidebar` + `.host-sidebar-tab`.

use crate::models::{HostReachability, SessionStatus};
use crate::state::{now_ms, AppState};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

pub const TAB_HEIGHT: f32 = 32.0;

pub fn show(ui: &mut Ui, app: &mut AppState, rect: Rect) {
    let painter = ui.painter().clone();
    theme::glass_panel(&painter, rect, theme::RADIUS_LG);

    let collapsed = app.sidebar_collapsed;

    // `.host-sidebar-inner { padding: 10px 6px 6px; }`
    let content = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 10.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
    );

    // Bottom stack, matching `.host-sidebar-add` / `.host-sidebar-add-icon`:
    //   expanded  — full-width 28px dashed button flush with the content box;
    //   collapsed — 32px dashed icon with `margin-bottom: 36px` so it clears
    //               the centred collapse chevron.
    let add_h = if collapsed { 32.0 } else { 28.0 };
    let add_margin_bottom = if collapsed { 36.0 } else { 0.0 };
    let add_top = content.bottom() - add_margin_bottom - add_h;
    let tabs_rect = Rect::from_min_max(
        content.min,
        Pos2::new(content.right(), add_top - 6.0),
    );

    // ── Host tab list (`.host-sidebar-tabs`, gap 3px) ────────────────────────
    if app.host_tabs.is_empty() {
        painter.text(
            tabs_rect.center(),
            Align2::CENTER_CENTER,
            if collapsed { "◍" } else { "无主机" },
            theme::f_xs(),
            theme::TEXT_MUTED,
        );
    } else {
        let mut scroll = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(tabs_rect)
                .layout(egui::Layout::top_down(egui::Align::LEFT)),
        );
        egui::ScrollArea::vertical()
            .id_salt("host-tabs")
            .auto_shrink([false, false])
            .show(&mut scroll, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                let ids: Vec<String> = app.host_tabs.iter().map(|t| t.id.clone()).collect();
                for (index, tab_id) in ids.iter().enumerate() {
                    host_tab_row(ui, app, tab_id, index, collapsed);
                }
            });
    }

    // ── "主机管理" button ────────────────────────────────────────────────────
    let btn_rect = if collapsed {
        // Centre the 32 px icon in the collapsed rail instead of pinning it to
        // the 6 px content padding.
        Rect::from_min_size(
            Pos2::new(rect.center().x - 16.0, add_top),
            Vec2::new(32.0, 32.0),
        )
    } else {
        // Leave room for the collapse chevron so the two share one row and one
        // centre line instead of overlapping (the chevron used to sit at the
        // top-right corner of this button).
        Rect::from_min_size(
            Pos2::new(content.left(), add_top),
            Vec2::new(content.width() - 30.0, 28.0),
        )
    };
    let r = ui.interact(btn_rect, ui.id().with("hosts-modal-btn"), Sense::click());
    let painter = ui.painter();
    if r.hovered() {
        painter.rect_filled(
            btn_rect,
            CornerRadius::same(theme::RADIUS_SM),
            Color32::from_rgba_premultiplied(22, 49, 96, 110),
        );
    }
    painter.rect_stroke(
        btn_rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(
            1.0,
            if r.hovered() {
                theme::BORDER_ACTIVE
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );
    let fg = if r.hovered() {
        Color32::from_rgba_premultiplied(200, 210, 255, 230)
    } else {
        Color32::from_rgba_premultiplied(95, 121, 148, 153)
    };
    if collapsed {
        widgets::gear(painter, btn_rect.center(), 7.0, fg);
    } else {
        widgets::gear(painter, Pos2::new(btn_rect.left() + 10.0 + 6.5, btn_rect.center().y), 6.5, fg);
        painter.text(
            Pos2::new(btn_rect.left() + 10.0 + 13.0 + 6.0, btn_rect.center().y),
            Align2::LEFT_CENTER,
            "主机管理",
            theme::f_xs(),
            fg,
        );
    }
    if r.clicked() {
        app.modal = crate::state::ModalKind::Hosts;
    }

    // ── Collapse chevron (`.sidebar-collapse-btn`, bottom 10px) ──────────────
    let collapse_rect = if collapsed {
        Rect::from_center_size(
            Pos2::new(rect.center().x, rect.bottom() - 22.0),
            Vec2::splat(24.0),
        )
    } else {
        // Vertically centred on the "主机管理" row.
        Rect::from_center_size(
            Pos2::new(content.right() - 12.0, add_top + 14.0),
            Vec2::splat(24.0),
        )
    };
    let r = ui.interact(collapse_rect, ui.id().with("sidebar-collapse"), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(
        collapse_rect,
        CornerRadius::same(theme::RADIUS_SM),
        if r.hovered() {
            Color32::from_rgba_premultiplied(41, 82, 133, 200)
        } else {
            Color32::from_rgba_premultiplied(23, 44, 77, 120)
        },
    );
    painter.rect_stroke(
        collapse_rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );
    widgets::chevron(
        painter,
        collapse_rect.center(),
        9.0,
        !collapsed,
        if r.hovered() {
            Color32::WHITE
        } else {
            Color32::from_rgba_premultiplied(125, 112, 161, 180)
        },
    );
    if r.clicked() {
        app.sidebar_collapsed = !collapsed;
    }
}

fn host_tab_row(ui: &mut Ui, app: &mut AppState, tab_id: &str, index: usize, collapsed: bool) {
    let Some(tab) = app.host_tab(tab_id) else {
        return;
    };
    let active = app.active_host_tab.as_deref() == Some(tab_id);
    let status = tab.active().map(|s| s.status).unwrap_or(SessionStatus::Disconnected);
    let host_id = tab.host_id.clone();
    let title = tab.title.clone();
    let unreachable = app
        .host_reachability
        .get(&host_id)
        .map(|r| *r == HostReachability::Unreachable)
        .unwrap_or(false);
    let flashing = app.host_probe_flash.contains_key(&host_id);
    let accent = app
        .bookmark(&host_id)
        .map(|b| b.accent())
        .unwrap_or(Color32::from_rgb(0x7c, 0x5c, 0xbf));

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), TAB_HEIGHT),
        Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter();
    let hovered = response.hovered();
    if active {
        painter.rect_filled(
            rect,
            CornerRadius::same(theme::RADIUS_SM),
            Color32::from_rgba_premultiplied(28, 49, 84, 190),
        );
    } else if hovered {
        painter.rect_filled(
            rect,
            CornerRadius::same(theme::RADIUS_SM),
            Color32::from_rgba_premultiplied(22, 49, 96, 120),
        );
    }

    // Left accent bar on the active tab.
    if active {
        let bar = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + rect.height() * 0.14),
            Vec2::new(4.0, rect.height() * 0.72),
        );
        painter.rect_filled(
            bar,
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: theme::RADIUS_XS,
                se: theme::RADIUS_XS,
            },
            accent,
        );
        for i in 1..=3 {
            painter.rect_stroke(
                bar.expand(i as f32 * 0.6),
                CornerRadius::same(theme::RADIUS_XS),
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 40 / i),
                ),
                StrokeKind::Outside,
            );
        }
    }

    // Number dot
    let dot_size = 18.0;
    let dot_center = if collapsed {
        rect.center()
    } else {
        Pos2::new(rect.left() + 12.0 + dot_size * 0.5, rect.center().y)
    };
    let dot_color = if unreachable {
        Color32::from_rgba_premultiplied(124, 124, 133, 204)
    } else {
        match status {
            SessionStatus::Connected => theme::SUCCESS,
            SessionStatus::Connecting => theme::WARNING,
            SessionStatus::Error => theme::ERROR,
            SessionStatus::Disconnected => Color32::from_rgba_premultiplied(107, 90, 148, 115),
        }
    };
    let pulse = if status == SessionStatus::Connecting {
        0.45 + 0.55 * ((ui.input(|i| i.time) * 6.2831).sin() * 0.5 + 0.5) as f32
    } else {
        1.0
    };
    let dot_rect = Rect::from_center_size(dot_center, Vec2::splat(dot_size));
    painter.rect_filled(
        dot_rect,
        CornerRadius::same(theme::RADIUS_XS),
        dot_color.gamma_multiply(pulse),
    );
    if status == SessionStatus::Connected && !unreachable {
        widgets::status_dot(&painter, dot_center, dot_size * 0.5, Color32::TRANSPARENT);
    }
    if flashing && !unreachable {
        let t = ((now_ms() % 420.0) / 420.0) as f32;
        let s = 1.0 + 0.35 * (1.0 - t);
        painter.rect_stroke(
            Rect::from_center_size(dot_center, Vec2::splat(dot_size * s)),
            CornerRadius::same(theme::RADIUS_XS),
            Stroke::new(1.5, theme::SUCCESS),
            StrokeKind::Outside,
        );
    }
    painter.text(
        dot_center,
        Align2::CENTER_CENTER,
        (index + 1).to_string(),
        theme::font_sans(theme::TEXT_XS),
        Color32::WHITE,
    );

    if !collapsed {
        let text_left = dot_rect.right() + 8.0;
        let text_right = rect.right() - 28.0;
        let label = theme::truncate(
            painter,
            &title,
            &theme::f_sm(),
            (text_right - text_left).max(10.0),
        );
        painter.text(
            Pos2::new(text_left, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            theme::f_sm(),
            if active {
                Color32::WHITE
            } else if hovered {
                Color32::from_rgba_premultiplied(196, 204, 217, 242)
            } else {
                Color32::from_rgba_premultiplied(143, 165, 189, 214)
            }
            .gamma_multiply(if unreachable { 0.58 } else { 1.0 }),
        );

        // Close button
        let close_rect = Rect::from_center_size(
            Pos2::new(rect.right() - 17.0, rect.center().y),
            Vec2::splat(15.0),
        );
        let close = ui.interact(
            close_rect,
            ui.id().with(("host-close", tab_id)),
            Sense::click(),
        );
        let opacity = if close.hovered() { 1.0 } else { 0.45 };
        widgets::cross(
            painter,
            close_rect.center(),
            9.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, (opacity * 255.0) as u8),
        );
        if close.clicked() {
            app.remove_host_tab(tab_id);
            return;
        }
    }

    if response.clicked() {
        app.set_active_host_tab(tab_id);
    }
}

