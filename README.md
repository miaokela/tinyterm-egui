# TinyTerm (egui)

TinyTerm 的 **egui / eframe** 重写版本 —— 一个纯 Rust 的桌面 SSH 客户端，视觉与交互对齐原版
Tauri + React 实现（cosmic / glassmorphism 主题）。

```
tinyterm-egui/
├── Cargo.toml
├── README.md
├── docs/
│   ├── ANALYSIS.md             # 原版全量功能与 UI 细节分析（需求基线）
│   ├── spec-backend.md         # Rust 后端逐命令规格（SQL/SSH/SFTP/加密）
│   └── spec-filemanager.md     # 文件管理器逐交互规格
└── src/
    ├── main.rs                 # 入口：存储、tokio runtime、eframe
    ├── app.rs                  # eframe::App：布局、事件泵、弹窗路由
    ├── state.rs                # 应用状态（Host/Session 标签、文件管理器、弹窗、Toast）
    ├── actions.rs              # 状态迁移（等价于原版 Zustand store actions）
    ├── models.rs               # 数据模型
    ├── storage.rs              # SQLite（与原版同 schema）
    ├── crypto.rs               # ttenc:v1 密钥信封（RSA-OAEP + AES-256-GCM）
    ├── ssh.rs                  # russh：连接、指纹校验、认证、PTY、exec、SFTP
    ├── session.rs              # 会话管理 + 事件总线
    ├── remote_fs.rs            # SFTP 远端文件操作 + tar 目录传输
    ├── local_fs.rs             # 本地文件操作与删除保护
    ├── transfer.rs             # 上传/下载编排（批次、tar、降级、冲突）
    ├── term.rs                 # vt100 终端仿真 + egui 网格渲染 + 按键编码
    ├── theme.rs                # 设计令牌（颜色/圆角/字号/光晕/星空背景）
    ├── widgets.rs              # 自绘控件（按钮、输入框、图标、加载动画）
    └── ui/                     # 各界面模块
        ├── sidebar.rs          # 左侧主机侧边栏
        ├── session_tabs.rs     # 顶部会话标签条
        ├── terminal_view.rs    # 终端面板（输入/选择/滚动/右键/状态覆盖层）
        ├── file_manager.rs     # 文件管理双栏 + 传输队列 + 右键菜单
        ├── quick_actions.rs    # 快捷操作栏 + 常用指令 + 历史命令
        ├── system_info.rs      # CPU / 内存 / 磁盘表格弹窗
        ├── hosts_modal.rs      # 主机管理弹窗 + 主机表单
        ├── credentials_modal.rs# 凭据管理弹窗 + 凭据表单
        ├── settings_modal.rs   # 设置面板（原版缺失，此处补齐）
        ├── dialogs.rs          # 确认/冲突/登录/粘贴确认
        └── toast.rs            # 右下角通知
```

## 构建与运行

需要 Rust 1.85+（开发使用 nightly 1.100）。

```bash
cargo run --release
```

首次构建会编译 eframe/wgpu 等依赖，耗时较长。若网络受限：

```bash
CARGO_NET_OFFLINE=true cargo build
```

## 测试

```bash
cargo test
```

13 个测试覆盖：加密信封往返、SQLite CRUD 与设置迁移、终端仿真与按键编码、
路径/进度/解析辅助函数、本地 tar 打包解包、删除保护，以及一个**进程内 russh
测试服务器驱动的 SSH 端到端测试**（首次连接指纹提示 → 信任后握手 → 错误密码被拒 →
密码认证成功 → exec / 远端 HOME / 远端 cwd → PTY shell 回显 → SFTP 子系统协商）。

## 数据位置

| 文件 | 路径 |
|---|---|
| SQLite | `~/Library/Application Support/com.tinyterm.app/tinyterm.db` |
| 密钥 | 同目录 `secret-key.pem` |
| 缩放 | 同目录 `zoom.txt` |

环境变量 `TINYTERM_DB` 可覆盖数据库路径（指向原版 `tinyterm.db` 即可直接复用历史主机与凭据）。
若平台数据目录不可写，会自动回退到 `~/.tinyterm-egui/tinyterm.db`。

数据库 schema 与原版 TinyTerm **完全一致**，且加密信封格式相同，因此可以直接复用已有的
`tinyterm.db`（凭据会自动解密）。

## 已实现功能

- 主机 / 凭据 CRUD、搜索、复制、连接
- 多主机标签 + 多会话标签，标签切换保留终端上下文
- SSH 连接、主机指纹信任流程（首次 / 变更）、密码与私钥认证
- 完整终端仿真：256 色、粗体/斜体/下划线/反显、光标样式与闪烁、scrollback、文本选择与复制粘贴
- 右侧辅助终端（独立 SSH 会话并排）
- 文件管理器：本地/远程双栏、隐藏文件、路径编辑、重命名、新建文件夹、删除保护
- 上传 / 下载：单文件、目录（tar 打包）、批次队列、进度、取消、冲突处理
- 终端 cwd 跟随远端目录
- 快捷操作栏：CPU / 内存 / 磁盘查询、常用指令库、shell 历史
- 设置面板：字体、scrollback、光标、隐藏文件、界面缩放、已信任指纹管理
- 主机可达性探测与自动重连
- Toast 通知、统一确认对话框、登录提示、粘贴确认
- 终端鼠标上报（SGR/X10，TUI 程序可用）、括号粘贴（bracketed paste）

## 图标与品牌资源

界面图标使用 [Phosphor Icons](https://phosphoricons.com)（MIT），字体文件内嵌在
`assets/Phosphor.ttf`，常量在 `src/icons.rs`。之所以不直接依赖 `egui-phosphor`：
该 crate 目前绑的是 egui 0.35，会把整个 egui 0.35/epaint 0.35 依赖树再编译一遍；
直接内嵌 TTF 只增加约 490 KB 二进制体积。

原版 TinyTerm 的品牌资源也一并复用：

| 文件 | 来源 | 用途 |
|---|---|---|
| `assets/icon.png` | `src-tauri/icons/icon.png`（512×512） | 窗口图标（`ViewportBuilder::with_icon`） |
| `assets/logo.png` | `public/assets/logo.png`（512×512） | 空状态 / 空会话里的 logo 贴图 |
| `assets/icon.icns` | `src-tauri/icons/icon.icns` | macOS `.app` 包图标 |

macOS 说明：winit 在 macOS 上**不支持** `set_window_icon`，Dock/Finder 图标只能来自
`.app` 包里的 `.icns`。因此提供了一个打包脚本：

```bash
./scripts/bundle-macos.sh      # 生成 target/release/TinyTerm.app
open target/release/TinyTerm.app
```

打包后 Dock 与 Finder 中就会显示原版图标。

## 与原版的差异（有意为之）

1. 终端仿真使用 `vt100` 而非 xterm.js；已覆盖常用 VT100/xterm 序列。
2. 使用 `russh`（纯 Rust）替代 libssh2，SFTP 与终端共用一条 SSH 连接（原版为独立连接）。
3. 增加连接超时（30s），避免黑洞主机无限等待。
4. 补齐原版缺失的完整设置面板。
5. 侧边栏折叠、缩放等交互改用 egui 原生实现（`Cmd/Ctrl + +/-/0` 仍然可用）。
