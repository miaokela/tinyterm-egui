//! Left host sidebar — `.host-sidebar` + `.host-sidebar-tab`.

use crate::models::{HostReachability, SessionStatus};
use crate::state::{now_ms, AppState};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

pub const TAB_HEIGHT: f32 = 32.0;

pub fn show(ui: &mut Ui, app: &mut AppState, rect: Rect) {
    let painter = ui.painter().clone();
    theme::glass_panel(&painter, rect, theme::RADIUS_PANEL);

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
    let host_count = app.bookmarks.len();
    let r = ui
        .interact(btn_rect, ui.id().with("hosts-modal-btn"), Sense::click())
        .on_hover_text(format!("已配置 {host_count} 台主机"));
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
        if host_count > 0 {
            let label = if host_count > 9 {
                "9+".to_string()
            } else {
                host_count.to_string()
            };
            let badge_center = Pos2::new(btn_rect.right() - 2.0, btn_rect.top() + 2.0);
            painter.circle_filled(badge_center, 7.0, theme::ACCENT);
            painter.text(
                badge_center,
                Align2::CENTER_CENTER,
                label,
                theme::font_sans(8.5),
                Color32::WHITE,
            );
        }
    } else {
        widgets::gear(painter, Pos2::new(btn_rect.left() + 10.0 + 6.5, btn_rect.center().y), 6.5, fg);
        painter.text(
            Pos2::new(btn_rect.left() + 10.0 + 13.0 + 6.0, btn_rect.center().y),
            Align2::LEFT_CENTER,
            "主机管理",
            theme::f_xs(),
            fg,
        );

        // Configured-host counter, right-aligned inside the button.
        let count_text = format!("{host_count} 台");
        let count_font = theme::f_xs();
        let count_w = painter
            .layout_no_wrap(count_text.clone(), count_font.clone(), Color32::WHITE)
            .size()
            .x;
        let pill = Rect::from_min_size(
            Pos2::new(
                btn_rect.right() - 8.0 - (count_w + 14.0),
                btn_rect.center().y - 9.0,
            ),
            Vec2::new(count_w + 14.0, 18.0),
        );
        painter.rect_filled(
            pill,
            CornerRadius::same(9),
            if r.hovered() {
                Color32::from_rgba_premultiplied(41, 82, 133, 140)
            } else {
                Color32::from_rgba_premultiplied(15, 35, 65, 90)
            },
        );
        painter.text(
            pill.center(),
            Align2::CENTER_CENTER,
            count_text,
            count_font,
            if r.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            },
        );
    }
    if r.clicked() {
        app.host_hint_visible = false;
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

    if !collapsed {
        host_hint(ui, app, content, btn_rect, host_count);
    }
}

/// First-run speech bubble above the host-management button. It disappears
/// after the user opens the Hosts modal through the button or the bubble.
fn host_hint(ui: &mut Ui, app: &mut AppState, content: Rect, anchor: Rect, host_count: usize) {
    if !app.host_hint_visible {
        return;
    }

    let text = if host_count == 0 {
        "这里配置主机"
    } else {
        "你的主机在这里"
    };
    let font = theme::f_sm();
    let painter = ui.painter().clone();
    let max_w = (content.width() - 4.0).max(48.0);
    let icon_size = 13.0;
    let gap = 7.0;
    let pad_x = 12.0;
    let label = theme::truncate(
        &painter,
        text,
        &font,
        (max_w - pad_x * 2.0 - icon_size - gap).max(20.0),
    );
    let text_w = painter
        .layout_no_wrap(label.clone(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let w = (text_w + icon_size + gap + pad_x * 2.0).min(max_w);
    let h = 34.0;
    let cx = anchor.center().x.clamp(
        content.left() + w * 0.5 + 2.0,
        content.right() - w * 0.5 - 2.0,
    );
    let bubble = Rect::from_center_size(
        Pos2::new(cx, anchor.top() - 13.0 - h * 0.5),
        Vec2::new(w, h),
    );

    let response = ui
        .interact(bubble, ui.id().with("hosts-hint"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    let (fill, border_color) = if hovered {
        (
            Color32::from_rgba_premultiplied(26, 66, 126, 226),
            Color32::from_rgba_unmultiplied(140, 200, 255, 190),
        )
    } else {
        (
            Color32::from_rgba_premultiplied(18, 48, 96, 210),
            Color32::from_rgba_unmultiplied(106, 181, 255, 130),
        )
    };
    let border = Stroke::new(1.0, border_color);
    let radius = 10;

    // Soft glow + translucent glass fill + a thin top sheen. The fill stays
    // partially transparent so the sidebar shows through instead of reading as
    // a flat, solid sticker.
    theme::glow(
        &painter,
        bubble,
        radius,
        theme::ACCENT,
        if hovered { 0.5 } else { 0.3 },
    );
    painter.rect_filled(bubble, CornerRadius::same(radius), fill);
    painter.rect_stroke(
        bubble,
        CornerRadius::same(radius),
        border,
        StrokeKind::Inside,
    );
    painter.line_segment(
        [
            Pos2::new(bubble.left() + 12.0, bubble.top() + 1.0),
            Pos2::new(bubble.right() - 12.0, bubble.top() + 1.0),
        ],
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(190, 225, 255, if hovered { 85 } else { 55 }),
        ),
    );

    // The tail needs an opaque fill: a translucent one lets the bubble's bottom
    // border and glow show through as a faint horizontal line at the base. The
    // opaque colour approximates the glass fill composited over the sidebar.
    let tail_color = if hovered {
        Color32::from_rgb(27, 70, 133)
    } else {
        Color32::from_rgb(20, 53, 106)
    };
    let bottom = bubble.bottom();
    let tail_fill = vec![
        Pos2::new(cx - 7.0, bottom - 1.0),
        Pos2::new(cx + 7.0, bottom - 1.0),
        Pos2::new(cx + 7.0, bottom),
        Pos2::new(cx, bottom + 7.0),
        Pos2::new(cx - 7.0, bottom),
    ];
    painter.add(egui::Shape::convex_polygon(
        tail_fill,
        tail_color,
        Stroke::NONE,
    ));
    let tip = Pos2::new(cx, bottom + 7.0);
    painter.line_segment([Pos2::new(cx - 7.0, bottom), tip], border);
    painter.line_segment([Pos2::new(cx + 7.0, bottom), tip], border);

    // Small terminal glyph + label, centred as one group.
    let content_left = bubble.left() + pad_x;
    widgets::icon(
        &painter,
        Pos2::new(content_left + icon_size * 0.5, bubble.center().y),
        icon_size,
        crate::icons::TERMINAL,
        Color32::from_rgba_unmultiplied(150, 200, 255, 230),
    );
    painter.text(
        Pos2::new(content_left + icon_size + gap, bubble.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        theme::TEXT_PRIMARY,
    );

    if response.clicked() {
        app.host_hint_visible = false;
        app.modal = crate::state::ModalKind::Hosts;
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
            SessionStatus::Connecting => theme::CONNECTING,
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
        app.host_hint_visible = false;
        app.set_active_host_tab(tab_id);
    }
}

