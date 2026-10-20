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
const ROW_GAP: f32 = 6.0;
const PAGE_FOOTER_HEIGHT: f32 = 34.0;
const CRED_COLUMNS: usize = 3;
const CRED_CARD_HEIGHT: f32 = 46.0;
const CRED_GAP: f32 = 6.0;
const CRED_PAGER_HEIGHT: f32 = 20.0;

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
    let previous_search = app.host_search.clone();
    widgets::text_input_ext(
        &mut search_ui,
        &mut app.host_search,
        "搜索主机名 / IP...",
        search_rect.width(),
        false,
        Some(crate::icons::MAGNIFYING_GLASS),
    );
    if app.host_search != previous_search {
        // A new filter starts from the first page.
        app.hosts_page = 0;
    }

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

    // ── List + pagination ────────────────────────────────────────────────────
    let footer = Rect::from_min_size(
        Pos2::new(
            rect.left() + 12.0,
            rect.bottom() - 12.0 - PAGE_FOOTER_HEIGHT,
        ),
        Vec2::new(width - 24.0, PAGE_FOOTER_HEIGHT),
    );
    let list_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, toolbar.bottom()),
        Pos2::new(rect.right() - 12.0, footer.top() - 4.0),
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

    let row_pitch = ROW_HEIGHT + ROW_GAP;
    let per_page = (((list_rect.height() + ROW_GAP) / row_pitch).floor() as usize).max(1);
    let total_pages = hosts.len().div_ceil(per_page).max(1);
    let current = app.hosts_page.min(total_pages - 1);
    app.hosts_page = current;
    let start = current * per_page;
    let page_hosts = &hosts[start..(start + per_page).min(hosts.len())];

    let mut rows_ui = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
    rows_ui.spacing_mut().item_spacing.y = ROW_GAP;
    for host in page_hosts {
        host_row(&mut rows_ui, app, host);
    }

    // Footer: count + page position + prev/next.
    painter.line_segment(
        [
            Pos2::new(footer.left(), footer.top() + 0.5),
            Pos2::new(footer.right(), footer.top() + 0.5),
        ],
        Stroke::new(1.0, Color32::from_rgba_premultiplied(17, 40, 77, 51)),
    );
    painter.text(
        Pos2::new(footer.left() + 4.0, footer.center().y),
        Align2::LEFT_CENTER,
        format!(
            "共 {} 台 · 第 {} / {} 页",
            hosts.len(),
            current + 1,
            total_pages
        ),
        theme::f_xs(),
        theme::TEXT_MUTED,
    );

    if total_pages > 1 {
        let next_rect = Rect::from_center_size(
            Pos2::new(footer.right() - 12.0, footer.center().y),
            Vec2::splat(22.0),
        );
        let prev_rect = Rect::from_center_size(
            Pos2::new(footer.right() - 38.0, footer.center().y),
            Vec2::splat(22.0),
        );
        let r_next = ui.interact(next_rect, ui.id().with("hosts-page-next"), Sense::click());
        let r_prev = ui.interact(prev_rect, ui.id().with("hosts-page-prev"), Sense::click());
        widgets::chevron(
            &painter,
            prev_rect.center(),
            9.0,
            true,
            if current == 0 {
                theme::TEXT_MUTED
            } else if r_prev.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            },
        );
        widgets::chevron(
            &painter,
            next_rect.center(),
            9.0,
            false,
            if current + 1 >= total_pages {
                theme::TEXT_MUTED
            } else if r_next.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            },
        );
        if r_prev.clicked() && current > 0 {
            app.hosts_page = current - 1;
        }
        if r_next.clicked() && current + 1 < total_pages {
            app.hosts_page = current + 1;
        }
    }
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
    let name_left = rect.left() + 40.0;
    // The credential badge owns a fixed column; the name must stop before it
    // instead of running underneath it when the host title is long.
    let badge_x = rect.left() + 180.0;
    let name_right = badge_x - 12.0;
    painter.text(
        Pos2::new(name_left, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        theme::truncate(
            painter,
            &host.display_name(),
            &theme::f_sm(),
            (name_right - name_left).max(20.0),
        ),
        theme::f_sm(),
        theme::TEXT_PRIMARY,
    );
    let addr = format!("{}:{}", host.host, host.port);
    painter.text(
        Pos2::new(name_left, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        theme::truncate(
            painter,
            &addr,
            &theme::font_mono(theme::TEXT_XS - 1.0),
            (name_right - name_left).max(20.0),
        ),
        theme::font_mono(theme::TEXT_XS - 1.0),
        theme::TEXT_MUTED,
    );

    // Credential badge. Cap it before the action buttons so a long credential
    // title cannot overlap connect/edit/duplicate/delete either.
    let cred_text = match app.profile(host.profile_id.as_deref().unwrap_or("")) {
        Some(p) => format!("{} · {}", if p.is_key() { "key" } else { "pwd" }, p.title),
        None if !host.username.is_empty() => format!("{} · 手动输入", host.username),
        None => "连接时输入".to_string(),
    };
    let badge_max_w = (rect.right() - 144.0 - badge_x).max(0.0);
    if badge_max_w >= 40.0 {
        let badge_font = theme::font_sans(theme::TEXT_XS - 1.0);
        let badge_text = theme::truncate(painter, &cred_text, &badge_font, badge_max_w - 16.0);
        let text_w = painter
            .layout_no_wrap(badge_text.clone(), badge_font.clone(), Color32::WHITE)
            .size()
            .x;
        let badge = Rect::from_min_size(
            Pos2::new(badge_x, rect.center().y - 9.0),
            Vec2::new((text_w + 16.0).min(badge_max_w), 18.0),
        );
        painter.rect_filled(
            badge,
            CornerRadius::same(9),
            Color32::from_rgba_premultiplied(15, 35, 65, 46),
        );
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            badge_text,
            badge_font,
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
        cred_page: 0,
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
        .max_height(body.height())
        // The form itself should not grow a visible scrollbar; it can still be
        // wheel-scrolled when the content is taller than the shell.
        .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            let full = ui.available_width();
            ui.set_width(full);
            widgets::labelled(ui, "名称（可选）", false);
            widgets::text_input(ui, &mut form.title, "My Production Server", full, false);

            widgets::labelled(ui, "主机地址", true);
            let host_w = full * 0.72;
            let mut row = ui.horizontal(|ui| {
                widgets::text_input(ui, &mut form.host, "192.168.1.1 / example.com", host_w - 8.0, false);
                widgets::text_input(ui, &mut form.port, "22", full - host_w, false);
            });
            let _ = &mut row;

            widgets::labelled(ui, "账号（可选）", false);
            let mut open_cred_form = false;
            ui.horizontal(|ui| {
                if widgets::chip_button(ui, "新增账号", true).clicked() {
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
            let mut creds = app.profiles.clone();
            // Keep the host's current account first so it is always visible on
            // the first page, even when the list is filtered/paged.
            creds.sort_by_key(|c| c.id.as_str() != form.profile_id.as_str());
            if creds.is_empty() {
                ui.label(
                    egui::RichText::new("暂无账号，连接时将提示输入用户名和密码")
                        .size(theme::TEXT_XS)
                        .color(theme::TEXT_MUTED),
                );
            } else {
                // Three cards per row, one row per page. Navigation is only
                // left/right; the pager text and controls sit on the right.
                let total_pages = creds.len().div_ceil(CRED_COLUMNS).max(1);
                form.cred_page = form.cred_page.min(total_pages - 1);
                let page = form.cred_page;
                let start_idx = page * CRED_COLUMNS;
                let end_idx = (start_idx + CRED_COLUMNS).min(creds.len());
                let page_creds = &creds[start_idx..end_idx];
                let grid_h = CRED_CARD_HEIGHT;

                ui.allocate_ui_with_layout(
                    Vec2::new(full, grid_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        let avail = ui.available_width();
                        ui.set_width(avail);
                        ui.spacing_mut().item_spacing = Vec2::new(CRED_GAP, CRED_GAP);
                        let card_w = ((avail - CRED_GAP * 2.0) / CRED_COLUMNS as f32)
                            .floor()
                            .max(40.0);

                        ui.horizontal_wrapped(|ui| {
                            for cred in page_creds {
                                let selected = form.profile_id == cred.id;
                                let (r, resp) = ui.allocate_exact_size(
                                    Vec2::new(card_w, CRED_CARD_HEIGHT),
                                    Sense::click(),
                                );
                                let resp =
                                    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                                let painter = ui.painter();
                                let radius = CornerRadius::same(theme::RADIUS_SM);
                                painter.rect_filled(
                                    r,
                                    radius,
                                    if selected {
                                        widgets::tint(theme::ACCENT, 0.26)
                                    } else if resp.hovered() {
                                        widgets::tint(theme::ACCENT, 0.12)
                                    } else {
                                        Color32::from_rgba_premultiplied(11, 24, 46, 60)
                                    },
                                );
                                if selected {
                                    painter.rect_stroke(
                                        r,
                                        radius,
                                        Stroke::new(
                                            1.0,
                                            widgets::tint(theme::ACCENT_LIGHT, 0.8),
                                        ),
                                        StrokeKind::Inside,
                                    );
                                    let bar = Rect::from_min_size(
                                        Pos2::new(r.left(), r.top() + 4.0),
                                        Vec2::new(3.0, r.height() - 8.0),
                                    );
                                    theme::glow(painter, bar, 2, theme::ACCENT, 0.9);
                                    painter.rect_filled(
                                        bar,
                                        CornerRadius {
                                            nw: 0,
                                            sw: 0,
                                            ne: 2,
                                            se: 2,
                                        },
                                        theme::ACCENT_LIGHT,
                                    );
                                }

                                let edit_rect = Rect::from_center_size(
                                    Pos2::new(r.right() - 13.0, r.top() + 12.0),
                                    Vec2::splat(18.0),
                                );
                                let r_edit = ui
                                    .interact(
                                        edit_rect,
                                        ui.id().with(("host-form-cred-edit", &cred.id)),
                                        Sense::click(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if r_edit.hovered() {
                                    painter.rect_filled(
                                        edit_rect,
                                        CornerRadius::same(6),
                                        widgets::tint(theme::ACCENT, 0.16),
                                    );
                                }
                                widgets::icon(
                                    painter,
                                    edit_rect.center(),
                                    11.0,
                                    crate::icons::PENCIL_SIMPLE,
                                    if r_edit.hovered() {
                                        theme::TEXT_PRIMARY
                                    } else {
                                        theme::TEXT_MUTED
                                    },
                                );

                                let del_rect = Rect::from_center_size(
                                    Pos2::new(r.right() - 32.0, r.top() + 12.0),
                                    Vec2::splat(18.0),
                                );
                                let r_del = ui
                                    .interact(
                                        del_rect,
                                        ui.id().with(("host-form-cred-del", &cred.id)),
                                        Sense::click(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if r_del.hovered() {
                                    painter.rect_filled(
                                        del_rect,
                                        CornerRadius::same(6),
                                        widgets::tint(theme::ERROR, 0.16),
                                    );
                                }
                                widgets::icon(
                                    painter,
                                    del_rect.center(),
                                    11.0,
                                    crate::icons::TRASH,
                                    if r_del.hovered() {
                                        theme::ERROR
                                    } else {
                                        theme::TEXT_MUTED
                                    },
                                );

                                painter.text(
                                    Pos2::new(r.left() + 10.0, r.top() + 13.0),
                                    Align2::LEFT_CENTER,
                                    if cred.is_key() { "KEY" } else { "PWD" },
                                    theme::font_sans(theme::TEXT_XS - 2.0),
                                    if selected {
                                        theme::ACCENT_HOVER
                                    } else {
                                        theme::ACCENT_LIGHT
                                    },
                                );
                                let title_x = r.left() + 44.0;
                                painter.text(
                                    Pos2::new(title_x, r.top() + 13.0),
                                    Align2::LEFT_CENTER,
                                    theme::truncate(
                                        painter,
                                        &cred.title,
                                        &theme::f_xs(),
                                        (r.right() - 44.0 - title_x).max(10.0),
                                    ),
                                    theme::f_xs(),
                                    if selected {
                                        theme::TEXT_PRIMARY
                                    } else {
                                        theme::TEXT_SECONDARY
                                    },
                                );
                                painter.text(
                                    Pos2::new(r.left() + 10.0, r.bottom() - 13.0),
                                    Align2::LEFT_CENTER,
                                    theme::truncate(
                                        painter,
                                        &cred.username,
                                        &theme::font_mono(theme::TEXT_XS - 1.0),
                                        (r.width() - 20.0).max(10.0),
                                    ),
                                    theme::font_mono(theme::TEXT_XS - 1.0),
                                    if selected {
                                        theme::TEXT_SECONDARY
                                    } else {
                                        theme::TEXT_MUTED
                                    },
                                );

                                let delete_clicked = r_del.clicked()
                                    || (resp.clicked()
                                        && ui.input(|i| {
                                            i.pointer
                                                .interact_pos()
                                                .is_some_and(|p| del_rect.contains(p))
                                        }));
                                let edit_clicked = r_edit.clicked()
                                    || (resp.clicked()
                                        && ui.input(|i| {
                                            i.pointer
                                                .interact_pos()
                                                .is_some_and(|p| edit_rect.contains(p))
                                        }));
                                if delete_clicked {
                                    app.confirm = Some(ConfirmRequest {
                                        title: "删除账号".into(),
                                        message: format!("确认删除账号「{}」？", cred.title),
                                        confirm_text: "删除".into(),
                                        cancel_text: "取消".into(),
                                        action: ConfirmAction::DeleteCredential(cred.id.clone()),
                                    });
                                } else if edit_clicked {
                                    app.credential_form = Some(
                                        crate::state::CredentialFormState {
                                            editing_id: Some(cred.id.clone()),
                                            title: cred.title.clone(),
                                            username: cred.username.clone(),
                                            auth_type: cred.auth_type.clone(),
                                            password: String::new(),
                                            private_key: String::new(),
                                            passphrase: String::new(),
                                            show_password: false,
                                            show_passphrase: false,
                                            error: None,
                                            saving: false,
                                            from_host_form: true,
                                        },
                                    );
                                    app.modal = ModalKind::CredentialForm;
                                } else if resp.clicked() {
                                    form.profile_id = if selected {
                                        String::new()
                                    } else {
                                        cred.id.clone()
                                    };
                                    form.cred_page = 0;
                                }
                            }
                        });
                    },
                );

                // Right-aligned pager: text + ◀ ▶ controls.
                let (pager_rect, _) = ui.allocate_exact_size(
                    Vec2::new(full, CRED_PAGER_HEIGHT),
                    Sense::hover(),
                );
                let painter = ui.painter();
                painter.line_segment(
                    [
                        Pos2::new(pager_rect.left() + 4.0, pager_rect.top() + 0.5),
                        Pos2::new(pager_rect.right() - 4.0, pager_rect.top() + 0.5),
                    ],
                    Stroke::new(1.0, Color32::from_rgba_premultiplied(17, 40, 77, 51)),
                );

                let btn = 16.0;
                let cy = pager_rect.center().y;
                let next_rect = Rect::from_center_size(
                    Pos2::new(pager_rect.right() - 10.0, cy),
                    Vec2::splat(btn),
                );
                let prev_rect = Rect::from_center_size(
                    Pos2::new(pager_rect.right() - 30.0, cy),
                    Vec2::splat(btn),
                );
                painter.text(
                    Pos2::new(pager_rect.right() - 50.0, cy),
                    Align2::RIGHT_CENTER,
                    format!("第 {} / {} 页", page + 1, total_pages),
                    theme::font_sans(10.0),
                    theme::TEXT_MUTED,
                );

                let can_prev = page > 0;
                let can_next = page + 1 < total_pages;
                let r_prev = ui
                    .interact(
                        prev_rect,
                        ui.id().with("host-form-cred-page-prev"),
                        Sense::click(),
                    );
                let r_next = ui
                    .interact(
                        next_rect,
                        ui.id().with("host-form-cred-page-next"),
                        Sense::click(),
                    );
                let arrow_color = |enabled: bool, hovered: bool| {
                    if !enabled {
                        theme::TEXT_MUTED.gamma_multiply(0.55)
                    } else if hovered {
                        theme::TEXT_PRIMARY
                    } else {
                        theme::TEXT_SECONDARY
                    }
                };
                widgets::chevron(
                    painter,
                    prev_rect.center(),
                    7.0,
                    true,
                    arrow_color(can_prev, r_prev.hovered()),
                );
                widgets::chevron(
                    painter,
                    next_rect.center(),
                    7.0,
                    false,
                    arrow_color(can_next, r_next.hovered()),
                );
                if r_prev.clicked() && can_prev {
                    form.cred_page = page - 1;
                }
                if r_next.clicked() && can_next {
                    form.cred_page = page + 1;
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




