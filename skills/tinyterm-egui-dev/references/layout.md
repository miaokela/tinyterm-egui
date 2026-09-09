# Layout: panels, regions and spacing

Composition lives in `src/app.rs` (`main_layout` → `content` → terminal / file
manager), sizes in `src/theme.rs`, view state in `src/state.rs`.

## 1. Window

```rust
ViewportBuilder::default()
    .with_inner_size([1100.0, 720.0])
    .with_min_inner_size([880.0, 700.0])
    .with_title("TinyTerm")
```

The whole app is drawn from the root `Ui` of `eframe::App::ui`; there are **no
`SidePanel`/`CentralPanel`** — every region is an explicit `Rect`. That keeps the
glass panels, the gaps between them, and the z-order fully under control.

Default zoom is `APP_ZOOM_MIN = 0.8` (range 0.8–1.4, step 0.1), applied with
`ctx.set_zoom_factor` and persisted next to the database (`zoom.txt`).

## 2. Region tree

```
viewport
└── body                = viewport inset by 10 px (bottom/sides) and 6 px (top)
    ├── sidebar         = 200 px wide (48 px collapsed), full body height
    └── content         = body minus sidebar minus 8 px gap
        ├── tabstrip    = 38 px tall  (session tabs of the active host)
        └── workspace   = content below the tabstrip
            ├── terminal area  (top, flexes)
            └── file manager   (bottom, FM_BAR_HEIGHT..FM_CONTENT_HEIGHT + bar)
                                separated from the terminal by a 6 px gap
```

```rust
let body = Rect::from_min_max(
    full.min + vec2(APP_BODY_PADDING, APP_BODY_TOP_PADDING),
    full.max - vec2(APP_BODY_PADDING, APP_BODY_PADDING));

let sidebar_rect = Rect::from_min_size(body.min, vec2(sidebar_width, body.height()));
let content_rect = Rect::from_min_max(
    pos2(sidebar_rect.right() + APP_BODY_GAP, body.top()), body.right_bottom());
```

## 3. Sizes that matter

| Constant | Value | Meaning |
|---|---|---|
| `APP_BODY_PADDING` | 10 | outer margin of the whole app |
| `APP_BODY_TOP_PADDING` | 6 | tighter top margin |
| `APP_BODY_GAP` | 8 | sidebar ↔ content |
| `SIDEBAR_WIDTH` / `_COLLAPSED_` | 200 / 48 | host list |
| `TABSTRIP_HEIGHT` | 38 | session tab strip |
| `CHROME_TAB_HEIGHT` | 30 | host tabs inside the strip |
| `FM_BAR_HEIGHT` | 28 | collapsed file manager header |
| `FM_CONTENT_HEIGHT` | 260 | file manager body when open |
| terminal ↔ file manager gap | 6 | hard-coded in `app.rs` |
| side-terminal gap | 6 | split inside the terminal area |

The file manager height is clamped so it can never eat the terminal:

```rust
let fm_height = (FM_CONTENT_HEIGHT + FM_BAR_HEIGHT)
    .min((workspace.height() - 80.0).max(FM_BAR_HEIGHT));
```

Keep the 6 px gap in **both** states — a collapsed file manager must still read
as a separate rounded bar, not as part of the terminal panel.

## 4. Panel chrome

```rust
// terminal area: rounded only at the bottom, flush with the tab strip on top
let cr = CornerRadius { nw: 0, ne: 0, sw: RADIUS_MD, se: RADIUS_MD };
painter.rect_filled(terminal_area, cr, theme::BG_PANEL);
widgets::stroke_open(&painter, terminal_area, RADIUS_MD,
    Stroke::new(1.0, theme::BORDER), widgets::OpenSide::Top);

// inner terminal surface, inset 4 px
painter.rect_filled(terminal_area.shrink(4.0),
    CornerRadius::same(RADIUS_XS), theme::TERMINAL_BG);
```

Rules:

- `theme::glass_panel(&painter, rect, radius)` for every card/modal.
- A 4 px inner inset around terminal content keeps the glass border readable.
- Collapsed headers use `widgets::stroke_open(..., OpenSide::Top)` so the panel
  appears open toward the content below.

## 5. Overlays and z-order

All modals, context menus and toasts are drawn in a single foreground `Area`
covering the viewport, after the main layout:

```rust
egui::Area::new(egui::Id::new("modals"))
    .order(egui::Order::Foreground)
    .fixed_pos(egui::Pos2::ZERO)
    .show(&ctx, |ui| {
        ui.set_clip_rect(ctx.input(|i| i.viewport_rect()));
        ui::dialogs::confirm_dialog(ui, &mut state);
        ui::toast::show(ui, &mut state);
        …
    });
```

- Anything drawn inside a collapsed panel's rect is clipped by that panel —
  context menus **must** use `Order::Foreground`.
- `ui.set_clip_rect(ui.max_rect())` at the start of `ui()` prevents content from
  bleeding outside the window while resizing.
- Dialog height is measured before painting (`dialogs::measure_dialog_height`) so
  buttons never overflow the card.

## 6. Empty and loading states

- No host tabs → full-width glass panel with the logo, "点击 + 新建终端连接".
- Host tab with no sessions → the same empty state inside the workspace.
- Loading a directory → `widgets::loading_blocks` in the list area, never a
  blocking overlay.
- Connection states (`connecting`, `error`, `closed`) are drawn as a status
  overlay inside the terminal rect, keeping the grid visible underneath.

## 7. Interaction details

| Behaviour | Implementation |
|---|---|
| Sidebar collapse | toggles `state.sidebar_collapsed`; width switches 200 ↔ 48, icons stay centred |
| File manager resize | drag handle updates a persisted height; clamped as in §3 |
| Side terminal | `session.side_terminal_open` splits the inner terminal rect 50/50 with a 6 px gap |
| Zoom | `Cmd/Ctrl + +/-/0`, persisted, re-applied every frame if it drifted |
| Tab close | middle-click or the ✕ glyph; never closes the last session silently |
