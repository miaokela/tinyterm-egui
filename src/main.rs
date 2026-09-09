//! TinyTerm (egui) — an egui/eframe reimplementation of the TinyTerm SSH client.

// The data model mirrors the web client's SQLite schema and command surface, so
// some fields/methods exist for schema compatibility and future wiring rather
// than for the current UI. Silence dead-code noise for those.
#![allow(dead_code)]
// Windows GUI app: without this the release .exe opens a console window behind
// the main window. Debug builds keep the console so logs stay visible.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod assets;
mod app;
mod crypto;
mod font_metrics;
mod icons;
mod local_fs;
mod logging;
mod models;
mod remote_fs;
mod session;
mod ssh;
mod state;
mod storage;
mod term;
mod theme;
mod transfer;
mod ui;
#[cfg(test)]
mod tests;
mod widgets;

use anyhow::Result;
use std::sync::Arc;

const APP_ZOOM_STORAGE_KEY: &str = "tinyterm-egui.appZoom";

fn main() {
    logging::init();
    logging::install_panic_hook();
    if let Err(err) = run() {
        // A GUI build has no console, so a returned error would otherwise make
        // the app look like it simply refuses to open.
        logging::fatal(&format!("{err:#}"));
    }
}

fn run() -> Result<()> {
    // ── Storage ──────────────────────────────────────────────────────────────
    let db_path = storage::default_db_path();
    let db = match storage::Db::open(&db_path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            let fallback = storage::fallback_db_path();
            log::warn!(
                "cannot use {} ({e}); falling back to {}",
                db_path.display(),
                fallback.display()
            );
            Arc::new(storage::Db::open(&fallback)?)
        }
    };
    log::info!("database: {}", db.path.display());

    let settings = db.get_settings().unwrap_or_default();

    // ── Async runtime ────────────────────────────────────────────────────────
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .thread_name("tinyterm-ssh")
        .build()?;
    let rt_handle = runtime.handle().clone();

    // ── Event bus + session manager ──────────────────────────────────────────
    let bus = session::EventBus::default();
    let mgr = Arc::new(session::SessionManager::new(
        rt_handle,
        bus,
        db.clone(),
    ));

    let app_zoom = read_zoom();
    let mut state = state::AppState::new(db, mgr, settings, app_zoom);
    state.load_all();

    // ── Window ───────────────────────────────────────────────────────────────
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TinyTerm")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([880.0, 700.0])
            .with_app_id("com.tinyterm.egui")
            .with_icon(std::sync::Arc::new(assets::window_icon())),
        ..Default::default()
    };

    let bus_for_app = state.mgr.bus.clone();
    log::info!("creating the window (renderer: glow/OpenGL)");
    let result = eframe::run_native(
        "TinyTerm",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx, &state.settings.font_family, state.settings.font_size as f32);
            cc.egui_ctx.set_zoom_factor(state.app_zoom);
            bus_for_app.attach(cc.egui_ctx.clone());
            Ok(Box::new(app::TinyTermApp::new(state)))
        }),
    );

    // Persist the zoom factor.
    write_zoom(app_zoom);
    drop(runtime);

    // The glow backend needs a real OpenGL 2.0+ driver. On a VM or an RDP
    // session Windows may only offer the 1.1 software renderer, and this is the
    // error that used to make the app close without any explanation.
    result.map_err(|e| {
        anyhow::anyhow!(
            "无法创建窗口：{e}\n\n\
             最常见的原因是显卡驱动不支持 OpenGL 2.0（虚拟机 / 远程桌面里很常见）。\n\
             请安装显卡驱动，或改用支持 OpenGL 的图形会话后重试。"
        )
    })
}

fn zoom_path() -> std::path::PathBuf {
    storage::default_db_path()
        .parent()
        .map(|p| p.join("zoom.txt"))
        .unwrap_or_else(|| std::path::PathBuf::from("zoom.txt"))
}

fn read_zoom() -> f32 {
    std::fs::read_to_string(zoom_path())
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .map(|z| z.clamp(state::APP_ZOOM_MIN, state::APP_ZOOM_MAX))
        .unwrap_or(state::APP_ZOOM_MIN)
}

fn write_zoom(zoom: f32) {
    let _ = std::fs::write(zoom_path(), format!("{zoom}"));
    let _ = APP_ZOOM_STORAGE_KEY;
}
