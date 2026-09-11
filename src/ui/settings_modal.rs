//! Settings modal — the web client stores these but has no full UI for them
//! (documented gap); this panel closes that gap.

use crate::state::{AppState, ModalKind};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

const WIDTH: f32 = 560.0;
const HEIGHT: f32 = 560.0;

pub fn show(ui: &mut Ui, app: &mut AppState) {
    if app.modal != ModalKind::Settings {
        return;
    }
    let screen = ui.ctx().input(|i| i.viewport_rect());
    ui.painter().rect_filled(
        screen,
        0,
        Color32::from_rgba_premultiplied(2, 6, 14, 184),
    );

    let width = WIDTH.min(screen.width() - 40.0);
    let height = HEIGHT.min(screen.height() - 60.0);
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_LG), theme::BG_CARD);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_LG),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    let header = Rect::from_min_size(rect.min, Vec2::new(width, 52.0));
    painter.text(
        Pos2::new(header.left() + 20.0, header.center().y),
        Align2::LEFT_CENTER,
        "设置",
        theme::f_lg(),
        theme::TEXT_PRIMARY,
    );
    painter.line_segment(
        [
            Pos2::new(header.left(), header.bottom() - 0.5),
            Pos2::new(header.right(), header.bottom() - 0.5),
        ],
        Stroke::new(1.0, theme::BORDER),
    );
    let close_rect = Rect::from_center_size(
        Pos2::new(header.right() - 24.0, header.center().y),
        Vec2::splat(28.0),
    );
    let r_close = ui.interact(close_rect, ui.id().with("settings-close"), Sense::click());
    widgets::cross(
        &painter,
        close_rect.center(),
        11.0,
        if r_close.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
    );
    if r_close.clicked() {
        app.modal = ModalKind::None;
        app.save_settings();
        return;
    }

    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 20.0, header.bottom() + 6.0),
        Pos2::new(rect.right() - 20.0, rect.bottom() - 12.0),
    );
    let full = body.width();
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(body));
    egui::ScrollArea::vertical()
        .id_salt("settings")
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.set_width(full);
            let mut changed = false;

            section(ui, "终端");
            changed |= slider_row(
                ui,
                "字体大小",
                &mut app.settings.font_size,
                8..=24,
                full,
            );
            changed |= slider_row(
                ui,
                "回滚行数 (scrollback)",
                &mut app.settings.scrollback,
                500..=100_000,
                full,
            );

            widgets::labelled(ui, "字体族", false);
            let families = [
                "Menlo, Monaco, 'Courier New', monospace",
                "SF Mono, Menlo, monospace",
                "Monaco, monospace",
            ];
            for family in families {
                let selected = app.settings.font_family == family;
                let (r, resp) = ui.allocate_exact_size(Vec2::new(full, 28.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(
                    r,
                    CornerRadius::same(theme::RADIUS_XS),
                    if selected {
                        Color32::from_rgba_premultiplied(29, 57, 102, 140)
                    } else if resp.hovered() {
                        Color32::from_rgba_premultiplied(20, 40, 70, 110)
                    } else {
                        Color32::TRANSPARENT
                    },
                );
                p.text(
                    Pos2::new(r.left() + 10.0, r.center().y),
                    Align2::LEFT_CENTER,
                    family,
                    theme::font_mono(theme::TEXT_XS),
                    if selected { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
                );
                if resp.clicked() && !selected {
                    app.settings.font_family = family.to_string();
                    changed = true;
                }
            }

            section(ui, "光标与显示");
            changed |= toggle_row(ui, "光标闪烁", &mut app.settings.cursor_blink, full);
            changed |= toggle_row(
                ui,
                "默认显示隐藏文件",
                &mut app.settings.show_hidden_files,
                full,
            );

            widgets::labelled(ui, "光标样式", false);
            let styles = [("block", "方块"), ("bar", "竖线"), ("underline", "下划线")];
            let mut row_ui = ui.horizontal(|ui| {
                for (style, label) in styles {
                    let selected = app.settings.cursor_style == style;
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(90.0, 28.0), Sense::click());
                    let p = ui.painter();
                    p.rect_filled(
                        r,
                        CornerRadius::same(theme::RADIUS_XS),
                        if selected {
                            Color32::from_rgba_premultiplied(29, 57, 102, 140)
                        } else if resp.hovered() {
                            Color32::from_rgba_premultiplied(20, 40, 70, 110)
                        } else {
                            Color32::TRANSPARENT
                        },
                    );
                    p.rect_stroke(
                        r,
                        CornerRadius::same(theme::RADIUS_XS),
                        Stroke::new(1.0, theme::BORDER),
                        StrokeKind::Inside,
                    );
                    p.text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        label,
                        theme::f_xs(),
                        if selected { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
                    );
                    if resp.clicked() {
                        app.settings.cursor_style = style.to_string();
                        changed = true;
                    }
                }
            });
            let _ = &mut row_ui;

            section(ui, "界面");
            changed |= zoom_row(ui, app, full);
            changed |= toggle_row(
                ui,
                "侧边栏折叠",
                &mut app.sidebar_collapsed,
                full,
            );

            section(ui, "已信任的 SSH 主机指纹");
            let keys = app.db.list_trusted_host_keys().unwrap_or_default();
            if keys.is_empty() {
                ui.label(
                    egui::RichText::new("暂无记录")
                        .size(theme::TEXT_XS)
                        .color(theme::TEXT_MUTED),
                );
            } else {
                let mut remove: Option<(String, u16)> = None;
                for key in &keys {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(full, 40.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(
                        r,
                        CornerRadius::same(theme::RADIUS_XS),
                        Color32::from_rgba_premultiplied(11, 24, 46, 40),
                    );
                    p.text(
                        Pos2::new(r.left() + 10.0, r.center().y - 8.0),
                        Align2::LEFT_CENTER,
                        format!("{}:{}", key.host, key.port),
                        theme::font_mono(theme::TEXT_XS),
                        theme::TEXT_PRIMARY,
                    );
                    p.text(
                        Pos2::new(r.left() + 10.0, r.center().y + 8.0),
                        Align2::LEFT_CENTER,
                        format!("{} {}", key.key_type, key.fingerprint),
                        theme::font_mono(theme::TEXT_XS - 2.0),
                        theme::TEXT_MUTED,
                    );
                    let del_rect = Rect::from_center_size(
                        Pos2::new(r.right() - 18.0, r.center().y),
                        Vec2::splat(24.0),
                    );
                    let rd = ui.interact(
                        del_rect,
                        ui.id().with(("trust-del", &key.host, key.port)),
                        Sense::click(),
                    );
                    widgets::trash(
                        p,
                        del_rect.center(),
                        if rd.hovered() { theme::ERROR } else { theme::TEXT_MUTED },
                    );
                    if rd.clicked() {
                        remove = Some((key.host.clone(), key.port));
                    }
                }
                if let Some((host, port)) = remove {
                    let _ = app.db.delete_trusted_host_key(&host, port);
                }
            }

            if changed {
                app.save_settings();
                ui.ctx().request_repaint();
            }
            ui.add_space(10.0);
        });

    // Footer
    let footer = Rect::from_min_size(
        Pos2::new(rect.right() - 20.0 - 100.0, rect.bottom() - 44.0),
        Vec2::new(100.0, 24.0),
    );
    let mut footer_ui = ui.new_child(egui::UiBuilder::new().max_rect(footer));
    if widgets::ghost_button(&mut footer_ui, "完成", true).clicked() {
        app.modal = ModalKind::None;
        app.save_settings();
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(title)
            .size(theme::TEXT_XS)
            .strong()
            .color(theme::ACCENT_LIGHT),
    );
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(
        r,
        0,
        Color32::from_rgba_premultiplied(17, 40, 77, 77),
    );
    ui.add_space(4.0);
}

fn slider_row(
    ui: &mut Ui,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    width: f32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(theme::TEXT_XS)
                .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(8.0);
        let mut v = *value as f32;
        let resp = ui.add_sized(
            Vec2::new(width - 150.0, 20.0),
            egui::Slider::new(&mut v, *range.start() as f32..=*range.end() as f32)
                .show_value(false),
        );
        ui.label(
            egui::RichText::new(format!("{}", v as u32))
                .size(theme::TEXT_XS)
                .color(theme::TEXT_PRIMARY),
        );
        if resp.changed() {
            *value = v as u32;
            changed = true;
        }
    });
    changed
}

fn toggle_row(ui: &mut Ui, label: &str, value: &mut bool, width: f32) -> bool {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());
    let p = ui.painter();
    p.text(
        Pos2::new(r.left() + 2.0, r.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::f_xs(),
        theme::TEXT_SECONDARY,
    );
    let track = Rect::from_center_size(
        Pos2::new(r.right() - 24.0, r.center().y),
        Vec2::new(36.0, 18.0),
    );
    p.rect_filled(
        track,
        CornerRadius::same(9),
        if *value {
            Color32::from_rgba_premultiplied(47, 125, 255, 200)
        } else {
            Color32::from_rgba_premultiplied(40, 55, 80, 200)
        },
    );
    let knob = Pos2::new(
        if *value { track.right() - 9.0 } else { track.left() + 9.0 },
        track.center().y,
    );
    p.circle_filled(knob, 7.0, Color32::WHITE);
    if resp.clicked() {
        *value = !*value;
        return true;
    }
    false
}

fn zoom_row(ui: &mut Ui, app: &mut AppState, width: f32) -> bool {
    let (r, _) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::hover());
    let p = ui.painter();
    p.text(
        Pos2::new(r.left() + 2.0, r.center().y),
        Align2::LEFT_CENTER,
        "界面缩放",
        theme::f_xs(),
        theme::TEXT_SECONDARY,
    );
    let minus = Rect::from_center_size(Pos2::new(r.right() - 110.0, r.center().y), Vec2::splat(22.0));
    let plus = Rect::from_center_size(Pos2::new(r.right() - 18.0, r.center().y), Vec2::splat(22.0));
    let r_minus = ui.interact(minus, ui.id().with("zoom-minus"), Sense::click());
    let r_plus = ui.interact(plus, ui.id().with("zoom-plus"), Sense::click());
    for (rect, hovered) in [(&minus, r_minus.hovered()), (&plus, r_plus.hovered())] {
        if hovered {
            p.rect_filled(
                *rect,
                CornerRadius::same(theme::RADIUS_XS),
                Color32::from_rgba_premultiplied(22, 49, 96, 120),
            );
        }
        p.rect_stroke(
            *rect,
            CornerRadius::same(theme::RADIUS_XS),
            Stroke::new(1.0, theme::BORDER),
            StrokeKind::Inside,
        );
    }
    p.text(
        minus.center(),
        Align2::CENTER_CENTER,
        "−",
        theme::f_sm(),
        theme::TEXT_PRIMARY,
    );
    p.text(
        plus.center(),
        Align2::CENTER_CENTER,
        "+",
        theme::f_sm(),
        theme::TEXT_PRIMARY,
    );
    p.text(
        Pos2::new(r.right() - 64.0, r.center().y),
        Align2::CENTER_CENTER,
        format!("{:.0}%", app.app_zoom * 100.0),
        theme::f_xs(),
        theme::TEXT_PRIMARY,
    );
    let mut changed = false;
    if r_minus.clicked() {
        app.app_zoom = (app.app_zoom - crate::state::APP_ZOOM_STEP).max(crate::state::APP_ZOOM_MIN);
        changed = true;
    }
    if r_plus.clicked() {
        app.app_zoom = (app.app_zoom + crate::state::APP_ZOOM_STEP).min(crate::state::APP_ZOOM_MAX);
        changed = true;
    }
    changed
}
