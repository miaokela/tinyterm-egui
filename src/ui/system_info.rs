//! System-info modal (CPU / memory / disk), ported from `SystemInfoModal.tsx`.

use crate::state::{AppState, SystemInfoKind};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

const PAGE_SIZE: usize = 15;
const MODAL_WIDTH: f32 = 720.0;
const MODAL_HEIGHT: f32 = 460.0;

pub fn parse_process_output(output: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for line in output.trim().lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // `PID VALUE COMMAND rest-of-args`
        let mut parts = trimmed.split_whitespace();
        let (Some(pid), Some(value), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if !value.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        let rest: Vec<&str> = parts.collect();
        let path = rest.join(" ");
        let path = if path.is_empty() || path.starts_with('[') {
            "-".to_string()
        } else {
            path
        };
        rows.push(vec![
            pid.to_string(),
            value.to_string(),
            name.to_string(),
            path,
        ]);
    }
    rows
}

pub fn parse_disk_output(output: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for line in output.trim().lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Filesystem") {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }
        rows.push(vec![
            parts[0].to_string(),
            parts[1].to_string(),
            parts[2].to_string(),
            parts[3].to_string(),
            parts[4].to_string(),
            parts[5..].join(" "),
        ]);
    }
    rows
}

pub fn parse_history(output: &str) -> Vec<String> {
    let mut result = Vec::new();
    for line in output.trim().lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // zsh_history: ": 1234567890:0;command"
        if let Some(rest) = trimmed.strip_prefix(": ") {
            if let Some(idx) = rest.find(';') {
                result.push(rest[idx + 1..].to_string());
                continue;
            }
        }
        // bash history with line numbers: "  123  command"
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        if let (Some(num), Some(cmd)) = (parts.next(), parts.next()) {
            if num.chars().all(|c| c.is_ascii_digit()) && !cmd.trim().is_empty() {
                result.push(cmd.trim().to_string());
                continue;
            }
        }
        result.push(trimmed.to_string());
    }
    let keep = result.len().saturating_sub(200);
    result.split_off(keep)
}

pub fn show(ui: &mut Ui, app: &mut AppState) {
    let Some(state) = app.system_info.clone() else {
        return;
    };
    let screen = ui.ctx().input(|i| i.viewport_rect());
    ui.painter().rect_filled(
        screen,
        0,
        Color32::from_rgba_premultiplied(2, 6, 14, 184),
    );

    let width = MODAL_WIDTH.min(screen.width() - 40.0);
    let height = MODAL_HEIGHT.min(screen.height() - 60.0);
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    let painter = ui.painter().clone();
    painter.rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_MD),
        Color32::from_rgba_premultiplied(12, 19, 33, 245),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_MD),
        Stroke::new(1.0, Color32::from_rgba_premultiplied(26, 41, 65, 46)),
        StrokeKind::Inside,
    );

    // ── Header ───────────────────────────────────────────────────────────────
    let header = Rect::from_min_size(rect.min, Vec2::new(width, 42.0));
    let title = match state.kind {
        SystemInfoKind::Cpu => "CPU 占用情况",
        SystemInfoKind::Memory => "内存占用情况",
        SystemInfoKind::Disk => "磁盘占用情况",
    };
    painter.text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::f_sm(),
        theme::TEXT_PRIMARY,
    );

    // Refresh + close
    let close_rect = Rect::from_center_size(
        Pos2::new(header.right() - 20.0, header.center().y),
        Vec2::splat(24.0),
    );
    let refresh_rect = Rect::from_center_size(
        Pos2::new(header.right() - 50.0, header.center().y),
        Vec2::splat(24.0),
    );
    let r_close = ui.interact(close_rect, ui.id().with("sysinfo-close"), Sense::click());
    let r_refresh = ui.interact(refresh_rect, ui.id().with("sysinfo-refresh"), Sense::click());
    widgets::cross(
        &painter,
        close_rect.center(),
        10.0,
        if r_close.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
    );
    if state.loading {
        widgets::spinner(
            &painter,
            refresh_rect.center(),
            7.0,
            ui.input(|i| i.time) as f32,
            theme::TEXT_SECONDARY,
        );
    } else {
        crate::widgets::icon(
            &painter,
            refresh_rect.center(),
            14.0,
            crate::icons::ARROW_CLOCKWISE,
            if r_refresh.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            },
        );
    }
    if r_close.clicked() {
        app.system_info = None;
        return;
    }
    if r_refresh.clicked() && !state.loading {
        let cmd = match state.kind {
            SystemInfoKind::Cpu => super::quick_actions::CPU_CMD,
            SystemInfoKind::Memory => super::quick_actions::MEM_CMD,
            SystemInfoKind::Disk => super::quick_actions::DISK_CMD,
        };
        if let Some(s) = app.system_info.as_mut() {
            s.loading = true;
            s.error = None;
        }
        let sid = state.session_id.clone();
        let request = app.mgr.request_id();
        app.mgr.query(request, &sid, cmd.to_string());
    }

    // ── Body ─────────────────────────────────────────────────────────────────
    let footer_height = if state.rows.is_empty() { 0.0 } else { 34.0 };
    let body = Rect::from_min_max(
        Pos2::new(rect.left(), header.bottom()),
        Pos2::new(rect.right(), rect.bottom() - footer_height),
    );

    if state.loading {
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(body));
        c.vertical_centered(|ui| {
            ui.add_space(body.height() * 0.4);
            widgets::loading_blocks(ui, ui.input(|i| i.time) as f32, 1.6);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("正在获取数据...")
                    .size(theme::TEXT_XS)
                    .color(theme::TEXT_SECONDARY),
            );
        });
        return;
    }
    if let Some(err) = &state.error {
        painter.text(
            body.center(),
            Align2::CENTER_CENTER,
            format!("获取失败: {err}"),
            theme::f_xs(),
            theme::ERROR,
        );
        return;
    }
    if state.rows.is_empty() {
        painter.text(
            body.center(),
            Align2::CENTER_CENTER,
            "暂无数据",
            theme::f_xs(),
            theme::TEXT_MUTED,
        );
        return;
    }

    let is_disk = state.kind == SystemInfoKind::Disk;
    let headers: Vec<&str> = if is_disk {
        vec!["文件系统", "总容量", "已用", "可用", "使用率", "挂载点"]
    } else {
        vec!["PID", if state.kind == SystemInfoKind::Cpu { "CPU%" } else { "内存%" }, "程序名称", "执行路径 / 参数"]
    };

    let total_pages = state.rows.len().div_ceil(PAGE_SIZE).max(1);
    let current = state.page.min(total_pages - 1);
    let start = current * PAGE_SIZE;
    let page_rows = &state.rows[start..(start + PAGE_SIZE).min(state.rows.len())];

    let row_height = 24.0;
    let header_height = 26.0;
    let table_top = body.top() + 4.0;

    // Header row
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(body.left() + 8.0, table_top),
            Vec2::new(body.width() - 16.0, header_height),
        ),
        CornerRadius::same(theme::RADIUS_XS),
        Color32::from_rgba_premultiplied(17, 28, 47, 242),
    );

    let col_count = headers.len();
    let col_width = (body.width() - 16.0) / col_count as f32;
    for (i, h) in headers.iter().enumerate() {
        let x = body.left() + 8.0 + i as f32 * col_width + 8.0;
        painter.text(
            Pos2::new(x, table_top + header_height * 0.5),
            Align2::LEFT_CENTER,
            *h,
            theme::font_sans(theme::TEXT_XS - 1.0),
            Color32::from_rgba_premultiplied(160, 185, 220, 179),
        );
    }

    // Rows
    let mut y = table_top + header_height;
    for (ri, row) in page_rows.iter().enumerate() {
        if y + row_height > body.bottom() - 2.0 {
            break;
        }
        let row_rect = Rect::from_min_size(
            Pos2::new(body.left() + 8.0, y),
            Vec2::new(body.width() - 16.0, row_height),
        );
        if ri % 2 == 1 {
            painter.rect_filled(
                row_rect,
                0,
                Color32::from_rgba_premultiplied(4, 4, 4, 8),
            );
        }
        painter.line_segment(
            [
                Pos2::new(row_rect.left(), row_rect.bottom()),
                Pos2::new(row_rect.right(), row_rect.bottom()),
            ],
            Stroke::new(1.0, Color32::from_rgba_premultiplied(4, 6, 13, 12)),
        );
        for (ci, cell) in row.iter().enumerate() {
            let x = row_rect.left() + ci as f32 * col_width + 8.0;
            let color = if is_disk && ci == 4 {
                let pct: f32 = cell.trim_end_matches('%').parse().unwrap_or(0.0);
                if pct > 80.0 { theme::ERROR } else { theme::TEXT_SECONDARY }
            } else if ci < col_count - 1 {
                theme::TEXT_SECONDARY
            } else {
                theme::TEXT_PRIMARY
            };
            let font = if ci == col_count - 1 {
                theme::font_sans(theme::TEXT_XS - 1.0)
            } else {
                theme::font_mono(theme::TEXT_XS - 1.0)
            };
            let text = theme::truncate(&painter, cell, &font, col_width - 14.0);
            painter.text(
                Pos2::new(x, row_rect.center().y),
                Align2::LEFT_CENTER,
                text,
                font,
                color,
            );
        }
        y += row_height;
    }

    // ── Footer ───────────────────────────────────────────────────────────────
    let footer = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - footer_height),
        Vec2::new(rect.width(), footer_height),
    );
    painter.line_segment(
        [
            Pos2::new(footer.left() + 8.0, footer.top()),
            Pos2::new(footer.right() - 8.0, footer.top()),
        ],
        Stroke::new(1.0, Color32::from_rgba_premultiplied(26, 41, 65, 46)),
    );
    painter.text(
        Pos2::new(footer.left() + 16.0, footer.center().y),
        Align2::LEFT_CENTER,
        format!(
            "共 {} 条 · 第 {} / {} 页",
            state.rows.len(),
            current + 1,
            total_pages
        ),
        theme::f_xs(),
        theme::TEXT_MUTED,
    );

    let next_rect = Rect::from_center_size(
        Pos2::new(footer.right() - 20.0, footer.center().y),
        Vec2::splat(22.0),
    );
    let prev_rect = Rect::from_center_size(
        Pos2::new(footer.right() - 46.0, footer.center().y),
        Vec2::splat(22.0),
    );
    let r_next = ui.interact(next_rect, ui.id().with("sysinfo-next"), Sense::click());
    let r_prev = ui.interact(prev_rect, ui.id().with("sysinfo-prev"), Sense::click());
    widgets::chevron(
        &painter,
        prev_rect.center(),
        9.0,
        true,
        if current == 0 { theme::TEXT_MUTED } else if r_prev.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
    );
    widgets::chevron(
        &painter,
        next_rect.center(),
        9.0,
        false,
        if current + 1 >= total_pages { theme::TEXT_MUTED } else if r_next.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
    );
    if r_prev.clicked() && current > 0 {
        if let Some(s) = app.system_info.as_mut() {
            s.page = current - 1;
        }
    }
    if r_next.clicked() && current + 1 < total_pages {
        if let Some(s) = app.system_info.as_mut() {
            s.page = current + 1;
        }
    }
}
