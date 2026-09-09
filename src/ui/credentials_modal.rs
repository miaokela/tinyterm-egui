//! Credentials modal (`CredentialsModal.tsx`): reusable auth profiles.

use crate::models::Profile;
use crate::state::{
    AppState, ConfirmAction, ConfirmRequest, CredentialFormState, ModalKind,
};
use crate::theme;
use crate::widgets;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

const SHELL_WIDTH: f32 = 520.0;
const SHELL_HEIGHT: f32 = 430.0;
const FORM_WIDTH: f32 = 460.0;
const FORM_HEIGHT: f32 = 540.0;

pub fn show(ui: &mut Ui, app: &mut AppState) {
    if app.modal != ModalKind::Credentials && app.modal != ModalKind::CredentialForm {
        return;
    }
    let screen = ui.ctx().input(|i| i.viewport_rect());
    ui.painter().rect_filled(
        screen,
        0,
        Color32::from_rgba_premultiplied(2, 6, 14, 184),
    );
    if app.modal == ModalKind::Credentials {
        list(ui, app, screen);
    }
    if app.modal == ModalKind::CredentialForm {
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

    let header = Rect::from_min_size(rect.min, Vec2::new(width, 52.0));
    painter.text(
        Pos2::new(header.left() + 20.0, header.center().y),
        Align2::LEFT_CENTER,
        "Credentials",
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
    let r_close = ui.interact(close_rect, ui.id().with("creds-close"), Sense::click());
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

    let bar = Rect::from_min_size(
        Pos2::new(rect.left(), header.bottom()),
        Vec2::new(width, 42.0),
    );
    painter.text(
        Pos2::new(bar.left() + 20.0, bar.center().y),
        Align2::LEFT_CENTER,
        "可复用的认证配置，在 Hosts 中选择引用。",
        theme::f_xs(),
        theme::TEXT_MUTED,
    );
    let add_rect = Rect::from_center_size(
        Pos2::new(bar.right() - 32.0, bar.center().y),
        Vec2::splat(30.0),
    );
    let mut add_ui = ui.new_child(egui::UiBuilder::new().max_rect(add_rect));
    let r_add = widgets::orb_button(&mut add_ui, 30.0, |p, r, c| {
        widgets::plus(p, r.center(), 13.0, c)
    });
    if r_add.clicked() {
        app.credential_form = Some(CredentialFormState {
            auth_type: "password".into(),
            ..Default::default()
        });
        app.modal = ModalKind::CredentialForm;
    }

    let list_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, bar.bottom()),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 12.0),
    );
    if app.profiles.is_empty() {
        painter.text(
            list_rect.center(),
            Align2::CENTER_CENTER,
            "暂无认证配置",
            theme::f_sm(),
            theme::TEXT_MUTED,
        );
        return;
    }

    let profiles = app.profiles.clone();
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(list_rect));
    egui::ScrollArea::vertical()
        .id_salt("creds-list")
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for profile in &profiles {
                credential_row(ui, app, profile);
            }
        });
}

fn credential_row(ui: &mut Ui, app: &mut AppState, profile: &Profile) {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 52.0),
        Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
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
        Stroke::new(1.0, theme::BORDER),
        StrokeKind::Inside,
    );

    // Icon tile
    let icon = Rect::from_center_size(
        Pos2::new(rect.left() + 30.0, rect.center().y),
        Vec2::splat(32.0),
    );
    painter.rect_filled(
        icon,
        CornerRadius::same(theme::RADIUS_SM),
        Color32::from_rgba_premultiplied(15, 35, 65, 38),
    );
    if profile.is_key() {
        widgets::key_glyph(painter, icon.center(), theme::ACCENT_LIGHT);
    } else {
        widgets::lock_glyph(painter, icon.center(), theme::ACCENT_LIGHT);
    }

    painter.text(
        Pos2::new(rect.left() + 56.0, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        theme::truncate(painter, &profile.title, &theme::f_sm(), rect.width() - 160.0),
        theme::f_sm(),
        theme::TEXT_PRIMARY,
    );
    painter.text(
        Pos2::new(rect.left() + 56.0, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        &profile.username,
        theme::font_mono(theme::TEXT_XS - 1.0),
        theme::TEXT_MUTED,
    );
    let badge_text = if profile.is_key() {
        "Private Key"
    } else {
        "Password"
    };
    let bw = painter
        .layout_no_wrap(badge_text.to_string(), theme::font_sans(theme::TEXT_XS - 1.0), Color32::WHITE)
        .size()
        .x;
    let badge = Rect::from_min_size(
        Pos2::new(rect.left() + 150.0, rect.center().y + 1.0),
        Vec2::new(bw + 14.0, 18.0),
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
        theme::font_sans(theme::TEXT_XS - 1.0),
        theme::TEXT_SECONDARY,
    );

    // Actions
    let del_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 22.0, rect.center().y),
        Vec2::splat(26.0),
    );
    let edit_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 52.0, rect.center().y),
        Vec2::splat(26.0),
    );
    let r_del = ui.interact(del_rect, ui.id().with(("cred-del", &profile.id)), Sense::click());
    let r_edit = ui.interact(edit_rect, ui.id().with(("cred-edit", &profile.id)), Sense::click());
    widgets::trash(
        painter,
        del_rect.center(),
        if r_del.hovered() { theme::ERROR } else { theme::TEXT_MUTED },
    );
    widgets::pencil(
        painter,
        edit_rect.center(),
        if r_edit.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
    );

    if r_edit.clicked() {
        app.credential_form = Some(CredentialFormState {
            editing_id: Some(profile.id.clone()),
            title: profile.title.clone(),
            username: profile.username.clone(),
            auth_type: profile.auth_type.clone(),
            password: String::new(),
            private_key: String::new(),
            passphrase: String::new(),
            show_password: false,
            show_passphrase: false,
            error: None,
            saving: false,
            from_host_form: false,
        });
        app.modal = ModalKind::CredentialForm;
    }
    if r_del.clicked() {
        app.confirm = Some(ConfirmRequest {
            title: "删除 Credential".into(),
            message: "确认删除该认证配置？".into(),
            confirm_text: "删除".into(),
            cancel_text: "取消".into(),
            action: ConfirmAction::DeleteCredential(profile.id.clone()),
        });
    }
}

fn form(ui: &mut Ui, app: &mut AppState, screen: Rect) {
    let Some(mut form) = app.credential_form.clone() else {
        app.modal = ModalKind::Credentials;
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

    let header = Rect::from_min_size(rect.min, Vec2::new(width, 50.0));
    painter.text(
        Pos2::new(header.left() + 20.0, header.center().y),
        Align2::LEFT_CENTER,
        if form.editing_id.is_some() {
            "编辑 Credential"
        } else {
            "新建 Credential"
        },
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
    let r_close = ui.interact(close_rect, ui.id().with("credform-close"), Sense::click());
    widgets::cross(
        &painter,
        close_rect.center(),
        11.0,
        if r_close.hovered() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
    );
    if r_close.clicked() {
        app.modal = if form.from_host_form {
            ModalKind::HostForm
        } else {
            ModalKind::Credentials
        };
        app.credential_form = None;
        return;
    }

    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 20.0, header.bottom() + 8.0),
        Pos2::new(rect.right() - 20.0, rect.bottom() - 56.0),
    );
    let full = body.width();
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(body));
    egui::ScrollArea::vertical()
        .id_salt("cred-form")
        .auto_shrink([false, false])
        .show(&mut scroll, |ui| {
            ui.set_width(full);
            widgets::labelled(ui, "配置名称", true);
            widgets::text_input(ui, &mut form.title, "例如: Production Root Key", full, false);

            widgets::labelled(ui, "用户名", true);
            widgets::text_input(ui, &mut form.username, "root", full, false);

            widgets::labelled(ui, "认证方式", false);
            let (r, _) = ui.allocate_exact_size(Vec2::new(full, 30.0), Sense::hover());
            let half = full * 0.5;
            let pw_rect = Rect::from_min_size(r.min, Vec2::new(half, 30.0));
            let key_rect = Rect::from_min_size(Pos2::new(r.left() + half, r.top()), Vec2::new(half, 30.0));
            let painter = ui.painter();
            painter.rect_stroke(
                r,
                CornerRadius::same(theme::RADIUS_SM),
                Stroke::new(1.0, theme::BORDER),
                StrokeKind::Inside,
            );
            let r_pw = ui.interact(pw_rect, ui.id().with("auth-pw"), Sense::click());
            let r_key = ui.interact(key_rect, ui.id().with("auth-key"), Sense::click());
            if form.auth_type == "password" {
                painter.rect_filled(
                    pw_rect,
                    CornerRadius {
                        nw: theme::RADIUS_SM,
                        sw: theme::RADIUS_SM,
                        ne: 0,
                        se: 0,
                    },
                    Color32::from_rgba_premultiplied(29, 57, 102, 140),
                );
            } else {
                painter.rect_filled(
                    key_rect,
                    CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: theme::RADIUS_SM,
                        se: theme::RADIUS_SM,
                    },
                    Color32::from_rgba_premultiplied(29, 57, 102, 140),
                );
            }
            painter.text(
                pw_rect.center(),
                Align2::CENTER_CENTER,
                "密码",
                theme::f_sm(),
                if form.auth_type == "password" { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
            );
            painter.text(
                key_rect.center(),
                Align2::CENTER_CENTER,
                "私钥",
                theme::f_sm(),
                if form.auth_type == "privateKey" { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED },
            );
            if r_pw.clicked() {
                form.auth_type = "password".into();
            }
            if r_key.clicked() {
                form.auth_type = "privateKey".into();
            }

            if form.auth_type == "password" {
                widgets::labelled(ui, "密码", false);
                let placeholder = if form.editing_id.is_some() {
                    "留空则保持原密码"
                } else {
                    "登录密码"
                };
                widgets::text_input(ui, &mut form.password, placeholder, full, true);
            } else {
                widgets::labelled(ui, "私钥内容", false);
                let placeholder = if form.editing_id.is_some() {
                    "留空则保持原私钥"
                } else {
                    "-----BEGIN OPENSSH PRIVATE KEY-----"
                };
                widgets::text_area(ui, &mut form.private_key, placeholder, full, 140.0);
                widgets::labelled(ui, "私钥密码（可选）", false);
                let placeholder = if form.editing_id.is_some() {
                    "留空则保持原私钥密码"
                } else {
                    "私钥保护密码"
                };
                widgets::text_input(ui, &mut form.passphrase, placeholder, full, true);
            }

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

    let footer = Rect::from_min_size(
        Pos2::new(rect.right() - 20.0 - 170.0, rect.bottom() - 46.0),
        Vec2::new(170.0, 30.0),
    );
    let labels = [
        ("取消", true),
        (
            if form.editing_id.is_some() { "更新" } else { "创建" },
            !form.title.trim().is_empty() && !form.username.trim().is_empty(),
        ),
    ];
    let clicked = widgets::button_row(ui, footer, &labels);

    app.credential_form = Some(form.clone());

    if clicked == Some(0) {
        app.modal = if form.from_host_form {
            ModalKind::HostForm
        } else {
            ModalKind::Credentials
        };
        app.credential_form = None;
        return;
    }
    if clicked == Some(1) {
        if form.title.trim().is_empty() {
            form.error = Some("请填写配置名称".into());
            app.credential_form = Some(form);
            return;
        }
        if form.username.trim().is_empty() {
            form.error = Some("请填写用户名".into());
            app.credential_form = Some(form);
            return;
        }
        let profile = Profile {
            id: form
                .editing_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            title: form.title.clone(),
            username: form.username.clone(),
            auth_type: form.auth_type.clone(),
            password: None,
            password_encrypted: false,
            private_key: None,
            passphrase: None,
            created_at: crate::models::now_unix(),
        };
        let secret = if form.auth_type == "password" && !form.password.is_empty() {
            Some(form.password.clone())
        } else {
            None
        };
        let key = if form.auth_type == "privateKey" && !form.private_key.is_empty() {
            Some(form.private_key.clone())
        } else {
            None
        };
        let passphrase = if form.auth_type == "privateKey" && !form.passphrase.is_empty() {
            Some(form.passphrase.clone())
        } else {
            None
        };
        app.upsert_profile(profile, secret, key, passphrase);
        app.modal = if form.from_host_form {
            ModalKind::HostForm
        } else {
            ModalKind::Credentials
        };
        app.credential_form = None;
    }
}
