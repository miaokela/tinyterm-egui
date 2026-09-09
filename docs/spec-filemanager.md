# TinyTerm File Manager — Exhaustive Implementation Specification

> **Purpose.** This document specifies the complete behaviour of the existing TinyTerm **File Manager**
> (the dual-pane local/remote SFTP browser docked under the terminal), in enough detail to
> reimplement it as a Rust/`egui` application with no Tauri and no WebView. It was produced by
> reading the following files **in full**:
>
> | File | LOC | Role |
> |---|---|---|
> | `tinyterm/src/components/FileManager.tsx` | 2764 | the whole feature: panels, queue, dialogs, transfer orchestration |
> | `tinyterm/src/components/FileManager.css` | 786 | every style rule that affects the feature |
> | `tinyterm/src-tauri/src/commands/sftp.rs` | 1025 | SFTP/SCP listing, upload/download, delete, mkdir, rename |
> | `tinyterm/src-tauri/src/commands/local_fs.rs` | 318 | local `tar` pack/unpack used for directory transfers |
>
> Supporting files were consulted **only** to resolve exact types, store actions, design tokens and
> the two IPC events: `src/types/index.ts`, `src/store/index.ts`, `src/App.tsx`,
> `src/styles/global.css`, `src/styles/app.css`, `src/components/AppDialogHost.tsx`,
> `src/components/AppDialogHost.css`, `src/components/CredentialsModal.css`,
> `src/components/ElectricBorder.tsx`, `src-tauri/src/models.rs`,
> `src-tauri/src/commands/ssh.rs`. Backend internals (SFTP session reuse, chunking, lock model,
> guards) are specified in `spec-backend.md`; this document is the **UI + orchestration** spec and
> re-states every contract the UI depends on so it can be implemented standalone.
>
> **Markers used below**
>
> * `[jsx]` — taken verbatim from `FileManager.tsx`.
> * `[css]` — taken verbatim from `FileManager.css` (or a named global stylesheet).
> * `[measured]` — empirically measured by loading the real DOM + real stylesheets in headless
>   Chrome (1400×900 viewport, macOS) and reading `getBoundingClientRect()`. Used where CSS alone is
>   ambiguous.
> * `[ipc]` — an IPC call/event contract.
> * `[quirk]` — behaviour that looks like a bug; preserved here so the port can decide deliberately.

---

## 1. Feature overview

The File Manager is a **per-session**, collapsible dual-pane file browser embedded in the terminal
area of one SSH session tab. It has:

* a **collapse bar** (`文件管理`) that toggles the whole feature;
* a **local panel** (left, `本地`, monitor icon) backed by direct filesystem access;
* a **remote panel** (right, `远程`, server icon) backed by SFTP over a *second* SSH connection;
* a **center divider** holding the two transfer buttons (upload →, download ←) with selection-count
  badges and busy spinners;
* a **transfer queue** above the panels showing per-item and per-batch progress;
* **context menus** (rename / delete), an **inline rename/new-folder modal**, a **confirm dialog**
  component (upload/download/conflict prompts) and per-item **conflict resolution**.

Directory transfers are **not** recursive SFTP walks by default: the feature first probes the remote
host for `tar` and, when available, packs the folder into a temp tarball, moves that single file, and
unpacks it remotely/locally. A recursive per-file fallback exists for hosts without `tar`.

### 1.1 Mount point and lifetime

`[jsx]` `App.tsx` renders it as the **last flex child of `.terminal-inner`**, only while the session
is connected:

```tsx
<div className="terminal-inner" style={{ display: 'flex', flexDirection: 'row',
                                        gap: session.sideTerminalOpen ? '6px' : '0' }}>
  <div style={{ flex: session.sideTerminalOpen ? '1 1 50%' : '1 1 100%', minWidth: 0, minHeight: 0 }}>
    <TerminalView ... />
  </div>
  {session.sideTerminalOpen && ... /* optional side terminal */}

  {session.status === 'connected' && (
    <FileManager session={session} bookmarkTabId={bookmarkTab.id} />
  )}
</div>
```

Consequences to preserve in the port:

1. The File Manager exists **only for connected sessions**; on disconnect it unmounts and all local
   component state (paths, selection, queue rows) is lost. The store's `transfers` for that session
   are also dropped when the session tab is closed (`transfers.filter(t => t.session_id !== sessionTabId)`).
2. **All sessions are mounted at once** (inactive ones are hidden with `.terminal-area.hidden {
   opacity: 0; pointer-events: none; position: absolute; visibility: hidden }`). Therefore *every*
   connected session owns a live `FileManager` instance whose effects keep running while invisible
   (`get_remote_cwd` sync, `terminalPath` live-follow, `tar` probe). `[quirk]` In the port, give each
   session its own File Manager state object and keep non-visible ones "active but not drawn".
3. Props: `{ session: SessionTab; bookmarkTabId: string }`.

```ts
interface SessionTab {
  id: string; title: string; bookmarkId: string
  sessionId?: string                     // backend session id (SSH), absent until connected
  status: 'connecting' | 'connected' | 'disconnected' | 'error'
  error?: string
  cols: number; rows: number
  localPath: string                      // set to local $HOME when the session connects
  remotePath: string                     // initialised to '/'
  terminalPath?: string                  // cwd reported by the terminal; drives the remote panel
  fmOpen?: boolean                       // file manager expanded?
  sideTerminalOpen?: boolean
  sideTerminalSessionId?: string
  sideTerminalStatus?: 'connecting' | 'connected' | 'disconnected' | 'error'
  sideTerminalError?: string
}
```

`collapsed = !session.fmOpen` — `fmOpen` is `undefined` on a fresh session, so the feature starts
collapsed.

---

## 2. Layout, geometry and styling

### 2.1 DOM tree (exact nesting)

```tsx
<div className={`fm-root${collapsed ? ' fm-root--collapsed' : ''}`} onClick={() => ctxMenu && setCtxMenu(null)}>
  {!collapsed && (
    <div className="fm-content glass-panel">
      <TransferQueue transfers={transfers} onCancel={handleCancelTransfer} />   {/* may render null */}
      <div className="fm-panels">
        <Panel side="local"  ... />                {/* .fm-panel.fm-panel--local  */}
        <div className="fm-divider">
          <div className="fm-divider-line" />
          <div className="fm-divider-arrows">
            <button className="fm-transfer-btn ...">{ArrowRight | spinner}{badge}</button>
            <button className="fm-transfer-btn ...">{ArrowLeft  | spinner}{badge}</button>
          </div>
          <div className="fm-divider-line" />
        </div>
        <Panel side="remote" ... />                {/* .fm-panel.fm-panel--remote */}
      </div>
    </div>
  )}

  {confirmDialog && <ConfirmDialog ... />}          {/* portal → document.body */}
  {transferConflict && <ConfirmDialog ... />}       {/* portal → document.body */}
  {inlineAction && <div className="modal-overlay" style={{zIndex:2100}}>…</div>}

  {/* Collapse handle — last child in DOM */}
  <div className="fm-bar" onClick={() => toggleFm(bookmarkTabId, session.id)}>
    {collapsed ? <ChevronDown size={12} strokeWidth={2.2} className="fm-bar-arrow" />
               : <ChevronUp   size={12} strokeWidth={2.2} className="fm-bar-arrow" />}
    <HardDrive size={12} strokeWidth={1.8} className="fm-bar-icon" />
    <span className="fm-bar-title">文件管理</span>
    {activeTransfers.length > 0 && <span className="fm-bar-badge">{activeTransfers.length}</span>}
  </div>

  {ctxMenu && <ContextMenu ... />}                  {/* portal → document.body, z-index 2000 */}
</div>
```

`Panel` in turn renders:

```tsx
<div className={`fm-panel fm-panel--${side}${disabled ? ' fm-panel--disabled' : ''}`}>
  <div className="fm-panel-header">
    <span className="fm-panel-icon">{icon}</span>
    <span className="fm-panel-title">{title}</span>        {/* 本地 / 远程 */}
    <div className="fm-panel-actions">
      <button className="fm-icon-btn" title={showHidden ? '隐藏隐藏文件' : '显示隐藏文件'}>{Eye|EyeOff 13}</button>
      <button className="fm-icon-btn" title="刷新">{RefreshCw 13}</button>
      <button className="fm-icon-btn" title="新建文件夹">{FolderPlus 13}</button>
    </div>
  </div>

  <div className="fm-path-bar">
    <button className="fm-icon-btn fm-up-btn" title="上级目录"><ChevronUp size={13} strokeWidth={2.2} /></button>
    {editingPath
      ? <input className="fm-path-input" value={pathInput} autoFocus
               onChange={e => setPathInput(e.target.value)} onBlur={commitPath}
               onKeyDown={e => { if (e.key === 'Enter') commitPath()
                                 if (e.key === 'Escape') { setEditingPath(false); setPathInput(currentPath) } }} />
      : <div className="fm-path-display" title={currentPath} onClick={…}>{currentPath}</div>}
  </div>

  <div className="fm-list">
    {loading ? <div className="fm-status loading"><div className="fm-spinner" /></div>
     : error  ? <div className="fm-status error">{error}</div>
     : files.length === 0 ? <div className="fm-status muted">空目录</div>
     : files.map(file => (
         <div className={`fm-item${isSelected ? ' fm-item--selected' : ''}`}
              onClick={…} onDoubleClick={…} onContextMenu={…}>
           <FileItemIcon isDir={file.is_dir} name={file.name} />
           <span className="fm-item-name">{file.name}</span>
           {!file.is_dir && <span className="fm-item-size">{formatSize(file.size)}</span>}
         </div>
       ))}
  </div>

  {disabled && <div className="fm-panel-overlay"><span>{busyLabel || '处理中...'}</span></div>}
</div>
```

### 2.2 Root container

```css
.fm-root {
  display: flex;
  flex-direction: column-reverse;   /* [css] children are laid out from the BOTTOM up */
  flex-shrink: 0;
  position: relative;
}
```

`[measured]` With the real ancestor chain (`.app-root` → `.app-container` → `.app-body` →
`.workspace` → `.terminal-area` → `.terminal-inner`, window 1400×900, sidebar 200 px):

| element | top | left | width | height | bottom |
|---|---|---|---|---|---|
| `.terminal-inner` | 6 | 219 | 1170 | 740 | 746 |
| `.fm-root` (expanded) | 10 | 925.8 | 459.2 | **732** (stretched) | 742 |
| `.fm-bar` (expanded) | 454 | 925.8 | 459.2 | 28 | 482 |
| `.fm-content` (expanded) | 482 | 925.8 | 459.2 | **260** | 742 |
| `.fm-root` (collapsed) | 10 | 1252.5 | 132.5 | 732 | 742 |
| `.fm-bar` (collapsed) | 711 | 1252.5 | 132.5 | 31 | 742 |

Facts that matter for the port:

1. `.fm-root` **stretches to the full height** of `.terminal-inner` (cross-axis `stretch`) and is a
   **right-hand column whose width is content-determined** — there is no `width`, `flex-basis` or
   percentage anywhere on `.fm-root` or `.fm-content`. Its width equals the max-content width of its
   children (459 px with a transfer row and typical names; 132 px when collapsed).
2. Because the direction is `column-reverse`, the **first DOM child is placed at the bottom**: the
   260 px content block is pinned to the bottom of the terminal area and the collapse bar sits
   **directly above it**, with empty space above the bar.
3. `[quirk]` The code comments (`/* content is above bar (column-reverse) */`,
   `/* Expanded content — rendered BEFORE bar in DOM so column-reverse puts it above */`) state the
   opposite of what actually renders. `[measured]` shows bar **above** content. The border-radius
   rules are consistent with the *comments* (bar has rounded bottom corners when collapsed), so the
   author's intent was clearly "content above, bar docked at the bottom".
   **Recommendation for the egui port:** implement the intended layout — a full-width bar docked at
   the bottom of the terminal area, with a 260 px content area expanding **upward above** it — and
   treat the current right-hand-column/bar-on-top geometry as a defect. If pixel-fidelity to the
   current build is required instead, reproduce §2.2 verbatim.

### 2.3 Collapse bar

```css
.fm-bar {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 5px 14px;
  cursor: pointer;
  background: rgba(7, 18, 36, 0.8);
  border: none;
  border-top: 1px solid rgba(58, 132, 255, 0.2);
  border-radius: 0 0 var(--radius-md) var(--radius-md);   /* = 0 0 12px 12px */
  transition: background 0.15s, border-radius 0.15s;
  user-select: none;
  flex-shrink: 0;
}
.fm-root:not(.fm-root--collapsed) .fm-bar { border-radius: 0; }
.fm-root--collapsed .fm-bar { border-radius: 0 0 var(--radius-md) var(--radius-md); }

.fm-bar-arrow { color: var(--color-text-muted); flex-shrink: 0; }
.fm-bar-icon  { color: var(--color-text-muted); flex-shrink: 0; }
.fm-bar-title { font-size: 12px; font-weight: 600; color: var(--color-text-secondary); flex-shrink: 0; }
.fm-bar-badge {
  display: inline-flex; align-items: center; justify-content: center;
  min-width: 20px; height: 20px; padding: 0 5px;
  border-radius: var(--radius-sm); background: var(--color-accent); color: #fff;
  font-size: var(--text-xs); font-weight: 700; flex-shrink: 0;
}
```

* `[measured]` bar height **28 px** (icons/text only) and **31 px** when the badge is present
  (badge is 20 px + 2×5 px padding = 30 px content box).
* There is **no `:hover` rule** for `.fm-bar` (removed in commit `5f0a40f`).
* Badge shows `activeTransfers.length` = number of this session's transfers whose status is **not**
  `done` (so `pending`, `transferring`, `error` and `conflict` all count).
* Bar click → `toggleFm(bookmarkTabId, session.id)` (store action toggling `session.fmOpen`).
* `.fm-bar-hint` exists in CSS but is **never rendered** — dead rule.

### 2.4 Content container

```css
.fm-content {
  display: flex;
  flex-direction: column;
  height: 260px;                                   /* fixed */
  border-bottom: 1px solid rgba(58, 132, 255, 0.2);
  border-top: none;
  overflow: hidden;
}
.fm-content.glass-panel {
  border-top-left-radius: 0;
  border-top-right-radius: 0;
}
```

`.glass-panel` (global) supplies `background: var(--color-bg-panel)` = `rgba(7, 22, 43, 0.86)`,
`backdrop-filter: blur(12px)`, `border: 1px solid var(--color-border)`, `border-radius: 16px`,
`box-shadow: var(--glass-hi), var(--shadow-panel)`.

### 2.5 Transfer queue

```css
.fm-transfer-queue {
  display: flex; flex-direction: column;
  padding: 5px 10px;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
  background: rgba(0, 0, 0, 0.25);
}
.fm-tc-row {
  display: flex; align-items: center; gap: 0; padding: 0;
  border-radius: var(--radius-sm);
  background: rgba(255, 255, 255, 0.03);
  min-height: 28px; overflow: visible;
}
.fm-tc-region--active {           /* region 1: left */
  flex: 1; display: flex; align-items: center; gap: 6px; min-width: 0; padding: 4px 10px;
}
.fm-tc-dir   { color: var(--color-accent); display: flex; align-items: center; flex-shrink: 0; }
.fm-tc-action{ font-size: var(--text-xs); color: var(--color-text-secondary); flex-shrink: 0; }
.fm-tc-filename {
  font-size: var(--text-xs); color: var(--color-text-primary);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  max-width: 120px; flex-shrink: 0;
}
.fm-tc-track { flex: 1; height: 4px; background: rgba(0,0,0,0.3); border-radius: var(--radius-xs);
               overflow: hidden; min-width: 40px; }
.fm-tc-fill  { height: 100%; transition: width 0.15s ease-out; }
.fm-tc-pct   { font-size: var(--text-xs); font-variant-numeric: tabular-nums;
               color: var(--color-text-secondary); flex-shrink: 0; min-width: 28px; text-align: right; }
.fm-tc-region--pending {          /* region 2: center */
  display: flex; align-items: center; justify-content: center; gap: 5px;
  flex-shrink: 0; padding: 4px 12px;
  border-left: 1px solid rgba(255,255,255,0.06); min-width: 52px;
}
.fm-tc-region--done {             /* region 3: right */
  display: flex; align-items: center; justify-content: center; gap: 5px;
  flex-shrink: 0; padding: 4px 12px;
  border-left: 1px solid rgba(255,255,255,0.06); min-width: 52px;
}
.fm-tc-label { font-size: var(--text-xs); color: var(--color-text-muted); }
.fm-tc-count {
  display: inline-flex; align-items: center; justify-content: center;
  min-width: 20px; height: 20px; padding: 0 5px;
  border-radius: var(--radius-sm); font-size: var(--text-xs); font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.fm-tc-region--pending .fm-tc-count { background: rgba(240,160,64,0.18); color: var(--color-warning); }
.fm-tc-region--done    .fm-tc-count { background: rgba(87,227,165,0.16); color: var(--color-success); }
.fm-tc-cancel {
  display: flex; align-items: center; justify-content: center;
  width: 28px; height: 28px; padding: 0; border: none;
  border-left: 1px solid rgba(255,255,255,0.06);
  background: transparent; color: var(--color-text-muted); cursor: pointer;
  transition: background 0.12s, color 0.12s; flex-shrink: 0;
}
.fm-tc-cancel:hover { background: rgba(220,50,50,0.15); color: var(--color-error); }
.fm-tc-row > .fm-tc-error { font-size: var(--text-xs); color: var(--color-error);
                            white-space: nowrap; flex-shrink: 0; padding: 0 8px; }
```

* `[measured]` queue box height 39 px (5+5 padding + 28 px row); row height 28 px; cancel button
  28×28; progress track height 4 px; percentage cell 28 px min-width.
* Each queue row is wrapped in `<ElectricBorder>` with:
  `active={isActive}` (only when the row's status is `transferring`),
  `color={hasError ? '#e53935' : '#FFD700'}`, `speed={1.2}`, `chaos={0.08}`,
  `borderRadius={6}`, `offset={3}`, `style={{ marginBottom: 4 }}`.
  `ElectricBorder` is a `<canvas>`-drawn animated glow border (three flowing arcs). In the egui port
  this becomes a 1 px animated stroke around the row: `#FFD700` normally, `#e53935` when the row is
  in error/conflict, animated only while the row is actively transferring.
* The whole queue renders `null` when no group is "active" (see §8.7).
* `.fm-tc-meta` exists in CSS but is never rendered — dead rule.

### 2.6 Panels container and divider

```css
.fm-panels { flex: 1; display: flex; min-height: 0; overflow: hidden; }

.fm-divider {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  width: 32px; flex-shrink: 0; gap: 6px;
  background: rgba(0, 0, 0, 0.1);
  border-left: 1px solid var(--color-border);
  border-right: 1px solid var(--color-border);
}
.fm-divider-line   { flex: 1; width: 1px; background: var(--color-border); }
.fm-divider-arrows { display: flex; flex-direction: column; align-items: center; gap: 6px; }

.fm-transfer-btn {
  display: flex; align-items: center; justify-content: center; position: relative;
  width: 20px; height: 20px; padding: 0;
  border: 1px solid rgba(255,255,255,0.08);
  border-radius: var(--radius-sm);
  background: transparent; cursor: pointer;
  transition: background 0.15s, border-color 0.15s, transform 0.12s, box-shadow 0.15s;
}
.fm-transfer-btn:hover          { background: rgba(58,132,255,0.14); border-color: rgba(112,191,255,0.38); }
.fm-transfer-btn:active         { transform: scale(0.94); }
.fm-transfer-btn:focus-visible  { outline: none; border-color: rgba(112,191,255,0.5);
                                  box-shadow: 0 0 0 2px rgba(58,132,255,0.15); }
.fm-transfer-btn:disabled       { opacity: 0.35; cursor: not-allowed; background: transparent;
                                  border-color: rgba(255,255,255,0.05); box-shadow: none; }
.fm-transfer-btn.is-loading     { opacity: 1; background: rgba(58,132,255,0.14);
                                  border-color: rgba(112,191,255,0.32); }
.fm-transfer-btn.is-active      { border-color: rgba(112,191,255,0.62); background: rgba(58,132,255,0.18); }
.fm-transfer-btn.is-active .fm-divider-icon { color: var(--color-accent-light); opacity: 1; }
.fm-divider-icon { color: var(--color-text-muted); opacity: 0.55; transition: color 0.15s, opacity 0.15s; }
.fm-transfer-btn:hover .fm-divider-icon,
.fm-transfer-btn:focus-visible .fm-divider-icon { color: var(--color-text-primary); opacity: 0.85; }

.fm-transfer-spinner {
  width: 11px; height: 11px;
  border: 1.5px solid rgba(153,183,214,0.25);
  border-top-color: rgba(228,240,255,0.92);
  border-right-color: rgba(228,240,255,0.64);
  border-radius: 50%;
  animation: fm-spin 0.65s linear infinite;
}
.fm-transfer-badge {
  position: absolute; top: -5px;
  display: inline-flex; align-items: center; justify-content: center;
  min-width: 16px; height: 16px; padding: 0 2px;
  border-radius: var(--radius-sm); background: var(--color-accent); color: #fff;
  font-size: var(--text-xs); font-weight: 700; font-variant-numeric: tabular-nums;
  line-height: 1; pointer-events: none; box-shadow: 0 1px 3px rgba(0,0,0,0.3);
}
.fm-transfer-badge--left  { left: -4px; }
.fm-transfer-badge--right { right: -4px; }
```

* `[measured]` divider width 32 px, full panel height; transfer button 20×20; badge 16×16 offset
  −4 px on the outer side (`--left` on the upload button, `--right` on the download button).
* Two `fm-divider-line` elements (`flex: 1`, 1 px wide) fill the space above and below the arrow
  group.

### 2.7 Panel

```css
.fm-panel {
  flex: 1; display: flex; flex-direction: column;
  min-width: 0; min-height: 0;
  transition: background 0.15s; position: relative;
}
.fm-panel--disabled { opacity: 0.82; }
.fm-panel-overlay {
  position: absolute; inset: 0; display: flex; align-items: center; justify-content: center;
  background: rgba(8,10,18,0.42); backdrop-filter: blur(1px);
  z-index: 3; pointer-events: all;
}
.fm-panel-overlay span {
  padding: 6px 10px; border-radius: var(--radius-sm);
  background: rgba(0,0,0,0.55); border: 1px solid rgba(255,255,255,0.08);
  color: var(--color-text-primary); font-size: var(--text-xs);
}
.fm-panel--drop-target {          /* DEAD: never applied by the TSX */
  background: rgba(58,132,255,0.12);
  outline: 2px dashed var(--color-accent);
  outline-offset: -2px;
}

.fm-panel-header {
  display: flex; align-items: center; gap: 7px; padding: 5px 10px;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0; background: rgba(0,0,0,0.1);
}
.fm-panel--local  .fm-panel-header { border-top-left-radius: 0; }
.fm-panel--remote .fm-panel-header { border-top-right-radius: 0; }
.fm-panel-icon  { color: var(--color-text-muted); display: flex; align-items: center; flex-shrink: 0; }
.fm-panel-title {
  font-size: var(--text-xs); font-weight: 700; color: var(--color-text-secondary);
  text-transform: uppercase; letter-spacing: 0.5px; flex: 1;
}
.fm-panel-actions { display: flex; gap: 2px; flex-shrink: 0; }
```

`[measured]` header height **33 px** (5 px padding + 22 px icon button + 1 px border).

### 2.8 Path bar

```css
.fm-path-bar {
  display: flex; align-items: center; gap: 4px; padding: 3px 6px;
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0; background: rgba(0,0,0,0.08);
}
.fm-up-btn { width: 20px; height: 20px; flex-shrink: 0; }

.fm-path-display {
  flex: 1; font-size: var(--text-xs); font-family: var(--font-mono, monospace);
  color: var(--color-text-muted);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  cursor: text; padding: 2px 5px; border-radius: var(--radius-xs); min-width: 0;
  transition: background 0.12s, color 0.12s;
}
.fm-path-display:hover { background: rgba(58,132,255,0.12); color: var(--color-text-secondary); }

.fm-path-input {
  flex: 1; font-size: var(--text-xs); font-family: var(--font-mono, monospace);
  padding: 2px 6px; height: 22px;
  background: var(--color-bg-input); border: 1px solid var(--color-accent);
  border-radius: var(--radius-xs); color: var(--color-text-primary);
  min-width: 0; outline: none;
}
```

`[measured]` path bar height **29 px** (3+3 padding + 22 px input + 1 px border); the display variant
is 18 px tall so the bar is 25 px in display mode. The `fm-up-btn` overrides `.fm-icon-btn`'s 22×22
to 20×20.

### 2.9 File list and rows

```css
.fm-list {
  flex: 1; overflow-y: auto; padding: 3px 4px; min-height: 0;
  user-select: none; -webkit-user-select: none;
}
.fm-list::-webkit-scrollbar { width: 6px; }
.fm-list::-webkit-scrollbar-thumb { background: rgba(58,132,255,0.3); border-radius: var(--radius-xs); }

.fm-item {
  display: flex; align-items: center; gap: 7px; padding: 5px 8px;
  border-radius: var(--radius-xs); cursor: pointer;
  transition: background 0.1s, border-color 0.1s, box-shadow 0.1s;
  user-select: none; -webkit-user-select: none;
  border: 1px solid transparent;
}
.fm-item:hover { background: rgba(47,125,255,0.08); }
.fm-item--selected {
  background: rgba(47,125,255,0.14);
  border-color: transparent;
  border-left: 2px solid var(--color-accent-light);
  border-radius: 0 var(--radius-xs) var(--radius-xs) 0;
  padding-left: 7px;                    /* 8 - 1 to compensate the 2px border */
}
.fm-item--selected:hover { background: rgba(47,125,255,0.2); }
.fm-item--drop-target {                 /* DEAD: no drag implementation */
  background: rgba(47,125,255,0.14) !important;
  border-color: rgba(74,158,255,0.5) !important;
  border-left: 2px solid var(--color-accent-light) !important;
}
.fm-item[draggable="true"]      { cursor: grab; }      /* DEAD */
.fm-item[draggable="true"]:active { cursor: grabbing; opacity: 0.6; }  /* DEAD */

.fm-item-icon { flex-shrink: 0; width: 16px; }
.fm-item-icon.dir { color: var(--color-accent-light); }
.fm-item-name {
  flex: 1; font-size: 12px; color: var(--color-text-secondary);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  user-select: none; -webkit-user-select: none;
}
.fm-item--selected .fm-item-name { color: #e0ecff; }
.fm-item:has(.fm-item-icon.dir) .fm-item-name { color: var(--color-accent-light); font-weight: 500; }
.fm-item--selected:has(.fm-item-icon.dir) .fm-item-name { color: #b9d7ff; }
.fm-item-size {
  font-size: var(--text-xs); color: var(--color-text-muted); flex-shrink: 0;
  min-width: 38px; text-align: right; font-family: var(--font-mono, monospace);
}
.fm-item--selected .fm-item-size { color: var(--color-text-muted); }

.fm-list *::selection        { background: transparent; }
.fm-list *::-moz-selection   { background: transparent; }
```

`[measured]` row height **27 px**; icon box 16 px wide (glyph 14 px); size cell ≥38 px, right-aligned.

### 2.10 Status states

```css
.fm-status         { padding: 18px 10px; text-align: center; font-size: 12px; color: var(--color-text-secondary); }
.fm-status.error   { color: var(--color-error); }
.fm-status.muted   { color: var(--color-text-muted); }
.fm-status.loading { display: flex; justify-content: center; align-items: center; padding: 40px 10px; margin: 3px; }
.fm-spinner {
  width: 20px; height: 20px;
  border: 2px solid rgba(58,132,255,0.15);
  border-top-color: rgba(58,132,255,0.85);
  border-radius: 50%;
  animation: fm-spin 0.8s linear infinite;
}
@keyframes fm-spin { to { transform: rotate(360deg); } }
```

* Loading → 20 px spinner centred in a 40 px-tall block.
* Error → the raw `String(e)` from the failed IPC call, red.
* Empty (`files.length === 0` after hidden filtering) → the literal string `空目录`.
* `.fm-loading` (root-level spinner with 14 px gap) exists in CSS but is never rendered — dead rule.

### 2.11 Icon button (shared)

```css
.fm-icon-btn {
  display: flex; align-items: center; justify-content: center;
  width: 22px; height: 22px; border-radius: var(--radius-xs);
  background: transparent; border: none; color: var(--color-text-muted);
  cursor: pointer; transition: background 0.12s, color 0.12s; flex-shrink: 0;
}
.fm-icon-btn:hover { background: rgba(58,132,255,0.2); color: var(--color-text-primary); }
.fm-icon-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.fm-icon-btn:disabled:hover { background: transparent; color: var(--color-text-muted); }
```

### 2.12 Context menu

```css
.fm-ctx-menu {
  min-width: 120px; padding: 3px;
  border-radius: var(--radius-md);
  box-shadow: var(--glass-hi), var(--shadow-pop);
}
.fm-ctx-item {
  display: flex; align-items: center; gap: 6px; padding: 4px 10px;
  font-size: var(--text-xs); color: var(--color-text-primary);
  border-radius: var(--radius-sm); cursor: pointer;
  transition: background 0.1s; user-select: none;
}
.fm-ctx-item:hover        { background: rgba(58,132,255,0.2); }
.fm-ctx-item.danger       { color: var(--color-error); }
.fm-ctx-item.danger:hover { background: rgba(220,50,50,0.15); }
.fm-ctx-divider           { height: 1px; background: var(--color-border); margin: 2px 2px; }
```

Positioned `position: fixed; top: position.y; left: position.x; z-index: 2000`, rendered through
`createPortal(..., document.body)`, and also carries `.glass-panel` (translucent, blurred, 16 px
radius, border).

### 2.13 Dialogs

`ConfirmDialog` (used for upload/download/delete prompts and the per-item conflict prompt):

```tsx
createPortal(
  <div className="modal-overlay" style={{ zIndex: 3000 }}>
    <div className="cf-shell" onClick={e => e.stopPropagation()}>
      <div className="cm-header"><div className="cm-header-left"><span>{title}</span></div></div>
      <div className="cf-body">
        <p style={{ margin: 0, color: 'var(--color-text-primary)', whiteSpace: 'pre-line' }}>{message}</p>
      </div>
      <div className="cf-footer">
        <div className="cf-footer-group">
          {resolvedActions.map(a => (
            <button key={a.label} className={a.variant === 'ghost' ? 'btn-ghost' : 'btn-primary'}
                    onClick={a.onClick}>{a.label}</button>
          ))}
        </div>
      </div>
    </div>
  </div>, document.body)
```

* `whiteSpace: 'pre-line'` — **`\n` in messages renders as a line break**; the port must render
  multi-line message text literally.
* If no actions are supplied the dialog falls back to a single `取消` ghost button.
* Reused styles from `CredentialsModal.css` / `AppDialogHost.css`:
  `.modal-overlay` = fixed inset 0, `rgba(2,6,14,0.72)`, `backdrop-filter: blur(5px)`, centred;
  `.cf-shell` = 460 px wide, `var(--color-bg-card)` background, 16 px radius, max-height 82vh;
  `.cm-header` = 18px 20px 16px padding, 15 px/700 title, bottom border;
  `.cf-body` = 20 px padding, 16 px gap; `.cf-footer` = 14px 20px, right-aligned, top border;
  `.btn-primary` = `var(--color-accent)` background, 8px 20px padding, 13 px/600;
  `.btn-ghost` = transparent, 1 px `var(--color-border)`, 7px 16px padding.

The **inline rename/new-folder modal** uses the same shell but `zIndex: 2100` and a form field:

```tsx
<div className="modal-overlay" style={{ zIndex: 2100 }}>
  <div className="cf-shell" onClick={e => e.stopPropagation()}>
    <div className="cm-header"><div className="cm-header-left">
      <span>{inlineAction.type === 'rename'
              ? `重命名${inlineAction.file.is_dir ? '文件夹' : '文件'}`
              : '新建文件夹'}</span>
    </div></div>
    <div className="cf-body">
      <div className="cf-field full">
        <label className="cf-label">{inlineAction.type === 'rename' ? '名称' : '文件夹名称'}</label>
        <input className="form-input" value={inlineAction.value} autoFocus autoComplete="off"
               autoCapitalize="off" autoCorrect="off" spellCheck={false}
               onChange={…} onKeyDown={handleInlineActionKeyDown} />
      </div>
    </div>
    <div className="cf-footer"><div className="app-dialog-btn-group">
      <button className="btn-ghost" onClick={cancelInlineAction}>取消</button>
      <button className="btn-primary" onClick={submitInlineAction}>
        {inlineAction.type === 'rename' ? '确定重命名' : '创建'}
      </button>
    </div></div>
  </div>
</div>
```

The **store-level** dialogs used by the File Manager (`openConfirmDialog` / `openAlertDialog`) are
rendered by the global `AppDialogHost` (`.app-dialog-overlay` / `.app-dialog-shell`), not by
`FileManager.tsx`. Defaults: confirm → `confirmText = '确认'`, `cancelText = '取消'`;
alert → `confirmText = '知道了'`. Overlay click = cancel (confirm) / confirm (alert).

### 2.14 Design tokens (exact values)

```css
:root {
  --app-zoom: 0.8;                            /* only feeds font-size: calc(14px * var(--app-zoom)) */
  --color-bg-primary:   #07162d;
  --color-bg-secondary: #0b1f3d;
  --color-bg-panel:     rgba(7, 22, 43, 0.86);
  --color-bg-card:      #0c1f38;
  --color-bg-input:     rgba(6, 18, 36, 0.82);
  --color-terminal-bg:  #050b14;

  --color-accent:       #2f7dff;
  --color-accent-hover: #6ab5ff;
  --color-accent-active:#1c5fcc;
  --color-accent-2:     #57d8b2;
  --color-accent-light: #80b0ff;

  --color-text-primary:   #e7eff9;
  --color-text-secondary: #a8bdd1;
  --color-text-muted:     #7d93a9;

  --color-border:        rgba(58, 132, 255, 0.3);
  --color-border-active: rgba(112, 191, 255, 0.6);

  --color-success: #57e3a5;
  --color-error:   #e0575c;
  --color-warning: #f0a040;
  --color-terminal-text: #d7e3f0;

  --font-mono: 'Menlo', 'Monaco', 'Courier New', monospace;
  --font-sans: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;

  --radius-xs: 4px;  --radius-sm: 8px;  --radius-md: 12px;
  --radius-lg: 16px; --radius-pill: 999px;

  --text-xs: 12px; --text-sm: 13px; --text-md: 14px; --text-lg: 16px; --text-xl: 20px;

  --glass-blur: 12px; --glass-blur-strong: 18px;
  --glass-hi: inset 0 1px 0 rgba(255, 255, 255, 0.06);
  --shadow-panel: 0 4px 24px rgba(0, 0, 0, 0.4);
  --shadow-pop:   0 18px 48px rgba(0, 0, 0, 0.55);
  --glow-accent:  0 0 12px rgba(72, 161, 255, 0.4);
}
```

Extra literals used by the feature that are **not** tokens: `#e53935` (error border),
`#FFD700` (queue border), `#e0ecff` / `#b9d7ff` (selected names), `rgba(220,50,50,0.8)` (error
progress fill), `rgba(47,125,255,0.08/0.14/0.2)` (row hover/selection).

### 2.15 File-type colour map (icons)

```ts
function getFileColor(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    txt: '#a0c0e0', md: '#a0c0e0', json: '#f5c842', js: '#f5c842',
    ts: '#4fc3f7', tsx: '#4fc3f7', jsx: '#4fc3f7', py: '#4caf8a',
    rs: '#f4732a', go: '#00bcd4', sh: '#70a0ff', bash: '#70a0ff',
    png: '#e57373', jpg: '#e57373', jpeg: '#e57373', gif: '#e57373',
    svg: '#ffb74d', zip: '#70a0ff', tar: '#70a0ff', gz: '#70a0ff',
    pdf: '#ef5350', html: '#ff8a65', css: '#42a5f5',
  }
  return map[ext] ?? '#7a7a9a'
}
```

Icon rules: directories → lucide `Folder`, `size={14}`, `strokeWidth={1.6}`, class
`fm-item-icon dir` (colour from `--color-accent-light`). Files → lucide `File`, same size/stroke,
inline `color: getFileColor(name)`.

### 2.16 Size formatting

```ts
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`
}
```

Binary divisors with SI labels; `B` has no decimals, `KB`/`MB` one decimal, `GB` two. Only rendered
for **files** (directories show no size). `modified`, `permissions` and `owner` are returned by the
backend but **never displayed** anywhere in the feature.

---

## 3. State model

### 3.1 Component state (exhaustive)

| # | Declaration | Type / initial | Semantics |
|---|---|---|---|
| 1 | `localFiles` | `useState<FileInfo[]>([])` | raw listing of the local dir (unfiltered by hidden) |
| 2 | `localPath` | `useState(session.localPath \|\| '')` | local panel cwd; empty string until first load |
| 3 | `localLoading` | `useState(false)` | local panel shows spinner |
| 4 | `localError` | `useState<string>()` | `String(e)` of the last failed `list_local_dir` |
| 5 | `localDeleting` | `useState(false)` | local panel disabled + overlay `删除中...` |
| 6 | `remoteFiles` | `useState<FileInfo[]>([])` | raw remote listing |
| 7 | `remotePath` | `useState(session.remotePath \|\| '/')` | remote panel cwd (local to the component; only `get_remote_cwd` sync writes back to the store) |
| 8 | `remoteLoading` | `useState(false)` | remote panel spinner |
| 9 | `remoteError` | `useState<string>()` | `String(e)` of the last failed `list_remote_dir` |
| 10 | `remoteDeleting` | `useState(false)` | remote panel disabled + overlay |
| 11 | `showLocalHidden` | `useState(false)` | local dot-file visibility |
| 12 | `showRemoteHidden` | `useState(false)` | remote dot-file visibility |
| 13 | `selectedLocalPaths` | `useState<string[]>([])` | ordered list of selected **paths** (not indices) |
| 14 | `selectedRemotePaths` | `useState<string[]>([])` | same for remote |
| 15 | `lastSelectedLocalPath` | `useState<string \| null>(null)` | shift-range anchor for the local panel |
| 16 | `lastSelectedRemotePath` | `useState<string \| null>(null)` | shift-range anchor for the remote panel |
| 17 | `ctxMenu` | `useState<CtxMenu \| null>(null)` | `{ x, y, file, side }`, zoom-normalised coordinates |
| 18 | `inlineAction` | `useState<InlineAction \| null>(null)` | `{type:'rename', side, file, value}` or `{type:'new-folder', side, value}` |
| 19 | `confirmDialog` | `useState<ConfirmState \| null>(null)` | `{ title, message, actions[] }` for the in-component `ConfirmDialog` |
| 20 | `transferConflict` | `useState<TransferConflictState \| null>(null)` | pending per-item conflict: `{transferId, direction, fileName, targetPath, remainingPaths, applyToAll}` |
| 21 | `remoteTarSupport` | `useState<boolean \| null>(null)` | `null` = unknown; `true/false` = probe result; reset when `session.sessionId` changes |
| 22 | `remoteTarChecking` | `useState(false)` | probe in flight (prevents duplicate probes) |
| 23 | `prevFmOpenRef` | `useRef<boolean>(!!session.fmOpen)` | previous `fmOpen` for rising-edge detection |
| — | `Panel` internal: `editingPath` | `useState(false)` | path bar in edit mode |
| — | `Panel` internal: `pathInput` | `useState(currentPath)` | draft text; re-synced from `currentPath` whenever not editing |
| — | `Panel` internal: `inputRef` | `useRef<HTMLInputElement>(null)` | assigned but **never used** |
| — | `ContextMenu` internal: `position` | `useState({x,y})` | clamped position |
| — | `ContextMenu` internal: `ref` | `useRef<HTMLDivElement>` | used for outside-click detection and clamping |

Store slices consumed:

```ts
const allTransfers      = useStore(s => s.transfers)
const transfers         = useMemo(() => allTransfers.filter(t => t.session_id === session.id),
                                  [allTransfers, session.id])
const updateTransfer    = useStore(s => s.updateTransfer)
const toggleFm          = useStore(s => s.toggleFm)
const updateSessionPath = useStore(s => s.updateSessionPath)
const openConfirmDialog = useStore(s => s.openConfirmDialog)
const openAlertDialog   = useStore(s => s.openAlertDialog)
const collapsed         = !session.fmOpen
```

* `transfers` is filtered by **`session.id`** (the frontend session-tab id), *not* the backend
  `sessionId`. Every transfer record the File Manager creates sets `session_id: session.id`.
* `useStore.getState().transfers` is read synchronously inside loops (to check cancellation and to
  read the current batch record) — the port must have a synchronous read of the transfer registry.

### 3.2 Derived values

```ts
const selectedLocalPathSet  = useMemo(() => new Set(selectedLocalPaths),  [selectedLocalPaths])
const selectedRemotePathSet = useMemo(() => new Set(selectedRemotePaths), [selectedRemotePaths])

const visibleLocalFiles  = useMemo(
  () => showLocalHidden  ? localFiles  : localFiles.filter(f  => !f.name.startsWith('.')),
  [localFiles, showLocalHidden])
const visibleRemoteFiles = useMemo(
  () => showRemoteHidden ? remoteFiles : remoteFiles.filter(f => !f.name.startsWith('.')),
  [remoteFiles, showRemoteHidden])

const activeTransfers  = transfers.filter(t => t.status !== 'done')            // collapse-bar badge
const uploadBusy       = transfers.some(t => t.direction === 'upload'
                       && (t.status === 'pending' || t.status === 'transferring' || t.status === 'conflict'))
const downloadBusy     = transfers.some(t => t.direction === 'download'
                       && (t.status === 'pending' || t.status === 'transferring' || t.status === 'conflict'))

const selectedLocalTransferItems = visibleLocalFiles
  .filter(item => selectedLocalPathSet.has(item.path))       // files AND directories
const selectedRemoteItems        = visibleRemoteFiles
  .filter(item => selectedRemotePathSet.has(item.path))      // files AND directories
```

`[quirk]` Selection counts shown on the divider buttons use `selectedLocalPaths.length` /
`selectedRemotePaths.length` (the raw arrays) while the actual transfer uses the **visible-file
intersection**. After a directory change the arrays are pruned, so the two normally agree; a stale
array entry could otherwise make the badge disagree with the payload.

### 3.3 Transfer record model

```ts
interface TransferProgress {
  id: string
  file_name: string
  direction: 'upload' | 'download'
  total: number
  transferred: number
  transferred_bytes?: number          // set only by the frontend for folder records (always 0)
  status: 'pending' | 'transferring' | 'done' | 'error' | 'conflict'
  error?: string
  target_path?: string
  conflict_path?: string
  conflict_is_dir?: boolean
  session_id?: string                 // frontend session-tab id
  group_id?: string                   // batch group id (also equals the parent record's id)
}
```

Store upsert semantics (`updateTransfer`): key = `progress.id || \`${direction}:${file_name}\``;
if a record with that key exists it is **shallow-merged** (`{...existing, ...progress, id: key}`),
otherwise appended. Omitting a field in a later update leaves the old value intact.

**Transfer-id conventions (critical):**

| record | id |
|---|---|
| single file upload | `` `upload:${remoteTargetPath}` `` |
| single file download | `` `download:${localTargetPath}` `` |
| batch group parent | `` `batch-upload:${session.id}:${Date.now()}` `` / `` `batch-download:${session.id}:${Date.now()}` `` |
| folder (tar) upload/download | same as a single file: `` `upload:${remoteTarget}` `` / `` `download:${localTarget}` `` |
| folder (fallback) upload/download | same id, reused for every inner file of that folder |

`transferId` is also the id sent to the backend (`transferId` argument) so the backend's
`transfer-progress` events match the row.

---

## 4. Lifecycle and data loading

### 4.1 Opening (rising edge) — exact sequence

`useLayoutEffect` primes loading state **before paint** on the `false → true` edge of `fmOpen`:

```ts
useLayoutEffect(() => {
  if (!!session.fmOpen && !prevFmOpenRef.current) {
    setLocalLoading(true)
    if (session.sessionId && session.status === 'connected') setRemoteLoading(true)
  }
}, [session.fmOpen, session.sessionId, session.status])
```

The loading effect runs on the **rising edge only** (`prevFmOpenRef` is updated inside it, and the
layout effect has not yet mutated it when the load effect runs):

```ts
useEffect(() => {
  const isOpen = !!session.fmOpen
  const wasOpen = prevFmOpenRef.current
  prevFmOpenRef.current = isOpen
  if (!isOpen) return
  if (wasOpen) return                       // [quirk] a component that MOUNTS with fmOpen=true never loads
  const timeoutId = setTimeout(() => { /* … see below … */ }, 50)   // force the spinner to paint
  return () => clearTimeout(timeoutId)
}, [session.fmOpen])
```

After the 50 ms delay:

1. **Local**: `localPath ? loadLocal(localPath) : homeDir() → loadLocal(home)`, with
   `catch(() => loadLocal('/'))`.
2. **Remote** (only when `session.sessionId && session.status === 'connected'`):
   * *Phase 1 (instant)*: `knownPath = session.terminalPath || remotePath || '/'`;
     `loadRemote(knownPath)`.
   * *Phase 2 (background)*: `invoke<string>('get_remote_cwd', { sessionId })` →
     `updateSessionPath(bookmarkTabId, session.id, realCwd)` (which sets **both** `terminalPath` and
     `remotePath` on the session) → if `realCwd !== knownPath`, `loadRemote(realCwd)`.
     Any rejection is swallowed (`/* non-Linux or exec failed — phase 1 result is fine */`).
3. `Promise.allSettled([localPromise, remotePromise])`.

`[quirk]` Phase 2's `updateSessionPath` mutates `session.terminalPath`, which retriggers the
live-follow effect (§4.5) and can issue a **third** `loadRemote` for the same path. Harmless but
visible as repeated spinners/requests.

### 4.2 `loadLocal(path)`

```ts
if (!path) return
setLocalLoading(true); setLocalError(undefined)
try {
  const files = await invoke<FileInfo[]>('list_local_dir', { path })
  setLocalFiles(files)
  setLocalPath(path)
  setSelectedLocalPaths(prev => prev.filter(p => files.some(f => f.path === p)))  // prune stale
} catch (e) { setLocalError(String(e)) }
finally { setLocalLoading(false) }
```

### 4.3 `loadRemote(path)` and the missing-path walk-up

```ts
if (!session.sessionId) return
setRemoteLoading(true); setRemoteError(undefined)
try {
  const files = await invoke<FileInfo[]>('list_remote_dir', { sessionId: session.sessionId, path })
  setRemoteFiles(files); setRemotePath(path)
  setSelectedRemotePaths(prev => prev.filter(p => files.some(f => f.path === p)))
} catch (e) {
  const errMsg = String(e)
  const isNoSuchFile = /no such file|SFTP\(2\)/i.test(errMsg)
  if (isNoSuchFile && path !== '/') {
    const parent = path.replace(/\/[^/]+\/?$/, '') || '/'
    if (parent !== path) { setRemoteLoading(false); return loadRemote(parent) }   // walk up recursively
    try {
      const home = await invoke<string>('get_remote_cwd', { sessionId: session.sessionId })
      if (home && home !== path) { setRemoteLoading(false); return loadRemote(home) }
    } catch { /* ignore */ }
    if (path !== '/') { setRemoteLoading(false); return loadRemote('/') }         // last resort
  }
  setRemoteError(errMsg)
} finally { setRemoteLoading(false) }
```

Recovery ladder for a vanished remote cwd: **parent dir → (recursively) its parent → remote `$HOME`
via `get_remote_cwd` → `/`**. The regex matches libssh2 messages such as
`readdir failed: SFTP(2) no such file` and English `No such file`. Only when everything fails does
`remoteError` get set (and the panel shows it in red).

### 4.4 Live-follow of the terminal cwd

```ts
useEffect(() => {
  if (!session.terminalPath || collapsed || !session.sessionId) return
  if (session.terminalPath !== remotePath) loadRemote(session.terminalPath)
}, [session.terminalPath])          // ← dependency is terminalPath ONLY
```

* Triggered whenever the terminal reports a new cwd while the File Manager is open.
* `terminalPath` is written by the store action `updateSessionPath(bookmarkTabId, sessionId, path)`,
  which sets **both** `terminalPath` and `remotePath`.
* Because the dependency array omits `remotePath`/`collapsed`, the check runs only when
  `terminalPath` changes. `[quirk]` A manual navigation in the remote panel does **not** write back
  to the store, so `session.remotePath`/`terminalPath` can disagree with the panel until the next
  `cd`.

### 4.5 `tar` capability probe

```ts
const ensureRemoteTarSupport = useCallback(async (): Promise<boolean> => {
  if (!session.sessionId) return false
  if (remoteTarSupport !== null) return remoteTarSupport          // cached
  setRemoteTarChecking(true)
  try {
    const output = await invoke<string>('execute_remote_command', {
      sessionId: session.sessionId,
      command: "command -v tar >/dev/null 2>&1 && printf '__TINYTERM_TAR_OK__' || true",
    })
    const supported = output.includes('__TINYTERM_TAR_OK__')
    setRemoteTarSupport(supported)
    return supported
  } catch { setRemoteTarSupport(false); return false }
  finally { setRemoteTarChecking(false) }
}, [session.sessionId, remoteTarSupport])
```

* Reset to `null` whenever `session.sessionId` changes.
* Probed eagerly on open:

```ts
useEffect(() => {
  if (!session.fmOpen || !session.sessionId || session.status !== 'connected') return
  if (remoteTarSupport !== null || remoteTarChecking) return
  ensureRemoteTarSupport().catch(() => {})
}, [session.fmOpen, session.sessionId, session.status, remoteTarSupport, remoteTarChecking, ensureRemoteTarSupport])
```

* `ensureRemoteTarSupport()` is awaited at the start of every `doUpload`/`doDownload`, so a first
  directory transfer on a slow host blocks on the probe.

### 4.6 Navigation helpers

```ts
const goLocalUp = async () => {
  const parts = localPath.replace(/\/$/, '').split('/')
  setLocalLoading(true)
  await new Promise(resolve => setTimeout(resolve, 50))     // force spinner to paint
  loadLocal(parts.slice(0, -1).join('/') || '/')
}
// goRemoteUp: identical against remotePath / loadRemote

const joinPath = (dir: string, name: string) => `${dir.replace(/\/$/, '')}/${name}`
const shellQuote = (value: string) => `'${value.replace(/'/g, `'"'"'`)}'`
```

`joinPath` collapses exactly one trailing slash of `dir`; `shellQuote` is the POSIX single-quote
escape (`'` → `'"'"'`).

### 4.7 Refresh triggers (complete list)

| trigger | what reloads |
|---|---|
| File Manager opened (rising edge) | local (+ remote if connected) |
| `session.terminalPath` changed while open | remote |
| refresh button (`title="刷新"`) | that panel: `loadLocal(localPath)` / `loadRemote(remotePath)` |
| double-click a directory | that panel |
| path bar commit (only if text changed) | that panel |
| up button | that panel |
| hidden-files toggle | nothing (client-side filter only) |
| after upload completes | remote panel → target dir |
| after download completes | local panel → target dir |
| after delete | that panel (current dir) |
| after rename | that panel (current dir) |
| after new folder | that panel (current dir) |
| after conflict resolution (`跳过`/`覆盖`) | panel of the target side |

There is **no polling, no `fs` watcher, no timer-based refresh** anywhere in the feature.

---

## 5. Panel behaviours (identical for local and remote)

`PanelProps`:

```ts
interface PanelProps {
  side: 'local' | 'remote'
  title: string                                   // '本地' | '远程'
  icon: React.ReactNode                           // <Monitor size={13} strokeWidth={1.8}/> | <Server …/>
  files: FileInfo[]                               // already hidden-filtered
  currentPath: string
  loading: boolean
  error?: string
  selectedPaths: Set<string>
  onSelectionChange: (file: FileInfo, mode: 'single' | 'toggle' | 'range') => void
  onNavigate: (path: string) => void
  onGoUp: () => void
  onToggleHidden: () => void
  showHidden: boolean
  disabled?: boolean                              // = localDeleting / remoteDeleting
  busyLabel?: string                              // '删除中...'
  onRefresh: () => void
  onNewFolder: () => void
  onContextMenu: (e: React.MouseEvent, file: FileInfo) => void
  onNavigateStart?: () => void                    // sets loading true synchronously before navigation
}
```

### 5.1 Header actions

| control | title attr | enabled | effect |
|---|---|---|---|
| eye | `showHidden ? '隐藏隐藏文件' : '显示隐藏文件'` | not `disabled` | flips the per-panel hidden flag; icon is `Eye` when shown, `EyeOff` when hidden |
| refresh | `刷新` | not `disabled` | `loadLocal(localPath)` / `loadRemote(remotePath)` |
| new folder | `新建文件夹` | not `disabled` | `handleNewFolder(side)` → inline modal |

### 5.2 Path bar

* Display mode: `.fm-path-display` shows `currentPath` verbatim, `title={currentPath}`, mono font,
  ellipsis, `cursor: text`.
* Click → if `disabled`, ignore; else `editingPath = true`, `pathInput = currentPath`, input
  autofocused.
* `commitPath()` (async, used by **both** `Enter` and `blur`):

```ts
const commitPath = async () => {
  setEditingPath(false)
  if (pathInput !== currentPath) {
    onNavigateStart?.()                          // show spinner immediately
    await new Promise(resolve => setTimeout(resolve, 50))
    onNavigate(pathInput)
  }
}
```

* `Escape` → `setEditingPath(false); setPathInput(currentPath)` (no navigation).
* `pathInput` is re-synced from `currentPath` in an effect whenever `editingPath` is false.
* Path is used verbatim: no `~` expansion, no normalisation, no validation. A bad path simply
  produces a load error (or triggers the remote walk-up ladder).

### 5.3 Up navigation

`goLocalUp`/`goRemoteUp` (§4.6) strip the last `/`-segment; the root case yields `'/'`. The local
panel uses POSIX semantics only (no Windows drive handling in the UI layer).

### 5.4 Sorting

Sorting is done **entirely in the backend** and is the same for both sides:

```rust
files.sort_by(|a, b| if a.is_dir == b.is_dir { a.name.cmp(&b.name) } else { b.is_dir.cmp(&a.is_dir) });
```

Directories first, then plain lexicographic (`String::cmp`, i.e. byte order — uppercase before
lowercase, `.` sorts before letters) by name. There is **no sort UI, no column headers, no
client-side re-sort, and no "modified/size" ordering**.

### 5.5 Hidden-file toggle

```ts
visibleLocalFiles = showLocalHidden ? localFiles : localFiles.filter(f => !f.name.startsWith('.'))
```

Dot-prefixed names only. Default is **hidden** for both panels on every mount. The global setting
`settings.show_hidden_files` is **not consulted** by the File Manager.

### 5.6 Selection model

```ts
const updateSelection = (files, target, mode, selectedPaths, setSelectedPaths, lastPath, setLastPath) => {
  if (mode === 'single') { setSelectedPaths([target.path]); setLastPath(target.path); return }

  if (mode === 'toggle') {
    setSelectedPaths(prev => prev.includes(target.path)
      ? prev.filter(p => p !== target.path) : [...prev, target.path])
    setLastPath(target.path)
    return
  }

  // range (shift)
  const anchorPath = lastPath ?? selectedPaths[selectedPaths.length - 1] ?? target.path
  const anchorIndex = files.findIndex(f => f.path === anchorPath)
  const targetIndex = files.findIndex(f => f.path === target.path)
  if (anchorIndex === -1 || targetIndex === -1) { setSelectedPaths([target.path]); setLastPath(target.path); return }
  const [start, end] = anchorIndex < targetIndex ? [anchorIndex, targetIndex] : [targetIndex, anchorIndex]
  setSelectedPaths(files.slice(start, end + 1).map(f => f.path))
  setLastPath(target.path)
}
```

* `files` is always the **visible (hidden-filtered)** array, so shift-ranges span visible rows only.
* Range selection **replaces** the selection (it does not union with previous ranges).
* The anchor is updated on every single/toggle/range click.
* Selection is keyed by absolute path; after a reload, entries whose path no longer exists are
  pruned (`setSelectedX(prev => prev.filter(p => files.some(f => f.path === p)))`).
* There is no "select all", no ctrl+A, no rubber-band selection, and no deselect-on-empty-click
  (clicking a row always selects something).

### 5.7 Row rendering

* `key={file.path}` (paths must be unique per listing).
* Name always visible; size only for files.
* Rows are `user-select: none`; text selection inside the list is disabled globally
  (`::selection { background: transparent }`).
* No tooltips on rows (the `title` attribute is absent), no inline editing, no drag handle.

### 5.8 Disabled overlay

When `disabled` (i.e. `localDeleting` / `remoteDeleting`):

* panel gets `.fm-panel--disabled` (opacity 0.82);
* all header buttons, the up button, the path input and every row interaction become no-ops
  (`if (disabled) return`) and native `disabled` is set on the buttons/input;
* a right-click while disabled is swallowed (`preventDefault()` + `stopPropagation()`) so no context
  menu appears;
* `.fm-panel-overlay` covers the panel with `rgba(8,10,18,0.42)` + `blur(1px)` and a centred pill
  showing `busyLabel` (`删除中...`) — `pointer-events: all`, so it also blocks clicks.

---

## 6. Interaction matrix

### 6.1 Mouse

| gesture | target | behaviour |
|---|---|---|
| left click | collapse bar | `toggleFm(bookmarkTabId, session.id)` |
| left click | file row | `onSelectionChange(file, 'single')` → replaces selection, sets anchor |
| ctrl-click / cmd-click | file row | `onSelectionChange(file, 'toggle')` → add/remove, sets anchor |
| shift-click | file row | `onSelectionChange(file, 'range')` → contiguous range from anchor |
| double click | file row, `is_dir === true` | `onNavigateStart?.()` (spinner) → `await 50 ms` → `onNavigate(file.path)` |
| double click | file row, file | **nothing** (no open, no download) |
| right click | file row | `preventDefault()`, `stopPropagation()`, open context menu at the pointer |
| right click | panel/empty area | no File-Manager handler; the app installs a global `document.addEventListener('contextmenu', e => e.preventDefault())` **in production builds only** (`src/main.tsx`, guarded by `import.meta.env.PROD`), so the native menu is suppressed app-wide in release and appears in dev builds |
| click | path display | enter path-edit mode |
| click | upload button | `handleTransferToRemote()` |
| click | download button | `handleTransferToLocal()` |
| click | queue cancel (single row) | `handleCancelTransfer(item.id)` |
| click | queue cancel (batch row) | `items.forEach(t => onCancel(t.id))` — cancels **every** record of the group, including the parent |
| click | `.fm-root` background | closes the context menu if open |
| click | `ConfirmDialog` overlay | does **not** close (the portal shell stops propagation; only the buttons act) |
| click | inline modal overlay | does **not** close (shell stops propagation) |

Right-click coordinates are zoom-normalised:

```ts
function getNormalizedPointerPosition(e: ReactMouseEvent) {
  const zoomValue = Number(getComputedStyle(document.documentElement).zoom || '1')
  const zoom = Number.isFinite(zoomValue) && zoomValue > 0 ? zoomValue : 1
  return { x: e.clientX / zoom, y: e.clientY / zoom }
}
```

`[quirk]` Nothing in the app ever sets a CSS `zoom` on `<html>` (the app-zoom feature only sets
`--app-zoom`, used by `font-size: calc(14px * var(--app-zoom))`), so the computed value is `normal`,
`Number('normal')` is `NaN`, and `zoom` falls back to `1` — the function is currently an identity.
Keep it for parity; in egui it is a no-op.

### 6.2 Context-menu clamping

`useLayoutEffect` (runs before paint to avoid flicker):

```ts
const rect = node.getBoundingClientRect()
const margin = 4
const vw = window.innerWidth / zoom
const vh = window.innerHeight / zoom
const mw = rect.width / zoom
const mh = rect.height / zoom
const nextX = Math.min(menu.x, Math.max(margin, vw - mw - margin))
const nextY = Math.min(menu.y, Math.max(margin, vh - mh - margin))
setPosition({ x: nextX, y: nextY })
```

The menu closes on any `mousedown` outside it (`document` listener added while mounted).

### 6.3 Keyboard

There are **no global keyboard shortcuts** in the File Manager (no `Delete` key handler, no
`F2`, no `Ctrl+A`, no `Enter`-to-open, no arrow-key navigation, no focus management). The only key
handling is inside text inputs:

| context | key | action |
|---|---|---|
| path bar input | `Enter` | `commitPath()` |
| path bar input | `Escape` | cancel edit, restore `currentPath` |
| inline rename/new-folder input | `Enter` | `submitInlineAction()` (with `preventDefault()`) |
| inline rename/new-folder input | `Escape` | `cancelInlineAction()` (with `preventDefault()`) |

`Delete`/`F2` are **not** bound; deletion and rename happen only through the context menu.

### 6.4 Drag and drop

**Not implemented.** `.fm-item[draggable="true"]`, `.fm-item--drop-target` and
`.fm-panel--drop-target` exist in `FileManager.css` but no element ever receives a `draggable`
attribute and no drag event handler exists in the TSX. Treat them as dead rules; a port may
implement drag-to-transfer as an enhancement, but it is not part of the current behaviour.

---

## 7. Context menu and CRUD operations

### 7.1 Menu content (exact)

```tsx
<div className="fm-ctx-menu glass-panel" style={{position:'fixed', top, left, zIndex:2000}}>
  <div className="fm-ctx-item" onClick={() => { onRename(menu.file, menu.side); onClose() }}>
    <Pencil size={11} strokeWidth={1.8} /> 重命名
  </div>
  <div className="fm-ctx-divider" />
  <div className="fm-ctx-item danger" onClick={() => { onDelete(menu.file, menu.side); onClose() }}>
    <Trash2 size={11} strokeWidth={1.8} /> {deleteLabel}
  </div>
</div>
```

* `deleteLabel` is computed by the parent:
  `` count > 1 ? `删除 ${count} 项` : '删除' `` where
  `` count = selectedPaths.includes(ctxMenu.file.path) ? selectedPaths.length : 1 ``.
* Items are **only** `重命名` and the delete entry. There is **no** `复制路径`, `打开`, `下载`,
  `上传`, `新建文件夹`, `属性` or `重命名`-adjacent item in the row context menu. New folder exists
  only as a panel-header button. "Copy path" and "open file" do not exist anywhere in the feature.
* Icons: `Pencil` 11 px / stroke 1.8, `Trash2` 11 px / stroke 1.8.
* Item padding 4px 10px, gap 6 px, font `--text-xs` (12 px); delete item red with a red hover wash.

### 7.2 Rename

```ts
const handleRename = async (file, side) => {
  setCtxMenu(null)
  setInlineAction({ type: 'rename', side, file, value: file.name })
}
```

Submission:

```ts
const rawValue = inlineAction.value.trim()
if (!rawValue) → openAlertDialog({ title: '重命名提示', message: '请输入新名称' })   // returns without closing
if (rawValue === file.name) → setInlineAction(null); return                       // no-op
const dir = file.path.substring(0, file.path.lastIndexOf('/') + 1)
const newPath = dir + rawValue
local  → invoke('rename_local',  { oldPath: file.path, newPath })
remote → invoke('rename_remote', { sessionId: session.sessionId, oldPath: file.path, newPath })
then setInlineAction(null) + loadLocal(localPath) / loadRemote(remotePath)
catch  → openAlertDialog({ title: '重命名失败', message: String(e) })
```

* Only the **basename** is editable; the new name is appended to the original parent directory.
* `[quirk]` A `/` inside the new name creates nested paths (no validation). `..` is not blocked
  either. The modal is **not** closed on validation failure or on error.
* Dialog title: `` `重命名${file.is_dir ? '文件夹' : '文件'}` `` → `重命名文件夹` / `重命名文件`.
* Field label `名称`; buttons `取消` / `确定重命名`.

### 7.3 New folder

```ts
const handleNewFolder = async (side) => {
  if ((side === 'local' && localDeleting) || (side === 'remote' && remoteDeleting)) return
  setCtxMenu(null)
  setInlineAction({ type: 'new-folder', side, value: '' })
}
```

Submission: `rawValue = value.trim()`; empty → `openAlertDialog({title:'新建目录提示', message:'请输入文件夹名称'})`;
otherwise:

```ts
local  → invoke('create_local_dir',  { path: joinPath(localPath, rawValue) })
remote → invoke('create_remote_dir', { sessionId, path: joinPath(remotePath, rawValue) })
then setInlineAction(null) + reload the panel
catch  → openAlertDialog({ title: '创建失败', message: String(e) })
```

Dialog title `新建文件夹`, label `文件夹名称`, buttons `取消` / `创建`.

### 7.4 Delete

```ts
const handleDelete = async (file, side) => {
  if (side === 'local' ? localDeleting : remoteDeleting) return

  const selectedPaths = side === 'local' ? selectedLocalPaths : selectedRemotePaths
  const visibleFiles  = side === 'local' ? visibleLocalFiles  : visibleRemoteFiles
  const selectedSet   = new Set(selectedPaths)
  const selectedItems = selectedSet.has(file.path)
    ? visibleFiles.filter(item => selectedSet.has(item.path))   // whole (visible) selection
    : [file]                                                    // only the right-clicked row

  const label = selectedItems.length === 1 ? `"${selectedItems[0].name}"` : `${selectedItems.length} 项`

  const confirmed = await openConfirmDialog({
    title: '删除确认',
    message: `确认删除 ${label} ?`,
    confirmText: '删除',
    cancelText: '取消',
  })
  if (!confirmed) return
  …
}
```

* Confirm dialog text examples: `确认删除 "notes.txt" ?` / `确认删除 3 项 ?` (note the space before `?`).
* Execution sets the panel's `deleting` flag (disables the panel and shows `删除中...`), then:
  * local: `await invoke('delete_local', { path: item.path, isDir: item.is_dir })` sequentially;
  * remote: `waitForRemoteDelete(item.path, item.is_dir)` per item — registers a
    `remote-delete-status` listener, then calls `delete_remote_async`, and resolves/rejects when a
    payload with **exactly matching `path` and `is_dir`** arrives:

```ts
const waitForRemoteDelete = (path: string, isDir: boolean) => new Promise<void>((resolve, reject) => {
  let settled = false, unlistenFn: null | (() => void) = null
  const finish = (handler: () => void) => { if (settled) return; settled = true; if (unlistenFn) unlistenFn(); handler() }
  const unlistenPromise = listen<RemoteDeleteStatus>('remote-delete-status', event => {
    const p = event.payload
    if (p.path !== path || p.is_dir !== isDir) return
    if (p.success) finish(() => resolve()); else finish(() => reject(new Error(p.error || '远端删除失败')))
  })
  unlistenPromise.then(u => { unlistenFn = u }).catch(e => finish(() => reject(e)))
  invoke('delete_remote_async', { sessionId: session.sessionId, path, isDir })
    .catch(e => finish(() => reject(e)))
})
```

* On success: clear that side's selection and anchor, then reload the current directory.
* On any error: `openAlertDialog({ title: '删除失败', message: String(e) })`; the panel stays where
  it was (no reload).
* `finally` clears the `deleting` flag.
* **Guards (backend, §10.2)**: deleting the filesystem root, the local home directory, remote `/`
  or the remote `$HOME` is refused with an error string that is surfaced verbatim in the
  `删除失败` alert.
* Deletion is **sequential**, not parallel, and there is no per-item progress indicator.

### 7.5 Operations that do **not** exist

Explicitly absent (do not invent them in the port unless intended as new features):
copy/move, cut/paste, duplicate, "open with", "open in terminal", "copy path", "properties",
permissions editing, symlink creation, multi-select via drag, search/filter, breadcrumb segments,
column headers, file preview, upload-by-drag, download-to-desktop shortcut.

---

## 8. Transfer flows

### 8.1 Divider buttons

```tsx
<button className={`fm-transfer-btn${uploadBusy ? ' is-loading' : ''}${selectedLocalPaths.length > 0 ? ' is-active' : ''}`}
        onClick={handleTransferToRemote}
        title={uploadBusy ? '上传中...'
             : selectedLocalPaths.length > 0 ? `上传 ${selectedLocalPaths.length} 项到远程`
             : '上传选中文件到远程当前目录'}
        type="button"
        disabled={localDeleting || remoteDeleting || uploadBusy}>
  {uploadBusy ? <span className="fm-transfer-spinner" />
              : <><ArrowRight size={12} strokeWidth={2} className="fm-divider-icon" />
                  {selectedLocalPaths.length > 0 &&
                   <span className="fm-transfer-badge fm-transfer-badge--left">{selectedLocalPaths.length}</span>}</>}
</button>
```

The download button is symmetric (`ArrowLeft`, `下载中...`, `下载 ${n} 项到本地`,
`下载选中文件到本地当前目录`, badge `--right`, `downloadBusy`, `selectedRemotePaths`).

* A button is disabled when **either** panel is deleting **or** that direction is busy
  (`pending`/`transferring`/`conflict`). This means a stuck conflict row blocks that direction until
  the conflict dialog is resolved.
* `is-loading` (spinner) replaces the arrow and the badge entirely while busy.

### 8.2 Pre-flight conflict detection (client-side, name-based)

`handleTransferToRemote` (download is symmetric against `visibleLocalFiles`):

```ts
if (localDeleting || remoteDeleting) return
if (selectedLocalTransferItems.length === 0) {
  await openAlertDialog({ title: '上传提示', message: '请先在本地面板选择要上传的文件或文件夹' }); return
}

// 1. folder-name conflicts (dir vs ANY same-named entry in the target listing)
const folderConflicts = selectedLocalTransferItems.filter(localItem =>
  localItem.is_dir && visibleRemoteFiles.some(remoteItem => remoteItem.name === localItem.name))

// 2. file-name conflicts
const fileConflicts = selectedLocalTransferItems.filter(localItem =>
  !localItem.is_dir && visibleRemoteFiles.some(remoteItem => remoteItem.name === localItem.name))
```

Exact dialogs (upload shown; download differs only where noted):

| case | title | message | buttons (label / variant) |
|---|---|---|---|
| folder conflict | `文件夹合并/覆盖确认` | `` `目标目录中已存在 ${n} 个同名文件夹（如：${names}）。\n继续上传将合并目录。若遇到同名文件，请选择处理方式：` `` | `取消` (ghost) → `setConfirmDialog(null)`; `跳过现有文件` (primary) → `doUpload(items, remotePath, false)`; `全部覆盖` (primary) → `doUpload(items, remotePath, true)` |
| file conflict | `文件覆盖确认` | `` `目标目录中已存在 ${n} 个同名文件（如：${names}）。\n请选择处理方式：` `` | `取消` (ghost); `逐个询问` (primary) → `doUpload(..., false)`; `全部覆盖` (primary) → `doUpload(..., true)` |
| no conflict | `确认上传` | `` `确定上传 ${itemCount} ${typeLabel}到远程目录？\n${names}` `` | `取消` (ghost); `开始上传` (primary) → `doUpload(..., false)` |

* `names = conflicts.map(c => c.name).join(', ')`, truncated with
  `` `${names.slice(0, 50)}${names.length > 50 ? '...' : ''}` `` for both conflict dialogs.
* For the plain confirm, `names` is truncated at 100 characters the same way.
* `typeLabel = hasFolder ? '个项目' : '个文件'` where `hasFolder = items.some(i => i.is_dir)`;
  message text is e.g. `确定上传 3 个项目到远程目录？` (space before `到`).
* Download variants: titles `文件夹合并/覆盖确认` (message says `继续下载将合并目录…`) and
  `文件覆盖确认`; plain dialog `确认下载` with
  `` `确定下载 ${itemCount} ${typeLabel}到本地目录？\n${names}` `` and the primary button `开始下载`.
* The download no-selection alert is `{ title: '下载提示', message: '请先在远程面板选择要下载的文件或文件夹' }`.
* `[quirk]` Conflict detection compares **names only against the currently visible (hidden-filtered)
  listing** of the target panel. A same-named target that is hidden, or the target panel showing a
  different directory than the transfer destination, is not detected here — it surfaces later as a
  per-item `CONFLICT:` from the backend.

### 8.3 `startTransferTask` — the single primitive

```ts
const startTransferTask = (
  direction: 'upload' | 'download',
  sourcePath: string,
  targetPath: string,
  overwrite = false,
  options?: TransferTaskOptions,
): Promise<{ transferId: string; fileName: string; conflict: boolean; error?: string }>

type TransferTaskOptions = {
  transferId?: string          // row id; default `${direction}:${targetPath}`
  displayName?: string         // row label; default basename of sourcePath
  progressTotal?: number       // stage total (default 0)
  progressStart?: number       // stage start (default 0)
  progressSpan?: number        // stage span
  displayTargetPath?: string   // shown/logged target; default targetPath
  sessionId?: string           // frontend session-tab id stored on the row
  groupId?: string             // batch group id stored on the row
}
```

Sequence:

1. Upsert the row: `{ id, file_name, direction, total: progressTotal ?? 0, transferred: progressStart ?? 0,
   status: 'pending', target_path: displayTargetPath ?? targetPath, session_id, group_id }`.
2. Register a `transfer-progress` listener; **only events whose `payload.id === transferId` are
   considered**:
   * `status === 'done'` → unlisten, resolve `{transferId, fileName, conflict:false}`;
   * `status === 'conflict'` → unlisten, resolve `{…, conflict:true}`;
   * `status === 'error'` → unlisten, resolve `{…, error: payload.error}`.
3. Invoke the backend (`[ipc]` argument names exactly as below):
   * upload: `invoke('upload_file', { sessionId, localPath: sourcePath, remotePath: targetPath, overwrite,
     transferId, displayName, progressTotal, progressStart, progressSpan, targetPathOverride: displayTargetPath })`
   * download: `invoke('download_file', { sessionId, remotePath: sourcePath, localPath: targetPath, overwrite,
     transferId, displayName, progressTotal, progressStart, progressSpan, targetPathOverride: displayTargetPath })`
   * Optional keys that are `undefined` are omitted by the serializer, so the Rust `Option<…>` args
     arrive as `None`.
4. If the invoke **rejects**: unlisten, then
   * message contains `'CONFLICT:'` → upsert `{status:'conflict', total:0, transferred:0, error: message,
     conflict_path: displayTargetPath}` and resolve `{conflict:true}`;
   * otherwise → upsert `{status:'error', total:0, transferred:0, error: message}` and resolve with `error`.
5. If the invoke **resolves**, nothing else happens here: the row's `done` state comes from the
   global `transfer-progress` listener in `App.tsx` (`updateTransfer(event.payload)`). **The feature
   therefore depends on that global listener existing.**

`waitForStageProgress(transferId, expectedProgress, start)` is used between tar stages: it listens
for `transfer-progress` with the same id, rejects on `status === 'error'` (message
`阶段任务失败` when `error` is absent), and resolves once `transferred >= expectedProgress`.

### 8.4 `doUpload(items: FileInfo[], targetRemoteDir: string, overwriteAll = false)`

```
if (!session.sessionId) return
batchGroupId = items.length > 1 ? `batch-upload:${session.id}:${Date.now()}` : undefined
completedCount = 0; hasError = false

if batchGroupId: upsert parent { id: batchGroupId, file_name: `上传 ${items.length} 项`,
                                 direction:'upload', total:items.length, transferred:0,
                                 status:'pending', target_path: targetRemoteDir, session_id: session.id,
                                 group_id: batchGroupId }

canUseTar = await ensureRemoteTarSupport()
```

**A. tar path (`canUseTar === true`)**

```ts
const { tempDir } = await import('@tauri-apps/api/path')
const localTmpDir = await tempDir()
const folderItems = items.filter(i => i.is_dir)
const fileItems   = items.filter(i => !i.is_dir)

// pre-create one row per folder so the batch group stays visible during packing
for (const folder of folderItems) {
  const remoteTarget = joinPath(targetRemoteDir, folder.name)
  updateTransfer({ id: `upload:${remoteTarget}`, file_name: folder.name, direction: 'upload',
                   total: 100, transferred: 0, transferred_bytes: 0, status: 'pending',
                   target_path: remoteTarget, session_id: session.id, group_id: batchGroupId })
}

for (const folder of folderItems) {
  const stamp       = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  const localSubTmp = joinPath(localTmpDir, `tinyterm-pack-${stamp}`)
  const tmpTarLocal = joinPath(localSubTmp, '.tinyterm-pack.tar')
  const tmpTarRemote = joinPath(targetRemoteDir, `.tinyterm-pack-${stamp}.tar`)
  const remoteTarget = joinPath(targetRemoteDir, folder.name)
  const transferId   = `upload:${remoteTarget}`
  try {
    await invoke('create_local_dir', { path: localSubTmp })

    // stage 0 → 20 : local tar
    await waitForStageProgress(transferId, 20, () => invoke('pack_local_dir', {
      sourceDir: folder.path, targetTarPath: tmpTarLocal,
      transferId, displayName: folder.name, direction: 'upload',
      progressTotal: 100, progressStart: 0, progressSpan: 20, targetPath: remoteTarget,
    }))

    // stage 20 → 80 : SFTP upload of the tarball, overwrite ALWAYS true
    const result = await startTransferTask('upload', tmpTarLocal, tmpTarRemote, true, {
      transferId, displayName: folder.name,
      progressTotal: 100, progressStart: 20, progressSpan: 60,
      displayTargetPath: remoteTarget, sessionId: session.id, groupId: batchGroupId,
    })
    if (result.error) throw new Error(result.error)

    // manual 90 % mark while the remote tar runs
    updateTransfer({ id: transferId, …, total: 100, transferred: 90, status: 'transferring' })

    if (overwriteAll) {
      try { await invoke('delete_remote', { sessionId, path: remoteTarget, isDir: true }) }
      catch (e) { console.warn('Failed to delete remote folder before unpack:', e) }
    }

    const tarCmd = overwriteAll
      ? `mkdir -p ${shellQuote(targetRemoteDir)} && tar -xf ${shellQuote(tmpTarRemote)} -C ${shellQuote(targetRemoteDir)}`
      : `mkdir -p ${shellQuote(targetRemoteDir)} && tar -k -xf ${shellQuote(tmpTarRemote)} -C ${shellQuote(targetRemoteDir)}`
    await invoke('execute_remote_command', { sessionId, command: tarCmd })

    updateTransfer({ id: transferId, …, total: 100, transferred: 100, status: 'done' })
  } catch (e) {
    hasError = true
    updateTransfer({ id: transferId, …, total: 100, transferred: 0, status: 'error', error: String(e) })
  } finally {
    await invoke('execute_remote_command', { sessionId, command: `rm -f ${shellQuote(tmpTarRemote)}` }).catch(() => {})
    await invoke('delete_local', { path: tmpTarLocal, isDir: false }).catch(() => {})
    await invoke('delete_local', { path: localSubTmp, isDir: true }).catch(() => {})
  }
  completedCount += 1
  if (batchGroupId) updateTransfer(parent, transferred: completedCount,
                                  status: completedCount >= items.length ? 'done' : 'transferring')
}

// files
if (fileItems.length > 0) {
  if (batchGroupId) {
    await runUploadQueue(fileItems.map(i => i.path), targetRemoteDir, 0, overwriteAll, batchGroupId)
    completedCount += fileItems.length
    updateTransfer(parent, transferred: completedCount, status: hasError ? 'error' : 'done',
                   error: hasError ? '部分文件传输失败' : undefined)
  } else {
    await runUploadQueue(fileItems.map(i => i.path), targetRemoteDir, 0, overwriteAll)
  }
} else {
  if (batchGroupId) updateTransfer(parent, transferred: completedCount,
                                  status: hasError ? 'error' : 'done',
                                  error: hasError ? '部分文件传输失败' : undefined)
  await loadRemote(targetRemoteDir)
}
```

Key facts:

* `tar -k -xf` (keep old files) is used when the user did **not** choose 全部覆盖; `-xf` overwrites
  after the existing target folder was `rm -rf`'d when 全部覆盖 **was** chosen.
* The tarball is uploaded with `overwrite = true` **always** (the temp name is unique, and the
  conflict check must not fire on it).
* `target_path` on the row is the **folder** (`displayTargetPath`), never the temp tar.
* Temp artefacts: local `<tempDir>/tinyterm-pack-<ts>-<rand6>/.tinyterm-pack.tar`, remote
  `<targetRemoteDir>/.tinyterm-pack-<ts>-<rand6>.tar`; all three cleanup calls are best-effort
  (`.catch(() => {})`).
* `[quirk]` The `tar -k` case reports success even when files were skipped by tar.

**B. fallback path (`canUseTar === false`)** — recursive per-file SFTP, no tar:

```ts
for (const folder of folderItems) {
  const remoteTarget = joinPath(targetRemoteDir, folder.name)
  const transferId   = `upload:${remoteTarget}`
  // pre-create row { total: 1, transferred: 0, status: 'pending' } (no transferred_bytes here)
  try {
    await invoke('create_remote_dir', { sessionId, path: remoteTarget }).catch(() => {})
    const tasks = await collectLocalUploadTasks(folder.path, remoteTarget)   // see below
    if (tasks.length === 0) {            // empty folder
      updateTransfer({ id: transferId, …, total: 1, transferred: 1, status: 'done' })
      completedCount += 1; /* update parent … continue */
      continue
    }
    for (let index = 0; index < tasks.length; index += 1) {
      const result = await startTransferTask('upload', tasks[index].localPath, tasks[index].remotePath,
        overwriteAll, { transferId, displayName: folder.name, progressTotal: tasks.length,
                        progressStart: index, displayTargetPath: remoteTarget,
                        sessionId: session.id, groupId: batchGroupId })
      if (result.error) throw new Error(result.error)
      if (result.conflict && !overwriteAll) {
        updateTransfer({ id: transferId, …, total: tasks.length, transferred: index, status: 'error',
          error: '存在同名文件，已跳过冲突项。可重试并选择全部覆盖。' })
        break
      }
    }
    if (currentRow.status !== 'error')
      updateTransfer({ id: transferId, …, total: tasks.length, transferred: tasks.length, status: 'done' })
    else hasError = true
  } catch (e) {
    hasError = true
    updateTransfer({ id: transferId, …, total: 1, transferred: 0, status: 'error', error: String(e) })
  }
  completedCount += 1; /* update parent */
}
// files: identical to the tar path (runUploadQueue with/without the group id)
```

`collectLocalUploadTasks(sourceDir, targetRemoteDir)` walks the local tree:

```ts
const entries = await invoke<FileInfo[]>('list_local_dir', { path: sourceDir })
for (const entry of entries) {
  const remoteEntryPath = joinPath(targetRemoteDir, entry.name)
  if (entry.is_dir) {
    await invoke('create_remote_dir', { sessionId, path: remoteEntryPath }).catch(() => {})
    tasks.push(...await collectLocalUploadTasks(entry.path, remoteEntryPath))
  } else {
    tasks.push({ localPath: entry.path, remotePath: remoteEntryPath })
  }
}
```

→ remote directories (including empty ones) are pre-created; only files become transfer tasks.
The **whole folder is one row** whose `total` = number of files and `transferred` = file index, so
the queue shows file-count progress for the folder.

### 8.5 `doDownload(items: FileInfo[], targetLocalDir: string, overwriteAll = false)`

Symmetric, with these differences:

* batch id prefix `batch-download:`; parent `file_name: `下载 ${items.length} 项``.
* **A. tar path**:
  ```ts
  const remoteParent = folder.path.substring(0, folder.path.lastIndexOf('/')) || '/'
  const tmpTarRemote = joinPath(remoteParent, `.tinyterm-pack-${stamp}.tar`)
  const localSubTmp  = joinPath(localTmpDir, `tinyterm-pack-${stamp}`)
  const tmpTarLocal  = joinPath(localSubTmp, '.tinyterm-pack.tar')

  updateTransfer({ id, total: 100, transferred: 10, status: 'transferring' })
  await invoke('execute_remote_command', { sessionId,
    command: `tar -cf ${shellQuote(tmpTarRemote)} -C ${shellQuote(remoteParent)} ${shellQuote(folder.name)}` })
  updateTransfer({ id, total: 100, transferred: 20, status: 'transferring' })

  const result = await startTransferTask('download', tmpTarRemote, tmpTarLocal, true, {
    transferId, displayName: folder.name, progressTotal: 100, progressStart: 20, progressSpan: 60,
    displayTargetPath: localTarget, sessionId: session.id, groupId: batchGroupId })
  if (result.error) throw new Error(result.error)

  if (overwriteAll) {
    try { await invoke('delete_local', { path: localTarget, isDir: true }) }
    catch (e) { console.warn('Failed to delete local folder before unpack:', e) }
  }

  await waitForStageProgress(transferId, 100, () => invoke('unpack_local_dir', {
    tarPath: tmpTarLocal, targetDir: targetLocalDir, overwrite: overwriteAll,
    transferId, displayName: folder.name, direction: 'download',
    progressTotal: 100, progressStart: 80, progressSpan: 20, targetPath: localTarget }))

  updateTransfer({ id, total: 100, transferred: 100, status: 'done' })
  ```
  `finally` → `rm -f <tmpTarRemote>` remotely, delete local tar + local temp dir.
* **B. fallback path**: `collectRemoteDownloadTasks(folder.path, localTarget)`:

  ```ts
  const base = sourceRemoteDir.replace(/\/$/, '') || '/'
  const files = await invoke<FileInfo[]>('scan_remote_folder', { sessionId, path: sourceRemoteDir })
  return files.map(file => {
    const relative = file.path.startsWith(`${base}/`) ? file.path.slice(base.length + 1) : file.name
    return { remotePath: file.path, localPath: joinPath(targetLocalDir, relative) }
  })
  ```
  `scan_remote_folder` returns a **flat list of files only** (directories are traversed, never
  emitted), so `[quirk]` **empty remote directories are not recreated locally** in this path.
  Each file's local parent directory is created first
  (`await invoke('create_local_dir', { path: parent }).catch(() => {})`), then the file is
  downloaded with the folder-level `transferId`/progress (`progressTotal: tasks.length`,
  `progressStart: index`).
* Post-transfer refresh: `loadLocal(targetLocalDir)` when there were no files (or from
  `runDownloadQueue` otherwise).

### 8.6 `runUploadQueue` / `runDownloadQueue` (single-file queues)

```ts
const runUploadQueue = async (localFilePaths: string[], targetRemoteDir: string,
                              startIndex = 0, overwriteAll = false, groupId?: string) => {
  if (!session.sessionId || localFilePaths.length === 0) return

  const ownGroup    = !groupId && localFilePaths.length > 1
  const batchGroupId = groupId || (ownGroup ? `batch-upload:${session.id}:${Date.now()}` : undefined)

  if (startIndex === 0) {
    if (batchGroupId && ownGroup) upsert parent { file_name: `上传 ${n} 项`, total: n, … }
    localFilePaths.forEach(p => {                       // pre-create every row as pending
      const fn = p.split('/').pop() ?? 'file'
      const target = joinPath(targetRemoteDir, fn)
      updateTransfer({ id: `upload:${target}`, file_name: fn, direction: 'upload',
                       total: 0, transferred: 0, status: 'pending', target_path: target,
                       session_id: session.id, group_id: batchGroupId })
    })
  }

  let hasError = false
  for (let index = startIndex; index < localFilePaths.length; index += 1) {
    const localFilePath = localFilePaths[index]
    const fileName      = localFilePath.split('/').pop() ?? 'file'
    const remoteTarget  = joinPath(targetRemoteDir, fileName)
    const transferId    = `upload:${remoteTarget}`

    if (useStore.getState().transfers.find(t => t.id === transferId)?.status === 'error') continue  // cancelled → skip

    const result = await startTransferTask('upload', localFilePath, remoteTarget, overwriteAll,
                                           { sessionId: session.id, groupId: batchGroupId })
    if (result.conflict) {
      hasError = true
      setTransferConflict({ transferId: result.transferId, direction: 'upload', fileName,
                            targetPath: remoteTarget, remainingPaths: localFilePaths.slice(index),
                            applyToAll: false })
      return                                    // stop the loop; the dialog resumes it
    }
    if (result.error) hasError = true

    if (ownGroup && batchGroupId) { /* update parent transferred: index+1, status: 'transferring' */ }
  }

  if (ownGroup && batchGroupId) { /* final parent: transferred = n, status: hasError ? 'error' : 'done',
                                     error: hasError ? '部分文件传输失败' : undefined */ }
  await loadRemote(targetRemoteDir)
}
```

`runDownloadQueue` is identical with `download:` ids, `localTarget`, `loadLocal(targetLocalDir)`.

* Files are transferred **sequentially**, one at a time (`await` inside the loop).
* The pre-created `pending` rows mean the batch group renders immediately with a full
  `待传` count before the first byte moves.
* `remainingPaths` includes the **conflicted item itself** (`slice(index)`), which the conflict
  handler relies on (`remainingPaths[0]` is the conflicted source).

### 8.7 Batch-group queue rendering (`TransferQueue`)

```ts
const groups = new Map<string | undefined, TransferProgress[]>()
for (const t of transfers) { const gid = t.group_id; if (!groups.has(gid)) groups.set(gid, []); groups.get(gid)!.push(t) }

const activeGroups = Array.from(groups.entries()).filter(([groupId, items]) => {
  if (!groupId) return items.some(t => t.status !== 'done')          // ungrouped bucket
  const subItems = items.filter(t => t.id !== groupId)               // exclude the parent record
  return subItems.some(t => t.status !== 'done')
})

if (activeGroups.length === 0) return null
```

Per batch group (`groupId` truthy):

```ts
const subItems     = items.filter(t => t.id !== groupId)
const transferring = subItems.find(t => t.status === 'transferring')
const done         = subItems.filter(t => t.status === 'done').length
const pending      = Math.max(0, subItems.length - done - (transferring ? 1 : 0))
const hasError     = subItems.some(t => t.status === 'error' || t.status === 'conflict')
const current      = transferring || subItems.find(t => t.status === 'pending') || subItems[0]

const dir            = current?.direction ?? 'upload'
const currentPercent = current && current.total > 0
  ? Math.min(100, Math.round((current.transferred / current.total) * 100)) : 0
const overallPercent = subItems.length > 0
  ? Math.min(100, Math.round((done / subItems.length) * 100)) : 0
const showPercent    = transferring ? currentPercent : overallPercent
const actionLabel    = dir === 'upload' ? '正在上传' : '正在下载'
```

Rendered row: region 1 = direction arrow (`ArrowRight`/`ArrowLeft` 12 px, stroke 2.5),
`{actionLabel}:`, `current?.file_name || '...'` (title = the file name), 4 px track whose width is
`showPercent%` (fill `var(--color-accent)`, or `rgba(220,50,50,0.8)` when `hasError`), and
`{showPercent}%`; region 2 = `待传` + `pending`; region 3 = `完成` + `done`; cancel button
`title="全部取消"` cancels every id in `items` (parent included).

Per ungrouped item (status ≠ `done`): direction arrow, file name (title =
`` `${file_name}\n${error}` `` when errored, else the name), track + percent, and — when
`status` is `error` or `conflict` — an error chip showing `已取消` if
`error === '用户取消' || error === 'Cancelled'`, otherwise `失败` (title = the raw error).
A cancel button is rendered **only** for `pending` / `transferring`.

`[quirk]` `error`/`conflict` rows have **no cancel button and are never auto-dismissed**; they stay
in the queue (and keep the bar badge and the direction "busy" flag for `conflict`) until the session
tab is closed. The batch parent's own `error` state (`部分文件传输失败`) is never displayed because
only sub-items are inspected.

### 8.8 Per-item conflict resolution

Dialog (rendered by `ConfirmDialog`, not the store dialog):

```
title:   检测到同名目标
message: `目标中已存在同名项：${fileName}\n${targetPath}\n\n请选择如何处理当前冲突项。`
actions: 取消剩余传输 (ghost) | 跳过当前项 (primary) | 覆盖当前项 (primary)
onCancel: setTransferConflict(null)     // same as 取消剩余传输
```

`handleConflictSkip()`:

```ts
updateTransfer({ id: transferConflict.transferId, …, total: 0, transferred: 0, status: 'error',
                 error: '已跳过', conflict_path: transferConflict.targetPath })
const remaining = transferConflict.remainingPaths.slice(1)     // drop the conflicted item
const { direction, targetPath } = transferConflict
setTransferConflict(null)
if (remaining.length === 0) { direction === 'upload' ? await loadRemote(remotePath)
                                                       : await loadLocal(localPath); return }
direction === 'upload' ? await runUploadQueue(remaining, remotePath)
                       : await runDownloadQueue(remaining, localPath)
if (targetPath) { direction === 'upload' ? await loadRemote(remotePath) : await loadLocal(localPath) }
```

`handleConflictOverwrite()`:

```ts
const { direction, remainingPaths, targetPath } = transferConflict
const sourcePath = remainingPaths[0]            // the conflicted source
const remaining  = remainingPaths.slice(1)
setTransferConflict(null)

await startTransferTask(direction, sourcePath, targetPath, true, { sessionId: session.id })  // overwrite
if (remaining.length === 0) { direction === 'upload' ? await loadRemote(remotePath)
                                                       : await loadLocal(localPath); return }
direction === 'upload' ? await runUploadQueue(remaining, remotePath)
                       : await runDownloadQueue(remaining, localPath)
```

Important semantics of the resume:

* The remaining queue is re-run against the **current panel path** (`remotePath` / `localPath`), not
  the original destination captured at dialog-open time.
* No `groupId` is passed on resume, so a **new batch group id** is minted (when >1 item remains) and
  the resumed rows are re-parented to it; the old group record keeps its stale progress.
* `startIndex` defaults to 0, so the `pending` rows are pre-created again (idempotent upsert by id).
* `applyToAll` exists in the state shape but is **never set to `true`** and is never read —
  "apply to all" is realised through the pre-flight `全部覆盖` choice instead. There is no
  "apply to all remaining conflicts" option in the per-item dialog.

### 8.9 Cancellation

```ts
const handleCancelTransfer = async (transferId: string) => {
  const activeTransfer = transfers.find(t => t.id === transferId)
  if (!activeTransfer) return

  try { await invoke('cancel_transfer', { transferId }) }
  catch (e) { console.warn('Failed to cancel transfer on backend:', e) }

  updateTransfer({ …activeTransfer, status: 'error', error: '用户取消' })   // row shows 已取消

  // "点击取消，隔2秒就消失"
  setTimeout(() => {
    updateTransfer({ id: transferId, file_name, direction, total, transferred,
                     status: 'done', session_id, group_id })              // row disappears
  }, 2000)
}
```

* `cancel_transfer` only inserts the id into a cooperative flag set; a running worker stops at the
  next 32 KiB chunk and emits `status:'error', error:'Cancelled'` (the UI maps that to `已取消`).
* Cancelling a **batch parent id** is a backend no-op (no such worker) — the group row still flips to
  `已取消` and disappears after 2 s; the sub-item cancel calls are what actually stop the workers.
* Rows cancelled while still `pending` are skipped by the queue loop because of the
  `status === 'error'` check (`// Skip if cancelled`).
* `pack_local_dir` / `unpack_local_dir` cannot be cancelled (no id check in their loops), so a
  folder pack/unpack keeps running after the row disappears.
* The 2-second removal is the only automatic cleanup of a transfer row in the feature.

### 8.10 Retry

There is **no retry button and no automatic retry**. An `error` row simply persists with a `失败`
chip. Retrying means re-selecting the item and pressing the arrow again (or resolving a conflict with
`覆盖当前项`). The fallback folder-conflict message explicitly instructs the user:
`存在同名文件，已跳过冲突项。可重试并选择全部覆盖。`

### 8.11 Stage mapping summary (directory transfers)

| direction | stage | progressTotal / Start / Span | emitter |
|---|---|---|---|
| upload | local tar pack | 100 / 0 / 20 | `pack_local_dir` |
| upload | SFTP upload of tar | 100 / 20 / 60 | `upload_file` |
| upload | remote unpack (marker) | set to 90 manually | `updateTransfer` |
| upload | done | 100 | `updateTransfer` |
| download | remote tar pack (marker) | 10 then 20 manually | `updateTransfer` |
| download | SCP download of tar | 100 / 20 / 60 | `download_file` |
| download | local unpack | 100 / 80 / 20 | `unpack_local_dir` |
| download | done | 100 | `updateTransfer` |

`pack_local_dir`/`unpack_local_dir` map per-entry progress with their own
`map_stage_progress(index+1, total_entries, start, span)` (the final entry lands at `span-1`, then an
explicit `start+span` emit), which is why `waitForStageProgress(transferId, 20, …)` / `(…, 100, …)`
are used as stage barriers. Both only ever emit `status: "transferring"` (plus a terminal
`status: "error"` on failure) — they never emit `"done"`, so the row's `done` state always comes from
the frontend's own `updateTransfer` call.

---

## 9. IPC contracts

All commands are invoked through Tauri `invoke` with **camelCase argument keys** (Tauri v2 converts
them to the Rust snake_case parameters). Optional keys that are `undefined` are omitted from the
payload and arrive as `None`.

### 9.1 Commands invoked by the File Manager

| command | JS arguments (exact keys) | returns |
|---|---|---|
| `list_local_dir` | `{ path }` | `FileInfo[]` |
| `list_remote_dir` | `{ sessionId, path }` | `FileInfo[]` |
| `scan_remote_folder` | `{ sessionId, path }` | `FileInfo[]` (flat, files only, recursive) |
| `create_local_dir` | `{ path }` | `void` |
| `create_remote_dir` | `{ sessionId, path }` | `void` |
| `rename_local` | `{ oldPath, newPath }` | `void` |
| `rename_remote` | `{ sessionId, oldPath, newPath }` | `void` |
| `delete_local` | `{ path, isDir }` | `void` |
| `delete_remote` | `{ sessionId, path, isDir }` | `void` (sync; used only before tar unpack) |
| `delete_remote_async` | `{ sessionId, path, isDir }` | `void` + `remote-delete-status` event |
| `upload_file` | `{ sessionId, localPath, remotePath, overwrite, transferId, displayName, progressTotal, progressStart, progressSpan, targetPathOverride }` | `void` |
| `download_file` | `{ sessionId, remotePath, localPath, overwrite, transferId, displayName, progressTotal, progressStart, progressSpan, targetPathOverride }` | `void` |
| `cancel_transfer` | `{ transferId }` | `void` |
| `pack_local_dir` | `{ sourceDir, targetTarPath, transferId, displayName, direction, progressTotal, progressStart, progressSpan, targetPath }` | `void` |
| `unpack_local_dir` | `{ tarPath, targetDir, overwrite, transferId, displayName, direction, progressTotal, progressStart, progressSpan, targetPath }` | `void` |
| `execute_remote_command` | `{ sessionId, command }` | `string` (stdout; non-zero exit → `Err("Command failed with exit code {n}: {stdout}, {stderr}")`) |
| `get_remote_cwd` | `{ sessionId }` | `string` (remote `pwd` of the terminal's shell) |

Rust signatures (verbatim, for the port):

```rust
pub fn list_remote_dir(session_manager, session_id: String, path: String) -> Result<Vec<FileInfo>, String>
pub fn list_local_dir(path: String) -> Result<Vec<FileInfo>, String>
pub fn scan_remote_folder(session_manager, session_id: String, path: String) -> Result<Vec<FileInfo>, String>
pub fn upload_file(session_manager, session_id: String, local_path: String, remote_path: String,
                   overwrite: bool, transfer_id: Option<String>, display_name: Option<String>,
                   progress_total: Option<u64>, progress_start: Option<u64>, progress_span: Option<u64>,
                   target_path_override: Option<String>, app: AppHandle) -> Result<(), String>
pub fn download_file(session_manager, session_id: String, remote_path: String, local_path: String,
                     overwrite: bool, transfer_id: Option<String>, display_name: Option<String>,
                     progress_total: Option<u64>, progress_start: Option<u64>, progress_span: Option<u64>,
                     target_path_override: Option<String>, app: AppHandle) -> Result<(), String>
pub fn cancel_transfer(session_manager, transfer_id: String) -> Result<(), String>
pub fn delete_remote(session_manager, session_id: String, path: String, is_dir: bool) -> Result<(), String>
pub fn delete_remote_async(session_manager, session_id: String, path: String, is_dir: bool, app: AppHandle) -> Result<(), String>
pub fn create_remote_dir(session_manager, session_id: String, path: String) -> Result<(), String>
pub fn rename_remote(session_manager, session_id: String, old_path: String, new_path: String) -> Result<(), String>
pub fn delete_local(path: String, _is_dir: bool) -> Result<(), String>
pub fn create_local_dir(path: String) -> Result<(), String>
pub fn rename_local(old_path: String, new_path: String) -> Result<(), String>
pub fn pack_local_dir(source_dir: String, target_tar_path: String, transfer_id: Option<String>,
                      display_name: Option<String>, direction: Option<String>,
                      progress_total: Option<u64>, progress_start: Option<u64>, progress_span: Option<u64>,
                      target_path: Option<String>, app: AppHandle) -> Result<(), String>
pub fn unpack_local_dir(tar_path: String, target_dir: String, overwrite: bool, transfer_id: Option<String>,
                        display_name: Option<String>, direction: Option<String>,
                        progress_total: Option<u64>, progress_start: Option<u64>, progress_span: Option<u64>,
                        target_path: Option<String>, app: AppHandle) -> Result<(), String>
```

Returned data shape:

```ts
interface FileInfo { name: string; path: string; is_dir: boolean; size: number
                     modified?: number; permissions?: string; owner?: string }
```

* `list_remote_dir` builds `path = format!("{}/{}", path.trim_end_matches('/'), name)`, so listing
  `/` yields `/name`; `permissions` is the octal string (`format!("{:o}", perm)`), `modified` is
  mtime in seconds.
* `list_local_dir` uses `entry.path()` for `path` and leaves `permissions`/`owner` `None`.
* `scan_remote_folder` skips `.`/`..` and never emits directory entries.
* `create_remote_dir` uses mode `0o755` and returns `Ok` if the path already exists as a directory.
* `delete_local` ignores its `is_dir` flag; symlinks are removed with `remove_file`.
* `upload_file`/`download_file` return **before** the transfer completes: they spawn a thread, emit
  the initial `pending` event synchronously, then stream. Completion is only observable through
  `transfer-progress`.
* `pack_local_dir`/`unpack_local_dir` also spawn a thread and emit progress; they do not support
  cancellation.

### 9.2 Events

**`transfer-progress`** — payload is the backend `TransferProgress` (snake_case; the backend never
sets `session_id`/`group_id`, the frontend adds them locally):

```ts
{
  id: string                 // transfer id (matches the row id / transferId argument)
  file_name: string
  direction: string          // "upload" | "download"
  total: number              // stage total (already mapped, not raw bytes)
  transferred: number        // stage progress
  status: string             // "pending" | "transferring" | "done" | "error" | "conflict"
  error: string | null
  target_path: string | null // display path (target_path_override wins)
  conflict_path: string | null
  conflict_is_dir: boolean | null
}
```

Emission rules: exactly one `pending` before the worker starts; `transferring` **at most once per 1 %
of raw bytes** (`pct > last_pct`); one terminal `done`/`error`. Zero `transferring` events for a
0-byte file. `done` reports `(stage_start + stage_span).min(stage_total)`.

Two listeners consume it:

1. `App.tsx` global: `listen<TransferProgress>('transfer-progress', e => updateTransfer(e.payload))`
   — this is what actually moves rows to `done`/`transferring`/`error` in the store.
2. Per-task listeners inside `startTransferTask` / `waitForStageProgress` — these only resolve or
   reject the awaited promise and are unlistened on the first terminal event for that id.

**`remote-delete-status`** — payload `{ path: string, is_dir: boolean, success: boolean, error: string | null }`;
emitted once per `delete_remote_async` call. `path` is the **caller's original path** (not the
canonicalised guard path), which is what makes the frontend's `(path, is_dir)` matching reliable.

### 9.3 Store actions used

```ts
updateTransfer(progress: TransferProgress): void      // upsert by id (see §3.3)
toggleFm(bookmarkTabId: string, sessionId: string): void
updateSessionPath(bookmarkTabId: string, sessionId: string, path: string): void
  // sets BOTH terminalPath and remotePath on the session
openConfirmDialog({ title, message, confirmText = '确认', cancelText = '取消' }): Promise<boolean>
openAlertDialog({ title, message, confirmText = '知道了' }): Promise<void>
```

`openConfirmDialog` resolves `true` only for the confirm action; overlay click = cancel.
`openAlertDialog` resolves when the single button is pressed.

---

## 10. Edge cases, guards and exact error strings

### 10.1 Frontend guards

| condition | behaviour |
|---|---|
| `loadLocal` called with `''` | returns immediately, no state change |
| `loadRemote` without `session.sessionId` | returns immediately |
| FM opened while session not connected | remote panel is not loaded at all (no spinner, no error) |
| component mounts with `fmOpen === true` | `[quirk]` no initial load (rising-edge check) |
| transfer invoked without a session id | `doUpload`/`doDownload`/`startTransferTask` return early |
| upload/download with empty selection | store alert dialog (see §8.2) |
| panel deleting | that panel's buttons/rows disabled; both transfer buttons disabled |
| transfer direction busy | that arrow button disabled and replaced by a spinner |
| empty directory listing | `空目录` |
| directory with only hidden files and hidden toggle off | `空目录` |
| delete of a multi-selection containing the right-clicked row | deletes every **visible** selected row |
| right-click while panel disabled | suppressed entirely |
| rename to the same name | silently closes the modal, no backend call |
| empty/whitespace rename or folder name | store alert, modal stays open |
| path bar committed unchanged | no navigation |
| path bar `Escape` | reverts the draft, no navigation |
| context menu near a screen edge | clamped to a 4 px margin |

### 10.2 Backend guard rails (must be preserved)

* `delete_local`: `symlink_metadata`; symlink → `remove_file`; otherwise `fs::canonicalize` then
  `guard_local_delete_target`:
  * filesystem root (`/`, or `C:\`) → `Err("Refusing to delete filesystem root")`
  * canonical path equal to canonical `dirs::home_dir()` → `Err("Refusing to delete local home directory")`
  * directories are removed with `remove_dir_all` (recursive, no trash).
* `delete_remote` / `delete_remote_async`: `guard_remote_delete_target` first:
  * normalised input empty or `/` → `Err("Refusing to delete remote root directory")`
  * `realpath` canonicalisation (falls back to the input on failure); canonical `/` →
    `Err("Refusing to delete remote root directory")`
  * canonical path equal to remote `$HOME` (obtained via `printf '%s' "$HOME"` over a fresh channel)
    → `Err("Refusing to delete remote home directory")`
  * then `rm -rf -- '<path>'` / `rm -f -- '<path>'` over an exec channel; on failure falls back to a
    recursive SFTP delete (`unlink` + `rmdir`).
* `upload_file` conflict: `sftp.stat(remote_path).is_ok()` (skipped when `overwrite`) →
  `Err("CONFLICT:{remote_path}")`.
* `download_file` conflict: `Path::new(&local_path).exists()` (skipped when `overwrite`) →
  `Err("CONFLICT:{local_path}")`.
* `unpack_local_dir` with `overwrite == false` silently skips entries whose destination exists.
* `create_remote_dir`: `mkdir 0o755`; if it already exists **and is a directory**, returns `Ok`.
* Remote listing failure strings that trigger the walk-up ladder: any message matching
  `/no such file|SFTP\(2\)/i` (e.g. `readdir failed: SFTP(2) no such file`).

### 10.3 User-visible error strings (complete)

| where | text |
|---|---|
| panel load failure | raw `String(e)`, e.g. `readdir failed: SFTP(2) no such file`, `No such file or directory (os error 2)` |
| remote delete failure alert | title `删除失败`, message `String(e)` (guard strings above, or `远端删除失败` when the event carried no error) |
| rename validation | title `重命名提示`, message `请输入新名称` |
| rename failure | title `重命名失败`, message `String(e)` |
| new-folder validation | title `新建目录提示`, message `请输入文件夹名称` |
| new-folder failure | title `创建失败`, message `String(e)` |
| no upload selection | title `上传提示`, message `请先在本地面板选择要上传的文件或文件夹` |
| no download selection | title `下载提示`, message `请先在远程面板选择要下载的文件或文件夹` |
| fallback folder conflict | row error `存在同名文件，已跳过冲突项。可重试并选择全部覆盖。` |
| batch partial failure | parent row error `部分文件传输失败` |
| skipped conflict item | row error `已跳过` |
| cancelled transfer | row error `用户取消`; a backend `Cancelled` is displayed as `已取消` |
| stage failure without message | `阶段任务失败` (rejection inside `waitForStageProgress`) |
| generic queue failure chip | `失败` |

### 10.4 Permission / special-file cases

* Permission denied on listing → panel error text; no retry logic.
* Deleting a file inside a non-writable remote directory → `rm -f` fails, SFTP fallback fails, error
  surfaces in the `删除失败` alert.
* Broken symlinks locally: `delete_local` removes the link itself (`symlink_metadata` + `remove_file`).
* Files whose name has no dot: `getFileColor` yields the default `#7a7a9a`.
* Names containing `'` are safe in every composed shell command thanks to `shellQuote`.
* A remote folder whose name starts with `.` is hidden by the toggle but still transferable.

---

## 11. Auto-refresh, polling and watchers

**None.** The feature performs no polling, holds no file watchers, and sets no refresh timers. The
only asynchronous background behaviours are:

1. the 50 ms `setTimeout` that defers the initial load so the spinner can paint;
2. the 50 ms `await` before each navigation (path commit / double-click / up) with the same purpose;
3. the `get_remote_cwd` phase-2 probe on open;
4. the `terminalPath` live-follow effect (event-driven, not polled);
5. the one-shot `tar` capability probe per session;
6. the 2000 ms `setTimeout` that removes a cancelled transfer row;
7. per-transfer `transfer-progress` subscriptions.

Everything else is refreshed explicitly (see §4.7).

---

## 12. Known quirks and port guidance (checklist)

| # | Quirk | Suggested port decision |
|---|---|---|
| 1 | `column-reverse` renders the collapse bar **above** the content while the CSS comments claim the opposite `[measured]` | Implement the documented intent: bottom-docked bar, content expanding upward |
| 2 | `.fm-root` is a content-width right-hand column (no width rule) | Give the panel an explicit width/`100%` in the port |
| 3 | A component that mounts with `fmOpen === true` never loads | Trigger the initial load on "first visible" instead of a strict rising edge |
| 4 | Phase-2 cwd sync can cause a duplicate `loadRemote` via the live-follow effect | De-duplicate in-flight loads per path |
| 5 | Manual remote navigation is never written back to `session.remotePath` | Persist the panel path per session |
| 6 | `error`/`conflict` transfer rows have no cancel button and never disappear | Add dismissal or auto-expiry |
| 7 | Batch parent `error` state is invisible (only sub-items are inspected) | Surface the aggregate error |
| 8 | Resumed conflict queues re-target the **current** panel dir and mint a new group id | Decide deliberately (re-target vs. original destination) |
| 9 | `unpack_local_dir` silently skips existing entries with `overwrite=false`, yet the row reports `done` | Report skipped counts |
| 10 | Fallback folder download loses empty directories (`scan_remote_folder` emits files only) | Emit directories too |
| 11 | `tar -k` success hides skipped files | Parse tar output or use `--keep-old-files` with exit-code handling |
| 12 | `applyToAll` field is dead | Drop it or implement "apply to all remaining" |
| 13 | `getNormalizedPointerPosition` / clamp zoom maths is a no-op today | Drop the zoom division in egui |
| 14 | Dead CSS: `.fm-loading`, `.fm-bar-hint`, `.fm-tc-meta`, `.fm-item--drop-target`, `.fm-panel--drop-target`, `.fm-item[draggable]` | Skip or implement drag-and-drop as a new feature |
| 15 | No keyboard shortcuts at all (no `Delete`, `F2`, arrow navigation, `Ctrl+A`) | Add them deliberately if desired |
| 16 | Hidden-file visibility ignores the global `show_hidden_files` setting | Decide which source wins |
| 17 | Selection badge uses the raw array length while the payload uses the visible intersection | Derive both from one source |
| 18 | File Manager instances of hidden session tabs keep running effects | Keep per-session state; gate expensive work on visibility if desired |
| 19 | Every `FileManager` mounts only while `status === 'connected'`; disconnecting destroys panel paths/selection | Persist per-session UI state if continuity is wanted |
| 20 | Transfer ids are path-derived (`upload:/a/b`), so two identical destinations share a row | Keep the scheme for compatibility, or switch to UUIDs and map events explicitly |
