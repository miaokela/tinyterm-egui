//! Cosmic / glassmorphism theme — a 1:1 port of TinyTerm's `global.css` design tokens.
//!
//! Every colour, radius and text size below is copied from the web client's
//! `:root` custom properties so the egui build is visually identical.

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Stroke, StrokeKind, TextStyle,
    Vec2,
};

// ── Colours ──────────────────────────────────────────────────────────────────

/// `--color-bg-primary: #07162d`
pub const BG_PRIMARY: Color32 = Color32::from_rgb(0x07, 0x16, 0x2d);
/// `--color-bg-secondary: #0b1f3d`
pub const BG_SECONDARY: Color32 = Color32::from_rgb(0x0b, 0x1f, 0x3d);
/// `--color-bg-panel: rgba(7, 22, 43, 0.86)`
pub const BG_PANEL: Color32 = Color32::from_rgba_premultiplied(6, 19, 37, 219);
/// `--color-bg-card: #0c1f38`
pub const BG_CARD: Color32 = Color32::from_rgb(0x0c, 0x1f, 0x38);
/// `--color-bg-input: rgba(6, 18, 36, 0.82)`
pub const BG_INPUT: Color32 = Color32::from_rgba_premultiplied(5, 15, 30, 209);
/// `--color-terminal-bg: #050b14`
pub const TERMINAL_BG: Color32 = Color32::from_rgb(0x05, 0x0b, 0x14);

/// `--color-accent: #2f7dff`
pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0x7d, 0xff);
/// `--color-accent-hover: #6ab5ff`
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x6a, 0xb5, 0xff);
/// `--color-accent-active: #1c5fcc`
pub const ACCENT_ACTIVE: Color32 = Color32::from_rgb(0x1c, 0x5f, 0xcc);
/// `--color-accent-2: #57d8b2`
pub const ACCENT_2: Color32 = Color32::from_rgb(0x57, 0xd8, 0xb2);
/// `--color-accent-light: #80b0ff`
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x80, 0xb0, 0xff);

/// `--color-text-primary: #e7eff9`
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xe7, 0xef, 0xf9);
/// `--color-text-secondary: #a8bdd1`
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xa8, 0xbd, 0xd1);
/// `--color-text-muted: #7d93a9`
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x7d, 0x93, 0xa9);

/// `--color-border: rgba(58, 132, 255, 0.3)`
pub const BORDER: Color32 = Color32::from_rgba_premultiplied(17, 40, 77, 77);
/// `--color-border-active: rgba(112, 191, 255, 0.6)`
pub const BORDER_ACTIVE: Color32 = Color32::from_rgba_premultiplied(67, 115, 153, 153);

/// `--color-success: #57e3a5`
pub const SUCCESS: Color32 = Color32::from_rgb(0x57, 0xe3, 0xa5);
/// `--color-error: #e0575c`
pub const ERROR: Color32 = Color32::from_rgb(0xe0, 0x57, 0x5c);
/// `--color-warning: #f0a040`
pub const WARNING: Color32 = Color32::from_rgb(0xf0, 0xa0, 0x40);

/// `--color-terminal-text: #d7e3f0`
pub const TERMINAL_TEXT: Color32 = Color32::from_rgb(0xd7, 0xe3, 0xf0);

// ── Terminal ANSI palette (from `TERMINAL_THEME` in TerminalView.tsx) ────────

pub const ANSI: [Color32; 16] = [
    Color32::from_rgb(0x16, 0x20, 0x2b), // black
    Color32::from_rgb(0xef, 0x6b, 0x73), // red
    Color32::from_rgb(0x7c, 0xcf, 0x92), // green
    Color32::from_rgb(0xe7, 0xc3, 0x6f), // yellow
    Color32::from_rgb(0x73, 0xa7, 0xff), // blue
    Color32::from_rgb(0xc7, 0x92, 0xea), // magenta
    Color32::from_rgb(0x66, 0xc7, 0xd1), // cyan
    Color32::from_rgb(0xb7, 0xc4, 0xcf), // white
    Color32::from_rgb(0x55, 0x60, 0x6d), // bright black
    Color32::from_rgb(0xff, 0x8b, 0x94), // bright red
    Color32::from_rgb(0x99, 0xe6, 0xa8), // bright green
    Color32::from_rgb(0xff, 0xd9, 0x8a), // bright yellow
    Color32::from_rgb(0x94, 0xc2, 0xff), // bright blue
    Color32::from_rgb(0xdd, 0xb3, 0xff), // bright magenta
    Color32::from_rgb(0x8b, 0xe0, 0xe8), // bright cyan
    Color32::from_rgb(0xd7, 0xe1, 0xea), // bright white
];

/// Terminal foreground `#b6c5d3`
pub const TERM_FG: Color32 = Color32::from_rgb(0xb6, 0xc5, 0xd3);
/// Terminal cursor `#ffbf69`
pub const TERM_CURSOR: Color32 = Color32::from_rgb(0xff, 0xbf, 0x69);
/// `selectionBackground: rgba(115, 167, 255, 0.24)`
pub const TERM_SELECTION: Color32 = Color32::from_rgba_premultiplied(28, 40, 61, 61);

// ── Radii (CSS px) ───────────────────────────────────────────────────────────

pub const RADIUS_XS: u8 = 4;
pub const RADIUS_SM: u8 = 8;
pub const RADIUS_MD: u8 = 12;
pub const RADIUS_LG: u8 = 16;

// ── Text sizes (CSS px, before the app zoom factor) ──────────────────────────

pub const TEXT_XS: f32 = 12.0;
pub const TEXT_SM: f32 = 13.0;
pub const TEXT_MD: f32 = 14.0;
pub const TEXT_LG: f32 = 16.0;
pub const TEXT_XL: f32 = 20.0;

// ── Layout constants ─────────────────────────────────────────────────────────

pub const SIDEBAR_WIDTH: f32 = 200.0;
pub const SIDEBAR_COLLAPSED_WIDTH: f32 = 48.0;
pub const TABSTRIP_HEIGHT: f32 = 38.0;
pub const CHROME_TAB_HEIGHT: f32 = 30.0;
pub const APP_BODY_PADDING: f32 = 10.0;
pub const APP_BODY_TOP_PADDING: f32 = 6.0;
pub const APP_BODY_GAP: f32 = 8.0;

// ── Font helpers ─────────────────────────────────────────────────────────────

pub fn font_sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn font_mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Semantic font ids matching the CSS type scale.
pub fn f_xs() -> FontId {
    font_sans(TEXT_XS)
}
pub fn f_sm() -> FontId {
    font_sans(TEXT_SM)
}
pub fn f_md() -> FontId {
    font_sans(TEXT_MD)
}
pub fn f_lg() -> FontId {
    font_sans(TEXT_LG)
}
pub fn f_xl() -> FontId {
    font_sans(TEXT_XL)
}

// ── Painting helpers ─────────────────────────────────────────────────────────

/// `.glass-panel` — translucent panel + hairline border + top inner highlight.
pub fn glass_panel(painter: &egui::Painter, rect: Rect, radius: u8) {
    let cr = CornerRadius::same(radius);
    painter.rect_filled(rect, cr, BG_PANEL);
    painter.rect_stroke(rect, cr, Stroke::new(1.0, BORDER), StrokeKind::Inside);
    // inset 0 1px 0 rgba(255,255,255,0.06)
    painter.line_segment(
        [
            Pos2::new(rect.left() + radius as f32, rect.top() + 0.5),
            Pos2::new(rect.right() - radius as f32, rect.top() + 0.5),
        ],
        Stroke::new(1.0, Color32::from_rgba_premultiplied(15, 15, 15, 15)),
    );
}


/// `--glow-accent: 0 0 12px rgba(72, 161, 255, 0.4)` — approximated with
/// concentric rounded strokes since egui has no blur.
pub fn glow(painter: &egui::Painter, rect: Rect, radius: u8, color: Color32, strength: f32) {
    let cr = CornerRadius::same(radius);
    for i in 1..=3 {
        let spread = i as f32;
        let alpha = (0.22 * strength / spread).clamp(0.0, 1.0);
        let c = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (alpha * 255.0) as u8);
        painter.rect_stroke(
            rect.expand(spread),
            cr,
            Stroke::new(1.0, c),
            StrokeKind::Outside,
        );
    }
}

/// The cosmic background: deep-blue radial wash + mint glow + vertical gradient.
///
/// egui cannot blur, so the radial gradients are emulated with a coarse grid of
/// alpha-blended cells (cheap: a few hundred rects, drawn once per frame into a
/// cached texture by [`CosmicBackground`]).
pub fn paint_cosmic_background(painter: &egui::Painter, rect: Rect) {
    // Base linear gradient 160deg: #07162d → #061325 → #04101f
    const STEPS: usize = 64;
    for i in 0..STEPS {
        let t = i as f32 / (STEPS - 1) as f32;
        let c = if t < 0.58 {
            let k = t / 0.58;
            lerp_color(
                Color32::from_rgb(0x07, 0x16, 0x2d),
                Color32::from_rgb(0x06, 0x13, 0x25),
                k,
            )
        } else {
            let k = (t - 0.58) / 0.42;
            lerp_color(
                Color32::from_rgb(0x06, 0x13, 0x25),
                Color32::from_rgb(0x04, 0x10, 0x1f),
                k,
            )
        };
        let y0 = rect.top() + rect.height() * i as f32 / STEPS as f32;
        let y1 = rect.top() + rect.height() * (i + 1) as f32 / STEPS as f32;
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(rect.left(), y0), Pos2::new(rect.right(), y1)),
            0,
            c,
        );
    }

    // radial-gradient at top-left: #0e3a72 0% → transparent 52%
    paint_radial(painter, rect, rect.left_top(), rect.width() * 0.75, Color32::from_rgb(0x0e, 0x3a, 0x72), 0.52);
    // radial-gradient at bottom-right: rgba(54,224,142,0.22) 0% → transparent 42%
    paint_radial_alpha(
        painter,
        rect,
        rect.right_bottom(),
        rect.width() * 0.6,
        Color32::from_rgb(0x36, 0xe0, 0x8e),
        0.22,
        0.42,
    );
}

fn paint_radial(
    painter: &egui::Painter,
    rect: Rect,
    center: Pos2,
    radius: f32,
    color: Color32,
    stop: f32,
) {
    paint_radial_alpha(painter, rect, center, radius, color, 1.0, stop);
}

fn paint_radial_alpha(
    painter: &egui::Painter,
    rect: Rect,
    center: Pos2,
    radius: f32,
    color: Color32,
    peak_alpha: f32,
    stop: f32,
) {
    const RINGS: usize = 26;
    let max_r = radius * stop;
    for i in 0..RINGS {
        let t0 = i as f32 / RINGS as f32;
        let t1 = (i + 1) as f32 / RINGS as f32;
        // Alpha falls off linearly to 0 at `stop`.
        let a = (1.0 - t1) * peak_alpha;
        if a <= 0.002 {
            continue;
        }
        let c = Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            (a * 255.0).clamp(0.0, 255.0) as u8,
        );
        let r0 = max_r * t0;
        let r1 = max_r * t1;
        let r = (r0 + r1) * 0.5;
        // A ring is drawn as a rounded rect to stay cheap.
        let ring = Rect::from_center_size(center, Vec2::splat(r * 2.0));
        painter.rect_filled(ring, CornerRadius::same(255), c);
    }
    let _ = rect;
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}


/// Truncate a string with an ellipsis so it fits `max_width`.
pub fn truncate(painter: &egui::Painter, text: &str, font: &FontId, max_width: f32) -> String {
    if painter
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
        <= max_width
    {
        return text.to_owned();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let candidate = format!("{out}{ch}…");
        let w = painter
            .layout_no_wrap(candidate.clone(), font.clone(), Color32::WHITE)
            .size()
            .x;
        if w > max_width {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        "…".to_owned()
    } else {
        format!("{out}…")
    }
}

/// Animated "floating grid" background: a slowly drifting wireframe with a
/// moving light field, for a sci-fi feel that stays out of the way of the UI.
pub fn paint_grid_background(painter: &egui::Painter, rect: Rect, time: f32) {
    const CELL: f32 = 46.0;

    // Grid phase drifts diagonally, so the mesh appears to float.
    let drift = Vec2::new(
        (time * 5.0).rem_euclid(CELL),
        (time * 2.6).rem_euclid(CELL),
    );

    // A light source wanders around, brightening nearby lines.
    let focus = Pos2::new(
        rect.center().x + (time * 0.11).sin() * rect.width() * 0.24,
        rect.center().y + (time * 0.07).cos() * rect.height() * 0.24,
    );
    let falloff = (rect.width().min(rect.height()) * 0.55).max(160.0);

    // Soft glow under the mesh.
    paint_radial_alpha(
        painter,
        rect,
        focus,
        falloff * 1.15,
        Color32::from_rgb(0x2f, 0x7d, 0xff),
        0.10,
        1.0,
    );

    let line_color = |distance: f32| {
        let t = (1.0 - (distance / falloff).min(1.0)).powi(3);
        Color32::from_rgba_unmultiplied(96, 165, 255, (14.0 + 120.0 * t) as u8)
    };
    let node_color = |distance: f32| {
        let t = (1.0 - (distance / falloff).min(1.0)).powi(4);
        Color32::from_rgba_unmultiplied(150, 205, 255, (110.0 * t) as u8)
    };

    // Vertical lines.
    let mut x = rect.left() - drift.x;
    while x <= rect.right() + CELL {
        let distance = (x - focus.x).abs();
        if x >= rect.left() - 0.5 {
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0, line_color(distance)),
            );
        }
        x += CELL;
    }

    // Horizontal lines.
    let mut y = rect.top() - drift.y;
    while y <= rect.bottom() + CELL {
        let distance = (y - focus.y).abs();
        if y >= rect.top() - 0.5 {
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(1.0, line_color(distance)),
            );
        }
        y += CELL;
    }

    // Bright nodes at the crossings nearest the light.
    let mut y = rect.top() - drift.y;
    while y <= rect.bottom() + CELL {
        if (y - focus.y).abs() < falloff {
            let mut x = rect.left() - drift.x;
            while x <= rect.right() + CELL {
                let d = Vec2::new(x - focus.x, y - focus.y).length();
                if d < falloff * 0.8 {
                    let color = node_color(d);
                    if color.a() > 6 {
                        painter.rect_filled(
                            Rect::from_center_size(Pos2::new(x, y), Vec2::splat(2.0)),
                            CornerRadius::same(1),
                            color,
                        );
                    }
                }
                x += CELL;
            }
        }
        y += CELL;
    }

    // A couple of slow "scan" arcs for extra motion.
    for (index, speed) in [0.05_f32, 0.031].into_iter().enumerate() {
        let phase = time * speed + index as f32 * 2.4;
        let radius = falloff * (0.55 + 0.35 * (phase.sin() * 0.5 + 0.5));
        let alpha = (10.0 + 18.0 * (phase * 1.7).sin().abs()) as u8;
        painter.circle_stroke(
            focus,
            radius,
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(0x57, 0xd8, 0xb2, alpha),
            ),
        );
    }
}

// ── egui style installation ──────────────────────────────────────────────────

/// Install the theme into an egui context (fonts + visuals + spacing).
pub fn install(ctx: &egui::Context, font_family: &str, font_size: f32) {
    install_fonts(ctx, font_family, font_size);
    install_visuals(ctx);
}

/// Register the vendored icon font, the system monospace font (terminal) and a
/// CJK-capable face.
///
/// The CJK face is installed as the **primary** proportional font on purpose:
/// mixing a Latin primary with a CJK fallback gives each text run a different
/// row height, which is exactly why labels used to look vertically off-centre.
/// One font covering both scripts keeps every row's metrics identical.
pub fn install_fonts(ctx: &egui::Context, mono_hint: &str, mono_size: f32) {
    let mut fonts = egui::FontDefinitions::default();

    // ── Icons (Phosphor, MIT — vendored in assets/) ──────────────────────────
    fonts.font_data.insert(
        "phosphor".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/Phosphor.ttf"
        ))),
    );

    // ── Monospace (terminal) ─────────────────────────────────────────────────
    let mut mono_name: Option<String> = None;
    for (path, index) in mono_candidates(mono_hint) {
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            let mut data = egui::FontData::from_owned(bytes);
            data.index = index;
            fonts
                .font_data
                .insert("TinyTermMono".to_owned(), std::sync::Arc::new(data));
            mono_name = Some("TinyTermMono".to_owned());
            break;
        }
    }

    // ── CJK ──────────────────────────────────────────────────────────────────
    let mut cjk_name: Option<String> = None;
    // Ordered by `lineGap` first: epaint adds the whole line gap *below* the
    // baseline, so a font with a large gap (Hiragino: 0.5 em) makes every label
    // sit visibly above the centre of its own line box. STHeiti declares 0.03 em
    // and therefore needs (almost) no compensation.
    for (path, index) in [
        ("/System/Library/Fonts/PingFang.ttc", 0u32),
        ("/System/Library/Fonts/STHeiti Medium.ttc", 0),
        ("/System/Library/Fonts/STHeiti Light.ttc", 0),
        ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
        ("/System/Library/Fonts/Supplemental/Songti.ttc", 0),
        ("/Library/Fonts/Arial Unicode.ttf", 0),
        // Windows: Microsoft YaHei, then the older SimSun / DengXian faces.
        ("C:\\Windows\\Fonts\\msyh.ttc", 0),
        ("C:\\Windows\\Fonts\\msyhl.ttc", 0),
        ("C:\\Windows\\Fonts\\Deng.ttf", 0),
        ("C:\\Windows\\Fonts\\simsun.ttc", 0),
        ("C:\\Windows\\Fonts\\simhei.ttf", 0),
    ] {
        if !std::path::Path::new(path).exists() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(path) {
            // A large `lineGap` (Hiragino declares 0.5 em) is added below the
            // baseline by epaint, which pushes glyphs above the centre of their
            // own line box. Compensate using the font's real metrics.
            let offset_factor = crate::font_metrics::read(&bytes, index)
                .map(|m| m.centring_offset_factor())
                .unwrap_or(0.0);
            let mut data = egui::FontData::from_owned(bytes);
            data.index = index;
            if offset_factor > 0.005 {
                data = data.tweak(egui::FontTweak {
                    y_offset_factor: offset_factor,
                    ..Default::default()
                });
            }
            fonts
                .font_data
                .insert("TinyTermCJK".to_owned(), std::sync::Arc::new(data));
            cjk_name = Some("TinyTermCJK".to_owned());
            break;
        }
    }

    // ── Family order ─────────────────────────────────────────────────────────
    {
        let family = fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default();
        if let Some(cjk) = &cjk_name {
            family.insert(0, cjk.clone());
            family.insert(1, "phosphor".to_owned());
        } else {
            family.insert(0, "phosphor".to_owned());
        }
    }
    {
        let family = fonts.families.entry(FontFamily::Monospace).or_default();
        let mut order: Vec<String> = Vec::new();
        if let Some(mono) = &mono_name {
            order.push(mono.clone());
        }
        order.push("phosphor".to_owned());
        if let Some(cjk) = &cjk_name {
            order.push(cjk.clone());
        }
        for (i, name) in order.into_iter().enumerate() {
            family.insert(i, name);
        }
    }

    ctx.set_fonts(fonts);

    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Small, font_sans(TEXT_XS)),
        (TextStyle::Body, font_sans(TEXT_SM)),
        (TextStyle::Button, font_sans(TEXT_SM)),
        (TextStyle::Heading, font_sans(TEXT_LG)),
        (TextStyle::Monospace, font_mono(mono_size)),
    ]
    .into();
    ctx.set_global_style(style);
}

fn mono_candidates(hint: &str) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    let hint_l = hint.to_lowercase();
    if hint_l.contains("menlo") || hint_l.contains("monaco") || hint.is_empty() {
        out.push(("/System/Library/Fonts/Menlo.ttc".into(), 0));
        out.push(("/System/Library/Fonts/Monaco.ttf".into(), 0));
    }
    if hint_l.contains("sf mono") || hint_l.contains("sfmono") {
        out.push(("/System/Library/Fonts/SFNSMono.ttf".into(), 0));
    }
    // Generic fallbacks, always appended.
    // Windows: Consolas, then Lucida Console / Courier New.
    out.push(("C:\\Windows\\Fonts\\consola.ttf".into(), 0));
    out.push(("C:\\Windows\\Fonts\\lucon.ttf".into(), 0));
    out.push(("C:\\Windows\\Fonts\\cour.ttf".into(), 0));
    out.push(("/System/Library/Fonts/Menlo.ttc".into(), 0));
    out.push(("/System/Library/Fonts/SFNSMono.ttf".into(), 0));
    out.push(("/System/Library/Fonts/Monaco.ttf".into(), 0));
    out.push((
        "/System/Library/Fonts/Supplemental/Andale Mono.ttf".into(),
        0,
    ));
    out
}

pub fn install_visuals(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();

    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::TRANSPARENT;
    style.visuals.window_fill = BG_CARD;
    style.visuals.window_stroke = Stroke::new(1.0, BORDER);
    style.visuals.window_corner_radius = CornerRadius::same(RADIUS_LG);
    style.visuals.extreme_bg_color = BG_INPUT;
    style.visuals.faint_bg_color = Color32::from_rgba_premultiplied(9, 24, 44, 120);
    style.visuals.override_text_color = Some(TEXT_PRIMARY);
    style.visuals.selection.bg_fill = Color32::from_rgba_premultiplied(29, 57, 102, 140);
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT_HOVER);
    style.visuals.hyperlink_color = ACCENT_LIGHT;
    style.visuals.warn_fg_color = WARNING;
    style.visuals.error_fg_color = ERROR;

    let w = &mut style.visuals.widgets;
    w.noninteractive.bg_fill = BG_PANEL;
    w.noninteractive.weak_bg_fill = BG_PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    w.noninteractive.corner_radius = CornerRadius::same(RADIUS_SM);

    w.inactive.bg_fill = Color32::from_rgba_premultiplied(13, 30, 54, 130);
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
    w.inactive.corner_radius = CornerRadius::same(RADIUS_SM);

    w.hovered.bg_fill = Color32::from_rgba_premultiplied(22, 49, 96, 160);
    w.hovered.weak_bg_fill = Color32::from_rgba_premultiplied(22, 49, 96, 130);
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER_ACTIVE);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    w.hovered.corner_radius = CornerRadius::same(RADIUS_SM);

    w.active.bg_fill = Color32::from_rgba_premultiplied(28, 61, 117, 190);
    w.active.weak_bg_fill = Color32::from_rgba_premultiplied(28, 61, 117, 170);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT_HOVER);
    w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    w.active.corner_radius = CornerRadius::same(RADIUS_SM);

    w.open.bg_fill = BG_CARD;
    w.open.bg_stroke = Stroke::new(1.0, BORDER);

    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(0i8);
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    style.spacing.scroll.bar_width = 6.0;
    style.spacing.scroll.bar_inner_margin = 2.0;
    style.spacing.scroll.bar_outer_margin = 0.0;
    style.spacing.scroll.floating = false;

    ctx.set_global_style(style);
}
