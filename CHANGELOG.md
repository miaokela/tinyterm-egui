# Changelog

本项目遵循 [语义化版本](https://semver.org/lang/zh-CN/)；版本号取自 `Cargo.toml`，
推送 `v*` tag 会触发 GitHub Actions 构建 macOS `.dmg` 与 Windows 安装包，
并把产物（含 SHA-256 校验和）发布成 GitHub Release。

## [0.1.3] - 2026-10-09

### 修复

- **账号弹窗按钮上方的一大片空白**：新建 / 编辑账号的表单原来固定 540px 高，密码模式下只有
  4 个字段，按钮上方空出近 200px。现在先量出表单体需要的高度再定壳高——只矮不高（上限仍是
  原来的 540px），装不下时才交给滚动。实测：密码模式 540 → 363px，私钥模式 540 → 531px。
- **本地 `cargo test` 报 `Dropped TexturesDelta with 1 unapplied deltas`**：这是 epaint 在 debug
  构建下对未消费的字体纹理增量做的断言（CI 用 `--release`，所以一直没暴露）。测试里显式丢弃
  该增量后，22 个测试全绿。

### 变更

- **界面文案统一为「账号」**：列表标题（原 `Credentials`）、表单标题（`新建 / 编辑 Credential`）、
  Host 表单字段名（`Credential（可选）`）、按钮、空状态、删除确认与 Toast（`凭据已保存 / 已删除`）
  都不再出现「凭据 / Credential」。
- **布局面板圆角统一收小**：左侧主机栏、终端工作区、会话标签条、文件管理统一 8px（原来是
  16px 与 12px 混用）；内层终端背景保持 4px，与外框同心。
- **「正在连接…」不再像告警**：改用终端同款等宽字体、14px、浅蓝 `#7cc9ff`——原版的
  `--color-warning` 本就是浅蓝，橙色只用在警告提示上；侧栏主机行与会话标签的「连接中」
  状态点同步改色。
- **Host 表单的「新增账号」按钮重做**：accent 胶囊，22px 高、8px 圆角、`+` 字形 + accent 文字，
  比旁边的账号行小一档，hover 有亮边与外发光。
- **选中的账号行更容易辨认**：蓝色底 + 亮蓝描边 + 左侧发光条（与侧栏「当前主机」同一套选中
  语言），未选中行的标题降为次级色。
- **README 重写为中英双语**：`README.md`（英文，默认）与 `README.zh-CN.md`（简体中文）互相链接
  切换，补齐亮点、功能清单、安装与首次启动说明、快捷键、数据与安全、架构与已知限制。

### 新增

- `widgets::measure_height()`：在不可见子 UI 中把表单体布局一遍量出高度，供弹窗按内容定高。
- `theme::RADIUS_PANEL`（面板统一圆角）与 `theme::CONNECTING`（连接中配色）两个设计令牌。
- 3 个 UI 测试：量高探针与真实布局逐像素一致、账号弹窗高度随内容拟合、模拟点击仍能命中真实
  控件（证明探针不会抢占事件）。

## [0.1.2] - 2026-09-11

### 修复

- **终端里按 Tab 后无法继续输入**：焦点被 egui 的焦点导航抢走。egui 默认把 Tab
  （以及方向键、Esc）当成「把焦点移到下一个控件」，焦点跳出终端后按键就不再送给
  shell 了。现在终端获得焦点时会安装 `EventFilter`，把 Tab / 左右方向键 / 上下方向键 /
  Esc 锁在终端内：它们仍作为普通按键事件交给 `encode_key`，正常发送 `\t`、`\x1b[A`、
  `\x1b` 等，只是不再触发控件间跳转。
- **光标样式设置无效**：`cursor_style` 一直存在（方块 / 竖线 / 下划线）但渲染时被忽略，
  永远画方块。现已按设置渲染三种形状。

### 变更

- 默认光标改为**闪烁的下划线**：不再用整格方块挡住字符，而是在字符下方画一条短线。
  旧数据库里遗留的 `cursor_style='block'`（此前该设置不生效，等同默认值）会在启动时
  一次性迁移为 `underline`；迁移后手动选择的样式不会被覆盖。
- 设置 → 光标样式的三个选项改为中文标签（方块 / 竖线 / 下划线）。

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
