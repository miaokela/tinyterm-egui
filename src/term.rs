//! Terminal emulation (vt100) + egui rendering + key encoding.
//!
//! The web client uses xterm.js; this module provides the equivalent surface:
//! a `vt100::Parser` fed by the SSH reader task, a cell-accurate painter, and
//! a `keydown → bytes` encoder ported from `TerminalView.tsx::encodeKeyEvent`.

use crate::theme;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Vec2};
use std::sync::Arc;
use vt100::{Color as VtColor, Parser, Screen};

/// Cell metrics for the current font size, cached per (font_size, family).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
    pub baseline: f32,
}

impl CellMetrics {
    pub fn measure(ctx: &egui::Context, font_size: f32) -> Self {
        let font = FontId::new(font_size, egui::FontFamily::Monospace);
        ctx.fonts_mut(|f| {
            let width = f.glyph_width(&font, 'M').max(1.0);
            let height = f.row_height(&font);
            let baseline = height * 0.8;
            Self {
                width,
                height,
                baseline,
            }
        })
    }
}

/// Map a vt100 colour to an egui colour.
pub fn vt_color(color: VtColor, default: Color32) -> Color32 {
    match color {
        VtColor::Default => default,
        VtColor::Idx(i) => idx_color(i),
        VtColor::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// xterm-256 palette: 0-15 named, 16-231 6×6×6 cube, 232-255 grayscale ramp.
pub fn idx_color(i: u8) -> Color32 {
    match i {
        0..=15 => theme::ANSI[i as usize],
        16..=231 => {
            let i = i - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let scale = |v: u8| -> u8 {
                if v == 0 {
                    0
                } else {
                    (55 + 40 * v as u16).min(255) as u8
                }
            };
            Color32::from_rgb(scale(r), scale(g), scale(b))
        }
        232..=255 => {
            let v = 8 + (i as u16 - 232) * 10;
            Color32::from_rgb(v as u8, v as u8, v as u8)
        }
    }
}

/// A run of adjacent cells sharing identical attributes.
struct Run {
    col: u16,
    len: u16,
    fg: Color32,
    bg: Color32,
    bold: bool,
    italic: bool,
    underline: bool,
}

/// Terminal scroll/selection state kept by the UI (not by vt100).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Selection {
    pub start: (u16, u16),
    pub end: (u16, u16),
    pub active: bool,
}

impl Selection {
    pub fn normalized(&self) -> ((u16, u16), (u16, u16)) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn contains(&self, row: u16, col: u16) -> bool {
        let (a, b) = self.normalized();
        (row, col) >= a && (row, col) <= b
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

pub struct Terminal {
    pub parser: Parser,
    /// Rows scrolled back from the live view (0 = live).
    pub scroll_offset: usize,
}

impl Terminal {
    pub fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        Self {
            parser: Parser::new(rows, cols, scrollback),
            scroll_offset: 0,
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        // Jump back to the live view when new output arrives while scrolled up
        // only if the user is already at the bottom.
    }

    pub fn screen(&self) -> &Screen {
        self.parser.screen()
    }

    pub fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (r, c) = self.size();
        if r != rows || c != cols {
            self.parser.screen_mut().set_size(rows, cols);
        }
    }

    /// Apply the scroll offset to the screen (must be called before rendering).
    pub fn apply_scroll(&mut self) {
        let max = self.parser.screen().scrollback();
        if self.scroll_offset > max {
            self.scroll_offset = max;
        }
        self.parser.screen_mut().set_scrollback(self.scroll_offset);
    }

    pub fn scroll_by(&mut self, delta: i32) {
        let max = self.parser.screen().scrollback();
        let next = (self.scroll_offset as i32 + delta).clamp(0, max as i32);
        self.scroll_offset = next as usize;
        self.parser.screen_mut().set_scrollback(self.scroll_offset);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.parser.screen_mut().set_scrollback(0);
    }

    /// Text between two cells. The selection lives in the UI state, so it is
    /// passed in rather than read from `self`.
    pub fn selected_text(&self, selection: Selection) -> String {
        if selection.is_empty() {
            return String::new();
        }
        let (a, b) = selection.normalized();
        self.parser
            .screen()
            .contents_between(a.0, a.1, b.0, b.1)
            .trim_end()
            .to_string()
    }

    /// Number of columns/rows that fit in `rect` for the given metrics.
    pub fn fit(rect: Rect, metrics: CellMetrics) -> (u16, u16) {
        let cols = (rect.width() / metrics.width).floor().max(1.0) as u16;
        let rows = (rect.height() / metrics.height).floor().max(1.0) as u16;
        (cols.max(2), rows.max(2))
    }

    /// Cell coordinates for a screen position, clamped to the grid.
    pub fn cell_at(rect: Rect, metrics: CellMetrics, pos: Pos2, cols: u16, rows: u16) -> (u16, u16) {
        let col = ((pos.x - rect.left()) / metrics.width).floor();
        let row = ((pos.y - rect.top()) / metrics.height).floor();
        (
            col.clamp(0.0, (cols.saturating_sub(1)) as f32) as u16,
            row.clamp(0.0, (rows.saturating_sub(1)) as f32) as u16,
        )
    }
}

/// Result of painting one frame.
pub struct PaintResult {
    pub used_cols: u16,
    pub used_rows: u16,
}

/// Paint the terminal grid into `rect`.
pub fn paint(
    painter: &egui::Painter,
    rect: Rect,
    term: &Terminal,
    font_size: f32,
    metrics: CellMetrics,
    cursor_blink_on: bool,
    selection: Selection,
) -> PaintResult {
    let screen = term.screen();
    let (rows, cols) = screen.size();
    let font = FontId::new(font_size, egui::FontFamily::Monospace);

    // Background
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_XS), theme::TERMINAL_BG);

    let cursor = screen.cursor_position();
    let cursor_hidden = screen.hide_cursor();
    let show_cursor = !cursor_hidden && term.scroll_offset == 0;

    for row in 0..rows {
        let y = rect.top() + row as f32 * metrics.height;
        if y > rect.bottom() {
            break;
        }

        // ── Collect runs ─────────────────────────────────────────────────────
        let mut runs: Vec<Run> = Vec::new();
        let mut col = 0u16;
        while col < cols {
            let Some(cell) = screen.cell(row, col) else {
                col += 1;
                continue;
            };
            let mut fg = vt_color(cell.fgcolor(), theme::TERM_FG);
            let mut bg = vt_color(cell.bgcolor(), theme::TERMINAL_BG);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.dim() {
                fg = Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 160);
            }
            let run = Run {
                col,
                len: 1,
                fg,
                bg,
                bold: cell.bold(),
                italic: cell.italic(),
                underline: cell.underline(),
            };
            match runs.last_mut() {
                Some(prev)
                    if prev.fg == run.fg
                        && prev.bg == run.bg
                        && prev.bold == run.bold
                        && prev.italic == run.italic
                        && prev.underline == run.underline =>
                {
                    prev.len += 1;
                }
                _ => runs.push(run),
            }
            col += 1;
        }

        // ── Paint backgrounds ────────────────────────────────────────────────
        for run in &runs {
            if run.bg == theme::TERMINAL_BG {
                continue;
            }
            let x0 = rect.left() + run.col as f32 * metrics.width;
            let x1 = x0 + run.len as f32 * metrics.width;
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x0, y),
                    Pos2::new(x1.min(rect.right()), y + metrics.height),
                ),
                0,
                run.bg,
            );
        }

        // ── Selection highlight ──────────────────────────────────────────────
        if !selection.is_empty() {
            let (a, b) = selection.normalized();
            if row >= a.0 && row <= b.0 {
                let start_col = if row == a.0 { a.1 } else { 0 };
                let end_col = if row == b.0 { b.1 } else { cols.saturating_sub(1) };
                let x0 = rect.left() + start_col as f32 * metrics.width;
                let x1 = rect.left() + (end_col + 1) as f32 * metrics.width;
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(x0, y),
                        Pos2::new(x1.min(rect.right()), y + metrics.height),
                    ),
                    0,
                    theme::TERM_SELECTION,
                );
            }
        }

        // ── Paint glyphs ─────────────────────────────────────────────────────
        for run in &runs {
            let mut text = String::new();
            for c in run.col..run.col + run.len {
                if let Some(cell) = screen.cell(row, c) {
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let contents = cell.contents();
                    if contents.is_empty() {
                        text.push(' ');
                    } else {
                        text.push_str(&contents);
                    }
                }
            }
            if text.trim().is_empty() {
                continue;
            }
            let x = rect.left() + run.col as f32 * metrics.width;
            let font_id = if run.bold {
                font.clone()
            } else if run.italic {
                font.clone()
            } else {
                font.clone()
            };
            let galley = painter.layout_no_wrap(text, font_id, run.fg);
            let pos = Pos2::new(x, y + (metrics.height - galley.size().y) * 0.5);
            painter.galley(pos, galley.clone(), run.fg);
            if run.underline {
                painter.line_segment(
                    [
                        Pos2::new(x, y + metrics.height - 1.5),
                        Pos2::new(x + run.len as f32 * metrics.width, y + metrics.height - 1.5),
                    ],
                    Stroke::new(1.0, run.fg),
                );
            }
        }

        // ── Cursor ───────────────────────────────────────────────────────────
        if show_cursor && cursor.0 == row && cursor_blink_on {
            let x = rect.left() + cursor.1 as f32 * metrics.width;
            let cursor_rect = Rect::from_min_size(
                Pos2::new(x, y),
                Vec2::new(metrics.width, metrics.height),
            );
            painter.rect_filled(cursor_rect, 0, theme::TERM_CURSOR);
            // Re-draw the glyph under the cursor in the terminal background
            // colour so a block cursor does not hide the character.
            if let Some(cell) = screen.cell(cursor.0, cursor.1) {
                let contents = cell.contents();
                if !contents.is_empty() {
                    painter.text(
                        cursor_rect.center(),
                        Align2::CENTER_CENTER,
                        contents,
                        font.clone(),
                        theme::TERMINAL_BG,
                    );
                }
            }
        }
    }

    PaintResult {
        used_cols: cols,
        used_rows: rows,
    }
}

// ── Key encoding ─────────────────────────────────────────────────────────────

/// Port of `encodeKeyEvent` from `TerminalView.tsx`.
///
/// `key` is the logical key name (`egui::Key::name()` normalised), `text` is the
/// printable text egui produced for this event (may be empty).
pub fn encode_key(
    key: Option<egui::Key>,
    text: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    app_cursor_keys: bool,
) -> Option<String> {
    // Ctrl+<letter> → control code.
    if ctrl && !alt {
        if let Some(k) = key {
            if let Some(ch) = key_letter(k) {
                if shift {
                    return Some(ch.to_ascii_uppercase().to_string());
                }
                let code = ch.to_ascii_uppercase() as u8;
                return Some(((code - 64) as char).to_string());
            }
        }
        if let Some(ch) = text.chars().next() {
            match ch {
                ' ' => return Some("\x00".into()),
                '[' => return Some("\x1b".into()),
                '\\' => return Some("\x1c".into()),
                ']' => return Some("\x1d".into()),
                '^' => return Some("\x1e".into()),
                '_' => return Some("\x1f".into()),
                _ => {}
            }
        }
    }

    let esc = if app_cursor_keys { "\x1bO" } else { "\x1b[" };

    if let Some(k) = key {
        use egui::Key::*;
        let s: Option<String> = match k {
            Enter => Some("\r".into()),
            Tab => Some(if shift { "\x1b[Z".into() } else { "\t".into() }),
            Backspace => Some("\x7f".into()),
            Escape => Some("\x1b".into()),
            ArrowUp => Some(format!("{esc}A")),
            ArrowDown => Some(format!("{esc}B")),
            ArrowRight => Some(format!("{esc}C")),
            ArrowLeft => Some(format!("{esc}D")),
            Home => Some(format!("{esc}H")),
            End => Some(format!("{esc}F")),
            Delete => Some("\x1b[3~".into()),
            Insert => Some("\x1b[2~".into()),
            PageUp => Some("\x1b[5~".into()),
            PageDown => Some("\x1b[6~".into()),
            _ => None,
        };
        if s.is_some() {
            return s;
        }
    }

    if !ctrl && !alt {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    if alt && !ctrl && !text.is_empty() {
        return Some(format!("\x1b{text}"));
    }
    None
}

fn key_letter(key: egui::Key) -> Option<char> {
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
        _ => return None,
    })
}

/// Shared handle to a terminal owned by a background session task.
pub type SharedTerminal = Arc<parking_lot::Mutex<Terminal>>;
