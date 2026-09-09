//! Terminal quick-actions toolbar (`TerminalQuickActions.tsx`):
//! CPU / memory / disk probes, a curated command library and shell history.

use crate::state::{AppState, QuickPopup, SystemInfoKind, SystemInfoState};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

pub const COMMAND_CATEGORIES: &[(&str, &[(&str, &str)])] = &[
    (
        "服务管理",
        &[
            ("查看服务状态", "systemctl status "),
            ("启动服务", "systemctl start "),
            ("停止服务", "systemctl stop "),
            ("重启服务", "systemctl restart "),
            ("重载服务", "systemctl reload "),
            ("启用开机自启", "systemctl enable "),
            ("禁用开机自启", "systemctl disable "),
            (
                "查看运行中服务",
                "systemctl list-units --type=service --state=running",
            ),
            ("查看失败服务", "systemctl list-units --failed"),
            ("查看服务日志", "journalctl -u "),
            ("实时跟踪日志", "journalctl -u  -f"),
            ("查看最近日志", "journalctl -n 50"),
        ],
    ),
    (
        "进程管理",
        &[
            ("按名称查进程", "ps -ef | grep "),
            ("按名称精确查进程", "pgrep -a "),
            ("按名称杀进程", "pkill -9 "),
            ("按PID杀进程", "kill -9 "),
            ("查看进程树", "pstree -p "),
            ("查看进程详情", "ps aux | grep "),
            ("查看端口占用进程", "lsof -i :"),
            ("查看文件占用进程", "lsof "),
            ("查看进程打开的文件", "lsof -p "),
            ("优雅终止进程", "kill -15 "),
        ],
    ),
    (
        "网络诊断",
        &[
            ("查看监听端口", "netstat -tlnp"),
            ("查看套接字状态", "ss -tlnp"),
            ("测试连通性", "ping -c 4 "),
            ("路由追踪", "traceroute "),
            ("查看外网IP", "curl -s ip.sb"),
            ("HTTP请求头", "curl -I -L --max-time 10 "),
            ("DNS查询", "dig +short "),
            ("查看路由表", "ip route"),
            ("查看网络接口", "ip addr"),
            ("抓包过滤", "tcpdump -i any -nn host "),
        ],
    ),
    (
        "文件与磁盘",
        &[
            ("查看目录大小", "du -sh "),
            ("查找大文件", "du -ah . | sort -rh | head -n 20"),
            ("查找空目录", "find . -type d -empty"),
            ("按名称查找文件", "find . -name "),
            ("查找最近修改文件", "find . -type f -mtime -1"),
            ("压缩目录", "tar -czvf archive.tar.gz "),
            ("解压tar.gz", "tar -xzvf "),
            ("查看文件编码", "file "),
            ("清空日志文件", "> "),
            ("查看文件前N行", "head -n 50 "),
            ("查看文件后N行", "tail -n 50 -f "),
            ("统计代码行数", "wc -l "),
        ],
    ),
    (
        "系统与权限",
        &[
            ("查看系统负载", "uptime"),
            ("查看系统信息", "uname -a"),
            ("查看当前用户", "whoami"),
            ("查看用户信息", "id"),
            ("查看登录用户", "who"),
            ("添加执行权限", "chmod +x "),
            ("递归改权限", "chmod -R 755 "),
            ("递归改属主", "chown -R $(whoami):$(whoami) "),
            ("查看环境变量", "env | grep "),
            ("查看定时任务", "crontab -l"),
            ("查看已安装包", "rpm -qa | grep "),
        ],
    ),
];

pub const CPU_CMD: &str =
    "ps -eo pid,pcpu,comm,args | awk 'NR==1{next} {print}' | sort -k2 -nr | head -n 100";
pub const MEM_CMD: &str =
    "ps -eo pid,pmem,comm,args | awk 'NR==1{next} {print}' | sort -k2 -nr | head -n 100";
pub const DISK_CMD: &str = "df -h";
pub const HISTORY_CMD: &str =
    "cat ~/.zsh_history 2>/dev/null || cat ~/.bash_history 2>/dev/null || echo \"\"";

const BAR_HEIGHT: f32 = 24.0;
const BTN_SIZE: f32 = 18.0;

pub fn show(ui: &mut Ui, app: &mut AppState, session_id: &str, term_rect: Rect) {
    let fm_open = app
        .session_tab(session_id)
        .map(|(_, s)| s.fm_open)
        .unwrap_or(false);
    let expanded = app.quick_mut(session_id).expanded;
    let popup = app.quick_mut(session_id).popup;

    let bar_width = if expanded {
        BTN_SIZE * 6.0 + 5.0 * 2.0 + 6.0
    } else {
        BTN_SIZE + 6.0
    };
    let bar_rect = Rect::from_min_size(
        Pos2::new(term_rect.right() - 10.0 - bar_width, term_rect.top() + 10.0),
        Vec2::new(bar_width, BAR_HEIGHT),
    );

    let painter = ui.painter().clone();
    painter.rect_filled(
        bar_rect,
        CornerRadius::same(theme::RADIUS_SM),
        Color32::from_rgba_premultiplied(16, 24, 39, 210),
    );
    painter.rect_stroke(
        bar_rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(1.0, Color32::from_rgba_premultiplied(40, 65, 102, 46)),
        StrokeKind::Inside,
    );

    let mut x = bar_rect.left() + 3.0;
    if !expanded {
        let r = icon_button(ui, Pos2::new(x + BTN_SIZE * 0.5, bar_rect.center().y), session_id, 0, false);
        chevrons(&painter, r.rect.center(), true, theme::TEXT_SECONDARY);
        if r.clicked() {
            app.quick_mut(session_id).expanded = true;
        }
        return;
    }

    let buttons: [(&str, u8); 6] = [
        ("CPU", 0),
        ("内存", 1),
        ("磁盘", 2),
        ("指令", 3),
        ("历史", 4),
        ("收起", 5),
    ];
    for (_label, idx) in buttons {
        let center = Pos2::new(x + BTN_SIZE * 0.5, bar_rect.center().y);
        let active = (idx == 3 && popup == QuickPopup::Commands)
            || (idx == 4 && popup == QuickPopup::History);
        let r = icon_button(ui, center, session_id, idx, active);
        let color = if active {
            Color32::from_rgba_premultiplied(220, 235, 255, 242)
        } else if r.hovered() {
            Color32::from_rgba_premultiplied(220, 235, 255, 230)
        } else {
            Color32::from_rgba_premultiplied(180, 200, 240, 179)
        };
        let p = ui.painter();
        match idx {
            0 => widgets::icon(p, center, 12.0, crate::icons::CPU, color),
            1 => widgets::icon(p, center, 12.0, crate::icons::DATABASE, color),
            2 => widgets::icon(p, center, 12.0, crate::icons::HARD_DRIVES, color),
            3 => widgets::icon(p, center, 12.0, crate::icons::BOOK_OPEN, color),
            4 => widgets::icon(p, center, 12.0, crate::icons::CLOCK, color),
            _ => chevrons(p, center, false, color),
        }
        if r.clicked() {
            match idx {
                0 | 1 | 2 => {
                    let kind = match idx {
                        0 => SystemInfoKind::Cpu,
                        1 => SystemInfoKind::Memory,
                        _ => SystemInfoKind::Disk,
                    };
                    let cmd = match kind {
                        SystemInfoKind::Cpu => CPU_CMD,
                        SystemInfoKind::Memory => MEM_CMD,
                        SystemInfoKind::Disk => DISK_CMD,
                    };
                    app.system_info = Some(SystemInfoState {
                        kind,
                        session_id: session_id.to_string(),
                        loading: true,
                        error: None,
                        rows: Vec::new(),
                        page: 0,
                    });
                    let request = app.mgr.request_id();
                    app.mgr.query(request, session_id, cmd.to_string());
                }
                3 => {
                    let q = app.quick_mut(session_id);
                    q.popup = if q.popup == QuickPopup::Commands {
                        QuickPopup::None
                    } else {
                        QuickPopup::Commands
                    };
                }
                4 => {
                    let request = app.mgr.request_id();
                    let q = app.quick_mut(session_id);
                    if q.popup == QuickPopup::History {
                        q.popup = QuickPopup::None;
                    } else {
                        q.popup = QuickPopup::History;
                        q.history_loading = true;
                        q.history_error = None;
                        q.history_request = Some(request);
                        app.mgr.query(request, session_id, HISTORY_CMD.to_string());
                    }
                }
                _ => {
                    let q = app.quick_mut(session_id);
                    q.expanded = false;
                    q.popup = QuickPopup::None;
                }
            }
        }
        x += BTN_SIZE + 2.0;
    }

    // ── Popups ───────────────────────────────────────────────────────────────
    let popup_width = 300.0;
    let popup_top = bar_rect.bottom() + 6.0;
    let max_height = (term_rect.bottom() - popup_top - 8.0).clamp(120.0, 420.0);
    let popup_rect = Rect::from_min_size(
        Pos2::new(bar_rect.right() - popup_width, popup_top),
        Vec2::new(popup_width, max_height),
    );

    match popup {
        QuickPopup::Commands => commands_popup(ui, app, session_id, popup_rect),
        QuickPopup::History => history_popup(ui, app, session_id, popup_rect),
        QuickPopup::None => {}
    }
    let _ = fm_open;
}

fn icon_button(
    ui: &mut Ui,
    center: Pos2,
    session_id: &str,
    idx: u8,
    active: bool,
) -> egui::Response {
    let rect = Rect::from_center_size(center, Vec2::splat(BTN_SIZE));
    // Keyed by session so the main and auxiliary panes never collide.
    let r = ui.interact(
        rect,
        ui.id().with(("qa-btn", session_id, idx)),
        Sense::click(),
    );
    if r.hovered() || active {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(theme::RADIUS_XS),
            if active {
                Color32::from_rgba_premultiplied(35, 65, 107, 190)
            } else {
                Color32::from_rgba_premultiplied(35, 59, 95, 120)
            },
        );
    }
    r
}

fn chevrons(painter: &egui::Painter, c: Pos2, left: bool, color: Color32) {
    let glyph = if left {
        crate::icons::CARET_LEFT
    } else {
        crate::icons::CARET_RIGHT
    };
    widgets::icon(painter, c - Vec2::new(2.0, 0.0), 12.0, glyph, color);
    widgets::icon(painter, c + Vec2::new(2.0, 0.0), 12.0, glyph, color);
}






fn popup_frame(ui: &Ui, rect: Rect) {
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        Color32::from_rgba_premultiplied(10, 17, 29, 240),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(1.0, Color32::from_rgba_premultiplied(40, 65, 102, 46)),
        StrokeKind::Inside,
    );
}

fn popup_header(ui: &mut Ui, app: &mut AppState, session_id: &str, rect: Rect, title: &str) {
    let painter = ui.painter().clone();
    painter.text(
        Pos2::new(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::font_sans(theme::TEXT_XS - 1.0),
        Color32::from_rgba_premultiplied(180, 200, 240, 230),
    );
    let close_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 14.0, rect.center().y),
        Vec2::splat(18.0),
    );
    let r = ui.interact(close_rect, ui.id().with(("qa-close", session_id)), Sense::click());
    widgets::cross(
        &painter,
        close_rect.center(),
        8.0,
        if r.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
    );
    if r.clicked() {
        app.quick_mut(session_id).popup = QuickPopup::None;
    }
}

fn commands_popup(ui: &mut Ui, app: &mut AppState, session_id: &str, rect: Rect) {
    popup_frame(ui, rect);
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 28.0));
    popup_header(ui, app, session_id, header, "常用指令（双击输入）");

    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, header.bottom() + 4.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
    );
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(body));
    let mut write: Option<String> = None;
    egui::ScrollArea::vertical()
        .id_salt(("qa-commands", session_id))
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let width = ui.available_width();
            for (index, (category, items)) in COMMAND_CATEGORIES.iter().enumerate() {
                if index > 0 {
                    ui.add_space(10.0);
                } else {
                    ui.add_space(4.0);
                }

                // Category header with a hairline underneath.
                let (r, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
                let painter = ui.painter();
                painter.text(
                    Pos2::new(r.left() + 4.0, r.center().y),
                    Align2::LEFT_CENTER,
                    *category,
                    theme::f_xs(),
                    theme::ACCENT_LIGHT,
                );
                painter.text(
                    Pos2::new(r.right() - 4.0, r.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{} 条", items.len()),
                    theme::font_sans(theme::TEXT_XS - 2.0),
                    theme::TEXT_MUTED,
                );
                painter.line_segment(
                    [
                        Pos2::new(r.left() + 2.0, r.bottom() - 1.0),
                        Pos2::new(r.right() - 2.0, r.bottom() - 1.0),
                    ],
                    Stroke::new(1.0, Color32::from_rgba_premultiplied(26, 41, 65, 46)),
                );
                ui.add_space(2.0);

                for (label, cmd) in items.iter() {
                    let (r, resp) =
                        ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::click());
                    let painter = ui.painter();
                    if resp.hovered() {
                        painter.rect_filled(
                            r,
                            CornerRadius::same(theme::RADIUS_XS),
                            Color32::from_rgba_premultiplied(24, 40, 63, 90),
                        );
                    }
                    painter.text(
                        Pos2::new(r.left() + 8.0, r.center().y),
                        Align2::LEFT_CENTER,
                        *label,
                        theme::f_xs(),
                        if resp.hovered() {
                            theme::TEXT_PRIMARY
                        } else {
                            theme::TEXT_SECONDARY
                        },
                    );
                    let label_w = painter
                        .layout_no_wrap(
                            (*label).to_owned(),
                            theme::f_xs(),
                            Color32::WHITE,
                        )
                        .size()
                        .x;
                    painter.text(
                        Pos2::new(r.right() - 8.0, r.center().y),
                        Align2::RIGHT_CENTER,
                        theme::truncate(
                            &painter,
                            cmd,
                            &theme::font_mono(theme::TEXT_XS - 2.0),
                            (r.width() - label_w - 26.0).max(40.0),
                        ),
                        theme::font_mono(theme::TEXT_XS - 2.0),
                        Color32::from_rgba_premultiplied(160, 190, 230, 200),
                    );
                    if resp.double_clicked() {
                        write = Some((*cmd).to_string());
                    }
                    if resp.hovered() {
                        resp.on_hover_text(format!("双击输入: {cmd}"));
                    }
                }
            }
            ui.add_space(8.0);
        });
    if let Some(cmd) = write {
        app.mgr.write(session_id, cmd.into_bytes());
        app.quick_mut(session_id).popup = QuickPopup::None;
    }
}

fn history_popup(ui: &mut Ui, app: &mut AppState, session_id: &str, rect: Rect) {
    popup_frame(ui, rect);
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 26.0));
    popup_header(ui, app, session_id, header, "历史命令");

    let state = app.quick_mut(session_id);
    let loading = state.history_loading;
    let error = state.history_error.clone();
    let history = state.history.clone();

    let body = Rect::from_min_max(
        Pos2::new(rect.left(), header.bottom()),
        rect.right_bottom(),
    );
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(body));

    if loading {
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(body));
        c.vertical_centered(|ui| {
            ui.add_space(20.0);
            widgets::loading_blocks(ui, ui.input(|i| i.time) as f32, 1.2);
            ui.add_space(6.0);
            ui.label(egui::RichText::new("加载中...").size(theme::TEXT_XS).color(theme::TEXT_MUTED));
        });
        return;
    }
    if let Some(err) = error {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            theme::truncate(ui.painter(), &err, &theme::f_xs(), body.width() - 20.0),
            theme::f_xs(),
            theme::ERROR,
        );
        return;
    }
    if history.is_empty() {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            "无历史命令",
            theme::f_xs(),
            theme::TEXT_MUTED,
        );
        return;
    }

    let mut execute: Option<String> = None;
    let mut insert: Option<String> = None;
    egui::ScrollArea::vertical()
        .id_salt(("qa-history", session_id))
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.add_space(4.0);
            for (index, cmd) in history.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width() - 8.0, 20.0),
                    Sense::click(),
                );
                let painter = ui.painter();
                if resp.hovered() {
                    painter.rect_filled(
                        r,
                        CornerRadius::same(theme::RADIUS_XS),
                        Color32::from_rgba_premultiplied(24, 40, 63, 90),
                    );
                }
                painter.text(
                    Pos2::new(r.left() + 6.0, r.center().y),
                    Align2::LEFT_CENTER,
                    theme::truncate(painter, cmd, &theme::font_mono(theme::TEXT_XS - 2.0), r.width() - 28.0),
                    theme::font_mono(theme::TEXT_XS - 2.0),
                    theme::TEXT_SECONDARY,
                );
                // Play button
                let play_rect = Rect::from_center_size(
                    Pos2::new(r.right() - 10.0, r.center().y),
                    Vec2::splat(14.0),
                );
                let pr = ui.interact(
                    play_rect,
                    // `cmd` alone is not unique: shell history repeats commands.
                    ui.id().with(("qa-play", session_id, index)),
                    Sense::click(),
                );
                let color = if pr.hovered() { theme::SUCCESS } else { theme::TEXT_MUTED };
                widgets::icon(ui.painter(), play_rect.center(), 11.0, crate::icons::PLAY, color);
                if pr.clicked() {
                    execute = Some(cmd.clone());
                }
                if resp.double_clicked() {
                    insert = Some(cmd.clone());
                }
            }
            ui.add_space(6.0);
        });

    if let Some(cmd) = execute {
        app.mgr.write(session_id, format!("{cmd}\r").into_bytes());
        app.quick_mut(session_id).popup = QuickPopup::None;
    } else if let Some(cmd) = insert {
        app.mgr.write(session_id, cmd.into_bytes());
        app.quick_mut(session_id).popup = QuickPopup::None;
    }
}
