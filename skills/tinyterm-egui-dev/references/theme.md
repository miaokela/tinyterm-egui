# Theme: cosmic / glassmorphism for egui

Source of truth: `src/theme.rs` (tokens + fonts + visuals), `src/widgets.rs`
(self-drawn controls). Every value below is copied from those files; they are a
1:1 port of the original web client's `global.css` `:root` variables.

## 1. Colour tokens

```rust
// backgrounds
BG_PRIMARY      #07162d                       // window backdrop
BG_SECONDARY    #0b1f3d
BG_PANEL        rgba(7,22,43,0.86)            // frosted panel
BG_CARD         #0c1f38                       // modal / card fill
BG_INPUT        rgba(6,18,36,0.82)
TERMINAL_BG     #050b14

// accent
ACCENT          #2f7dff   // primary electric blue
ACCENT_HOVER    #6ab5ff
ACCENT_ACTIVE   #1c5fcc
ACCENT_LIGHT    #80b0ff
ACCENT_2        #57d8b2   // secondary mint/teal

// text
TEXT_PRIMARY    #e7eff9
TEXT_SECONDARY  #a8bdd1
TEXT_MUTED      #7d93a9

// borders / status
BORDER          rgba(58,132,255,0.30)
BORDER_ACTIVE   rgba(112,191,255,0.60)
SUCCESS         #57e3a5
ERROR           #e0575c
WARNING         #f0a040

// terminal
TERMINAL_TEXT   #d7e3f0
TERM_FG         #b6c5d3
TERM_CURSOR     #ffbf69
TERM_SELECTION  rgba(115,167,255,0.24)
```

> In `theme.rs` translucent colours are declared with
> `Color32::from_rgba_premultiplied(r*a/255, g*a/255, b*a/255, a)` — egui's
> `from_rgba_unmultiplied` is **not** interchangeable when you also paint with
> `StrokeKind::Inside`, because the premultiplied form is what the painter
> expects for correct compositing over the starfield.

### Terminal ANSI palette (`theme::ANSI`)

| # | normal | bright |
|---|---|---|
| 0 black | `#16202b` | `#55606d` |
| 1 red | `#ef6b73` | `#ff8b94` |
| 2 green | `#7ccf92` | `#99e6a8` |
| 3 yellow | `#e7c36f` | `#ffd98a` |
| 4 blue | `#73a7ff` | `#94c2ff` |
| 5 magenta | `#c792ea` | `#ddb3ff` |
| 6 cyan | `#66c7d1` | `#8be0e8` |
| 7 white | `#b7c4cf` | `#d7e1ea` |

## 2. Geometry and type scale

```rust
RADIUS_XS 4   RADIUS_SM 8   RADIUS_MD 12   RADIUS_LG 16   // CornerRadius (u8!)

TEXT_XS 12.0  TEXT_SM 13.0  TEXT_MD 14.0  TEXT_LG 16.0  TEXT_XL 20.0

SIDEBAR_WIDTH            200.0
SIDEBAR_COLLAPSED_WIDTH   48.0
TABSTRIP_HEIGHT           38.0
CHROME_TAB_HEIGHT         30.0
APP_BODY_PADDING          10.0
APP_BODY_TOP_PADDING       6.0
APP_BODY_GAP               8.0
```

Helper `theme::glow(painter, rect, radius, color, strength)` paints the neon
outer glow (`0 0 24px rgba(66,157,255,.28)` in CSS). Use it on hover/active
states only — it is expensive and looks noisy if applied everywhere.

## 3. Installing fonts (`theme::install_fonts`)

Three faces are registered into `egui::FontDefinitions`:

1. `"phosphor"` — vendored `assets/Phosphor.ttf` (MIT), always present.
2. `"TinyTermMono"` — first existing system monospace face (terminal).
3. `"TinyTermCJK"` — first existing CJK face, installed as the **primary
   proportional font**.

```rust
// Family order that keeps Latin + CJK on identical row metrics.
Proportional: [TinyTermCJK, phosphor, <egui defaults>]
Monospace:    [TinyTermMono, phosphor, TinyTermCJK, <egui defaults>]
```

Font candidate lists are platform-aware:

| Role | macOS | Windows |
|---|---|---|
| CJK | PingFang → STHeiti Medium → STHeiti Light → Hiragino Sans GB → Songti → Arial Unicode | msyh.ttc (雅黑) → msyhl.ttc → Deng.ttf → simsun.ttc → simhei.ttf |
| Mono | Menlo → SFNSMono → Monaco → Andale Mono | Consolas → Lucida Console → Courier New |

Missing candidates are skipped silently; if none exists the app still runs with
egui's bundled fonts (Chinese then renders as boxes — that is the symptom of a
missing CJK face, not a bug in the UI code).

### Vertical centring fix (important)

epaint adds a font's whole `lineGap` **below** the baseline, so a face that
declares a large gap (Hiragino Sans GB: 0.5 em) pushes every glyph above the
centre of its line box. `font_metrics::read(bytes, index)` parses `hhea`/`OS/2`
and `centring_offset_factor()` returns `lineGap / 2`, which is applied as:

```rust
let mut data = egui::FontData::from_owned(bytes);
data.index = ttc_index;                       // .ttc collection index
if offset_factor > 0.005 {
    data = data.tweak(egui::FontTweak { y_offset_factor: offset_factor, ..Default::default() });
}
```

Do this whenever you install a new system face, otherwise text sits visibly high
in buttons and rows.

## 4. Visuals (`theme::install_visuals`)

```rust
style.visuals = egui::Visuals::dark();
style.visuals.panel_fill  = Color32::TRANSPARENT;   // the starfield shows through
style.visuals.window_fill = theme::BG_CARD;
style.visuals.window_stroke = Stroke::new(1.0, theme::BORDER);
```

The backdrop is painted by `theme::paint_cosmic_background` +
`theme::paint_grid_background` (drifting grid), called once per frame from
`app.rs` before any panel.

## 5. Widget recipes (`src/widgets.rs`)

All controls allocate their own rect and paint themselves; they return
`egui::Response` so callers can still use `.clicked()`, `.hovered()`, etc.

| Function | Notes |
|---|---|
| `primary_button(ui, label, enabled)` | 30 px tall, width = text + 40, accent fill, glow on hover, 3 disabled states |
| `ghost_button(ui, label, enabled)` | transparent fill + border, used for every dialog action (「取消」等) |
| `button_row(ui, rect, &[(label, enabled)]) -> Option<usize>` | right-aligned dialog action row; returns the clicked index |
| `orb_button(ui, size, draw)` | ghost circular icon button: nearly transparent at rest, with four rotating HUD brackets inside the rim; hover adds accent tint, accent border, glow and a brighter glyph (host-row 连接, collapse toggles) |
| `primary_orb_button(ui, size, draw)` | accent-filled orb for a panel's primary action (新增主机 / 新增凭据): solid disc, bright rim, a radar sweep along the rim and a breathing halo — keep the centre clear for the glyph. Use it instead of filling a circle by hand |
| `tick_ring(painter, center, radius, color, width, segments, span, phase)` | private helper behind both orbs — a ring of short arc segments; `phase` spins it (`ui.input(\|i\| i.time)`), which works because the app already repaints at ~30 fps for the drifting background |
| `tint(color, alpha)` | premultiplied translucent variant of a theme colour; use it rather than `from_rgba_unmultiplied`, which reads muddy through the painter |
| `text_input(ui, value, placeholder, width, password)` | 30 px tall, `BG_INPUT` fill, accent border when focused |
| `text_input_ext(..., leading_icon)` | same, with the text inset 30 px so a magnifier never overlaps the placeholder |
| `text_area(ui, value, placeholder, width, height)` | multi-line (private keys) |
| `labelled(ui, label, required)` | field label, `*` in accent for required |
| `status_dot` / `icon` / `chevron` / `cross` / `plus` / `folder_glyph` / `file_glyph` / `gear` / `pencil` / `trash` / `copy_glyph` / `plug` / `key_glyph` / `lock_glyph` | painter-level glyphs; prefer these over ad-hoc text |
| `spinner(painter, center, radius, t, color)` / `css_spinner(ui, size)` | rotating arc, driven by `ui.input(|i| i.time)` |
| `loading_blocks(ui, time, scale)` | the 3-block loading animation |
| `stroke_open(painter, rect, radius, stroke, open_side)` | rounded rect with one side left open (collapsed panel headers) |

### Pattern: measure text, then allocate

```rust
let font = theme::f_sm();
let text_w = ui.painter()
    .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE)
    .size().x;
let (rect, response) = ui.allocate_exact_size(Vec2::new(text_w + 40.0, 30.0), Sense::click());
if !ui.is_rect_visible(rect) { return response; }   // skip work when scrolled away
```

### Pattern: inset text over a hand-drawn frame

```rust
painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_SM), theme::BG_INPUT);
painter.rect_stroke(rect, CornerRadius::same(theme::RADIUS_SM),
    Stroke::new(1.0, if focused { theme::ACCENT } else { theme::BORDER }),
    StrokeKind::Inside);
// then a TextEdit with its own frame disabled, placed inside an inset rect:
let inner = Rect::from_min_max(
    Pos2::new(rect.left() + left_pad, rect.top() + 4.0),
    Pos2::new(rect.right() - 10.0, rect.bottom() - 4.0));
ui.put(inner, egui::TextEdit::singleline(value).frame(egui::Frame::NONE));
```

## 6. egui 0.36 API notes

- `eframe::App` has **`fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame)`**,
  not `update(ctx, frame)`. Draw the background from the root `Ui`.
- `ctx.global_style()` / `ctx.set_global_style()` replaced `ctx.style()`.
- `CornerRadius` components are **`u8`**, not `f32`.
- `TextEdit::frame(egui::Frame::NONE)` — `Frame::none()` is gone.
- Measure text without a frame: `ctx.run_ui(egui::RawInput::default(), |ui| { … })`.
- Viewport size: `ctx.input(|i| i.viewport_rect())`.
- Popups/context menus: `egui::Area::new(egui::Id::new(...)).order(egui::Order::Foreground)`
  — anything that must float above a collapsed panel header needs `Foreground`.

## 7. Pitfalls that cost real debugging time

| Symptom | Cause / fix |
|---|---|
| A diagonal line across a panel's rounded corner (and across its open side) | `stroke_open`'s corner arc was traversed backwards, so the polyline jumped between the arc's start and the previous point. Arcs must run `from → to` in the direction that continues the outline. |
| Buttons stack vertically in a dialog footer | A `Ui` created with `ui.new_child(UiBuilder::new().max_rect(r))` **inherits the parent's top-down layout**; pass `.layout(Layout::right_to_left(Align::Center))`. |
| Two rows share hover/click state | Duplicate widget ids. Give repeated controls `ui.push_id(index, |ui| …)` or fold the session id into the id. |
| Text looks vertically high in labels | Font `lineGap`; see §3 (`FontTweak.y_offset_factor`). |
| Placeholder overlaps a leading icon | Inset the text rect (`text_input_ext`) instead of only padding the frame. |
| Context menu disappears behind a collapsed sidebar | Draw it in an `Area` with `Order::Foreground`. |
| Selection copy returns nothing | The selection state must live where `selected_text()` reads it — keep one source of truth (`Terminal.selection`), not a copy in the UI state. |
| Progress percentage never updates | Compare against `Option<u64>` (`last_pct != Some(pct)`), never a sentinel like `u64::MAX`. |
