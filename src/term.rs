//! Terminal emulation (vt100) + egui rendering + key encoding.
//!
//! The web client uses xterm.js; this module provides the equivalent surface:
//! a `vt100::Parser` fed by the SSH reader task, a cell-accurate painter, and
//! a `keydown → bytes` encoder ported from `TerminalView.tsx::encodeKeyEvent`.

use crate::theme;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Vec2};
use std::collections::BTreeMap;
use std::sync::Arc;
use vt100::{Color as VtColor, Parser, Screen};

/// How many alternate-screen lines are kept for selection copy. The alternate
/// screen has no scrollback in vt100, so rows that scroll off its top are
/// captured here.
const ALT_HISTORY_LIMIT: usize = 8192;

/// Terminal cursor shape, driven by `Settings::cursor_style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    /// Filled cell with the glyph re-drawn in the background colour.
    Block,
    /// Thin vertical bar on the left edge of the cell.
    Bar,
    /// Short line along the bottom of the cell (the default).
    #[default]
    Underline,
}

impl CursorStyle {
    /// Map the persisted `cursor_style` value to a shape. Unknown values fall
    /// back to the default underline rather than rendering nothing.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "block" => Self::Block,
            "bar" | "beam" => Self::Bar,
            _ => Self::Underline,
        }
    }
}

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

/// Terminal selection kept by the UI (not by vt100).
///
/// Endpoints are *absolute* line numbers in the same coordinate space as
/// [`Terminal::line_top`], not visible rows. That lets a selection keep its
/// anchor while the view auto-scrolls underneath it, which is what makes a
/// selection taller than the pane possible.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Selection {
    pub start: (i64, u16),
    pub end: (i64, u16),
    pub active: bool,
}

impl Selection {
    pub fn normalized(&self) -> ((i64, u16), (i64, u16)) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn contains(&self, line: i64, col: u16) -> bool {
        let (a, b) = self.normalized();
        (line, col) >= a && (line, col) <= b
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// One captured alternate-screen row: its text plus the wrap flag, which
/// decides whether the row is joined to the next one when copying.
#[derive(Debug, Clone)]
struct CapturedRow {
    text: String,
    wrapped: bool,
}

pub struct Terminal {
    pub parser: Parser,
    /// Rows scrolled back from the live view (0 = live).
    pub scroll_offset: usize,
    /// Number of lines in the primary screen's scrollback. Mirrored so absolute
    /// line numbers can be computed on the UI thread without moving the view.
    history_len: usize,
    /// Rows of the alternate screen as of the last processed output chunk.
    alt_prev: Vec<CapturedRow>,
    /// Absolute line number of the first visible alternate-screen row. May go
    /// negative: dragging upwards reveals lines that existed before the drag
    /// started, which have no captured history yet.
    alt_top: i64,
    /// Alternate-screen lines that scrolled off the top, keyed by absolute line.
    alt_history: BTreeMap<i64, CapturedRow>,
}

impl Terminal {
    pub fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        Self {
            parser: Parser::new(rows, cols, scrollback),
            scroll_offset: 0,
            history_len: 0,
            alt_prev: Vec::new(),
            alt_top: 0,
            alt_history: BTreeMap::new(),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        self.sync_scrollback_len();
        self.track_alt_scroll();
    }

    /// Refresh `history_len` from the emulator. `Screen::scrollback()` reports
    /// the *current* offset, so the maximum has to be read by moving to it and
    /// restoring the view afterwards.
    fn sync_scrollback_len(&mut self) {
        // The alternate grid has no scrollback; keep the primary value so a
        // selection made right after leaving a TUI still maps correctly.
        if self.parser.screen().alternate_screen() {
            return;
        }
        let saved = self.parser.screen().scrollback();
        {
            let screen = self.parser.screen_mut();
            screen.set_scrollback(usize::MAX);
            self.history_len = screen.scrollback();
            screen.set_scrollback(saved);
        }
        // Output while scrolled back makes vt100 advance the offset to keep the
        // same content anchored; mirror that so absolute selection coordinates
        // stay stable across the next `apply_scroll`.
        self.scroll_offset = saved;
    }

    /// Detect vertical movement of the alternate screen and remember the rows
    /// that leave the top. vt100 keeps no scrollback for the alternate grid,
    /// so this is the only way a selection can be copied after it scrolled out
    /// of view (vim, less, …).
    fn track_alt_scroll(&mut self) {
        if !self.parser.screen().alternate_screen() {
            self.alt_prev.clear();
            self.alt_top = 0;
            self.alt_history.clear();
            return;
        }

        let cur = self.snapshot_alt_rows();
        if self.alt_prev.len() == cur.len() && !cur.is_empty() {
            let shift = detect_alt_shift(&self.alt_prev, &cur);
            if shift > 0 {
                let k = shift as usize;
                let leaving: Vec<CapturedRow> = self.alt_prev.iter().take(k).cloned().collect();
                let top = self.alt_top;
                for (i, row) in leaving.into_iter().enumerate() {
                    self.push_alt_history(top + i as i64, row);
                }
                self.alt_top += k as i64;
            } else if shift < 0 {
                // The view moved back up over content we may never have seen;
                // no history is needed because those rows are on screen.
                self.alt_top += shift as i64;
            }
        }
        self.alt_prev = cur;
    }

    fn snapshot_alt_rows(&self) -> Vec<CapturedRow> {
        let screen = self.parser.screen();
        let cols = screen.size().1;
        screen
            .rows(0, cols)
            .enumerate()
            .map(|(row, text)| CapturedRow {
                text,
                wrapped: screen.row_wrapped(row as u16),
            })
            .collect()
    }

    fn push_alt_history(&mut self, line: i64, row: CapturedRow) {
        self.alt_history.insert(line, row);
        while self.alt_history.len() > ALT_HISTORY_LIMIT {
            self.alt_history.pop_first();
        }
    }

    /// Start a fresh selection coordinate space. Called when a drag begins so
    /// the anchor is expressed relative to the screen content at that moment.
    pub fn begin_selection(&mut self) {
        self.alt_top = 0;
        self.alt_history.clear();
        self.alt_prev = if self.parser.screen().alternate_screen() {
            self.snapshot_alt_rows()
        } else {
            Vec::new()
        };
    }

    /// Absolute line number of the first visible row.
    pub fn line_top(&self) -> i64 {
        if self.parser.screen().alternate_screen() {
            self.alt_top
        } else {
            self.history_len as i64 - self.scroll_offset as i64
        }
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
            self.sync_scrollback_len();
            self.alt_prev = if self.parser.screen().alternate_screen() {
                self.snapshot_alt_rows()
            } else {
                Vec::new()
            };
        }
    }

    /// Apply the scroll offset to the screen (must be called before rendering).
    pub fn apply_scroll(&mut self) {
        // `Screen::scrollback()` returns the *current* offset, not the maximum.
        // `set_scrollback()` clamps to the real maximum, so write first and read
        // the clamped value back.
        self.parser.screen_mut().set_scrollback(self.scroll_offset);
        self.scroll_offset = self.parser.screen().scrollback();
    }

    pub fn scroll_by(&mut self, delta: i32) {
        let current = self.parser.screen().scrollback() as i32;
        let next = (current + delta).max(0) as usize;
        // The emulator clamps `next` to the actual scrollback length; mirror the
        // clamped value so the UI never holds an offset it cannot display.
        self.parser.screen_mut().set_scrollback(next);
        self.scroll_offset = self.parser.screen().scrollback();
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.parser.screen_mut().set_scrollback(0);
    }

    /// Text covered by `selection`.
    ///
    /// The selection lives in the UI state, so it is passed in rather than read
    /// from `self`. Endpoints are inclusive (the cell under the pointer is part
    /// of the selection, matching the highlight). Lines above the visible area
    /// are read back from the primary scrollback (or from the captured
    /// alternate-screen history).
    pub fn selected_text(&mut self, selection: Selection) -> String {
        if selection.is_empty() {
            return String::new();
        }
        let ((start_line, start_col), (end_line, end_col)) = selection.normalized();
        let (_rows, cols) = self.size();
        let alt = self.parser.screen().alternate_screen();
        let top = self.line_top();
        let mut out = String::new();

        for line in start_line..=end_line {
            let from = if line == start_line { start_col } else { 0 };
            let to = if line == end_line {
                end_col.saturating_add(1).min(cols)
            } else {
                cols
            };
            if to > from {
                let (text, _wrapped) = if alt {
                    self.alt_line(line, top, from, to)
                } else {
                    self.primary_line(line, from, to)
                };
                out.push_str(&text);
            }
            if line != end_line {
                let wrapped = if alt {
                    self.alt_line_wrapped(line, top)
                } else {
                    self.primary_line_wrapped(line)
                };
                if !wrapped {
                    out.push('\n');
                }
            }
        }
        out.trim_end().to_string()
    }

    /// Text of one row of the alternate screen at absolute `line`, restricted
    /// to columns `[from, to)`.
    fn alt_line(&self, line: i64, top: i64, from: u16, to: u16) -> (String, bool) {
        if line >= top {
            let screen = self.parser.screen();
            let row = line - top;
            if row < screen.size().0 as i64 {
                let row = row as u16;
                let text = screen
                    .rows(from, to - from)
                    .nth(row as usize)
                    .unwrap_or_default();
                return (text, screen.row_wrapped(row));
            }
            return (String::new(), false);
        }

        if let Some(row) = self.alt_history.get(&line) {
            // History rows are plain text, so columns map to chars here. Wide
            // glyphs can shift the slice by a cell; acceptable for copy.
            let text: String = row
                .text
                .chars()
                .skip(from as usize)
                .take(to.saturating_sub(from) as usize)
                .collect();
            return (text, row.wrapped);
        }
        (String::new(), false)
    }

    fn alt_line_wrapped(&self, line: i64, top: i64) -> bool {
        if line >= top {
            let screen = self.parser.screen();
            let row = line - top;
            return row >= 0 && row < screen.size().0 as i64 && screen.row_wrapped(row as u16);
        }
        self.alt_history
            .get(&line)
            .map(|row| row.wrapped)
            .unwrap_or(false)
    }

    /// Text of one row of the primary screen at absolute `line`.
    fn primary_line(&mut self, line: i64, from: u16, to: u16) -> (String, bool) {
        let saved = self.parser.screen().scrollback();
        let history_len = self.history_len as i64;
        let offset = (history_len - line).clamp(0, history_len) as usize;
        self.parser.screen_mut().set_scrollback(offset);
        let (text, wrapped) = {
            let screen = self.parser.screen();
            let row = line - (history_len - offset as i64);
            if row < 0 || row >= screen.size().0 as i64 {
                (String::new(), false)
            } else {
                let row = row as u16;
                (
                    screen
                        .rows(from, to - from)
                        .nth(row as usize)
                        .unwrap_or_default(),
                    screen.row_wrapped(row),
                )
            }
        };
        self.parser.screen_mut().set_scrollback(saved);
        (text, wrapped)
    }

    fn primary_line_wrapped(&mut self, line: i64) -> bool {
        let saved = self.parser.screen().scrollback();
        let history_len = self.history_len as i64;
        let offset = (history_len - line).clamp(0, history_len) as usize;
        self.parser.screen_mut().set_scrollback(offset);
        let wrapped = {
            let screen = self.parser.screen();
            let row = line - (history_len - offset as i64);
            row >= 0 && row < screen.size().0 as i64 && screen.row_wrapped(row as u16)
        };
        self.parser.screen_mut().set_scrollback(saved);
        wrapped
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

/// Detect a vertical scrolling step in the alternate screen.
///
/// Returns how many rows the visible content moved *up* (positive) or down
/// (negative). vt100 does not report scrolling, so consecutive snapshots are
/// compared: identical rows shifted by `k` mean the app scrolled by `k`.
/// Fixed rows are allowed at the top (a vim tabline) and at the bottom (vim's
/// status/command lines), and one redrawn row is tolerated.
fn detect_alt_shift(prev: &[CapturedRow], cur: &[CapturedRow]) -> i32 {
    let rows = cur.len();
    if rows < 4 || prev.len() != rows {
        return 0;
    }
    if let Some(k) = find_shift(prev, cur, true) {
        return k as i32;
    }
    if let Some(k) = find_shift(prev, cur, false) {
        return -(k as i32);
    }
    0
}

fn find_shift(prev: &[CapturedRow], cur: &[CapturedRow], up: bool) -> Option<usize> {
    let rows = cur.len();
    for k in 1..rows {
        for top_keep in 0..=1 {
            for bottom_keep in 0..=2 {
                if top_keep + bottom_keep + k >= rows {
                    continue;
                }
                let region = rows - (top_keep + bottom_keep + k);
                if region < 3 {
                    continue;
                }
                // Rows outside the scrolling region must not have moved.
                if (0..top_keep).any(|i| cur[i].text != prev[i].text) {
                    continue;
                }
                if (0..bottom_keep).any(|i| cur[rows - 1 - i].text != prev[rows - 1 - i].text) {
                    continue;
                }
                // The edge of the moving region has to have changed, otherwise
                // a static screen full of identical rows would look like a
                // scroll.
                if cur[top_keep].text == prev[top_keep].text {
                    continue;
                }
                let mut diffs = 0;
                for r in 0..region {
                    let (a, b) = if up {
                        (top_keep + r, top_keep + k + r)
                    } else {
                        (top_keep + k + r, top_keep + r)
                    };
                    if cur[a].text != prev[b].text {
                        diffs += 1;
                        if diffs > 1 {
                            break;
                        }
                    }
                }
                if diffs <= 1 {
                    return Some(k);
                }
            }
        }
    }
    None
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
    cursor_style: CursorStyle,
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

    // Current input line: a faint wash behind the row that owns the cursor.
    // Shell prompts are plain text chosen by the remote `PS1`, so without this
    // the line where the user is supposed to type is just another row of
    // output. Drawn before the cell backgrounds so coloured blocks still win.
    if show_cursor && !screen.alternate_screen() {
        let y = rect.top() + cursor.0 as f32 * metrics.height;
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(rect.left(), y),
                Pos2::new(rect.right(), (y + metrics.height).min(rect.bottom())),
            ),
            0,
            theme::TERM_INPUT_LINE,
        );
    }

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
            // Bold text promotes the eight named colours to their bright
            // variants — the standard terminal cue for emphasis, and the only
            // one available here because egui has no synthetic bold face.
            let mut fg = match cell.fgcolor() {
                VtColor::Idx(i) if cell.bold() && i < 8 => idx_color(i + 8),
                other => vt_color(other, theme::TERM_FG),
            };
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
        let line = term.line_top() + row as i64;
        if !selection.is_empty() {
            let (a, b) = selection.normalized();
            if line >= a.0 && line <= b.0 {
                let start_col = if line == a.0 { a.1 } else { 0 };
                let end_col = if line == b.0 { b.1 } else { cols.saturating_sub(1) };
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
            let cell_rect = Rect::from_min_size(
                Pos2::new(x, y),
                Vec2::new(metrics.width, metrics.height),
            );
            match cursor_style {
                CursorStyle::Block => {
                    painter.rect_filled(cell_rect, 0, theme::TERM_CURSOR);
                    // Re-draw the glyph under the cursor in the terminal
                    // background colour so a block cursor does not hide the
                    // character.
                    if let Some(cell) = screen.cell(cursor.0, cursor.1) {
                        let contents = cell.contents();
                        if !contents.is_empty() {
                            painter.text(
                                cell_rect.center(),
                                Align2::CENTER_CENTER,
                                contents,
                                font.clone(),
                                theme::TERMINAL_BG,
                            );
                        }
                    }
                }
                CursorStyle::Bar => {
                    let width = (metrics.width * 0.16).clamp(1.0, 3.0);
                    painter.rect_filled(
                        Rect::from_min_size(cell_rect.min, Vec2::new(width, metrics.height)),
                        0,
                        theme::TERM_CURSOR,
                    );
                }
                CursorStyle::Underline => {
                    // A short bar under the character: it marks the insertion
                    // point without covering the glyph itself.
                    let height = (metrics.height * 0.12).clamp(1.5, 3.0);
                    painter.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(cell_rect.left(), cell_rect.bottom() - height),
                            cell_rect.max,
                        ),
                        0,
                        theme::TERM_CURSOR,
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
