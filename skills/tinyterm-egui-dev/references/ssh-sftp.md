# SSH / SFTP architecture

Source: `src/ssh.rs` (transport), `src/session.rs` (lifecycle + event bus),
`src/remote_fs.rs` (SFTP ops, tar, progress), `src/transfer.rs` (orchestration),
`src/crypto.rs` + `src/storage.rs` (secrets).

## 1. Crate choices

```toml
russh = { version = "0.63", default-features = false, features = ["ring", "flate2", "rsa"] }
russh-sftp = "3"
vt100 = "0.16"
rusqlite = { version = "0.40", features = ["bundled"] }
```

- `ring` instead of the default `aws-lc-rs`: no CMake / `aws-lc-sys` build step,
  which keeps CI images simple.
- One SSH connection carries **both** the PTY channel and the SFTP subsystem
  (the original Tauri app opened a second connection). Halves auth round-trips
  and makes cancellation coherent.
- `vt100` for emulation; the grid is rendered by egui, not a terminal widget.

## 2. Object graph

```
AppState ── Arc<SessionManager> ── tokio::runtime::Handle
                │                    ├── sessions: Mutex<HashMap<session_id, Arc<LiveSession>>>
                │                    ├── cancelled: Mutex<HashSet<transfer_id>>
                │                    └── next_request: AtomicU64
                └── EventBus ── attach(egui::Context)  → ctx.request_repaint()
                                 └── send(AppEvent) → drain() in the UI loop

LiveSession { session_id, handle: Arc<SshHandle>, term: Arc<Mutex<Terminal>>,
              sftp: Mutex<Option<SftpSlot>>, cancel, … }
```

The UI never touches `russh` directly: it calls `SessionManager` methods, which
spawn tasks, and receives `AppEvent`s back on the next frame.

## 3. Connect flow (`SessionManager::connect`)

```
bookmark_with_secrets(id)      → decrypt ttenc:v1 password/private key
resolve_auth(db, bookmark, …)  → ResolvedAuth { host, port, user, auth, … }
get_trusted_host_key(host,port) → Option<fingerprint>
ssh::connect(&auth, trusted, 30s)
   ├── ConnectError::HostKey(prompt) → AppEvent::Failed{ host_key: Some(prompt) }
   └── ConnectError::Message(msg)    → AppEvent::Failed{ error: msg }
ssh::authenticate(&mut handle, &auth, password_override)
open session + request PTY (cols, rows from the UI) + shell
spawn reader task: channel data → vt100 parser → AppEvent::Output
AppEvent::Ready { session_id, backend_id, home, cwd }
```

- A 30 s connect timeout prevents hanging on a black-holed host.
- Host-key verification is a *prompt*, not a hard failure: the UI shows the
  fingerprint and `AppState::trust_host_key(&prompt)` persists it through
  `Db::upsert_trusted_host_key`.
- Password overrides (the login dialog) never touch the database.

## 4. Event contract — the rule that matters

`SessionManager::query(request, session_id, command)` takes the request id from
the caller. **Never mint the id inside the manager**: the UI stores the id it
asked with, and a reply whose id was generated elsewhere can never be matched
(this silently broke the history list and the remote directory panel).

| Event | Payload | Correlated by |
|---|---|---|
| `Output { session_id }` | terminal repaint | session id |
| `Ready { session_id, backend_id, home, cwd }` | session is usable | session id |
| `Failed { session_id, error, host_key }` | error / host-key prompt | session id |
| `Closed { session_id, reason }` | shell exited | session id |
| `RemoteDir { request, session_id, path, result }` | directory listing | **path**, not request id |
| `RemoteCwd { session_id, path }` | polled cwd for "follow terminal" | session id |
| `Transfer(Box<TransferProgress>)` | queue row update | transfer id / group id |
| `Query { request, result }` | exec output (history, sysinfo) | request id |
| `HostProbe { host_id, reachable }` | port probe | host id |
| `Toast { message, kind }` | notification | — |

Remote listings are keyed by path because a refresh can be requested twice for
the same path (auto-follow + manual refresh) and the UI must accept whichever
reply arrives.

## 5. Terminal data path

```
reader task ──bytes──▶ Arc<Mutex<vt100::Parser>> ──▶ AppEvent::Output
                                                        │
UI frame ──▶ term.rs walks the grid ──▶ glyphs painted into the terminal rect
UI keys  ──▶ term::encode_key(…) ──▶ SessionManager::write(session_id, bytes)
UI resize──▶ SessionManager::resize(session_id, cols, rows)
```

- Selection lives in `Terminal.selection`; `selected_text(selection)` is the only
  reader. Keeping a second copy in the UI state is what made "copy" return empty.
- Bracketed paste + SGR/X10 mouse reporting are implemented in `term.rs`; a TUI
  program (htop, vim) therefore works inside the panel.
- The side terminal is a *separate* session with its own tab id; `AppEvent::Ready`
  must be routed to it too (`AppState::is_side_terminal`).

## 6. SFTP layer (`src/remote_fs.rs`)

`SftpSlot` lazily opens the subsystem on first use and is reused afterwards:

```rust
pub struct SftpSlot { /* Arc<SshHandle> + lazily created SftpSession */ }
```

| Operation | Implementation notes |
|---|---|
| list dir | `read_dir` + `stat` per entry; sort dirs first, then names |
| stat / exists | `metadata`, `try_exists` |
| mkdir / remove / rename | plain SFTP; delete uses `rm -rf --`/`rm -f --` via exec with `shell_quote` |
| upload / download file | `tokio::fs::File` ↔ `SftpSession` streams, chunked, progress every ≥1 % |
| directory transfer | local: `pack_local_dir` (tar) → upload → remote `tar -xf`; download: remote `tar -cf` → stream → `unpack_local_dir` |
| tar unavailable | per-file fallback loop (detected from the exec error) |

Path helpers (`join_path`, `parent_of`, `basename`, `normalize_remote_path`,
`shell_quote`) are the only place path logic may live.

## 7. Transfers

```rust
pub struct TransferJob {
    session_id, direction, items: Vec<FileInfo>, target_dir, overwrite_all,
}
SessionManager::start_transfer(job)     // spawns a task, returns immediately
```

Each item is driven by a `TransferCtx`:

```rust
pub struct TransferCtx {
    transfer_id, display_name, direction,
    progress_total, progress_start, progress_span,   // staged progress window
    target_path, session_id, group_id,
    cancelled: Arc<Mutex<HashSet<String>>>, bus: EventBus,
}
```

Rules encoded in that struct — keep them if you refactor:

1. `start()` **removes** its id from the cancel set. Transfer ids are derived from
   the destination path, so a stale flag from an earlier cancelled attempt would
   make the next attempt fail instantly with "Cancelled".
2. The terminal `emit(Done | Error)` also removes the id, for the same reason.
3. Progress is reported only when it changes: keep `last_pct: Option<u64>` and
   compare `last_pct != Some(pct)` — a sentinel like `u64::MAX` never compares
   greater, so the percentage freezes.
4. Directory transfers emit a *stage* window (`progress_start`/`progress_span`)
   so the bar advances monotonically through pack → upload → unpack
   (`map_stage_progress`).
5. `cancel_transfer(id)` cascades: cancelling a queue group cancels every row in
   it, and the queue marks the row `Done` ~2 s later (`pump_cancel_done`) so the
   user sees the cancellation take effect.
6. Conflicts never block the worker: the row is parked as `Conflict`, the dialog
   answers with overwrite / skip / overwrite-all, and the task resumes.

## 8. Secrets

- Passwords and private keys are stored as `ttenc:v1:<…>` envelopes:
  RSA-2048 OAEP (SHA-1) wrapping an AES-256-GCM key, compatible with the original
  app's database, so `TINYTERM_DB=/path/to/tinyterm.db` reads it directly.
- `ResolvedAuth::redacted()` exists so nothing ever logs a secret; there is no
  `println!`/`log::` call anywhere that touches a credential field.
- Trusted host keys live in SQLite (`get_trusted_host_key` / `trust_host_key`).

## 9. Testing

`src/tests.rs` runs an **in-process russh test server** and drives the real
client against it:

```
fingerprint prompt → trust → wrong password rejected → password auth
→ exec → remote HOME / cwd → PTY shell echo → SFTP subsystem negotiation
```

Prefer this over mocking: it exercises the actual `russh` state machine. Keep the
server on `127.0.0.1` with a generated key, and always delete the temp database.

## 10. Pitfalls

| Symptom | Cause |
|---|---|
| Remote panel stuck on "loading" | Reply id generated by the manager instead of the caller — see §4. |
| History list never fills | Same id mismatch; `query` must receive the id the UI stored. |
| Copy yields an empty string | Two selection states; read the one the terminal writes. |
| Cancel then re-transfer the same file fails instantly | Stale id in the cancel set — clear it in `start()` *and* on terminal emit. |
| Percentage frozen | Sentinel-based progress comparison (use `Option<u64>`). |
| Side terminal stuck on "connecting" | `AppEvent::Ready` not routed to the side session. |
| File manager does not follow the terminal's cwd | Do not write `fm.remote.path` before comparing it; keep `PanelState.auto_follow`. |
