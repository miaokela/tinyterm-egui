<div align="center">

<img src="assets/icon.png" width="112" alt="TinyTerm">

# TinyTerm

**macOS / Windows 上的原生 SSH & SFTP 客户端 —— 用 Rust + [egui](https://github.com/emilk/egui) 编写。
单个可执行文件，不带浏览器内核，不需要安装任何运行时。**

[![build](https://github.com/miaokela/tinyterm-egui/actions/workflows/build.yml/badge.svg)](https://github.com/miaokela/tinyterm-egui/actions/workflows/build.yml)
[![release](https://img.shields.io/github/v/release/miaokela/tinyterm-egui)](https://github.com/miaokela/tinyterm-egui/releases)
![rust](https://img.shields.io/badge/rust-1.85%2B-orange)
![platforms](https://img.shields.io/badge/platform-macOS%2011%2B%20%7C%20Windows%2010%2B-blue)

[English](README.md)&nbsp;&nbsp;|&nbsp;&nbsp;[**简体中文**](README.zh-CN.md)

<img src="assets/screenshot.png" width="900" alt="TinyTerm：左侧主机列表、右侧终端、下方本地/远端文件管理">

</div>

TinyTerm 的整个界面都是用 GPU 图形直接画出来的：外壳、面板、每一个按钮都是自绘，所以
毛玻璃质感、面板间距、层级顺序都完全可控。连接、终端仿真、文件传输、数据存储都跑在同一个
进程里 —— 没有旁路服务，也没有 WebView。

本项目是原版 Tauri + React 版 TinyTerm 的 egui/eframe 重写，并刻意保留了原版的**磁盘格式**：
同一套 SQLite schema、同一种密钥信封、同一个数据目录。直接指向已有的 `tinyterm.db`，
主机、账号、设置就都在。

## 亮点

- **完整的终端** —— VT100/ANSI 仿真、256 色、粗体/斜体/下划线/反显、可配置 scrollback、三种光标样式、TUI 程序的鼠标上报与括号粘贴。
- **标签切换不丢状态** —— 一个主机一个标签，一个主机可开多个会话；切换标签不会清掉滚动内容与选区。
- **账号而不只是密码** —— 可复用的密码 / 私钥账号，落盘加密，可绑定到主机，也可连接时再问。
- **自带文件管理器** —— 本地/远端双栏，文件与整目录上传下载，传输队列带进度与取消，覆盖前先处理冲突。
- **键盘优先** —— 缩放、设置、复制粘贴、多选，以及 CPU/内存/磁盘快照、常用指令、历史命令的快捷操作栏。
- **安全策略不打折** —— SHA-256 主机指纹校验（首次信任、变更告警）、密钥不落明文、无任何遥测。

## 功能

### 终端

- VT100/ANSI 仿真（`vt100`）：256 色、完整 16 色 ANSI 调色板、粗体 / 斜体 / 下划线 / 反显
- 可配置 scrollback，三种光标样式（方块 / 竖线 / 下划线）与闪烁开关
- 文本选择与复制粘贴（`Cmd/Ctrl` + `C` / `V`）、右键菜单、粘贴确认（预览 + `N 行 · M 字符`）
- TUI 程序（htop、vim 等）鼠标上报；按住 `Shift` 拖拽即可改为选中文本
- 括号粘贴（DECSET 2004），粘进 shell 和编辑器时行为与原始终端一致
- 侧边辅助终端：与当前会话并排的第二个独立 SSH 会话
- 快捷操作栏：CPU / 内存 / 磁盘快照、五类常用指令、shell 历史命令 —— 双击输入，一键执行
- 连接状态覆盖层：连接中提示与重连入口；认证失败时直接给出密码输入框

### 主机与账号

- 主机 CRUD：颜色标签、备注、远端/本地起始目录、端口与保活间隔
- 可复用账号：密码或私钥（可带 passphrase）；可绑定主机，也可留空在连接时询问
- 主机管理面板支持搜索、复制与删除（删除有确认）
- 主机指纹校验：`SHA256:` 指纹首次信任、变更检测，设置里有已信任指纹列表
- 可达性探测与自动重连：不可达主机自动降透明度，恢复后自动连上

### 文件管理

- 本地 + 远端双栏浏览，收起时是窗口底部的一条折叠栏
- 单文件与整目录的上传 / 下载，按队列执行，带进度、字节数与取消
- 目录传输在服务端有 `tar` 时走打包（一次往返），没有时自动降级为逐文件 SFTP
- 传输开始前处理冲突：合并 / 覆盖，可逐个决定或全部应用
- 重命名、新建文件夹、删除（带保护）、复制路径、每个面板独立的隐藏文件开关、可编辑路径栏
- 远端面板跟随当前终端的工作目录

### 界面

- Cosmic / glassmorphism 主题：星空与漂移网格背景、毛玻璃面板、霓虹高光，面板共用一个圆角体系
- 设置面板：终端字体与字号、scrollback、光标样式与闪烁、默认是否显示隐藏文件、已信任指纹管理、界面缩放
- Toast 通知、统一确认对话框，无账号主机连接时的登录提示
- `Cmd/Ctrl` + `+` / `-` / `0` 调整界面缩放（0.8× – 1.6×）

## 安装

到 [**Releases**](https://github.com/miaokela/tinyterm-egui/releases) 下载对应平台的安装包：

| 平台 | 安装包 | 首次启动 |
|---|---|---|
| macOS 11+（Apple 芯片 / Intel） | `TinyTerm-macos-universal.dmg` | 未签名，首次需**右键 → 打开** |
| Windows 10/11 (x64) | `TinyTerm-windows-x86_64-setup.exe` | SmartScreen 提示时选**更多信息 → 仍要运行** |
| Linux 等其它平台 | 自行从源码构建 | 需要 X11 或 Wayland |

macOS 包由 CI 构建为通用二进制（arm64 + x86_64），每个 Release 同时提供 SHA-256 校验和。

## 快速上手

1. **添加主机** —— 点左侧栏的 `＋`，填地址和端口。账号可留空：留空时连接过程中会提示输入密码。
2. **连接** —— 点主机行上的圆形连接按钮。首次连接会要求确认该主机的 SSH 指纹。
3. **开更多会话** —— 标签条上的 `＋` 在同一主机下新增会话；标签条右侧的按钮打开侧边辅助终端。
4. **传文件** —— 点窗口底部的「文件管理」栏，在任一侧选中文件，用中间的 `→` / `←` 按钮传输（右键也有菜单）。
5. **按需调整** —— `Cmd/Ctrl` + `,` 打开设置：字体、光标、scrollback、隐藏文件、已信任指纹。

## 键盘快捷键

| 快捷键 | 作用 |
|---|---|
| `Cmd/Ctrl` + `+` / `-` | 放大 / 缩小界面（0.8× – 1.6×） |
| `Cmd/Ctrl` + `0` | 缩放复位到 100 % |
| `Cmd/Ctrl` + `,` | 打开设置 |
| `Cmd/Ctrl` + `C` / `V` | 复制选中内容 / 粘贴到终端 |
| `Cmd/Ctrl` + 点击 | 在文件列表中追加 / 取消选中 |
| `Shift` + 点击 | 范围选择文件 |
| 终端内 `Shift` + 拖拽 | TUI 程序占用鼠标时仍可选中文本 |
| 双击 | 输入常用指令 / 执行历史命令 |
| `Enter` / `Esc` | 确认 / 取消当前对话框 |

## 数据、安全与隐私

所有数据都在一个目录里：

| 平台 | 目录 |
|---|---|
| macOS | `~/Library/Application Support/com.tinyterm.app/` |
| Windows | `%APPDATA%\com.tinyterm.app\` |
| 回退（平台数据目录不可写时） | `~/.tinyterm-egui/` |

| 文件 | 内容 |
|---|---|
| `tinyterm.db` | SQLite 数据库：主机、账号、设置、已信任的主机指纹 |
| `secret-key.pem` | 用于加密账号密钥的 RSA-2048 私钥（Unix 下权限 `0600`） |
| `zoom.txt` | 界面缩放，仅在非默认值时才写入 |

设置环境变量 `TINYTERM_DB` 可以改用其它数据库文件。

**密钥如何落盘。** 密码与私钥以
`ttenc:v1:<包裹后的密钥>:<nonce>:<tag>:<密文>` 信封存储：每条记录一把
AES-256-GCM 密钥，再用本机 `secret-key.pem` 的 RSA-2048 OAEP/SHA-1 包裹。
因此把数据库拷到别的机器上，也解不出任何凭据内容。

**主机身份。** 认证之前会先用 `trusted_host_keys` 里的 `SHA256:` 指纹校验 SSH 主机密钥：
未知主机需要确认；指纹**发生变化**时会提示可能存在中间人攻击，可在设置里重新信任。

**网络行为。** TinyTerm 只会发起你主动建立的 SSH/SFTP 连接，没有遥测、没有更新探测、没有云端组件。

## 从源码构建

需要 **Rust 1.85+**（stable）与平台自带的 C 工具链。SQLite 已随包编译
（`rusqlite` 的 `bundled` 特性），无需额外安装。

```bash
git clone https://github.com/miaokela/tinyterm-egui.git
cd tinyterm-egui
cargo run --release
```

首次构建要从零编译 egui/eframe、russh 等依赖，需要几分钟。网络受限时：

```bash
CARGO_NET_OFFLINE=true cargo build
```

### 测试

```bash
cargo test
```

22 个测试，覆盖密钥信封往返、SQLite CRUD 与设置迁移、终端仿真与按键编码、路径 / 进度 /
历史解析、本地 `tar` 打包解包、删除保护、无头 UI 布局，以及一个由**进程内 `russh`
测试服务器**驱动的 SSH 端到端流程（未知指纹 → 信任 → 握手 → 错误密码被拒 → 认证成功 →
`exec` / 远端 `$HOME` / 远端 `cwd` → PTY 回显 → SFTP 子系统协商）。

### 打包

`scripts/bundle-macos.sh` + `scripts/make-dmg.sh` 生成 macOS `.app` 与带背景图的 `.dmg`；
`scripts/windows-installer.nsi` 生成 NSIS 安装包。推 `v*` tag 时
[`.github/workflows/build.yml`](.github/workflows/build.yml) 会在两个平台上构建、
以 release 模式跑测试，并把产物发布成 GitHub Release。

## 代码结构

```
src/
├── main.rs            入口：存储、tokio runtime、eframe
├── app.rs             eframe::App：布局、事件泵、弹窗路由、快捷键
├── state.rs           应用状态（标签、文件管理器、弹窗、Toast）
├── actions.rs         状态迁移（等价于原版的 store actions）
├── models.rs          数据模型（主机、账号、设置、传输）
├── storage.rs         SQLite 访问、schema、数据目录
├── crypto.rs          ttenc:v1 密钥信封（RSA-OAEP + AES-256-GCM）
├── ssh.rs             russh：连接、指纹校验、认证、PTY、exec、SFTP
├── session.rs         会话管理 + 事件总线
├── remote_fs.rs       SFTP 远端操作与 tar 目录传输
├── local_fs.rs        本地文件操作与删除保护
├── transfer.rs        上传/下载编排（批次、冲突）
├── term.rs            vt100 终端仿真、网格渲染、按键编码
├── theme.rs           设计令牌（颜色、圆角、字号、光晕、星空）
├── widgets.rs         自绘控件（按钮、输入框、图标、加载动画）
└── ui/                每个界面一个模块：侧栏、会话标签、终端、文件管理、
                       快捷操作、系统信息、主机、账号、设置、对话框、Toast
```

改动设计系统前先看 [`skills/tinyterm-egui-dev/`](skills/tinyterm-egui-dev/SKILL.md)：里面有
令牌表、布局常量、SSH/SFTP 事件契约、打包流水线，以及一路踩过的坑。`docs/` 里是需求基线
（`ANALYSIS.md`）与后端 / 文件管理器规格。

## 架构

| 层 | 选型 |
|---|---|
| 窗口与渲染 | `eframe` + `egui` 0.36 —— macOS / Linux 用 glow，Windows 用 wgpu（D3D12，虚拟机与远程桌面上回退到软件光栅化） |
| 异步运行时 | `tokio`（多线程），承载 SSH、SFTP、传输与文件 IO |
| SSH / SFTP | `russh` 0.63（ring + flate2 + rsa）与 `russh-sftp` 3 |
| 终端 | `vt100` 0.16 做仿真，上层自绘网格渲染 |
| 存储 | `rusqlite` + 随包编译的 SQLite |
| 密钥 | `rsa`、`aes-gcm`、`sha1` 实现 `ttenc:v1` 信封 |

窗口的每个区域都是 `src/theme.rs` 与 `src/widgets.rs` 画出来的显式 `Rect` —— 没有
`SidePanel` / `CentralPanel`，也没有保留式控件树。后台任务跑在 tokio runtime 上，
通过事件总线回报，由 `app.rs` 每帧取走一次。

## 与原版 TinyTerm 的兼容性

- **同一套数据库** —— egui 版沿用原版的 `app_data_dir` 与 schema，可以直接打开已有的 `tinyterm.db`（`TINYTERM_DB` 可覆盖路径）。
- **同一种密钥格式** —— `ttenc:v1` 信封互通，账号密钥在两边都能解密。
- **本版新增** —— 完整的设置面板（网页版缺失）、账号管理界面，以及不再依赖 WebView 的布局。
- **未继承** —— 见下方「已知限制」。

## 已知限制

- 只发布 macOS 与 Windows 安装包；Linux 需要自行从源码构建。
- 不支持端口转发、跳板机、SSH agent 转发。
- 退出后不恢复标签与会话（主机、账号、设置、已信任指纹是持久化的）。
- 文件管理器不支持双栏拖拽，也没有权限（`chmod`）编辑。
- 界面文案目前只有简体中文，还没有语言切换。
- 安装包未签名，macOS Gatekeeper 与 Windows SmartScreen 首次启动都会提示。

## 许可与致谢

以 **MIT 许可**发布（见 `Cargo.toml`）。

- 界面图标：[Phosphor Icons](https://phosphoricons.com)（MIT），以 `assets/Phosphor.ttf` 内嵌并注册为字体回退，避免为了图标再引入一棵 egui 依赖树。
- 图标与 logo 资源来自原版 TinyTerm 项目。
- 基于 [egui/eframe](https://github.com/emilk/egui)、[tokio](https://tokio.rs)、[russh](https://github.com/Eugeny/russh)、[vt100](https://github.com/doy/vt100-rust) 与 [rusqlite](https://github.com/rusqlite/rusqlite)。
