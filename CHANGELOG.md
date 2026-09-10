# Changelog

本项目遵循 [语义化版本](https://semver.org/lang/zh-CN/)；版本号取自 `Cargo.toml`，
推送 `v*` tag 会触发 GitHub Actions 构建 macOS `.dmg` 与 Windows 安装包（不创建 Release）。

## [0.1.1] - 2026-09-10

### 修复

- **终端面板左下/右下圆角处的斜线**：`stroke_open` 的圆角圆弧方向写反，折线在拐角处
  跳变，实测有 4 条非法斜线段（两个拐角各 17px，另有横穿底部与沿右侧的长斜线）。
- **整体默认缩放偏小**：`APP_ZOOM_DEFAULT` 由 0.8 提到 1.0，上限放宽到 1.6，
  `Cmd/Ctrl + 0` 由「回到最小值」改为「回到默认值」；同时只持久化非默认缩放，
  并把历史遗留的 0.8 视为未自定义，避免改默认值不生效。
- **Windows 在远程桌面 / 无 GPU 云主机上双击无反应**：glow 后端要求 OpenGL 2.0+，
  这类环境只提供 GDI 的 OpenGL 1.1。Windows 改用 wgpu（DX12），自定义适配器选择
  优先独显/集显，无可用 GPU 时退到软件光栅化（WARP）。
- **Windows 报 `VCRUNTIME140.dll was not found`**：`.cargo/config.toml` 为 MSVC 目标
  打开 `+crt-static`，运行库静态链入 exe，安装包不再依赖 VC++ Redistributable。
- **macOS 首次打开没有背景图**：挂载方式（`-mountpoint`）与 Finder 时序导致
  `Can't get disk "TinyTerm" (-1728)`，AppleScript 从未生效。改为挂到 `/Volumes`、
  等待 5 秒并重试，最后校验 `.DS_Store` 是否带背景图引用。
- **NSIS 报 `SetShellVarContext not valid outside Section or Function`**：该命令只能
  出现在 Section/Function 内，已移入两个 Section。
- Windows release 版启动失败不再静默：日志落盘 + 原生错误弹窗 + panic hook。

### 变更

- 文件管理上传/下载按钮图标由上下箭头改为**左右箭头**（← 下载、→ 上传），
  传输队列的方向图标同步统一；按钮仍为竖排。
- 主机管理/凭据管理的「新增」与「连接」圆钮统一为同一款式：静息态几乎透明 +
  四段缓慢自转的 HUD 括线，悬停才上 accent 底色、高亮描边与光晕。
- Windows 产物由 zip 改为 **NSIS 安装包**（按用户安装、免 UAC、含开始菜单与
  卸载项）；不再构建 Linux 版本，只保留 macOS 与 Windows。
- macOS 的 `Info.plist` 版本号改为从 `Cargo.toml` 读取，不再写死。
- README 顶部加入界面截图，并新增「打包与分发」章节。

### 新增

- macOS DMG 背景图与拖拽安装布局，背景图内说明两种 Gatekeeper 拦截的处理方式。
- Windows exe 图标与版本信息（`build.rs` + `winresource`，`assets/icon.ico`），
  NSIS 安装包与卸载项同样带图标。
- Windows 中文字体回退（微软雅黑 / SimSun / 黑体）与等宽字体候选
  （Consolas / Lucida Console / Courier New）。
- 启动日志 `%USERPROFILE%\.tinyterm-egui\tinyterm.log`（超过 512KB 自动轮转）。
- `skills/tinyterm-egui-dev/` 技能文档：配色令牌与控件配方、布局常量、
  russh/russh-sftp 事件契约与传输引擎、macOS/Windows 打包流水线与踩坑记录。

## [0.1.0] - 2026-09-09

首个版本：用 **egui / eframe** 重写 TinyTerm（原版为 Tauri + React），
纯 Rust 单二进制，视觉与交互对齐原版 cosmic / glassmorphism 主题。

- 终端：vt100 仿真、网格渲染、选区与复制粘贴、鼠标上报、括号粘贴、辅助终端分屏
- 文件管理：本地/远程双栏、上传下载与传输队列、目录 tar 打包传输、冲突处理、
  删除保护、跟随终端 cwd
- SSH/SFTP：`russh` + `russh-sftp` 共用一条连接、主机指纹校验、私钥/密码认证、
  连接超时、远端 exec 查询（历史命令、系统信息）
- 主机与凭据管理、设置面板、Toast 通知、统一确认对话框
- 存储：与原版同 schema 的 SQLite，`ttenc:v1` 密钥信封（RSA-OAEP + AES-256-GCM），
  可直接读取原版 `tinyterm.db`
