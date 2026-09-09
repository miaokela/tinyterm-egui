//! Session manager: background SSH tasks + the event bus the UI drains.

use crate::models::*;
use crate::ssh::{self, ConnectError, ResolvedAuth, SftpSlot, SshHandle, WriteCmd, WriteTx};
use crate::storage::Db;
use crate::term::{SharedTerminal, Terminal};
use parking_lot::Mutex;
use russh::{ChannelMsg, Disconnect};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Everything a background task can report back to the UI.
#[derive(Debug)]
pub enum AppEvent {
    /// New bytes were written into the shared terminal — repaint.
    Output { session_id: String },
    /// The remote shell ended.
    Closed {
        session_id: String,
        reason: String,
    },
    /// Session established: backend id + local/remote starting paths.
    Ready {
        session_id: String,
        backend_id: String,
        home: String,
        cwd: Option<String>,
    },
    /// Session could not be established.
    Failed {
        session_id: String,
        error: String,
        host_key: Option<Box<HostKeyVerificationPrompt>>,
    },
    /// Remote directory listing completed.
    RemoteDir {
        request: u64,
        session_id: String,
        path: String,
        result: Result<Vec<FileInfo>, String>,
    },
    /// Remote cwd poll completed.
    RemoteCwd {
        session_id: String,
        path: String,
    },
    /// Transfer queue update.
    Transfer(Box<TransferProgress>),
    /// Best-effort async delete report.
    RemoteDelete {
        path: String,
        is_dir: bool,
        success: bool,
        error: Option<String>,
    },
    /// A host port probe finished.
    HostProbe {
        host_id: String,
        reachable: bool,
    },
    /// Toast notification.
    Toast {
        message: String,
        kind: ToastKind,
    },
    /// System-info / history query result.
    Query {
        request: u64,
        result: Result<String, String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
    Info,
}

/// Cooperative cancellation flag set (`SessionManager.cancelled_transfers`).
pub type CancelSet = Arc<Mutex<HashSet<String>>>;

/// Thread-safe event queue + egui repaint nudge.
#[derive(Clone)]
pub struct EventBus {
    queue: Arc<Mutex<Vec<AppEvent>>>,
    ctx: Arc<Mutex<Option<egui::Context>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
            ctx: Arc::new(Mutex::new(None)),
        }
    }
}

impl EventBus {
    pub fn attach(&self, ctx: egui::Context) {
        *self.ctx.lock() = Some(ctx);
    }

    pub fn send(&self, event: AppEvent) {
        self.queue.lock().push(event);
        if let Some(ctx) = self.ctx.lock().as_ref() {
            ctx.request_repaint();
        }
    }

    pub fn drain(&self) -> Vec<AppEvent> {
        std::mem::take(&mut *self.queue.lock())
    }
}

/// A live SSH connection owned by the app.
pub struct LiveSession {
    pub id: String,
    pub backend_id: String,
    pub bookmark_id: String,
    pub write: WriteTx,
    pub handle: Arc<SshHandle>,
    pub terminal: SharedTerminal,
    pub alive: Arc<AtomicBool>,
    pub sftp: SftpSlot,
    pub auth: ResolvedAuth,
}

impl LiveSession {
    /// Enqueue PTY input; the writer task performs the actual SSH write.
    pub fn write_bytes(&self, data: &[u8]) -> anyhow::Result<()> {
        self.write
            .send(WriteCmd::Data(data.to_vec()))
            .map_err(|e| anyhow::anyhow!("write_to_session: {e}"))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.write
            .send(WriteCmd::Resize { cols, rows })
            .map_err(|e| anyhow::anyhow!("resize_terminal: {e}"))
    }
}

pub struct ConnectRequest {
    pub session_id: String,
    pub bookmark_id: String,
    pub cols: u16,
    pub rows: u16,
    pub username_override: Option<String>,
    pub password_override: Option<String>,
    pub scrollback: usize,
}

pub struct SessionManager {
    pub rt: tokio::runtime::Handle,
    pub bus: EventBus,
    pub db: Arc<Db>,
    sessions: Arc<Mutex<HashMap<String, Arc<LiveSession>>>>,
    cancelled: Arc<Mutex<HashSet<String>>>,
    next_request: Arc<AtomicU64>,
}

impl SessionManager {
    pub fn new(rt: tokio::runtime::Handle, bus: EventBus, db: Arc<Db>) -> Self {
        Self {
            rt,
            bus,
            db,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            next_request: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn request_id(&self) -> u64 {
        self.next_request.fetch_add(1, Ordering::Relaxed)
    }

    pub fn get(&self, session_id: &str) -> Option<Arc<LiveSession>> {
        self.sessions.lock().get(session_id).cloned()
    }

    /// Shared handle to the cooperative cancellation flag set.
    pub fn cancel_set(&self) -> CancelSet {
        self.cancelled.clone()
    }

    pub fn cancel_transfer(&self, transfer_id: &str) {
        self.cancelled.lock().insert(transfer_id.to_string());
    }

    // ── Connect ──────────────────────────────────────────────────────────────

    pub fn connect(&self, req: ConnectRequest) {
        let bus = self.bus.clone();
        let db = self.db.clone();
        let sessions = self.sessions.clone();

        self.rt.spawn(async move {
            let session_id = req.session_id.clone();
            let bookmark = match db.bookmark_with_secrets(&req.bookmark_id) {
                Ok(Some(b)) => b,
                Ok(None) => {
                    bus.send(AppEvent::Failed {
                        session_id,
                        error: "Bookmark not found".into(),
                        host_key: None,
                    });
                    return;
                }
                Err(e) => {
                    bus.send(AppEvent::Failed {
                        session_id,
                        error: e.to_string(),
                        host_key: None,
                    });
                    return;
                }
            };

            let auth = match ssh::resolve_auth(
                &db,
                &bookmark,
                req.username_override.as_deref(),
                req.password_override.as_deref(),
            ) {
                Ok(a) => a,
                Err(e) => {
                    bus.send(AppEvent::Failed {
                        session_id,
                        error: e.to_string(),
                        host_key: None,
                    });
                    return;
                }
            };

            let trusted = db
                .get_trusted_host_key(&auth.host, auth.port)
                .ok()
                .flatten();

            let mut connection = match ssh::connect(&auth, trusted, Duration::from_secs(30)).await {
                Ok(c) => c,
                Err(ConnectError::HostKey(prompt)) => {
                    bus.send(AppEvent::Failed {
                        session_id,
                        error: format!("HOST_KEY_PROMPT:{}", serde_json::to_string(&*prompt).unwrap()),
                        host_key: Some(prompt),
                    });
                    return;
                }
                Err(ConnectError::Message(m)) => {
                    bus.send(AppEvent::Failed {
                        session_id,
                        error: m,
                        host_key: None,
                    });
                    return;
                }
            };

            if let Err(e) = ssh::authenticate(
                &mut connection.handle,
                &auth,
                req.password_override.as_deref(),
            )
            .await
            {
                bus.send(AppEvent::Failed {
                    session_id,
                    error: e.to_string(),
                    host_key: None,
                });
                return;
            }

            let (read_half, write_tx) =
                match ssh::open_shell(&connection.handle, &auth.term, req.cols as u32, req.rows as u32)
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        bus.send(AppEvent::Failed {
                            session_id,
                            error: e.to_string(),
                            host_key: None,
                        });
                        return;
                    }
                };

            let handle = Arc::new(connection.handle);
            let terminal: SharedTerminal = Arc::new(Mutex::new(Terminal::new(
                req.rows.max(2),
                req.cols.max(2),
                req.scrollback,
            )));
            let alive = Arc::new(AtomicBool::new(true));

            let live = Arc::new(LiveSession {
                id: req.session_id.clone(),
                backend_id: req.session_id.clone(),
                bookmark_id: req.bookmark_id.clone(),
                write: write_tx,
                handle: handle.clone(),
                terminal: terminal.clone(),
                alive: alive.clone(),
                sftp: SftpSlot::new(handle.clone()),
                auth: auth.clone(),
            });
            sessions.lock().insert(req.session_id.clone(), live.clone());

            // ── reader task ──────────────────────────────────────────────────
            {
                let bus = bus.clone();
                let terminal = terminal.clone();
                let alive = alive.clone();
                let session_id = req.session_id.clone();
                let sessions = sessions.clone();
                let mut read = read_half;
                tokio::spawn(async move {
                    let mut eof_seen = false;
                    while let Some(msg) = read.wait().await {
                        match msg {
                            ChannelMsg::Data { data } => {
                                terminal.lock().process(&data);
                                bus.send(AppEvent::Output {
                                    session_id: session_id.clone(),
                                });
                            }
                            ChannelMsg::ExtendedData { data, ext } => {
                                // ext == 1 is stderr; surface it inline like xterm.
                                if ext == 1 {
                                    terminal.lock().process(&data);
                                    bus.send(AppEvent::Output {
                                        session_id: session_id.clone(),
                                    });
                                }
                            }
                            ChannelMsg::Eof => {
                                if !eof_seen {
                                    eof_seen = true;
                                    terminal
                                        .lock()
                                        .process(b"\r\n[Session closed]\r\n");
                                }
                            }
                            ChannelMsg::Close => break,
                            _ => {}
                        }
                    }
                    alive.store(false, Ordering::Relaxed);
                    sessions.lock().remove(&session_id);
                    bus.send(AppEvent::Closed {
                        session_id,
                        reason: "连接已断开，请点击重连。".into(),
                    });
                });
            }

            // ── report ready ─────────────────────────────────────────────────
            let home = ssh::remote_home(&handle).await.unwrap_or_else(|_| "/".into());
            let cwd = ssh::remote_cwd(&handle).await.ok();

            bus.send(AppEvent::Ready {
                session_id: req.session_id.clone(),
                backend_id: req.session_id,
                home,
                cwd,
            });
        });
    }

    pub fn close(&self, session_id: &str) {
        let Some(session) = self.sessions.lock().remove(session_id) else {
            return;
        };
        session.alive.store(false, Ordering::Relaxed);
        let bus = self.bus.clone();
        let sid = session_id.to_string();
        self.rt.spawn(async move {
            let _ = session.sftp.close().await;
            let _ = session.write.send(WriteCmd::Close);
            let _ = session
                .handle
                .disconnect(Disconnect::ByApplication, "", "")
                .await;
            bus.send(AppEvent::Closed {
                session_id: sid,
                reason: "会话已关闭".into(),
            });
        });
    }

    pub fn write(&self, session_id: &str, data: Vec<u8>) {
        let Some(session) = self.get(session_id) else {
            return;
        };
        let _ = session.write_bytes(&data);
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) {
        let Some(session) = self.get(session_id) else {
            return;
        };
        let _ = session.resize(cols, rows);
    }

    // ── Remote operations ────────────────────────────────────────────────────

    /// Ask the UI to reload a remote directory (used after rename / new folder /
    /// delete / transfer). Routed through `AppState::load_remote_dir` so the
    /// panel shows its spinner and registers the expected path.
    pub fn request_remote_refresh(&self, session_id: &str, path: String) {
        self.bus.send(AppEvent::RemoteDir {
            request: 0,
            session_id: session_id.to_string(),
            path,
            result: Err("__refresh__".into()),
        });
    }

    /// `list_remote_dir`
    pub fn list_remote_dir(&self, session_id: &str, path: String) {
        let request = self.request_id();
        let bus = self.bus.clone();
        let Some(session) = self.get(session_id) else {
            bus.send(AppEvent::RemoteDir {
                request,
                session_id: session_id.to_string(),
                path,
                result: Err("Session not found".into()),
            });
            return;
        };
        let sid = session_id.to_string();
        self.rt.spawn(async move {
            let result = crate::remote_fs::list_dir(&session, &path)
                .await
                .map_err(|e| e.to_string());
            bus.send(AppEvent::RemoteDir {
                request,
                session_id: sid,
                path,
                result,
            });
        });
    }

    /// `get_remote_cwd`
    pub fn refresh_remote_cwd(&self, session_id: &str) {
        let bus = self.bus.clone();
        let Some(session) = self.get(session_id) else {
            return;
        };
        let sid = session_id.to_string();
        self.rt.spawn(async move {
            if let Ok(path) = ssh::remote_cwd(&session.handle).await {
                bus.send(AppEvent::RemoteCwd {
                    session_id: sid,
                    path,
                });
            }
        });
    }

    /// `check_host_port`
    pub fn probe_host(&self, host_id: String, host: String, port: u16) {
        let bus = self.bus.clone();
        self.rt.spawn(async move {
            let reachable = matches!(
                tokio::time::timeout(
                    Duration::from_millis(1500),
                    tokio::net::TcpStream::connect((host.as_str(), port))
                )
                .await,
                Ok(Ok(_))
            );
            bus.send(AppEvent::HostProbe { host_id, reachable });
        });
    }

    /// Run an arbitrary remote query and deliver it as `AppEvent::Query`.
    /// Run a remote query; `request` is the caller's correlation id so the
    /// result can be matched back to the popup that asked for it.
    pub fn query(&self, request: u64, session_id: &str, command: String) {
        let bus = self.bus.clone();
        let Some(session) = self.get(session_id) else {
            bus.send(AppEvent::Query {
                request,
                result: Err("Session disconnected".into()),
            });
            return;
        };
        self.rt.spawn(async move {
            let result = ssh::exec_string(&session.handle, &command)
                .await
                .map_err(|e| e.to_string());
            bus.send(AppEvent::Query { request, result });
        });
    }
}
