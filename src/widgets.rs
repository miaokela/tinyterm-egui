//! Hand-painted widgets that reproduce the web client's button/input styling
//! (egui's default widget visuals cannot express the frosted-glass look).

use crate::theme;
use egui::{
    Align2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2,
};

/// `.btn-primary`
pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let font = theme::f_sm();
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let size = Vec2::new(text_w + 40.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();
    let bg = if !enabled {
        Color32::from_rgba_premultiplied(17, 44, 90, 115)
    } else if pressed {
        theme::ACCENT_ACTIVE
    } else if hovered {
        theme::ACCENT_HOVER
    } else {
        theme::ACCENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_SM), bg);
    if hovered {
        theme::glow(painter, rect, theme::RADIUS_SM, theme::ACCENT_HOVER, 1.0);
    }
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        if enabled {
            Color32::WHITE
        } else {
            Color32::from_rgba_premultiplied(90, 90, 90, 120)
        },
    );
    response
}

/// `.btn-ghost`
pub fn ghost_button(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let font = theme::f_sm();
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let size = Vec2::new(text_w + 32.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hovered = enabled && response.hovered();
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(
            rect,
            CornerRadius::same(theme::RADIUS_SM),
            Color32::from_rgba_premultiplied(22, 49, 96, 130),
        );
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(
            1.0,
            if hovered {
                theme::BORDER_ACTIVE
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        if !enabled {
            Color32::from_rgba_premultiplied(90, 90, 90, 120)
        } else if hovered {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_SECONDARY
        },
    );
    response
}

/// Translucent version of a theme colour.
///
/// egui's painter wants *premultiplied* channels, so scaling the colour and the
/// alpha together is what keeps a tint from looking washed out or muddy.
pub fn tint(color: Color32, alpha: f32) -> Color32 {
    let a = alpha.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (color.r() as f32 * a) as u8,
        (color.g() as f32 * a) as u8,
        (color.b() as f32 * a) as u8,
        (a * 255.0) as u8,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OrbKind {
    /// Row action: invisible until hovered.
    Ghost,
    /// Panel primary action: filled accent, always visible.
    Primary,
}

/// A ring of short arc segments — the "machined" detail that gives the orb
/// buttons their HUD look. `phase` rotates the whole ring, `span` is the length
/// of one segment in radians.
#[allow(clippy::too_many_arguments)]
fn tick_ring(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    color: Color32,
    width: f32,
    segments: usize,
    span: f32,
    phase: f32,
) {
    const STEPS: usize = 4;
    let step = std::f32::consts::TAU / segments as f32;
    for i in 0..segments {
        let from = phase + i as f32 * step;
        let points: Vec<Pos2> = (0..=STEPS)
            .map(|k| {
                let a = from + span * (k as f32 / STEPS as f32);
                center + Vec2::new(a.cos(), a.sin()) * radius
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(width, color)));
    }
}

/// Frosted circular icon button used by the Hosts/Credentials toolbars and host
/// rows.
///
/// Idle it stays nearly transparent so a list of rows does not turn into a wall
/// of buttons; on hover it takes an accent tint, an accent border, a soft glow
/// and a brighter glyph, which is enough to read as "this row can connect".
/// A slowly rotating tick ring inside the rim keeps it feeling powered.
pub fn orb_button(
    ui: &mut Ui,
    size: f32,
    draw: impl FnOnce(&egui::Painter, Rect, Color32),
) -> Response {
    orb_button_impl(ui, size, OrbKind::Ghost, draw)
}

/// Accent-filled orb for a panel's primary action (e.g. 新增主机).
///
/// Reads as a power core: solid accent disc, bright rim, eight machined dashes
/// around the glyph and a breathing halo so it stays the brightest control in
/// the toolbar without flashing.
pub fn primary_orb_button(
    ui: &mut Ui,
    size: f32,
    draw: impl FnOnce(&egui::Painter, Rect, Color32),
) -> Response {
    orb_button_impl(ui, size, OrbKind::Primary, draw)
}

fn orb_button_impl(
    ui: &mut Ui,
    size: f32,
    kind: OrbKind,
    draw: impl FnOnce(&egui::Painter, Rect, Color32),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let enabled = ui.is_enabled();
    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();
    let time = ui.input(|i| i.time) as f32;
    let painter = ui.painter();
    let center = rect.center();
    let radius = size * 0.5 - 1.0;
    let glow_radius = (size * 0.5).round().clamp(0.0, 255.0) as u8;
    // Constant, slow rotation: never snaps when the pointer leaves.
    let phase = time * 0.25;

    let (fill, rim, glyph) = match kind {
        OrbKind::Primary => {
            let fill = if !enabled {
                tint(theme::ACCENT, 0.3)
            } else if pressed {
                theme::ACCENT_ACTIVE
            } else if hovered {
                theme::ACCENT_HOVER
            } else {
                theme::ACCENT
            };
            if enabled {
                let pulse = 0.5 + 0.2 * (time * 1.6).sin();
                theme::glow(
                    painter,
                    rect,
                    glow_radius,
                    theme::ACCENT,
                    if hovered { 1.1 } else { pulse },
                );
            }
            (
                fill,
                tint(theme::ACCENT_LIGHT, if hovered { 0.95 } else { 0.7 }),
                if enabled {
                    Color32::WHITE
                } else {
                    tint(Color32::WHITE, 0.6)
                },
            )
        }
        OrbKind::Ghost => {
            let fill = if !enabled {
                tint(theme::BG_CARD, 0.25)
            } else if pressed {
                tint(theme::ACCENT, 0.34)
            } else if hovered {
                tint(theme::ACCENT, 0.22)
            } else {
                tint(theme::TEXT_PRIMARY, 0.04)
            };
            if hovered {
                theme::glow(painter, rect, glow_radius, theme::ACCENT, 0.6);
            }
            (
                fill,
                if hovered {
                    theme::BORDER_ACTIVE
                } else {
                    theme::BORDER
                },
                if !enabled {
                    tint(theme::TEXT_MUTED, 0.6)
                } else if hovered {
                    theme::TEXT_PRIMARY
                } else {
                    theme::TEXT_SECONDARY
                },
            )
        }
    };

    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(center, radius - 0.5, Stroke::new(1.0, rim));

    match kind {
        OrbKind::Primary => {
            // A radar sweep running along the rim, so the primary action reads
            // as "powered" while the list beside it stays calm. No dashes here:
            // the plus glyph needs the centre of the disc to itself.
            let sweep: Vec<Pos2> = (0..=6)
                .map(|k| {
                    let a = time * 1.2 + 0.9 * (k as f32 / 6.0);
                    center + Vec2::new(a.cos(), a.sin()) * (radius - 1.5)
                })
                .collect();
            painter.add(egui::Shape::line(
                sweep,
                Stroke::new(
                    1.5,
                    tint(theme::ACCENT_LIGHT, if hovered { 0.95 } else { 0.6 }),
                ),
            ));
        }
        // Four diagonal brackets, the HUD "target" marks.
        OrbKind::Ghost => tick_ring(
            painter,
            center,
            radius - 3.0,
            tint(theme::ACCENT, if hovered { 0.9 } else { 0.28 }),
            1.0,
            4,
            0.42,
            phase,
        ),
    }

    draw(painter, rect, glyph);
    response
}

/// A text input styled like `.form-input`.
pub fn text_input<'a>(
    ui: &mut Ui,
    value: &'a mut String,
    placeholder: &str,
    width: f32,
    password: bool,
) -> Response {
    text_input_ext(ui, value, placeholder, width, password, None)
}

/// Text input with an optional leading icon (e.g. a search magnifier). The text
/// area is inset so the placeholder never runs under the icon.
pub fn text_input_ext<'a>(
    ui: &mut Ui,
    value: &'a mut String,
    placeholder: &str,
    width: f32,
    password: bool,
    leading_icon: Option<&str>,
) -> Response {
    let desired = Vec2::new(width, 30.0);
    let (rect, response) = ui.allocate_exact_size(desired, Sense::click());
    let focused = ui.memory(|m| m.has_focus(response.id));
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_SM), theme::BG_INPUT);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(
            1.0,
            if focused {
                theme::ACCENT
            } else if response.hovered() {
                theme::BORDER_ACTIVE
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );

    let left_pad = if leading_icon.is_some() { 30.0 } else { 10.0 };
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + left_pad, rect.top() + 4.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - 4.0),
    );
    if let Some(glyph) = leading_icon {
        icon(
            painter,
            Pos2::new(rect.left() + 16.0, rect.center().y),
            13.0,
            glyph,
            theme::TEXT_MUTED,
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add(
        egui::TextEdit::singleline(value)
            .frame(egui::Frame::NONE)
            .password(password)
            .text_color(theme::TEXT_PRIMARY)
            .font(theme::f_sm())
            .hint_text(
                egui::RichText::new(placeholder)
                    .color(theme::TEXT_MUTED)
                    .size(theme::TEXT_SM),
            )
            .desired_width(inner.width()),
    );
    response
}

/// A multi-line text area styled like `.form-input`.
pub fn text_area(
    ui: &mut Ui,
    value: &mut String,
    placeholder: &str,
    width: f32,
    height: f32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let focused = ui.memory(|m| m.has_focus(response.id));
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_SM), theme::BG_INPUT);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(1.0, if focused { theme::ACCENT } else { theme::BORDER }),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(Vec2::new(10.0, 6.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );
    child.add(
        egui::TextEdit::multiline(value)
            .frame(egui::Frame::NONE)
            .font(theme::font_mono(theme::TEXT_SM))
            .text_color(theme::TEXT_PRIMARY)
            .hint_text(
                egui::RichText::new(placeholder)
                    .color(theme::TEXT_MUTED)
                    .size(theme::TEXT_SM),
            )
            .desired_width(inner.width())
            .desired_rows(6),
    );
    response
}

/// A small labelled field row used inside modal forms.
pub fn labelled(ui: &mut Ui, label: &str, required: bool) {
    let text = if required {
        format!("{label} *")
    } else {
        label.to_string()
    };
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(theme::TEXT_XS)
            .color(theme::TEXT_SECONDARY)
            .strong(),
    );
    ui.add_space(2.0);
}

/// A dot with a neon glow, used by host/session status indicators.
pub fn status_dot(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    painter.circle_filled(center, radius, color);
    for i in 1..=3 {
        let alpha = (60.0 / i as f32) as u8;
        painter.circle_stroke(
            center,
            radius + i as f32 * 0.8,
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
            ),
        );
    }
}

/// Draw an icon glyph from the Phosphor font centred at `center`.
pub fn icon(painter: &egui::Painter, center: Pos2, size: f32, glyph: &str, color: Color32) {
    painter.text(
        center,
        Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(size),
        color,
    );
}

/// Chevron glyph (`CARET_LEFT` / `CARET_RIGHT`).
pub fn chevron(painter: &egui::Painter, center: Pos2, size: f32, left: bool, color: Color32) {
    let glyph = if left {
        crate::icons::CARET_LEFT
    } else {
        crate::icons::CARET_RIGHT
    };
    icon(painter, center, size * 1.25, glyph, color);
}

/// Close glyph.
pub fn cross(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    icon(painter, center, size * 1.25, crate::icons::X, color);
}

/// Plus glyph.
pub fn plus(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    icon(painter, center, size * 1.25, crate::icons::PLUS, color);
}

/// Folder glyph filling `rect`.
pub fn folder_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    icon(
        painter,
        rect.center(),
        rect.height() * 1.05,
        crate::icons::FOLDER_SIMPLE,
        color,
    );
}

/// File glyph filling `rect`.
pub fn file_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    icon(
        painter,
        rect.center(),
        rect.height() * 1.05,
        crate::icons::FILE_TEXT,
        color,
    );
}

/// Settings "gear" glyph.
pub fn gear(painter: &egui::Painter, c: Pos2, radius: f32, color: Color32) {
    icon(painter, c, radius * 2.3, crate::icons::GEAR, color);
}

/// Edit (pencil) glyph.
pub fn pencil(painter: &egui::Painter, c: Pos2, color: Color32) {
    icon(painter, c, 14.0, crate::icons::PENCIL_SIMPLE, color);
}

/// Delete (trash) glyph.
pub fn trash(painter: &egui::Painter, c: Pos2, color: Color32) {
    icon(painter, c, 14.0, crate::icons::TRASH, color);
}

/// Duplicate (copy) glyph.
pub fn copy_glyph(painter: &egui::Painter, c: Pos2, color: Color32) {
    icon(painter, c, 14.0, crate::icons::COPY, color);
}

/// Connect (plug) glyph.
pub fn plug(painter: &egui::Painter, c: Pos2, color: Color32) {
    icon(painter, c, 15.0, crate::icons::PLUG, color);
}

/// Key glyph.
pub fn key_glyph(painter: &egui::Painter, c: Pos2, color: Color32) {
    icon(painter, c, 15.0, crate::icons::LOCK_KEY, color);
}

/// Padlock glyph.
pub fn lock_glyph(painter: &egui::Painter, c: Pos2, color: Color32) {
    icon(painter, c, 15.0, crate::icons::LOCK_SIMPLE, color);
}

/// Draw a spinner arc; `t` is seconds.
pub fn spinner(painter: &egui::Painter, center: Pos2, radius: f32, t: f32, color: Color32) {
    let segments = 24;
    let start = t * 4.0;
    for i in 0..segments {
        let a0 = start + (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a1 = a0 + std::f32::consts::TAU / segments as f32 * 1.4;
        let alpha = (i as f32 / segments as f32 * 255.0) as u8;
        painter.line_segment(
            [
                center + Vec2::new(a0.cos(), a0.sin()) * radius,
                center + Vec2::new(a1.cos(), a1.sin()) * radius,
            ],
            Stroke::new(
                2.0,
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha),
            ),
        );
    }
}

/// The "loading blocks" wave from `LoadingBlocks.tsx`.
pub fn loading_blocks(ui: &mut Ui, time: f32, scale: f32) {
    const DELAYS: [[f32; 4]; 4] = [
        [0.18, 0.18, -1.0, -1.0],
        [0.09, 0.0, 0.0, 0.09],
        [0.09, 0.0, 0.0, 0.09],
        [0.18, 0.18, -1.0, -1.0],
    ];
    let block = 3.0 * scale;
    let gap = 2.0 * scale;
    let width = 4.0 * block + 3.0 * gap;
    let height = 4.0 * block + 3.0 * gap;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    for (row_idx, row) in DELAYS.iter().enumerate() {
        let count = row.iter().filter(|d| **d >= 0.0).count();
        let row_w = count as f32 * block + (count.saturating_sub(1)) as f32 * gap;
        let x0 = rect.center().x - row_w * 0.5;
        for (i, delay) in row.iter().enumerate() {
            if *delay < 0.0 {
                continue;
            }
            let phase = (time * 6.2831) - delay * 6.2831;
            let wave = ((phase.sin() + 1.0) * 0.5).clamp(0.0, 1.0);
            let alpha = (0.12 + 0.78 * wave) as f32;
            let scale_factor = 1.0 + 0.4 * wave;
            let size = block * scale_factor;
            let center = Pos2::new(
                x0 + i as f32 * (block + gap) + block * 0.5,
                rect.top() + row_idx as f32 * (block + gap) + block * 0.5,
            );
            painter.rect_filled(
                Rect::from_center_size(center, Vec2::splat(size)),
                CornerRadius::same(1),
                Color32::from_rgba_unmultiplied(
                    58,
                    132,
                    255,
                    (alpha * 255.0).clamp(0.0, 255.0) as u8,
                ),
            );
        }
    }
}

/// Which horizontal edge of a rounded rectangle to leave unstroked, so two
/// stacked panels can share a seamless seam (CSS `border-bottom: none` /
/// `border-top: none`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenSide {
    Top,
    Bottom,
}

/// Stroke a rounded rectangle while omitting one horizontal edge.
///
/// The polyline must be continuous: `Shape::line` joins every consecutive pair,
/// so a corner arc that starts where the previous point already is. The arcs
/// below are therefore traversed in the direction that continues the outline
/// (`from` → `to`); running one backwards draws a diagonal chord across the
/// corner instead of the arc.
pub fn stroke_open(
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
    stroke: Stroke,
    open: OpenSide,
) {
    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    const STEPS: usize = 8;
    let r = radius as f32;
    let arc = |cx: f32, cy: f32, from: f32, to: f32| -> Vec<Pos2> {
        (0..=STEPS)
            .map(|i| {
                let a = from + (to - from) * (i as f32 / STEPS as f32);
                Pos2::new(cx + a.cos() * r, cy + a.sin() * r)
            })
            .collect()
    };

    let mut points: Vec<Pos2> = Vec::with_capacity(4 * STEPS + 8);
    match open {
        // Left, bottom and right edges — the top edge stays open.
        OpenSide::Top => {
            points.push(Pos2::new(rect.left(), rect.top()));
            points.push(Pos2::new(rect.left(), rect.bottom() - r));
            points.extend(arc(rect.left() + r, rect.bottom() - r, PI, FRAC_PI_2));
            points.push(Pos2::new(rect.right() - r, rect.bottom()));
            points.extend(arc(rect.right() - r, rect.bottom() - r, FRAC_PI_2, 0.0));
            points.push(Pos2::new(rect.right(), rect.top()));
        }
        // Left, top and right edges — the bottom edge stays open.
        OpenSide::Bottom => {
            points.push(Pos2::new(rect.left(), rect.bottom()));
            points.push(Pos2::new(rect.left(), rect.top() + r));
            points.extend(arc(rect.left() + r, rect.top() + r, PI, 3.0 * FRAC_PI_2));
            points.push(Pos2::new(rect.right() - r, rect.top()));
            points.extend(arc(rect.right() - r, rect.top() + r, 3.0 * FRAC_PI_2, TAU));
            points.push(Pos2::new(rect.right(), rect.bottom()));
        }
    }
    painter.add(egui::Shape::line(points, stroke));
}

/// Lay out a right-aligned row of dialog buttons inside `rect`, all in the
/// ghost style (matching the leftmost "取消" button). `labels` are given in
/// visual left-to-right order; returns the index of the clicked button.
pub fn button_row(ui: &mut Ui, rect: Rect, labels: &[(&str, bool)]) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 8.0;
    let mut clicked = None;
    // Added right-to-left, so iterate in reverse to keep `labels` order.
    for (index, (label, enabled)) in labels.iter().enumerate().rev() {
        if ghost_button(&mut child, label, *enabled).clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

/// `.fm-spinner` equivalent.
pub fn css_spinner(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    spinner(
        ui.painter(),
        rect.center(),
        size * 0.42,
        t,
        theme::ACCENT_HOVER,
    );
}

