//! Logging that survives a GUI launch.
//!
//! The Windows build uses `windows_subsystem = "windows"`, so it has no console
//! and everything written to stderr - including a fatal startup error - is lost.
//! That makes a failed launch look like "the app does not open" with no clue
//! why. Every line therefore also goes to `<data dir>/.tinyterm-egui/tinyterm.log`,
//! and fatal errors additionally raise a native message box.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}\n",
            timestamp(),
            record.level(),
            record.args()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());
        if let Some(file) = LOG_FILE.lock().unwrap().as_mut() {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }

    fn flush(&self) {
        if let Some(file) = LOG_FILE.lock().unwrap().as_mut() {
            let _ = file.flush();
        }
    }
}

static LOGGER: Logger = Logger;

/// `<data dir>/.tinyterm-egui/tinyterm.log` - next to the database, so it exists
/// even when the configured data directory is unusable.
pub fn log_path() -> PathBuf {
    crate::storage::fallback_db_path().with_file_name("tinyterm.log")
}

pub fn init() {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Keep one previous log so a crash is not overwritten by the next launch.
    if std::fs::metadata(&path).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    *LOG_FILE.lock().unwrap() = file;

    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| v.parse::<log::LevelFilter>().ok())
        .unwrap_or(log::LevelFilter::Info);
    log::set_max_level(level);
    let _ = log::set_logger(&LOGGER);

    log::info!("TinyTerm {} starting", env!("CARGO_PKG_VERSION"));
    log::info!("log file: {}", path.display());
}

/// Report an unrecoverable error, then terminate.
pub fn fatal(message: &str) -> ! {
    log::error!("{message}");
    let detail = format!(
        "{message}\n\n日志已写入：\n{}",
        log_path().display()
    );
    dialog("TinyTerm 启动失败", &detail);
    std::process::exit(1);
}

/// Install a panic hook that logs the panic and shows it instead of vanishing.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_owned());
        log::error!("panic at {location}: {info}");
        dialog(
            "TinyTerm 崩溃",
            &format!("{info}\n\n位置：{location}\n\n日志：{}", log_path().display()),
        );
    }));
}

#[cfg(windows)]
fn dialog(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let text = wide(message);
    let caption = wide(title);
    // SAFETY: both buffers are NUL terminated and outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn dialog(_title: &str, message: &str) {
    eprintln!("{message}");
}

/// `YYYY-MM-DD HH:MM:SS` in UTC, without pulling in a date crate.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's days-from-civil inverse.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
