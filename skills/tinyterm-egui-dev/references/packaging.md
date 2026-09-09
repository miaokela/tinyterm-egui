# Packaging: macOS .dmg and Windows installer

Scripts: `scripts/bundle-macos.sh`, `scripts/make-dmg.sh`,
`scripts/make-dmg-background.swift`, `scripts/make-windows-icon.swift`,
`scripts/windows-installer.nsi`, `build.rs`, `.cargo/config.toml`.
CI: `.github/workflows/build.yml`.

## 1. macOS

### `.app` bundle

winit ignores `set_window_icon` on macOS — the Dock/Finder icon only comes from a
bundle whose `Info.plist` points at an `.icns`:

```
TinyTerm.app/Contents/
├── MacOS/TinyTerm                 # universal binary (lipo of arm64 + x86_64)
├── Resources/AppIcon.icns
└── Info.plist
```

`Info.plist` keys that matter: `CFBundleExecutable` (must match the file in
`MacOS/`), `CFBundleIconFile` (`AppIcon`, no extension), `CFBundleIdentifier`,
`CFBundleShortVersionString` (taken from `Cargo.toml` in CI), `LSMinimumSystemVersion`,
`NSHighResolutionCapable`.

### DMG pipeline (`scripts/make-dmg.sh`)

```
stage/                     .background/background.png (+ @2x)
  TinyTerm.app             Applications -> /Applications symlink
      │
      ├─ hdiutil create -srcfolder … -format UDRW
      ├─ hdiutil attach (under /Volumes, no -mountpoint)
      ├─ sleep 5                              ← Finder must notice the volume
      ├─ osascript: icon view, background picture, icon positions, window bounds
      ├─ wait for .DS_Store to appear (poll ≤ 20 s)
      ├─ hdiutil detach
      ├─ hdiutil convert -format UDZO -imagekey zlib-level=9
      └─ remount read-only and grep .DS_Store for "background"
```

Finder is the only thing that can write the `.DS_Store` carrying the background
picture and icon positions, so the script drives it via AppleScript:

```applescript
set current view of container window to icon view
set toolbar visible of container window to false
set statusbar visible of container window to false
set the bounds of container window to {200, 120, 860, 560}   -- 660 x 440
set icon size of opts to 128
set text size of opts to 12
set background picture of opts to file ".background:background.png"
set position of item "TinyTerm.app"  of container window to {175, 165}
set position of item "Applications"  of container window to {485, 165}
```

**The `-1728` trap.** Mounting with `-mountpoint` into a temp directory and
running the AppleScript immediately fails with:

```
Finder got an error: Can't get disk "TinyTerm". (-1728)
```

The volume is simply not in Finder's disk list yet, so the background and icon
positions are silently never written and the DMG opens plain. Fixes (all three,
as in `create-dmg`): mount the normal way under `/Volumes` (no `-mountpoint`),
`sleep 5` before the AppleScript, retry 3×, then poll for `.DS_Store`. Address
the disk by `basename "$MOUNT_DIR"`, which is what Finder shows.

The script **fails the build** if the finished image has no background reference —
a silently unstyled DMG is worse than a red build.

### Background artwork

`scripts/make-dmg-background.swift` renders 660×440 (`@2x` 1320×880) with
CoreGraphics: drag hint on top, an **empty band at y = 75…254** where the icons
land (centres at y = 165), the two Gatekeeper cases at the bottom, and the last
40 px left clear in case Finder reserves space for the title bar.

> CoreGraphics gotcha: drawing a linear gradient with `options: []` corrupts the
> alpha of the *next* radial gradient (it paints a solid blob). Pass
> `.drawsBeforeStartLocation, .drawsAfterEndLocation` to the linear gradient.

Unsigned builds trigger two different dialogs; the artwork tells the user both:

1. 「TinyTerm 已损坏」→ `xattr -cr /Applications/TinyTerm.app`
2. 「无法验证开发者」→ 完成 → 系统设置 → 隐私与安全性 → 仍要打开

## 2. Windows

### Single self-contained exe

The app is one `.exe`; the only other installed file is `uninstall.exe`. That is
correct for a statically linked Rust binary — do not expect WebView2/Qt-style
DLL folders.

```toml
# .cargo/config.toml — MSVC links vcruntime140.dll dynamically by default, and a
# machine without the VC++ Redistributable fails with "VCRUNTIME140.dll was not
# found". +crt-static embeds the runtime.
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

```rust
// src/main.rs — no console window in release builds
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
```

Consequence: **stderr is invisible**. All fatal errors go through
`logging::fatal` (file log + `MessageBoxW`).

### Renderer: wgpu, not glow, on Windows

`glow` needs OpenGL 2.0+. A remote-desktop session or a GPU-less cloud VM only
offers the GDI OpenGL 1.1 software renderer, so `run_native` fails and the
process exits with **no window and no message** ("双击完全没反应"). Windows
therefore uses `Renderer::Wgpu` with a custom adapter selector:

```rust
// rank: DiscreteGpu > IntegratedGpu > VirtualGpu > Cpu (WARP) > other
adapters.iter()
    .filter(|a| surface.map_or(true, |s| a.is_surface_supported(s)))
    .min_by_key(|a| rank(a))
```

WARP is the D3D12 software rasterizer, available on Windows 10+, so the app also
runs over RDP. The feature is enabled only for the Windows target
(`[target.'cfg(windows)'.dependencies] eframe = { features = ["wgpu"] }`), so
macOS keeps glow and stays small.

### Icon and version info

`assets/icon.ico` (16–256, uncompressed BMP entries) is generated by
`scripts/make-windows-icon.swift` and embedded by `build.rs` via `winresource`
(Windows-only build-dependency; failures are warnings, never fatal). The same
`.ico` is the NSIS installer artwork.

### NSIS installer (`scripts/windows-installer.nsi`)

- Per-user install to `%LOCALAPPDATA%\Programs\TinyTerm`,
  `RequestExecutionLevel user` → **no UAC prompt**.
- `Unicode true` + `MUI_LANGUAGE "SimpChinese"` then `"English"`: the script
  itself is pure ASCII (NSIS needs a BOM for non-ASCII), yet Chinese systems get
  a Chinese wizard from the MUI language file.
- Start-menu group + desktop shortcut + `Apps & features` entry under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\TinyTerm`
  (including `EstimatedSize` from `${GetSize}`).
- The uninstaller removes program files only; the database with hosts and
  encrypted credentials in `%USERPROFILE%\.tinyterm-egui` is kept.
- `SetShellVarContext current` **must be inside a Section or Function** —
  NSIS rejects it at top level: `command SetShellVarContext not valid outside
  Section or Function`.

## 3. CI (`.github/workflows/build.yml`)

Two **independent jobs**, no matrix, no `if:` guards:

| Job | Runner | Produces |
|---|---|---|
| `macos` | `macos-latest` | `TinyTerm-macos-universal.dmg` (arm64 + x86_64 via `lipo`) |
| `windows` | `windows-latest` | `TinyTerm-windows-x86_64-setup.exe` (NSIS) |

- Triggered by a `v*` tag (and `workflow_dispatch`); **no GitHub Release** —
  binaries are workflow artifacts (`actions/upload-artifact@v4`).
- `Swatinem/rust-cache@v2` with a per-job `key`.
- Windows installs NSIS via `choco install nsis` (guarded by `Test-Path`), then
  runs `makensis /DAPP_VERSION=… /DAPP_EXE=… /DOUT_FILE=… /DICON_FILE=…`.
- A single matrix job shared by both platforms *works* but is confusing: the
  Windows job's step list still shows the macOS packaging steps (as `skipped`).
  Keep the jobs separate.

## 4. Failure modes seen so far

| Error | Cause | Fix |
|---|---|---|
| `VCRUNTIME140.dll was not found` | dynamic CRT | `+crt-static` |
| App exits silently, no window (RDP/VM) | glow needs OpenGL 2.0+ | wgpu + WARP selector, plus a fatal-error dialog |
| `Can't get disk "TinyTerm" (-1728)` | volume not yet known to Finder | mount under `/Volumes`, sleep 5 s, retry, poll `.DS_Store` |
| DMG opens with no background | the AppleScript above never ran | script now verifies `.DS_Store` and fails the build |
| `command SetShellVarContext not valid outside Section or Function` | NSIS scoping | move it into each `Section` |
| `compile_error!("The platform you're compiling for is not supported by winit")` on Linux | eframe built without `x11`/`wayland` | add both features |
| `cannot update the lock file … because --locked was passed` | manifest changed, lock stale | `cargo metadata` to refresh `Cargo.lock`, commit it |
| Font metrics test fails on CI (`No fonts available until first call to Context::run()`) | measuring outside a frame | measure inside `ctx.run_ui(...)`, or skip when the system font is absent |
| macOS job can't drive Finder | automation denied | the script warns and still produces a DMG, but the build then fails the `.DS_Store` check — treat it as a real failure |

## 5. Release checklist

```bash
cargo test --release --locked
./scripts/bundle-macos.sh && open target/release/TinyTerm.dmg   # visual check
git push && git tag -f vX.Y.Z && git push origin refs/tags/vX.Y.Z --force
```

Then confirm in the run log that the macOS job printed
`layout verified: background picture + icon positions stored`, and that both
artifacts exist.
