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
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TinyTerm")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([880.0, 700.0])
            .with_app_id("com.tinyterm.egui")
            .with_icon(std::sync::Arc::new(assets::window_icon())),
        ..Default::default()
    };

    #[cfg(windows)]
    {
        // The glow backend needs OpenGL 2.0+, which a remote-desktop session or a
        // GPU-less cloud VM does not provide (Windows then only exposes the GDI
        // OpenGL 1.1 software renderer), and the app would exit without ever
        // showing a window. wgpu can fall back to the D3D12 software rasterizer.
        options.renderer = eframe::Renderer::Wgpu;
        options.wgpu_options.wgpu_setup = wgpu_setup_with_software_fallback();
        log::info!("renderer: wgpu (DX12, hardware preferred, software fallback)");
    }
    #[cfg(not(windows))]
    log::info!("renderer: glow (OpenGL)");

    let bus_for_app = state.mgr.bus.clone();
    log::info!("creating the window");
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

    // Window/graphics-session failures used to end the process silently; the
    // dialog in logging::fatal now shows this text.
    result.map_err(|e| {
        anyhow::anyhow!(
            "无法创建窗口：{e}\n\n\
             显卡驱动/图形会话不满足要求。若在远程桌面或虚拟机里运行，\n\
             请确认已安装显卡驱动，或换用带 GPU 的图形会话。"
        )
    })
}

/// wgpu setup that still works without a GPU.
///
/// egui-wgpu's default adapter request only accepts hardware adapters, so on a
/// machine with none (a remote-desktop session, a bare cloud VM) it fails and
/// the window never appears. This selector prefers a real GPU but falls back to
/// the software rasterizer (WARP), which is what makes TinyTerm usable over RDP.
#[cfg(windows)]
fn wgpu_setup_with_software_fallback() -> eframe::egui_wgpu::WgpuSetup {
    use eframe::egui_wgpu::{NativeAdapterSelectorMethod, WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu;

    let selector: NativeAdapterSelectorMethod = std::sync::Arc::new(|adapters, surface| {
        let rank = |adapter: &wgpu::Adapter| match adapter.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::VirtualGpu => 2,
            wgpu::DeviceType::Cpu => 3,
            _ => 4,
        };
        let chosen = adapters
            .iter()
            .filter(|adapter| surface.map_or(true, |s| adapter.is_surface_supported(s)))
            .min_by_key(|adapter| rank(adapter))
            .cloned();
        match chosen {
            Some(adapter) => {
                let info = adapter.get_info();
                log::info!(
                    "wgpu adapter: {} ({:?}, {:?})",
                    info.name,
                    info.device_type,
                    info.backend
                );
                Ok(adapter)
            }
            None => Err("no usable wgpu adapter (no GPU and no software rasterizer)".to_owned()),
        }
    });

    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.native_adapter_selector = Some(selector);
    WgpuSetup::CreateNew(setup)
}

fn zoom_path() -> std::path::PathBuf {
    storage::default_db_path()
        .parent()
        .map(|p| p.join("zoom.txt"))
        .unwrap_or_else(|| std::path::PathBuf::from("zoom.txt"))
}

fn read_zoom() -> f32 {
    let stored = std::fs::read_to_string(zoom_path())
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok());
    match stored {
        // 0.8 used to be the default and every run wrote it back, so a stored
        // 0.8 means the user never chose it - fall through to the new default.
        Some(z) if (z - state::APP_ZOOM_LEGACY_DEFAULT).abs() > 0.001 => {
            z.clamp(state::APP_ZOOM_MIN, state::APP_ZOOM_MAX)
        }
        _ => state::APP_ZOOM_DEFAULT,
    }
}

fn write_zoom(zoom: f32) {
    // The default is not persisted: leaving the file absent lets a future change
    // of default actually take effect instead of being pinned by a stale value.
    if (zoom - state::APP_ZOOM_DEFAULT).abs() < 0.001 {
        let _ = std::fs::remove_file(zoom_path());
    } else {
        let _ = std::fs::write(zoom_path(), format!("{zoom}"));
    }
    let _ = APP_ZOOM_STORAGE_KEY;
}
