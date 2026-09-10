//! File manager — dual local/remote panels, transfer queue, context menus.
//! Layout and behaviour follow `docs/spec-filemanager.md`.

use crate::models::{FileInfo, TransferDirection, TransferStatus};
use crate::remote_fs;
use crate::state::{
    AppState, InlineAction, InlineKind, PanelSide, FM_BAR_HEIGHT, FM_CONTENT_HEIGHT,
};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

const ROW_HEIGHT: f32 = 22.0;
const DIVIDER_WIDTH: f32 = 32.0;
const PANEL_HEADER_HEIGHT: f32 = 26.0;
const PATH_BAR_HEIGHT: f32 = 24.0;
const QUEUE_ROW_HEIGHT: f32 = 28.0;

pub fn show(ui: &mut Ui, app: &mut AppState, session_id: &str, rect: Rect) {
    if rect.height() < 10.0 {
        return;
    }
    let collapsed = !app
        .session_tab(session_id)
        .map(|(_, s)| s.fm_open)
        .unwrap_or(false);

    let bar_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - FM_BAR_HEIGHT),
        Vec2::new(rect.width(), FM_BAR_HEIGHT),
    );

    if !collapsed {
        let content_height = FM_CONTENT_HEIGHT.min(rect.height() - FM_BAR_HEIGHT);
        let content_rect = Rect::from_min_size(
            Pos2::new(rect.left(), bar_rect.top() - content_height),
            Vec2::new(rect.width(), content_height),
        );
        content(ui, app, session_id, content_rect);
    }

    collapse_bar(ui, app, session_id, bar_rect, collapsed);
}

// ── Collapse bar ─────────────────────────────────────────────────────────────

fn collapse_bar(
    ui: &mut Ui,
    app: &mut AppState,
    session_id: &str,
    rect: Rect,
    collapsed: bool,
) {
    let active_count = app
        .transfers
        .iter()
        .filter(|t| t.session_id.as_deref() == Some(session_id) && t.status != TransferStatus::Done)
        .count();

    let response = ui.interact(rect, ui.id().with(("fm-bar", session_id)), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        if collapsed {
            CornerRadius::same(theme::RADIUS_MD)
        } else {
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: theme::RADIUS_MD,
                se: theme::RADIUS_MD,
            }
        },
        Color32::from_rgba_premultiplied(6, 15, 31, 204),
    );
    if collapsed {
        // Rounded outline all the way round; a straight top hairline would cut
        // across (and visually square off) the rounded corners.
        painter.rect_stroke(
            rect,
            CornerRadius::same(theme::RADIUS_MD),
            Stroke::new(1.0, theme::BORDER),
            StrokeKind::Inside,
        );
    } else {
        // Attached to the content panel above: only the shared edge needs a line.
        painter.line_segment(
            [
                Pos2::new(rect.left(), rect.top() + 0.5),
                Pos2::new(rect.right(), rect.top() + 0.5),
            ],
            Stroke::new(1.0, Color32::from_rgba_premultiplied(17, 40, 77, 51)),
        );
    }

    let mut x = rect.left() + 14.0;
    // Chevron
    widgets::chevron(
        painter,
        Pos2::new(x + 6.0, rect.center().y),
        9.0,
        false,
        theme::TEXT_MUTED,
    );
    if !collapsed {
        // rotate: up-chevron drawn as inverted
        painter.rect_filled(
            Rect::from_center_size(Pos2::new(x + 6.0, rect.center().y), Vec2::splat(0.1)),
            0,
            Color32::TRANSPARENT,
        );
    }
    x += 18.0;
    // Hard-drive glyph
    let drive = Rect::from_center_size(Pos2::new(x + 6.0, rect.center().y), Vec2::new(12.0, 9.0));
    painter.rect_stroke(
        drive,
        CornerRadius::same(2),
        Stroke::new(1.2, theme::TEXT_MUTED),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [
            Pos2::new(drive.left(), drive.center().y),
            Pos2::new(drive.right(), drive.center().y),
        ],
        Stroke::new(1.0, theme::TEXT_MUTED),
    );
    x += 20.0;
    painter.text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        "文件管理",
        theme::f_xs(),
        theme::TEXT_SECONDARY,
    );
    x += 62.0;

    if active_count > 0 {
        let badge = Rect::from_min_size(
            Pos2::new(x, rect.center().y - 10.0),
            Vec2::new(20.0_f32.max(18.0 + (active_count.to_string().len() as f32 - 1.0) * 7.0), 20.0),
        );
        painter.rect_filled(
            badge,
            CornerRadius::same(theme::RADIUS_SM),
            theme::ACCENT,
        );
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            active_count.to_string(),
            theme::font_sans(theme::TEXT_XS - 1.0),
            Color32::WHITE,
        );
    }

    if response.clicked() {
        if collapsed {
            app.open_file_manager(session_id);
        } else {
            app.close_file_manager(session_id);
        }
    }
}

// ── Content ──────────────────────────────────────────────────────────────────

fn content(ui: &mut Ui, app: &mut AppState, session_id: &str, rect: Rect) {
    let painter = ui.painter().clone();
    let top_round = CornerRadius {
        nw: theme::RADIUS_MD,
        ne: theme::RADIUS_MD,
        sw: 0,
        se: 0,
    };
    painter.rect_filled(rect, top_round, theme::BG_PANEL);
    painter.rect_stroke(
        rect,
        top_round,
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    let transfers: Vec<crate::models::TransferProgress> = app
        .transfers
        .iter()
        .filter(|t| t.session_id.as_deref() == Some(session_id))
        .cloned()
        .collect();

    // One row per batch group, one per unfinished loose transfer.
    let queue_rows = build_queue_rows(&transfers).len();
    let queue_height = if queue_rows == 0 {
        0.0
    } else {
        10.0 + queue_rows as f32 * (QUEUE_ROW_HEIGHT + 4.0)
    };
    let queue_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top()),
        Vec2::new(rect.width(), queue_height.min(rect.height() * 0.5)),
    );
    let panels_rect = Rect::from_min_max(
        Pos2::new(rect.left(), queue_rect.bottom()),
        rect.right_bottom(),
    );

    if !transfers.is_empty() {
        transfer_queue(ui, app, session_id, queue_rect, &transfers);
    }

    // Panels: local | divider | remote
    let panel_width = (panels_rect.width() - DIVIDER_WIDTH) * 0.5;
    let local_rect = Rect::from_min_size(
        panels_rect.min,
        Vec2::new(panel_width, panels_rect.height()),
    );
    let divider_rect = Rect::from_min_size(
        Pos2::new(local_rect.right(), panels_rect.top()),
        Vec2::new(DIVIDER_WIDTH, panels_rect.height()),
    );
    let remote_rect = Rect::from_min_size(
        Pos2::new(divider_rect.right(), panels_rect.top()),
        Vec2::new(panels_rect.right() - divider_rect.right(), panels_rect.height()),
    );

    panel(ui, app, session_id, PanelSide::Local, local_rect);
    divider(ui, app, session_id, divider_rect);
    panel(ui, app, session_id, PanelSide::Remote, remote_rect);

    context_menu(ui, app, session_id);
    inline_dialog(ui, app, session_id);
}

// ── Transfer queue ───────────────────────────────────────────────────────────

/// One drawn line of the transfer queue.
enum QueueRow {
    /// A batch: one line summarising every sub-item.
    Group {
        gid: String,
        items: Vec<crate::models::TransferProgress>,
    },
    /// A loose transfer: one line per unfinished item.
    Single(crate::models::TransferProgress),
}

/// Build the rows the queue will draw.
///
/// Mirrors `TransferQueue` in the web client: batch groups collapse to a single
/// line, loose transfers get one line each, and finished items disappear.
fn build_queue_rows(
    transfers: &[crate::models::TransferProgress],
) -> Vec<QueueRow> {
    let mut order: Vec<Option<String>> = Vec::new();
    let mut groups: std::collections::HashMap<
        Option<String>,
        Vec<crate::models::TransferProgress>,
    > = std::collections::HashMap::new();
    for t in transfers {
        let key = t.group_id.clone();
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(t.clone());
    }

    let mut rows = Vec::new();
    for key in order {
        let Some(items) = groups.get(&key) else {
            continue;
        };
        match &key {
            Some(gid) => {
                let sub: Vec<crate::models::TransferProgress> = items
                    .iter()
                    .filter(|t| &t.id != gid)
                    .cloned()
                    .collect();
                if sub.iter().any(|t| t.status != TransferStatus::Done) {
                    rows.push(QueueRow::Group {
                        gid: gid.clone(),
                        items: sub,
                    });
                }
            }
            None => {
                for item in items.iter().filter(|t| t.status != TransferStatus::Done) {
                    rows.push(QueueRow::Single(item.clone()));
                }
            }
        }
    }
    rows
}

fn transfer_queue(
    ui: &mut Ui,
    app: &mut AppState,
    session_id: &str,
    rect: Rect,
    transfers: &[crate::models::TransferProgress],
) {
    let painter = ui.painter().clone();
    painter.rect_filled(
        rect,
        CornerRadius {
            nw: theme::RADIUS_MD,
            ne: theme::RADIUS_MD,
            sw: 0,
            se: 0,
        },
        Color32::from_rgba_premultiplied(0, 0, 0, 64),
    );
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.bottom() - 0.5),
            Pos2::new(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1.0, theme::BORDER),
    );

    let mut y = rect.top() + 5.0;
    for row in build_queue_rows(transfers) {
        let row_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 10.0, y),
            Vec2::new(rect.width() - 20.0, QUEUE_ROW_HEIGHT),
        );
        y += QUEUE_ROW_HEIGHT + 4.0;
        if row_rect.bottom() > rect.bottom() {
            break;
        }
        painter.rect_filled(
            row_rect,
            CornerRadius::same(theme::RADIUS_SM),
            Color32::from_rgba_premultiplied(8, 8, 8, 8),
        );

        match row {
            QueueRow::Group { gid, items } => {
                let transferring = items
                    .iter()
                    .find(|t| t.status == TransferStatus::Transferring);
                let done = items
                    .iter()
                    .filter(|t| t.status == TransferStatus::Done)
                    .count();
                let pending = items
                    .len()
                    .saturating_sub(done)
                    .saturating_sub(usize::from(transferring.is_some()));
                let has_error = items
                    .iter()
                    .any(|t| matches!(t.status, TransferStatus::Error | TransferStatus::Conflict));
                let current = transferring
                    .or_else(|| items.iter().find(|t| t.status == TransferStatus::Pending))
                    .or_else(|| items.first());

                let dir = current.map(|t| t.direction).unwrap_or(TransferDirection::Upload);
                let current_percent = current.map(|t| t.percent()).unwrap_or(0.0);
                let overall_percent = if items.is_empty() {
                    0.0
                } else {
                    (done as f32 / items.len() as f32) * 100.0
                };
                let show_percent = if transferring.is_some() {
                    current_percent
                } else {
                    overall_percent
                };
                let action = match dir {
                    TransferDirection::Upload => "正在上传:",
                    TransferDirection::Download => "正在下载:",
                };

                widgets::icon(
                    &painter,
                    Pos2::new(row_rect.left() + 14.0, row_rect.center().y),
                    13.0,
                    // Same left/right language as the transfer buttons.
                    match dir {
                        TransferDirection::Upload => crate::icons::ARROW_RIGHT,
                        TransferDirection::Download => crate::icons::ARROW_LEFT,
                    },
                    theme::ACCENT,
                );
                painter.text(
                    Pos2::new(row_rect.left() + 28.0, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    action,
                    theme::f_xs(),
                    theme::TEXT_SECONDARY,
                );
                let name = current.map(|t| t.file_name.clone()).unwrap_or_default();
                let name_left = row_rect.left() + 96.0;
                painter.text(
                    Pos2::new(name_left, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    theme::truncate(
                        &painter,
                        &name,
                        &theme::f_xs(),
                        (row_rect.left() + 202.0 - name_left).max(40.0),
                    ),
                    theme::f_xs(),
                    theme::TEXT_PRIMARY,
                );
                // Columns from the right: [待传 60][完成 60][cancel 32]; the
                // percentage is right-aligned just left of them.
                let track = Rect::from_min_max(
                    Pos2::new(row_rect.left() + 210.0, row_rect.center().y - 2.0),
                    Pos2::new(row_rect.right() - 172.0, row_rect.center().y + 2.0),
                );
                if track.width() > 20.0 {
                    progress_track(&painter, track, show_percent / 100.0, has_error);
                }
                painter.text(
                    Pos2::new(row_rect.right() - 160.0, row_rect.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{show_percent:.0}%"),
                    theme::f_xs(),
                    theme::TEXT_SECONDARY,
                );

                let pending_rect = Rect::from_min_size(
                    Pos2::new(row_rect.right() - 156.0, row_rect.top()),
                    Vec2::new(60.0, row_rect.height()),
                );
                let done_rect = Rect::from_min_size(
                    Pos2::new(row_rect.right() - 96.0, row_rect.top()),
                    Vec2::new(60.0, row_rect.height()),
                );
                for (r, label, value, color) in [
                    (pending_rect, "待传", pending, theme::WARNING),
                    (done_rect, "完成", done, theme::SUCCESS),
                ] {
                    painter.line_segment(
                        [
                            Pos2::new(r.left(), r.top() + 4.0),
                            Pos2::new(r.left(), r.bottom() - 4.0),
                        ],
                        Stroke::new(1.0, Color32::from_rgba_premultiplied(13, 13, 13, 15)),
                    );
                    painter.text(
                        Pos2::new(r.left() + 8.0, r.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        theme::font_sans(theme::TEXT_XS - 1.0),
                        theme::TEXT_MUTED,
                    );
                    painter.text(
                        Pos2::new(r.left() + 40.0, r.center().y),
                        Align2::LEFT_CENTER,
                        value.to_string(),
                        theme::f_xs(),
                        color,
                    );
                }

                let cancel_rect = Rect::from_center_size(
                    Pos2::new(row_rect.right() - 20.0, row_rect.center().y),
                    Vec2::splat(24.0),
                );
                let r = ui.interact(
                    cancel_rect,
                    ui.id().with(("cancel-group", session_id, &gid)),
                    Sense::click(),
                );
                widgets::cross(
                    &painter,
                    cancel_rect.center(),
                    9.0,
                    if r.hovered() { theme::ERROR } else { theme::TEXT_MUTED },
                );
                if r.hovered() {
                    r.clone().on_hover_text("全部取消");
                }
                if r.clicked() {
                    app.cancel_transfer(&gid);
                }
            }
            QueueRow::Single(item) => {
                let percent = item.percent();
                let is_error = matches!(
                    item.status,
                    TransferStatus::Error | TransferStatus::Conflict
                );

                widgets::icon(
                    &painter,
                    Pos2::new(row_rect.left() + 14.0, row_rect.center().y),
                    13.0,
                    match item.direction {
                        TransferDirection::Upload => crate::icons::ARROW_RIGHT,
                        TransferDirection::Download => crate::icons::ARROW_LEFT,
                    },
                    theme::ACCENT,
                );
                painter.text(
                    Pos2::new(row_rect.left() + 30.0, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    theme::truncate(
                        &painter,
                        &item.file_name,
                        &theme::f_xs(),
                        (row_rect.left() + 184.0 - (row_rect.left() + 30.0)).max(40.0),
                    ),
                    theme::f_xs(),
                    theme::TEXT_PRIMARY,
                );
                // [percent][error label][cancel], laid out right-to-left so the
                // percentage never runs under the label or the button.
                let track = Rect::from_min_max(
                    Pos2::new(row_rect.left() + 192.0, row_rect.center().y - 2.0),
                    Pos2::new(row_rect.right() - 160.0, row_rect.center().y + 2.0),
                );
                if track.width() > 20.0 {
                    progress_track(&painter, track, percent / 100.0, is_error);
                }
                painter.text(
                    Pos2::new(row_rect.right() - 154.0, row_rect.center().y),
                    Align2::LEFT_CENTER,
                    format!("{percent:.0}%"),
                    theme::f_xs(),
                    theme::TEXT_SECONDARY,
                );

                if is_error {
                    let label = match item.error.as_deref() {
                        Some("用户取消") | Some("Cancelled") => "已取消",
                        _ => "失败",
                    };
                    let r = ui.interact(
                        Rect::from_min_size(
                            Pos2::new(row_rect.right() - 76.0, row_rect.top()),
                            Vec2::new(56.0, row_rect.height()),
                        ),
                        ui.id().with(("transfer-err", session_id, &item.id)),
                        Sense::hover(),
                    );
                    painter.text(
                        Pos2::new(row_rect.right() - 40.0, row_rect.center().y),
                        Align2::RIGHT_CENTER,
                        label,
                        theme::f_xs(),
                        theme::ERROR,
                    );
                    if let Some(err) = item.error.as_deref() {
                        r.on_hover_text(err);
                    }
                }

                if matches!(
                    item.status,
                    TransferStatus::Pending | TransferStatus::Transferring
                ) {
                    let cancel_rect = Rect::from_center_size(
                        Pos2::new(row_rect.right() - 20.0, row_rect.center().y),
                        Vec2::splat(24.0),
                    );
                    let r = ui.interact(
                        cancel_rect,
                        ui.id().with(("cancel-transfer", session_id, &item.id)),
                        Sense::click(),
                    );
                    widgets::cross(
                        &painter,
                        cancel_rect.center(),
                        9.0,
                        if r.hovered() { theme::ERROR } else { theme::TEXT_MUTED },
                    );
                    if r.hovered() {
                        r.clone().on_hover_text("取消");
                    }
                    if r.clicked() {
                        app.cancel_transfer(&item.id);
                    }
                }
            }
        }
    }
}

/// Shared progress track (`background: rgba(0,0,0,.3)`, 4 px tall).
fn progress_track(painter: &egui::Painter, track: Rect, fraction: f32, error: bool) {
    painter.rect_filled(
        track,
        CornerRadius::same(theme::RADIUS_XS),
        Color32::from_rgba_premultiplied(0, 0, 0, 77),
    );
    let fill = Rect::from_min_size(
        track.min,
        Vec2::new(track.width() * fraction.clamp(0.0, 1.0), track.height()),
    );
    painter.rect_filled(
        fill,
        CornerRadius::same(theme::RADIUS_XS),
        if error {
            Color32::from_rgba_premultiplied(150, 34, 34, 204)
        } else {
            theme::ACCENT
        },
    );
}

// ── Divider ──────────────────────────────────────────────────────────────────

fn divider(ui: &mut Ui, app: &mut AppState, session_id: &str, rect: Rect) {
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0, Color32::from_rgba_premultiplied(0, 0, 0, 26));
    painter.line_segment(
        [
            Pos2::new(rect.left() + 0.5, rect.top()),
            Pos2::new(rect.left() + 0.5, rect.bottom()),
        ],
        Stroke::new(1.0, theme::BORDER),
    );
    painter.line_segment(
        [
            Pos2::new(rect.right() - 0.5, rect.top()),
            Pos2::new(rect.right() - 0.5, rect.bottom()),
        ],
        Stroke::new(1.0, theme::BORDER),
    );

    let Some(fm) = app.fm.get(session_id) else {
        return;
    };
    let local_sel = fm.local.selected_items();
    let remote_sel = fm.remote.selected_items();
    let upload_busy = fm.upload_busy;
    let download_busy = fm.download_busy;
    let remote_path = fm.remote.path.clone();
    let local_path = fm.local.path.clone();
    let deleting = false;

    let btn_size = 20.0;
    let gap = 8.0;
    let center_y = rect.center().y;
    let upload_rect = Rect::from_center_size(
        Pos2::new(rect.center().x, center_y - btn_size * 0.5 - gap * 0.5),
        Vec2::splat(btn_size),
    );
    let download_rect = Rect::from_center_size(
        Pos2::new(rect.center().x, center_y + btn_size * 0.5 + gap * 0.5),
        Vec2::splat(btn_size),
    );

    // Vertical lines above / below the button pair
    painter.line_segment(
        [
            Pos2::new(rect.center().x, rect.top() + 6.0),
            Pos2::new(rect.center().x, upload_rect.top() - 4.0),
        ],
        Stroke::new(1.0, theme::BORDER),
    );
    painter.line_segment(
        [
            Pos2::new(rect.center().x, download_rect.bottom() + 4.0),
            Pos2::new(rect.center().x, rect.bottom() - 6.0),
        ],
        Stroke::new(1.0, theme::BORDER),
    );

    // Upload: local → remote.
    let up = ui.interact(upload_rect, ui.id().with(("fm-up", session_id)), Sense::click());
    let up_enabled = !upload_busy && !deleting;
    let up_active = !local_sel.is_empty();
    draw_transfer_btn(ui, upload_rect, up.hovered() && up_enabled, up_active, true, upload_busy);
    if up_active {
        draw_badge(&painter, upload_rect, local_sel.len(), false);
    }
    if up.clicked() && up_enabled {
        if local_sel.is_empty() {
            app.toast("请先在本地面板选择要上传的文件或文件夹", crate::session::ToastKind::Info);
        } else {
            app.request_transfer(session_id, TransferDirection::Upload, remote_path.clone());
        }
    }

    // Download: remote → local.
    let down = ui.interact(
        download_rect,
        ui.id().with(("fm-down", session_id)),
        Sense::click(),
    );
    let down_enabled = !download_busy && !deleting;
    let down_active = !remote_sel.is_empty();
    draw_transfer_btn(ui, download_rect, down.hovered() && down_enabled, down_active, false, download_busy);
    if down_active {
        draw_badge(&painter, download_rect, remote_sel.len(), true);
    }
    if down.clicked() && down_enabled {
        if remote_sel.is_empty() {
            app.toast("请先在远程面板选择要下载的文件或文件夹", crate::session::ToastKind::Info);
        } else {
            app.request_transfer(session_id, TransferDirection::Download, local_path.clone());
        }
    }
}

fn draw_transfer_btn(
    ui: &Ui,
    rect: Rect,
    hovered: bool,
    active: bool,
    to_remote: bool,
    busy: bool,
) {
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_XS),
        if active {
            Color32::from_rgba_premultiplied(17, 44, 90, 128)
        } else {
            Color32::TRANSPARENT
        },
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_XS),
        Stroke::new(
            1.0,
            if hovered {
                Color32::from_rgba_premultiplied(67, 115, 153, 140)
            } else if active {
                Color32::from_rgba_premultiplied(67, 115, 153, 90)
            } else {
                Color32::from_rgba_premultiplied(8, 8, 8, 20)
            },
        ),
        StrokeKind::Inside,
    );
    let c = rect.center();
    if busy {
        widgets::spinner(painter, c, 6.0, ui.input(|i| i.time) as f32, theme::TEXT_PRIMARY);
        return;
    }
    let color = if active {
        Color32::from_rgb(0x8e, 0xc5, 0xff)
    } else {
        Color32::from_rgba_premultiplied(120, 145, 175, 170)
    };
    // Arrows follow the panel layout: → uploads to the remote panel on the
    // right, ← downloads into the local panel on the left.
    widgets::icon(
        painter,
        c,
        14.0,
        if to_remote {
            crate::icons::ARROW_RIGHT
        } else {
            crate::icons::ARROW_LEFT
        },
        color,
    );
}

/// Count badge on the divider buttons — 16 px pill clipped onto the button's
/// top corner (the web client uses `top: -5px; left/right: -4px`).
fn draw_badge(painter: &egui::Painter, btn: Rect, count: usize, right: bool) {
    let text = count.to_string();
    let height = 16.0;
    let width = 16.0_f32.max(11.0 + text.len() as f32 * 7.0);
    // Push the pill further out than the web client's `-4px / -5px`: a larger
    // offset means *less* of the button is covered (the badge sits beside the
    // button instead of on top of the arrow glyph).
    let offset = 8.0;
    let x = if right {
        btn.right() + offset - width
    } else {
        btn.left() - offset
    };
    let badge = Rect::from_min_size(
        Pos2::new(x, btn.top() - 8.0),
        Vec2::new(width, height),
    );
    // Soft drop shadow, then the pill itself.
    painter.rect_filled(
        badge.translate(Vec2::new(0.0, 1.0)),
        CornerRadius::same(theme::RADIUS_SM),
        Color32::from_rgba_premultiplied(0, 0, 0, 60),
    );
    painter.rect_filled(
        badge,
        CornerRadius::same(theme::RADIUS_SM),
        theme::ACCENT,
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        text,
        theme::font_sans(theme::TEXT_XS - 1.0),
        Color32::WHITE,
    );
}

// ── Panel ────────────────────────────────────────────────────────────────────

fn panel(ui: &mut Ui, app: &mut AppState, session_id: &str, side: PanelSide, rect: Rect) {
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0, Color32::from_rgba_premultiplied(0, 0, 0, 15));

    let header_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), PANEL_HEADER_HEIGHT));
    let path_rect = Rect::from_min_size(
        Pos2::new(rect.left(), header_rect.bottom()),
        Vec2::new(rect.width(), PATH_BAR_HEIGHT),
    );
    let list_rect = Rect::from_min_max(
        Pos2::new(rect.left(), path_rect.bottom()),
        rect.right_bottom(),
    );

    panel_header(ui, app, session_id, &side, header_rect);
    panel_path_bar(ui, app, session_id, &side, path_rect);
    panel_list(ui, app, session_id, &side, list_rect);
}

fn panel_header(ui: &mut Ui, app: &mut AppState, session_id: &str, side: &PanelSide, rect: Rect) {
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0, Color32::from_rgba_premultiplied(0, 0, 0, 26));
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.bottom() - 0.5),
            Pos2::new(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1.0, theme::BORDER),
    );

    let (title, icon_dir, show_hidden) = {
        let Some(fm) = app.fm.get(session_id) else {
            return;
        };
        match side {
            PanelSide::Local => ("本地", true, fm.show_hidden_local),
            PanelSide::Remote => ("远程", false, fm.show_hidden_remote),
        }
    };
    let side = side.clone();

    // Panel icon
    let icon_center = Pos2::new(rect.left() + 12.0, rect.center().y);
    if icon_dir {
        widgets::folder_glyph(
            &painter,
            Rect::from_center_size(icon_center, Vec2::splat(12.0)),
            theme::ACCENT_LIGHT,
        );
    } else {
        widgets::icon(&painter, icon_center, 13.0, crate::icons::TERMINAL, theme::ACCENT_LIGHT);
    }

    painter.text(
        Pos2::new(rect.left() + 24.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::font_sans(theme::TEXT_XS - 1.0),
        theme::TEXT_SECONDARY,
    );

    // Action buttons: hidden toggle / refresh / new folder
    let mut bx = rect.right() - 8.0 - 22.0;
    // new folder
    let new_rect = Rect::from_center_size(Pos2::new(bx, rect.center().y), Vec2::splat(22.0));
    let r_new = ui.interact(new_rect, ui.id().with(("fm-newfolder", session_id, side.clone())), Sense::click());
    draw_icon_hover(ui, new_rect, r_new.hovered());
    folder_plus_glyph(&painter, new_rect.center(), if r_new.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY });
    if r_new.clicked() {
        let fm = app.fm_state_mut(session_id);
        fm.inline_action = Some(InlineAction {
            side: side.clone(),
            kind: InlineKind::NewFolder,
            value: String::new(),
            path: None,
        });
    }
    bx -= 24.0;
    // refresh
    let refresh_rect = Rect::from_center_size(Pos2::new(bx, rect.center().y), Vec2::splat(22.0));
    let r_ref = ui.interact(refresh_rect, ui.id().with(("fm-refresh", session_id, side.clone())), Sense::click());
    draw_icon_hover(ui, refresh_rect, r_ref.hovered());
    refresh_glyph(&painter, refresh_rect.center(), if r_ref.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY });
    if r_ref.clicked() {
        let path = match side {
            PanelSide::Local => app.fm.get(session_id).map(|f| f.local.path.clone()),
            PanelSide::Remote => app.fm.get(session_id).map(|f| f.remote.path.clone()),
        };
        if let Some(path) = path {
            match side {
                PanelSide::Local => app.load_local_dir(session_id, path),
                PanelSide::Remote => app.load_remote_dir(session_id, path),
            }
        }
    }
    bx -= 24.0;
    // hidden toggle
    let eye_rect = Rect::from_center_size(Pos2::new(bx, rect.center().y), Vec2::splat(22.0));
    let r_eye = ui.interact(eye_rect, ui.id().with(("fm-eye", session_id, side.clone())), Sense::click());
    draw_icon_hover(ui, eye_rect, r_eye.hovered());
    eye_glyph(
        &painter,
        eye_rect.center(),
        show_hidden,
        if r_eye.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
    );
    if r_eye.clicked() {
        let fm = app.fm_state_mut(session_id);
        match side {
            PanelSide::Local => fm.show_hidden_local = !fm.show_hidden_local,
            PanelSide::Remote => fm.show_hidden_remote = !fm.show_hidden_remote,
        }
    }
}

fn draw_icon_hover(ui: &Ui, rect: Rect, hovered: bool) {
    if hovered {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::RADIUS_XS),
            Color32::from_rgba_premultiplied(22, 49, 96, 110),
        );
    }
}

fn folder_plus_glyph(painter: &egui::Painter, c: Pos2, color: Color32) {
    widgets::icon(painter, c, 14.0, crate::icons::FOLDER_PLUS, color);
}

fn refresh_glyph(painter: &egui::Painter, c: Pos2, color: Color32) {
    widgets::icon(painter, c, 14.0, crate::icons::ARROW_CLOCKWISE, color);
}

fn eye_glyph(painter: &egui::Painter, c: Pos2, open: bool, color: Color32) {
    let glyph = if open {
        crate::icons::EYE_SLASH
    } else {
        crate::icons::EYE
    };
    widgets::icon(painter, c, 14.0, glyph, color);
}

fn panel_path_bar(ui: &mut Ui, app: &mut AppState, session_id: &str, side: &PanelSide, rect: Rect) {
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0, Color32::from_rgba_premultiplied(0, 0, 0, 20));
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.bottom() - 0.5),
            Pos2::new(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1.0, theme::BORDER),
    );

    let (path, editing, input) = {
        let Some(fm) = app.fm.get(session_id) else {
            return;
        };
        let p = match side {
            PanelSide::Local => &fm.local,
            PanelSide::Remote => &fm.remote,
        };
        (p.path.clone(), p.editing_path, p.path_input.clone())
    };

    // Up button
    let up_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Vec2::splat(20.0),
    );
    let r_up = ui.interact(up_rect, ui.id().with(("fm-updir", session_id, side.clone())), Sense::click());
    draw_icon_hover(ui, up_rect, r_up.hovered());
    widgets::icon(
        &painter,
        up_rect.center(),
        12.0,
        crate::icons::CARET_UP,
        if r_up.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_SECONDARY },
    );
    if r_up.clicked() {
        let parent = match side {
            PanelSide::Local => remote_fs::parent_of(&path.replace(std::path::MAIN_SEPARATOR, "/")),
            PanelSide::Remote => remote_fs::parent_of(&path),
        };
        {
            let fm = app.fm_state_mut(session_id);
            match side {
                PanelSide::Local => fm.local.auto_follow = false,
                PanelSide::Remote => fm.remote.auto_follow = false,
            }
        }
        match side {
            PanelSide::Local => {
                let parent = if parent == "/" { "/".to_string() } else { parent };
                app.load_local_dir(session_id, parent);
            }
            PanelSide::Remote => app.load_remote_dir(session_id, parent),
        }
    }

    let text_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 28.0, rect.top() + 2.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 2.0),
    );

    if editing {
        let mut value = input;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
        let edit = child.add(
            egui::TextEdit::singleline(&mut value)
                .frame(egui::Frame::NONE)
                .font(theme::font_mono(theme::TEXT_XS))
                .text_color(theme::TEXT_PRIMARY)
                .desired_width(text_rect.width()),
        );
        ui.painter().rect_filled(
            text_rect.expand2(Vec2::new(4.0, 1.0)),
            CornerRadius::same(theme::RADIUS_XS),
            theme::BG_INPUT,
        );
        child.painter().rect_stroke(
            text_rect.expand2(Vec2::new(4.0, 1.0)),
            CornerRadius::same(theme::RADIUS_XS),
            Stroke::new(1.0, theme::ACCENT),
            StrokeKind::Inside,
        );
        if edit.changed() {
            match side {
                PanelSide::Local => {
                    app.fm_state_mut(session_id).local.path_input = value.clone()
                }
                PanelSide::Remote => {
                    app.fm_state_mut(session_id).remote.path_input = value.clone()
                }
            }
        }
        let enter = child.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = child.input(|i| i.key_pressed(egui::Key::Escape));
        if enter {
            {
                let fm = app.fm_state_mut(session_id);
                match side {
                    PanelSide::Local => {
                        fm.local.editing_path = false;
                        fm.local.auto_follow = false;
                    }
                    PanelSide::Remote => {
                        fm.remote.editing_path = false;
                        fm.remote.auto_follow = false;
                    }
                }
            }
            match side {
                PanelSide::Local => app.load_local_dir(session_id, value),
                PanelSide::Remote => app.load_remote_dir(session_id, value),
            }
        } else if escape {
            let fm = app.fm_state_mut(session_id);
            match side {
                PanelSide::Local => fm.local.editing_path = false,
                PanelSide::Remote => fm.remote.editing_path = false,
            }
        }
    } else {
        let display = theme::truncate(&painter, &path, &theme::font_mono(theme::TEXT_XS), text_rect.width());
        let resp = ui.interact(text_rect, ui.id().with(("fm-path", session_id, side.clone())), Sense::click());
        if resp.hovered() {
            painter.rect_filled(
                text_rect.expand2(Vec2::new(4.0, 1.0)),
                CornerRadius::same(theme::RADIUS_XS),
                Color32::from_rgba_premultiplied(30, 67, 130, 40),
            );
        }
        painter.text(
            Pos2::new(text_rect.left(), text_rect.center().y),
            Align2::LEFT_CENTER,
            display,
            theme::font_mono(theme::TEXT_XS),
            if resp.hovered() { theme::TEXT_SECONDARY } else { theme::TEXT_MUTED },
        );
        if resp.clicked() {
            let fm = app.fm_state_mut(session_id);
            match side {
                PanelSide::Local => {
                    fm.local.editing_path = true;
                    fm.local.path_input = path.clone();
                }
                PanelSide::Remote => {
                    fm.remote.editing_path = true;
                    fm.remote.path_input = path.clone();
                }
            }
        }
    }
}

fn panel_list(ui: &mut Ui, app: &mut AppState, session_id: &str, side: &PanelSide, rect: Rect) {
    let (entries, selected, loading, error, show_hidden, path) = {
        let Some(fm) = app.fm.get(session_id) else {
            return;
        };
        let p = match side {
            PanelSide::Local => &fm.local,
            PanelSide::Remote => &fm.remote,
        };
        let hidden = match side {
            PanelSide::Local => fm.show_hidden_local,
            PanelSide::Remote => fm.show_hidden_remote,
        };
        (
            p.visible_entries(hidden).into_iter().cloned().collect::<Vec<FileInfo>>(),
            p.selected.clone(),
            p.loading,
            p.error.clone(),
            hidden,
            p.path.clone(),
        )
    };
    let _ = show_hidden;

    let painter = ui.painter().clone();

    // The web client renders the spinner for the whole list while loading.
    if loading {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.vertical_centered(|ui| {
            ui.add_space((rect.height() * 0.5 - 12.0).max(0.0));
            widgets::css_spinner(ui, 20.0);
        });
        return;
    }
    if let Some(err) = error {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            theme::truncate(&painter, &err, &theme::f_xs(), rect.width() - 16.0),
            theme::f_xs(),
            theme::ERROR,
        );
        return;
    }
    if entries.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "空目录",
            theme::f_xs(),
            theme::TEXT_MUTED,
        );
        return;
    }

    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    egui::ScrollArea::vertical()
        .id_salt(("fm-list", session_id, side.clone()))
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for entry in &entries {
                file_row(ui, app, session_id, &side, entry, &selected, &path);
            }
        });
}

fn file_row(
    ui: &mut Ui,
    app: &mut AppState,
    session_id: &str,
    side: &PanelSide,
    entry: &FileInfo,
    selected: &std::collections::BTreeSet<String>,
    panel_path: &str,
) {
    let is_selected = selected.contains(&entry.path);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ROW_HEIGHT),
        Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let row = rect.shrink2(Vec2::new(3.0, 0.0));

    if is_selected {
        painter.rect_filled(
            row,
            CornerRadius {
                nw: 0,
                sw: 0,
                ne: theme::RADIUS_XS,
                se: theme::RADIUS_XS,
            },
            Color32::from_rgba_premultiplied(12, 32, 65, 15),
        );
        painter.rect_filled(
            Rect::from_min_size(row.min, Vec2::new(2.0, row.height())),
            0,
            Color32::from_rgb(0x4a, 0x8f, 0xd8),
        );
    } else if response.hovered() {
        painter.rect_filled(
            row,
            CornerRadius::same(theme::RADIUS_XS),
            Color32::from_rgba_premultiplied(12, 32, 65, 20),
        );
    }

    let icon_rect = Rect::from_center_size(
        Pos2::new(row.left() + 12.0, row.center().y),
        Vec2::splat(11.0),
    );
    if entry.is_dir {
        widgets::folder_glyph(painter, icon_rect, Color32::from_rgb(0x4a, 0x8f, 0xd8));
    } else {
        let color = file_type_color(&entry.name);
        widgets::file_glyph(painter, icon_rect, color);
    }

    let size_text = entry.size_text();
    let size_w = if entry.is_dir { 0.0 } else { 62.0 };
    let name_left = row.left() + 24.0;
    let name_right = row.right() - 6.0 - size_w;
    let name_color = if is_selected {
        Color32::from_rgb(0xe0, 0xec, 0xff)
    } else if entry.is_dir {
        Color32::from_rgb(0x7e, 0xb8, 0xff)
    } else {
        theme::TEXT_SECONDARY
    };
    let label = theme::truncate(painter, &entry.name, &theme::f_xs(), (name_right - name_left).max(10.0));
    painter.text(
        Pos2::new(name_left, row.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::f_xs(),
        name_color,
    );
    if !entry.is_dir {
        painter.text(
            Pos2::new(row.right() - 6.0, row.center().y),
            Align2::RIGHT_CENTER,
            size_text,
            theme::font_sans(theme::TEXT_XS - 1.0),
            theme::TEXT_MUTED,
        );
    }

    // ── Interaction ──────────────────────────────────────────────────────────
    if response.clicked() {
        let mods = ui.input(|i| i.modifiers);
        let now = crate::state::now_ms();
        let fm = app.fm_state_mut(session_id);
        let hidden_local = fm.show_hidden_local;
        let hidden_remote = fm.show_hidden_remote;
        if mods.command {
            match side {
                PanelSide::Local => fm.local.select_toggle(&entry.path),
                PanelSide::Remote => fm.remote.select_toggle(&entry.path),
            }
        } else if mods.shift {
            match side {
                PanelSide::Local => fm.local.select_range(&entry.path, hidden_local),
                PanelSide::Remote => fm.remote.select_range(&entry.path, hidden_remote),
            }
        } else {
            let double = fm
                .last_click
                .as_ref()
                .map(|(p, t)| p == &entry.path && now - t < 400.0)
                .unwrap_or(false);
            if double && entry.is_dir {
                let path = entry.path.clone();
                match side {
                    PanelSide::Local => {
                        fm.local.select_single(&entry.path);
                        fm.local.auto_follow = false;
                    }
                    PanelSide::Remote => {
                        fm.remote.select_single(&entry.path);
                        fm.remote.auto_follow = false;
                    }
                }
                fm.last_click = None;
                let _ = fm;
                match side {
                    PanelSide::Local => app.load_local_dir(session_id, path),
                    PanelSide::Remote => app.load_remote_dir(session_id, path),
                }
                return;
            }
            match side {
                PanelSide::Local => fm.local.select_single(&entry.path),
                PanelSide::Remote => fm.remote.select_single(&entry.path),
            }
            fm.last_click = Some((entry.path.clone(), now));
        }
    }

    if response.double_clicked() && !entry.is_dir {
        // Non-directory double click: copy the path to the clipboard.
        ui.ctx().copy_text(entry.path.clone());
        app.toast("路径已复制", crate::session::ToastKind::Success);
    }

    if response.secondary_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let fm = app.fm_state_mut(session_id);
            let already = match side {
                PanelSide::Local => fm.local.selected.contains(&entry.path),
                PanelSide::Remote => fm.remote.selected.contains(&entry.path),
            };
            if !already {
                match side {
                    PanelSide::Local => fm.local.select_single(&entry.path),
                    PanelSide::Remote => fm.remote.select_single(&entry.path),
                }
            }
            fm.context_menu = Some((side.clone(), Some(entry.path.clone()), pos));
        }
    }
    let _ = panel_path;
}

fn file_type_color(name: &str) -> Color32 {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" => Color32::from_rgb(0xff, 0x9e, 0x64),
        "js" | "ts" | "tsx" | "jsx" => Color32::from_rgb(0xff, 0xd9, 0x8a),
        "json" => Color32::from_rgb(0xe7, 0xc3, 0x6f),
        "py" => Color32::from_rgb(0x94, 0xc2, 0xff),
        "md" | "txt" => Color32::from_rgb(0xa8, 0xbd, 0xd1),
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" => {
            Color32::from_rgb(0xc7, 0x92, 0xea)
        }
        "zip" | "gz" | "tar" | "xz" | "bz2" => Color32::from_rgb(0xe7, 0xc3, 0x6f),
        "sh" | "bash" | "zsh" => Color32::from_rgb(0x7c, 0xcf, 0x92),
        _ => Color32::from_rgb(0x8b, 0xa8, 0xc4),
    }
}

// ── Context menu ─────────────────────────────────────────────────────────────

fn context_menu(ui: &mut Ui, app: &mut AppState, session_id: &str) {
    let Some((side, path, pos)) = app
        .fm
        .get(session_id)
        .and_then(|fm| fm.context_menu.clone())
    else {
        return;
    };

    let has_path = path.is_some();
    let mut items: Vec<&str> = Vec::new();
    if has_path {
        items.push("打开");
        items.push("重命名");
    }
    items.push("新建文件夹");
    if has_path {
        items.push("删除");
        items.push("复制路径");
    }

    let width = 160.0;
    let row_h = 30.0;
    let pad = 6.0;
    let height = items.len() as f32 * row_h + pad * 2.0;

    // Keep the whole menu on screen.
    let screen = ui.ctx().input(|i| i.viewport_rect());
    let mut origin = pos;
    if origin.x + width > screen.right() - 4.0 {
        origin.x = (screen.right() - width - 4.0).max(screen.left() + 4.0);
    }
    if origin.y + height > screen.bottom() - 4.0 {
        origin.y = (screen.bottom() - height - 4.0).max(screen.top() + 4.0);
    }
    let menu = Rect::from_min_size(origin, Vec2::new(width, height));

    let mut action: Option<&str> = None;
    let mut close = false;

    // The file-manager panel clips its painter to its own rect, so a menu drawn
    // there gets cut off by the collapse bar below it. A foreground area is not
    // clipped by the panel and always draws on top.
    egui::Area::new(egui::Id::new(("fm-ctx-menu", session_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(origin)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(Vec2::new(width, height));
            let painter = ui.painter().clone();
            painter.rect_filled(
                menu,
                CornerRadius::same(theme::RADIUS_SM),
                Color32::from_rgba_premultiplied(11, 26, 49, 252),
            );
            painter.rect_stroke(
                menu,
                CornerRadius::same(theme::RADIUS_SM),
                Stroke::new(1.0, Color32::from_rgba_premultiplied(38, 61, 97, 89)),
                StrokeKind::Inside,
            );

            for (i, label) in items.iter().enumerate() {
                let item_rect = Rect::from_min_size(
                    Pos2::new(menu.left() + pad, menu.top() + pad + i as f32 * row_h),
                    Vec2::new(width - pad * 2.0, row_h - 2.0),
                );
                let r = ui.interact(
                    item_rect,
                    ui.id().with(("fm-ctx", session_id, i)),
                    Sense::click(),
                );
                if r.hovered() {
                    painter.rect_filled(
                        item_rect,
                        CornerRadius::same(theme::RADIUS_XS),
                        Color32::from_rgba_premultiplied(28, 49, 84, 170),
                    );
                }
                painter.text(
                    Pos2::new(item_rect.left() + 10.0, item_rect.center().y),
                    Align2::LEFT_CENTER,
                    *label,
                    theme::f_xs(),
                    if *label == "删除" && r.hovered() {
                        theme::ERROR
                    } else if r.hovered() {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_SECONDARY
                    },
                );
                if r.clicked() {
                    action = Some(label);
                }
            }

            // A click anywhere outside the menu closes it.
            if ui.input(|i| i.pointer.any_click())
                && !menu.contains(ui.input(|i| i.pointer.interact_pos().unwrap_or(Pos2::ZERO)))
            {
                close = true;
            }
        });

    if let Some(action) = action {
        app.clear_context_menu(session_id);
        match action {
            "打开" => {
                if let Some(path) = path.clone() {
                    let is_dir = app
                        .fm
                        .get(session_id)
                        .map(|fm| match side {
                            PanelSide::Local => fm.local.entries.iter().any(|e| e.path == path && e.is_dir),
                            PanelSide::Remote => fm.remote.entries.iter().any(|e| e.path == path && e.is_dir),
                        })
                        .unwrap_or(false);
                    if is_dir {
                        {
                            let fm = app.fm_state_mut(session_id);
                            match side {
                                PanelSide::Local => fm.local.auto_follow = false,
                                PanelSide::Remote => fm.remote.auto_follow = false,
                            }
                        }
                        match side {
                            PanelSide::Local => app.load_local_dir(session_id, path),
                            PanelSide::Remote => app.load_remote_dir(session_id, path),
                        }
                    }
                }
            }
            "重命名" => {
                if let Some(path) = path.clone() {
                    let name = remote_fs::basename(&path);
                    let fm = app.fm_state_mut(session_id);
                    fm.inline_action = Some(InlineAction {
                        side: side.clone(),
                        kind: InlineKind::Rename,
                        value: name,
                        path: Some(path),
                    });
                }
            }
            "新建文件夹" => {
                let fm = app.fm_state_mut(session_id);
                fm.inline_action = Some(InlineAction {
                    side: side.clone(),
                    kind: InlineKind::NewFolder,
                    value: String::new(),
                    path: None,
                });
            }
            "删除" => {
                let paths: Vec<String> = app
                    .fm
                    .get(session_id)
                    .map(|fm| match side {
                        PanelSide::Local => fm.local.selected_items(),
                        PanelSide::Remote => fm.remote.selected_items(),
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| f.path)
                    .collect();
                app.request_delete(session_id, side.clone(), paths);
            }
            "复制路径" => {
                if let Some(path) = path.clone() {
                    ui.ctx().copy_text(path);
                    app.toast("路径已复制", crate::session::ToastKind::Success);
                }
            }
            _ => {}
        }
        return;
    }

    if close {
        app.clear_context_menu(session_id);
    }
}

// ── Inline rename / new-folder dialog ────────────────────────────────────────

fn inline_dialog(ui: &mut Ui, app: &mut AppState, session_id: &str) {
    let Some(action) = app
        .fm
        .get(session_id)
        .and_then(|fm| fm.inline_action.clone())
    else {
        return;
    };
    let screen = ui.ctx().input(|i| i.viewport_rect());
    let width = 360.0;
    let height = 170.0;
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));

    // Dim the background
    ui.painter().rect_filled(
        screen,
        0,
        Color32::from_rgba_premultiplied(2, 6, 14, 184),
    );

    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_LG), theme::BG_CARD);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_LG),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    let title = match action.kind {
        InlineKind::NewFolder => "新建文件夹",
        InlineKind::Rename => "重命名",
    };
    painter.text(
        Pos2::new(rect.left() + 20.0, rect.top() + 22.0),
        Align2::LEFT_CENTER,
        title,
        theme::f_md(),
        theme::TEXT_PRIMARY,
    );

    let mut value = action.value.clone();
    let input_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 20.0, rect.top() + 54.0),
        Vec2::new(width - 40.0, 32.0),
    );
    // Paint the box *before* the text field: the fill is opaque, so drawing it
    // afterwards covered the text.
    let input_id = ui.make_persistent_id(("fm-inline-input", session_id));
    let focused = ui.memory(|m| m.has_focus(input_id));
    painter.rect_filled(
        input_rect,
        CornerRadius::same(theme::RADIUS_SM),
        theme::BG_INPUT,
    );
    painter.rect_stroke(
        input_rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(1.0, if focused { theme::ACCENT } else { theme::BORDER }),
        StrokeKind::Inside,
    );

    // 10 px inner padding so the text sits inside the frame.
    let inner = input_rect.shrink2(Vec2::new(10.0, 6.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add(
        egui::TextEdit::singleline(&mut value)
            .id(input_id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(theme::f_sm())
            .text_color(theme::TEXT_PRIMARY)
            .desired_width(inner.width()),
    );
    // Take focus only when nothing else owns it, so the buttons stay clickable.
    if ui.memory(|m| m.focused().is_none()) {
        ui.memory_mut(|m| m.request_focus(input_id));
    }

    // The button group measures ~144 px wide; a 130 px box made it overflow to
    // the dialog edge. Give it room and right-align it on the 20 px padding.
    let footer = Rect::from_min_size(
        Pos2::new(rect.right() - 20.0 - 170.0, rect.bottom() - 48.0),
        Vec2::new(170.0, 30.0),
    );
    let labels = [("取消", true), ("确定", !value.trim().is_empty())];
    let clicked = widgets::button_row(ui, footer, &labels);

    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let confirm = clicked == Some(1) || enter;

    if escape || clicked == Some(0) {
        if let Some(fm) = app.fm.get_mut(session_id) {
            fm.inline_action = None;
        }
        return;
    }

    if confirm {
        let side = action.side.clone();
        let kind = action.kind;
        let path = action.path.clone();
        if let Some(fm) = app.fm.get_mut(session_id) {
            fm.inline_action = None;
        }
        match kind {
            InlineKind::NewFolder => app.fm_new_folder(session_id, side, &value),
            InlineKind::Rename => {
                if let Some(path) = path {
                    app.fm_rename(session_id, side, &path, &value);
                }
            }
        }
    } else if let Some(fm) = app.fm.get_mut(session_id) {
        if let Some(a) = fm.inline_action.as_mut() {
            a.value = value;
        }
    }
}
