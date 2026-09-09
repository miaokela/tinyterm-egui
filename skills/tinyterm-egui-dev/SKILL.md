---
name: tinyterm-egui-dev
description: Build and extend the TinyTerm egui/eframe desktop SSH client — the cosmic/glassmorphism design tokens and self-drawn widget recipes, the panel layout, macOS .dmg and Windows NSIS packaging, and the russh/russh-sftp session + transfer architecture. Use when working on tinyterm-egui or when building any egui desktop app that needs this visual style, this packaging pipeline, or an SSH/SFTP backend.
argument-hint: "[theme | layout | ssh-sftp | packaging]"
license: MIT
metadata:
  author: tinyterm
  version: "1.0.0"
---

# TinyTerm egui Development Skill

Everything needed to extend `tinyterm-egui` (or start a sibling egui app) without
re-deriving the style, the layout rules, the SSH/SFTP wiring, or the release
pipeline. All numbers below are copied from the source, so they can be applied
verbatim.

## When to use this skill

- Adding or restyling a screen, panel, dialog or widget in an egui/eframe app
  that should look like TinyTerm (deep-space navy glass, electric-blue accent).
- Wiring SSH / SFTP features (connect, PTY shell, exec, file listing, upload,
  download, delete) on top of `russh` + `russh-sftp`.
- Producing a macOS `.dmg` or a Windows installer, or debugging why one of them
  misbehaves in CI.

## Project map (which file owns what)

```
src/theme.rs         design tokens + font installation + visuals (START HERE)
src/widgets.rs       every self-drawn control: buttons, inputs, icons, spinners
src/icons.rs         Phosphor glyph constants (vendored font in assets/)
src/app.rs           eframe::App::ui — panel/region composition, dialogs, events
src/state.rs         AppState: host/session tabs, file manager, dialogs, toasts
src/actions.rs       state transitions (the "store actions" of the app)
src/session.rs       SessionManager + EventBus + AppEvent contract
src/ssh.rs           russh connect/auth/PTY/exec, SftpSlot, host-key handling
src/remote_fs.rs     SFTP operations, path helpers, tar packing, TransferCtx
src/transfer.rs      upload/download orchestration (batches, conflicts, cancel)
src/local_fs.rs      local file operations + delete protection
src/term.rs          vt100 emulation, grid rendering, key encoding
src/crypto.rs        ttenc:v1 secret envelope (RSA-OAEP + AES-256-GCM)
src/storage.rs       SQLite (same schema as the original Tauri app)
src/logging.rs       file log + fatal-error dialog (GUI builds have no console)
src/ui/*.rs          one module per screen/panel
```

## Core principles

1. **Nothing uses egui's default look.** Every interactive control is drawn by
   hand in `src/widgets.rs` (rounded rect + stroke + glow, no `Button` styling).
   New controls belong there, not inline in a screen.
2. **One font covers both scripts.** A CJK face is installed as the *primary*
   proportional font so Latin and Chinese share row metrics; mixing a Latin
   primary with a CJK fallback gives each run a different row height and makes
   labels look vertically off-centre.
3. **Panels are rounded islands, never edge-to-edge.** Leave a gap (`6–8 px`)
   between the terminal and the file manager so each reads as its own glass card.
4. **State changes go through `actions.rs`; the backend talks back over
   `EventBus`.** UI code never blocks on SSH.
5. **Every request id comes from the caller.** `SessionManager::query(request,
   session_id, command)` takes the id; do not let the manager mint its own, or
   the reply cannot be correlated (this caused two real bugs).

## Reference files

| File | Contents |
|---|---|
| `references/theme.md` | Colour/radius/text tokens, ANSI palette, font installation, visuals, per-widget recipes, egui 0.36 API notes and pitfalls |
| `references/layout.md` | Window sizes, panel/region composition, spacing constants, collapsed states, z-order for dialogs and context menus |
| `references/ssh-sftp.md` | russh/russh-sftp architecture, connect flow, event contract, transfer engine, cancellation, security |
| `references/packaging.md` | macOS `.app`/`.dmg` pipeline, Windows NSIS installer, static CRT, wgpu-for-RDP, CI workflow, and the exact failure modes hit so far |

## Verification commands

```bash
cargo check                     # fast feedback while editing
cargo test                      # 13 tests: crypto, storage, term, parsers, SSH e2e
cargo run --release             # manual check of the visual result
./scripts/bundle-macos.sh       # .app + styled .dmg
```

CI (`.github/workflows/build.yml`) runs two independent jobs on a `v*` tag:
`macos-universal` → `TinyTerm-macos-universal.dmg`, `windows-x86_64` →
`TinyTerm-windows-x86_64-setup.exe`. No GitHub Release is created; the binaries
are workflow artifacts.

## Non-obvious rules (learned from real failures)

- Release builds set `windows_subsystem = "windows"`, so **anything written to
  stderr is invisible**. Report fatal errors through `logging::fatal`, which
  writes to `%USERPROFILE%\.tinyterm-egui\tinyterm.log` and raises a native
  message box.
- A child `Ui` inherits its parent's layout; when creating one with
  `new_child`, pass `.layout(Layout::right_to_left(Align::Center))` explicitly or
  buttons stack vertically.
- Repeated widgets (history rows, quick actions, queue rows) need unique ids —
  include the session id or the row index, or egui merges their state.
- The Windows build uses **wgpu** (not glow) so it starts over RDP; see
  `references/packaging.md`.
- The DMG must be mounted under `/Volumes` and given ~5 s before Finder is
  driven by AppleScript; otherwise the background silently disappears.
