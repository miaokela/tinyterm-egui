//! Hosts modal (`HostsModal.tsx`): searchable list + create/edit form.

use crate::models::Bookmark;
use crate::state::{AppState, ConfirmAction, ConfirmRequest, HostFormState, ModalKind};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

const SHELL_WIDTH: f32 = 620.0;
const SHELL_HEIGHT: f32 = 480.0;
const FORM_WIDTH: f32 = 500.0;
const FORM_HEIGHT: f32 = 560.0;
const ROW_HEIGHT: f32 = 56.0;

pub fn show(ui: &mut Ui, app: &mut AppState) {
    if app.modal != ModalKind::Hosts && app.modal != ModalKind::HostForm {
        return;
    }
    let screen = ui.ctx().input(|i| i.viewport_rect());
    ui.painter().rect_filled(
        screen,
        0,
        Color32::from_rgba_premultiplied(2, 6, 14, 184),
    );

    if app.modal == ModalKind::Hosts {
        list(ui, app, screen);
    }
    if app.modal == ModalKind::HostForm {
        form(ui, app, screen);
    }
}

fn list(ui: &mut Ui, app: &mut AppState, screen: Rect) {
    let width = SHELL_WIDTH.min(screen.width() - 40.0);
    let height = SHELL_HEIGHT.min(screen.height() - 60.0);
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_LG), theme::BG_CARD);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_LG),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    // ── Header ───────────────────────────────────────────────────────────────
    let header = Rect::from_min_size(rect.min, Vec2::new(width, 52.0));
    painter.text(
        Pos2::new(header.left() + 20.0, header.center().y),
        Align2::LEFT_CENTER,
        "Hosts",
        theme::f_lg(),
        theme::TEXT_PRIMARY,
    );
    painter.line_segment(
        [
            Pos2::new(header.left(), header.bottom() - 0.5),
            Pos2::new(header.right(), header.bottom() - 0.5),
        ],
        Stroke::new(1.0, theme::BORDER),
    );
    let close_rect = Rect::from_center_size(
        Pos2::new(header.right() - 24.0, header.center().y),
        Vec2::splat(28.0),
    );
    let r_close = ui.interact(close_rect, ui.id().with("hosts-close"), Sense::click());
    widgets::cross(
        &painter,
        close_rect.center(),
        11.0,
        if r_close.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
    );
    if r_close.clicked() {
        app.modal = ModalKind::None;
        return;
    }

    // ── Toolbar ──────────────────────────────────────────────────────────────
    let toolbar = Rect::from_min_size(
        Pos2::new(rect.left(), header.bottom()),
        Vec2::new(width, 46.0),
    );
    let search_rect = Rect::from_min_size(
        Pos2::new(toolbar.left() + 16.0, toolbar.top() + 8.0),
        Vec2::new(width - 16.0 - 60.0, 30.0),
    );
    let mut search_ui = ui.new_child(egui::UiBuilder::new().max_rect(search_rect));
    widgets::text_input_ext(
        &mut search_ui,
        &mut app.host_search,
        "搜索主机名 / IP...",
        search_rect.width(),
        false,
        Some(crate::icons::MAGNIFYING_GLASS),
    );

    // Add orb
    let add_rect = Rect::from_center_size(
        Pos2::new(toolbar.right() - 34.0, toolbar.center().y),
        Vec2::splat(30.0),
    );
    let mut add_ui = ui.new_child(egui::UiBuilder::new().max_rect(add_rect));
    let r_add = widgets::orb_button(&mut add_ui, 30.0, |p, r, c| {
        widgets::plus(p, r.center(), 13.0, c)
    })
    .on_hover_text("新增主机");
    if r_add.clicked() {
        app.host_form = Some(HostFormState {
            port: "22".into(),
            color: "#7c5cbf".into(),
            term: "xterm-256color".into(),
            encode: "utf8".into(),
            enable_sftp: true,
            keepalive_interval: "30000".into(),
            ..Default::default()
        });
        app.modal = ModalKind::HostForm;
    }

    // ── List ─────────────────────────────────────────────────────────────────
    let list_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, toolbar.bottom()),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 12.0),
    );
    let filter = app.host_search.to_lowercase();
    let hosts: Vec<Bookmark> = app
        .bookmarks
        .iter()
        .filter(|h| {
            filter.is_empty()
                || h.title.to_lowercase().contains(&filter)
                || h.host.to_lowercase().contains(&filter)
        })
        .cloned()
        .collect();

    if hosts.is_empty() {
        painter.text(
            list_rect.center(),
            Align2::CENTER_CENTER,
            if app.host_search.is_empty() {
                "暂无主机"
            } else {
                "未找到匹配主机"
            },
            theme::f_sm(),
            theme::TEXT_MUTED,
        );
        return;
    }

    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
    egui::ScrollArea::vertical()
        .id_salt("hosts-list")
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for host in &hosts {
                host_row(ui, app, host);
            }
        });
}

fn host_row(ui: &mut Ui, app: &mut AppState, host: &Bookmark) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ROW_HEIGHT),
        Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let _unreachable = app
        .host_reachability
        .get(&host.id)
        .map(|r| *r == crate::models::HostReachability::Unreachable)
        .unwrap_or(false);

    painter.rect_filled(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        if response.hovered() {
            Color32::from_rgba_premultiplied(20, 44, 85, 33)
        } else {
            Color32::from_rgba_premultiplied(11, 24, 46, 18)
        },
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_SM),
        Stroke::new(
            1.0,
            if response.hovered() {
                Color32::from_rgba_premultiplied(67, 115, 153, 102)
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );

    // Colour dot
    let dot = host.accent();
    let dot_center = Pos2::new(rect.left() + 22.0, rect.center().y);
    painter.circle_filled(dot_center, 5.0, dot);
    painter.circle_stroke(
        dot_center,
        6.5,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(dot.r(), dot.g(), dot.b(), 120)),
    );

    // Name + meta
    painter.text(
        Pos2::new(rect.left() + 40.0, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        theme::truncate(
            painter,
            &host.display_name(),
            &theme::f_sm(),
            rect.width() - 240.0,
        ),
        theme::f_sm(),
        theme::TEXT_PRIMARY,
    );
    painter.text(
        Pos2::new(rect.left() + 40.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        format!("{}:{}", host.host, host.port),
        theme::font_mono(theme::TEXT_XS - 1.0),
        theme::TEXT_MUTED,
    );

    // Credential badge
    let cred_text = match app.profile(host.profile_id.as_deref().unwrap_or("")) {
        Some(p) => format!("{} · {}", if p.is_key() { "key" } else { "pwd" }, p.title),
        None if !host.username.is_empty() => format!("{} · 手动输入", host.username),
        None => "连接时输入".to_string(),
    };
    let badge_x = rect.left() + 180.0;
    if badge_x < rect.right() - 180.0 {
        let text_w = painter
            .layout_no_wrap(cred_text.clone(), theme::font_sans(theme::TEXT_XS - 1.0), Color32::WHITE)
            .size()
            .x;
        let badge = Rect::from_min_size(
            Pos2::new(badge_x, rect.center().y - 9.0),
            Vec2::new(text_w + 16.0, 18.0),
        );
        painter.rect_filled(
            badge,
            CornerRadius::same(9),
            Color32::from_rgba_premultiplied(15, 35, 65, 46),
        );
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            cred_text,
            theme::font_sans(theme::TEXT_XS - 1.0),
            theme::TEXT_SECONDARY,
        );
    }

    // Actions: connect orb, edit, duplicate, delete
    let mut bx = rect.right() - 22.0;
    let btn = |ui: &mut Ui, id: &str, cx: f32| {
        let r = Rect::from_center_size(Pos2::new(cx, rect.center().y), Vec2::splat(26.0));
        ui.interact(r, ui.id().with(id), Sense::click())
    };

    // delete
    let del_rect = Rect::from_center_size(Pos2::new(bx, rect.center().y), Vec2::splat(26.0));
    let r_del = ui.interact(del_rect, ui.id().with(("host-del", &host.id)), Sense::click());
    widgets::trash(ui.painter(), del_rect.center(), if r_del.hovered() { theme::ERROR } else { theme::TEXT_MUTED });
    bx -= 30.0;
    // duplicate
    let dup_rect = Rect::from_center_size(Pos2::new(bx, rect.center().y), Vec2::splat(26.0));
    let r_dup = ui.interact(dup_rect, ui.id().with(("host-dup", &host.id)), Sense::click());
    widgets::copy_glyph(ui.painter(), dup_rect.center(), if r_dup.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED });
    bx -= 30.0;
    // edit
    let edit_rect = Rect::from_center_size(Pos2::new(bx, rect.center().y), Vec2::splat(26.0));
    let r_edit = ui.interact(edit_rect, ui.id().with(("host-edit", &host.id)), Sense::click());
    widgets::pencil(ui.painter(), edit_rect.center(), if r_edit.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED });
    bx -= 36.0;
    // connect orb
    let connect_rect = Rect::from_center_size(Pos2::new(bx, rect.center().y), Vec2::splat(28.0));
    let mut connect_ui = ui.new_child(egui::UiBuilder::new().max_rect(connect_rect));
    let r_connect = widgets::orb_button(&mut connect_ui, 28.0, |p, r, c| {
        widgets::plug(p, r.center(), c)
    })
    .on_hover_text("连接");

    let _ = btn;

    if r_connect.clicked() {
        app.open_host_tab(&host.id);
    }
    if r_edit.clicked() {
        open_form(app, host, false);
    }
    if r_dup.clicked() {
        open_form(app, host, true);
    }
    if r_del.clicked() {
        app.confirm = Some(ConfirmRequest {
            title: "删除 Host".into(),
            message: "确认删除该主机？".into(),
            confirm_text: "删除".into(),
            cancel_text: "取消".into(),
            action: ConfirmAction::DeleteHost(host.id.clone()),
        });
    }
}

fn open_form(app: &mut AppState, host: &Bookmark, duplicate: bool) {
    app.host_form = Some(HostFormState {
        editing_id: if duplicate { None } else { Some(host.id.clone()) },
        is_duplicate: duplicate,
        title: if duplicate {
            format!("{} (副本)", host.title)
        } else {
            host.title.clone()
        },
        host: host.host.clone(),
        port: host.port.to_string(),
        username: host.username.clone(),
        profile_id: host.profile_id.clone().unwrap_or_default(),
        color: host
            .color
            .clone()
            .unwrap_or_else(|| "#7c5cbf".to_string()),
        description: host.description.clone().unwrap_or_default(),
        start_directory_remote: host.start_directory_remote.clone().unwrap_or_default(),
        start_directory_local: host.start_directory_local.clone().unwrap_or_default(),
        term: host.term.clone(),
        encode: host.encode.clone(),
        enable_sftp: host.enable_sftp,
        keepalive_interval: host.keepalive_interval.to_string(),
        password: String::new(),
        error: None,
        saving: false,
    });
    app.modal = ModalKind::HostForm;
}

fn form(ui: &mut Ui, app: &mut AppState, screen: Rect) {
    let Some(mut form) = app.host_form.clone() else {
        app.modal = ModalKind::Hosts;
        return;
    };
    let width = FORM_WIDTH.min(screen.width() - 40.0);
    let height = FORM_HEIGHT.min(screen.height() - 60.0);
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_LG), theme::BG_CARD);
    painter.rect_stroke(
        rect,
        CornerRadius::same(theme::RADIUS_LG),
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    let title = if form.is_duplicate {
        "复制 Host"
    } else if form.editing_id.is_some() {
        "编辑 Host"
    } else {
        "新建 Host"
    };
    let header = Rect::from_min_size(rect.min, Vec2::new(width, 50.0));
    painter.text(
        Pos2::new(header.left() + 20.0, header.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::f_md(),
        theme::TEXT_PRIMARY,
    );
    painter.line_segment(
        [
            Pos2::new(header.left(), header.bottom() - 0.5),
            Pos2::new(header.right(), header.bottom() - 0.5),
        ],
        Stroke::new(1.0, theme::BORDER),
    );
    let close_rect = Rect::from_center_size(
        Pos2::new(header.right() - 24.0, header.center().y),
        Vec2::splat(28.0),
    );
    let r_close = ui.interact(close_rect, ui.id().with("hostform-close"), Sense::click());
    widgets::cross(
        &painter,
        close_rect.center(),
        11.0,
        if r_close.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
    );
    if r_close.clicked() {
        app.modal = ModalKind::Hosts;
        app.host_form = None;
        return;
    }

    // ── Body (scrollable) ────────────────────────────────────────────────────
    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 20.0, header.bottom() + 8.0),
        Pos2::new(rect.right() - 20.0, rect.bottom() - 56.0),
    );
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(body));
    egui::ScrollArea::vertical()
        .id_salt("host-form")
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.set_width(body.width());
            widgets::labelled(ui, "名称（可选）", false);
            let full = body.width();
            widgets::text_input(ui, &mut form.title, "My Production Server", full, false);

            widgets::labelled(ui, "主机地址", true);
            let host_w = full * 0.72;
            let mut row = ui.horizontal(|ui| {
                widgets::text_input(ui, &mut form.host, "192.168.1.1 / example.com", host_w - 8.0, false);
                widgets::text_input(ui, &mut form.port, "22", full - host_w, false);
            });
            let _ = &mut row;

            widgets::labelled(ui, "Credential（可选）", false);
            let mut open_cred_form = false;
            ui.horizontal(|ui| {
                if widgets::ghost_button(ui, "＋ 新增凭据", true).clicked() {
                    open_cred_form = true;
                }
            });
            if open_cred_form {
                app.credential_form = Some(crate::state::CredentialFormState {
                    auth_type: "password".into(),
                    from_host_form: true,
                    ..Default::default()
                });
                app.modal = ModalKind::CredentialForm;
            }
            let creds = app.profiles.clone();
            if creds.is_empty() {
                ui.label(
                    egui::RichText::new("暂无 Credential，连接时将提示输入用户名和密码")
                        .size(theme::TEXT_XS)
                        .color(theme::TEXT_MUTED),
                );
            } else {
                for cred in &creds {
                    let selected = form.profile_id == cred.id;
                    let (r, resp) = ui.allocate_exact_size(
                        Vec2::new(full, 30.0),
                        Sense::click(),
                    );
                    let painter = ui.painter();
                    painter.rect_filled(
                        r,
                        CornerRadius::same(theme::RADIUS_XS),
                        if selected {
                            Color32::from_rgba_premultiplied(28, 49, 84, 160)
                        } else if resp.hovered() {
                            Color32::from_rgba_premultiplied(20, 40, 70, 110)
                        } else {
                            Color32::from_rgba_premultiplied(11, 24, 46, 60)
                        },
                    );
                    painter.text(
                        Pos2::new(r.left() + 10.0, r.center().y),
                        Align2::LEFT_CENTER,
                        if cred.is_key() { "KEY" } else { "PWD" },
                        theme::font_sans(theme::TEXT_XS - 2.0),
                        theme::ACCENT_LIGHT,
                    );
                    painter.text(
                        Pos2::new(r.left() + 52.0, r.center().y),
                        Align2::LEFT_CENTER,
                        theme::truncate(painter, &cred.title, &theme::f_xs(), full * 0.4),
                        theme::f_xs(),
                        theme::TEXT_PRIMARY,
                    );
                    painter.text(
                        Pos2::new(r.right() - 12.0, r.center().y),
                        Align2::RIGHT_CENTER,
                        &cred.username,
                        theme::font_mono(theme::TEXT_XS - 1.0),
                        theme::TEXT_MUTED,
                    );
                    if resp.clicked() {
                        form.profile_id = if selected {
                            String::new()
                        } else {
                            cred.id.clone()
                        };
                    }
                }
            }
            if let Some(cred) = creds.iter().find(|c| c.id == form.profile_id) {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!(
                        "将使用用户名 {} ，认证方式：{}",
                        cred.username,
                        if cred.is_key() { "私钥" } else { "密码" }
                    ))
                    .size(theme::TEXT_XS)
                    .color(theme::TEXT_MUTED),
                );
            }

            widgets::labelled(ui, "远程初始目录（可选）", false);
            widgets::text_input(
                ui,
                &mut form.start_directory_remote,
                "/home/user",
                full,
                false,
            );

            widgets::labelled(ui, "标签颜色", false);
            let mut color = crate::models::parse_hex_color(&form.color)
                .unwrap_or(Color32::from_rgb(0x7c, 0x5c, 0xbf));
            let mut row = ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(46.0, 30.0), Sense::hover());
                ui.painter().rect_filled(
                    r,
                    CornerRadius::same(theme::RADIUS_SM),
                    color,
                );
                ui.painter().rect_stroke(
                    r,
                    CornerRadius::same(theme::RADIUS_SM),
                    Stroke::new(1.0, theme::BORDER),
                    StrokeKind::Inside,
                );
                let palette = [
                    Color32::from_rgb(0x7c, 0x5c, 0xbf),
                    Color32::from_rgb(0x2f, 0x7d, 0xff),
                    Color32::from_rgb(0x57, 0xd8, 0xb2),
                    Color32::from_rgb(0xe0, 0x57, 0x5c),
                    Color32::from_rgb(0xf0, 0xa0, 0x40),
                    Color32::from_rgb(0x57, 0xe3, 0xa5),
                ];
                for c in palette {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                    ui.painter().rect_filled(r, CornerRadius::same(4), c);
                    if color == c {
                        ui.painter().rect_stroke(
                            r,
                            CornerRadius::same(4),
                            Stroke::new(1.5, Color32::WHITE),
                            StrokeKind::Inside,
                        );
                    }
                    if resp.clicked() {
                        color = c;
                    }
                }
            });
            let _ = &mut row;
            form.color = crate::models::color_to_hex(color);

            widgets::labelled(ui, "备注", false);
            widgets::text_input(ui, &mut form.description, "可选", full, false);

            ui.add_space(6.0);
            if let Some(err) = &form.error {
                let (r, _) = ui.allocate_exact_size(Vec2::new(full, 26.0), Sense::hover());
                ui.painter().rect_filled(
                    r,
                    CornerRadius::same(theme::RADIUS_XS),
                    Color32::from_rgba_premultiplied(33, 8, 8, 33),
                );
                ui.painter().text(
                    Pos2::new(r.left() + 10.0, r.center().y),
                    Align2::LEFT_CENTER,
                    err,
                    theme::f_xs(),
                    theme::ERROR,
                );
            }
            ui.add_space(8.0);
        });

    // ── Footer ───────────────────────────────────────────────────────────────
    let footer = Rect::from_min_size(
        Pos2::new(rect.right() - 20.0 - 170.0, rect.bottom() - 46.0),
        Vec2::new(170.0, 30.0),
    );
    let labels = [
        ("取消", true),
        (
            if form.editing_id.is_some() { "更新" } else { "创建" },
            !form.host.trim().is_empty(),
        ),
    ];
    let clicked = widgets::button_row(ui, footer, &labels);

    app.host_form = Some(form.clone());

    if clicked == Some(0) {
        app.modal = ModalKind::Hosts;
        app.host_form = None;
        return;
    }
    if clicked == Some(1) {
        save_host(app, &form);
    }
}

fn save_host(app: &mut AppState, form: &HostFormState) {
    if form.host.trim().is_empty() {
        let mut f = form.clone();
        f.error = Some("请填写主机地址".into());
        app.host_form = Some(f);
        return;
    }
    let port: u16 = form.port.trim().parse().unwrap_or(22);
    let keepalive: u32 = form.keepalive_interval.trim().parse().unwrap_or(30000);
    let cred = app.profile(&form.profile_id).cloned();

    let mut bookmark = form
        .editing_id
        .as_deref()
        .and_then(|id| app.bookmark(id))
        .cloned()
        .unwrap_or_default();
    bookmark.title = form.title.clone();
    bookmark.host = form.host.trim().to_string();
    bookmark.port = port;
    bookmark.username = cred
        .as_ref()
        .map(|c| c.username.clone())
        .unwrap_or_else(|| form.username.clone());
    bookmark.auth_type = "profile".into();
    bookmark.profile_id = if form.profile_id.is_empty() {
        None
    } else {
        Some(form.profile_id.clone())
    };
    bookmark.color = Some(form.color.clone());
    bookmark.description = if form.description.is_empty() {
        None
    } else {
        Some(form.description.clone())
    };
    bookmark.start_directory_remote = if form.start_directory_remote.is_empty() {
        None
    } else {
        Some(form.start_directory_remote.clone())
    };
    bookmark.start_directory_local = if form.start_directory_local.is_empty() {
        None
    } else {
        Some(form.start_directory_local.clone())
    };
    bookmark.term = form.term.clone();
    bookmark.encode = form.encode.clone();
    bookmark.enable_sftp = form.enable_sftp;
    bookmark.keepalive_interval = keepalive;
    bookmark.updated_at = crate::models::now_unix();

    if form.editing_id.is_some() {
        app.update_bookmark(bookmark, None);
    } else {
        bookmark.id = uuid::Uuid::new_v4().to_string();
        bookmark.created_at = crate::models::now_unix();
        app.create_bookmark(bookmark, None);
    }
    app.modal = ModalKind::Hosts;
    app.host_form = None;
}

// ── Small glyphs ─────────────────────────────────────────────────────────────




