//! Toast notifications (`.toast-host` / `.toast-item`).

use crate::state::AppState;
use crate::theme;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

pub fn show(ui: &mut Ui, app: &mut AppState) {
    if app.toasts.is_empty() {
        return;
    }
    let screen = ui.ctx().input(|i| i.viewport_rect());
    let painter = ui.painter().clone();
    let mut y = screen.bottom() - 16.0;

    let toasts = app.toasts.clone();
    for toast in toasts.iter().rev() {
        let font = theme::f_sm();
        let text_w = painter
            .layout_no_wrap(toast.message.clone(), font.clone(), Color32::WHITE)
            .size()
            .x;
        let width = (text_w + 56.0).min(screen.width() - 60.0);
        let height = 38.0;
        let rect = Rect::from_min_size(
            Pos2::new(screen.right() - 16.0 - width, y - height),
            Vec2::new(width, height),
        );
        // Enter animation: slide up 12px over 220 ms.
        let age = (crate::state::now_ms() - toast.created_ms) as f32;
        let t = (age / 220.0).clamp(0.0, 1.0);
        let rect = rect.translate(Vec2::new(0.0, (1.0 - t) * 12.0));

        let border = match toast.kind {
            crate::session::ToastKind::Success => Color32::from_rgba_premultiplied(52, 136, 99, 102),
            crate::session::ToastKind::Error => Color32::from_rgba_premultiplied(143, 51, 51, 102),
            crate::session::ToastKind::Info => theme::BORDER,
        };
        let icon_color = match toast.kind {
            crate::session::ToastKind::Success => theme::SUCCESS,
            crate::session::ToastKind::Error => theme::ERROR,
            crate::session::ToastKind::Info => theme::WARNING,
        };

        painter.rect_filled(
            rect,
            CornerRadius::same(theme::RADIUS_SM),
            Color32::from_rgba_premultiplied(9, 25, 48, 242),
        );
        painter.rect_stroke(
            rect,
            CornerRadius::same(theme::RADIUS_SM),
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );

        // Icon
        let icon_center = Pos2::new(rect.left() + 20.0, rect.center().y);
        let glyph = match toast.kind {
            crate::session::ToastKind::Success => crate::icons::CHECK,
            crate::session::ToastKind::Error => crate::icons::X,
            crate::session::ToastKind::Info => crate::icons::INFO,
        };
        crate::widgets::icon(&painter, icon_center, 13.0, glyph, icon_color);

        painter.text(
            Pos2::new(rect.left() + 38.0, rect.center().y),
            Align2::LEFT_CENTER,
            theme::truncate(&painter, &toast.message, &font, rect.width() - 52.0),
            font,
            theme::TEXT_PRIMARY,
        );

        y -= height + 8.0;
    }

    // Request another frame while any toast is animating or alive.
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
}
