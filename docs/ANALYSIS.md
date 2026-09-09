# TinyTerm 全量功能与 UI 细节分析

> 分析对象：`../tinyterm/`（Tauri v2 + React + TypeScript + Zustand + xterm.js，Rust 后端 ssh2/rusqlite）
> 本文是 `tinyterm-egui` 重写的需求基线，逐条对应源码位置。

---

## 1. 总体架构

```
┌─ 前端 (React) ─────────────────────────────────────────────┐
│  main.tsx → App.tsx                                        │
│   ├─ 左侧 Host 侧边栏（主机标签）                          │
│   ├─ 顶部 Session 标签条（Chrome 风格）                    │
│   ├─ 终端区（xterm.js，主终端 + 右侧辅助终端）             │
│   ├─ 文件管理区（本地/远程双栏 + 传输队列）                │
│   └─ 弹窗：Hosts / Credentials / SystemInfo / 确认 / 登录   │
│  store/index.ts（Zustand，全局状态与动作）                 │
└────────────────────────────────────────────────────────────┘
                     │ Tauri invoke / events
┌─ 后端 (Rust) ──────────────────────────────────────────────┐
│  commands/ssh.rs   会话生命周期、PTY、exec、cwd            │
│  commands/sftp.rs  远端文件系统、上传/下载、进度事件       │
│  commands/local_fs.rs 本地打包/解包                        │
│  storage.rs        SQLite（bookmarks/profiles/settings/...）│
│  crypto.rs         RSA-OAEP + AES-256-GCM 密钥封装         │
│  ssh.rs            libssh2 封装、指纹计算                  │
└────────────────────────────────────────────────────────────┘
```

---

## 2. 数据模型

### 2.1 Bookmark（= Host，同一张表）

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | String (uuid) | 主键 |
| `title` | String | 显示名（空则用 `host`） |
| `host` / `port` | String / u16 | 地址端口，默认 22 |
| `username` | String | 用户名 |
| `auth_type` | String | `password` / `privateKey` / `profile` |
| `password` | Option<String> | 密文信封 |
| `password_encrypted` | bool | 旧 electerm 兼容标志 |
| `private_key` / `passphrase` | Option<String> | 私钥内容与保护密码（密文） |
| `profile_id` | Option<String> | 引用 Credential |
| `group_id` | Option<String> | 分组 |
| `term` | String | 默认 `xterm-256color` |
| `encode` | String | 默认 `utf8`（后端未使用） |
| `color` | Option<String> | 标签颜色，默认 `#7c5cbf` |
| `description` | Option<String> | 备注 |
| `start_directory_remote` / `_local` | Option<String> | 起始目录（仅存储） |
| `enable_sftp` | bool | 默认 true（仅存储） |
| `keepalive_interval` | u32 | 默认 30000（后端硬编码 30s，此值未生效） |
| `created_at` / `updated_at` | i64 | Unix 秒 |

### 2.2 Profile（= Credential）

`id, title, username, auth_type, password, password_encrypted, private_key, passphrase, created_at`

### 2.3 Settings（单行表 `id=1`）

`font_size=12, font_family="Menlo, Monaco, 'Courier New', monospace", theme="dark",
opacity=1.0, language="zh", scrollback=5000, show_hidden_files=false,
default_protocol="ssh", cursor_style="block", cursor_blink=true, bell_style="none"`

### 2.4 其他

- `BookmarkGroup{id,title,parent_id,order_index,created_at}`
- `FileInfo{name,path,is_dir,size,modified,permissions,owner}`
- `TransferProgress{id,file_name,direction,total,transferred,transferred_bytes,status,error,target_path,conflict_path,conflict_is_dir,session_id,group_id}`
- `TrustedHostKey{host,port,key_type,fingerprint,created_at,updated_at}`（主键 host+port）
- `HostKeyVerificationPrompt{host,port,key_type,fingerprint,reason}`

---

## 3. 功能清单

### 3.1 主机与凭据管理

- 凭据 CRUD：密码 / 私钥两种认证；编辑时留空表示保留原值
- 主机 CRUD：绑定凭据、地址端口、颜色、备注、远程起始目录
- 主机列表搜索（按 title/host 不区分大小写）、复制（副本）、删除（确认弹窗）
- 主机行显示：颜色圆点、名称、`host:port`、凭据徽标（`key/pwd · 名称`，无凭据时显示 `用户名 · 手动输入` 或 `连接时输入`）
- 连接按钮：加载态 spinner，点击后打开/激活 Host 标签

### 3.2 SSH 连接与主机指纹信任

- 顺序：TCP 连接 → 握手 → **主机指纹校验** → 认证 → 请求 PTY → 打开 shell
- 指纹格式 `SHA256:<base64无填充>`，key_type 如 `ssh-ed25519`
- 未信任（`unknown`）或变更（`mismatch`）时后端返回 `HOST_KEY_PROMPT:<json>`，前端弹确认框：
  - unknown：`首次连接到该主机，需要确认 SSH 指纹。`
  - mismatch：`检测到主机指纹变更。…这可能是主机重装，也可能是中间人攻击。…`
  - 按钮：`信任并继续` / `取消`；确认后写入 `trusted_host_keys` 并重连
- 认证：`auth_type=privateKey` 用私钥（可带 passphrase），否则用密码；连接时输入的密码优先
- 保活：TCP keepalive 60s/15s + SSH keepalive 30s
- 端口探测：每 15s 对已打开主机 `check_host_port`（1.5s 超时），连续 2 次失败 → 标记不可达并断开该主机所有会话；恢复可达时自动重连
- 侧边栏状态点：connected 绿、connecting 琥珀脉冲、error 红、disconnected 灰；探测成功时有一次心跳闪烁动画

### 3.3 双层标签

- **Host 标签（左侧）**：一个主机一个顶层标签，显示序号方块（按状态着色）、标题、关闭按钮；激活时左侧 4px 主色指示条 + 光晕；不可达时整体降透明度/降饱和
- **Session 标签（顶部）**：一个 Host 标签内可开多个终端会话；Chrome 风格圆角标签，含状态点、标题、关闭按钮；新建标签有 1.6s 淡入动画；`+` 按钮带 loading（最短 600ms）
- 切换标签不销毁终端（保留滚动上下文）

### 3.4 右侧辅助终端

- 同一会话可打开一个独立 SSH 会话并排显示（各占 50%）
- 状态：connecting（居中提示 `正在打开辅助终端...`）/ error（显示错误）/ connected
- 切换按钮位于标签条右侧，激活时绿色

### 3.5 终端交互

- 输入：自定义 `keydown → 字节序列` 编码（Ctrl+字母→控制码、Alt+字符→ESC 前缀、应用光标模式 `\x1bO` 前缀、Delete/Insert/PageUp/PageDown 等）
- 输出：后端读取线程按 5ms 窗口聚合后推送（≤4096 字节立即 flush）
- 尺寸：`FitAddon` + `ResizeObserver`，可见时才 fit，切回可见时重新 fit 并回传 cols/rows
- 主题：背景 `#050b14`、前景 `#b6c5d3`、光标 `#ffbf69`、选中 `rgba(115,167,255,0.24)`、完整 16 色 ANSI 调色板
- 右键菜单：`复制`（无选中时禁用）/ `粘贴`；菜单自动避让屏幕边界
- 粘贴确认：预览前 2000 字符，显示 `N 行 · M 字符`，Enter 确认 / Esc 取消
- 连接成功后 400ms 发送 `clear\r` 清除 MOTD，800ms 后隐藏 loading 遮罩（方块波动画）
- 错误/断开覆盖层：`⚠ 连接失败` / `⚠ 连接已断开` + 错误详情；认证类错误额外显示密码输入框；`↺ 重新连接` 按钮

### 3.6 终端快捷操作栏（右上角）

- 折叠态一个展开按钮；展开态：CPU / 内存 / 磁盘 / 常用指令 / 历史命令 / 收起
- CPU/内存：`ps -eo pid,pcpu|pmem,comm,args | sort -k2 -nr | head -n 100`
- 磁盘：`df -h`
- 常用指令：5 个分类（服务管理 12 条、进程管理 10 条、网络诊断 10 条、文件与磁盘 12 条、系统与权限 11 条），双击输入到终端
- 历史命令：`cat ~/.zsh_history || cat ~/.bash_history`，解析 zsh（`: ts:0;cmd`）与 bash（带行号）格式，保留最后 200 条，双击插入 / ▶ 执行

### 3.7 系统信息弹窗

- 表格分页（每页 15 条），列：PID / CPU%或内存% / 程序名称 / 执行路径
- 磁盘模式列：文件系统 / 总容量 / 已用 / 可用 / 使用率 / 挂载点，使用率 >80% 标红
- 刷新按钮（加载时转圈）、关闭按钮、`共 N 条 · 第 X / Y 页` + 上/下页

### 3.8 文件管理器

**布局**（固定高度 260px 内容区 + 28px 折叠条）

```
┌ 传输队列（有任务时）───────────────────────────┐
├ 本地面板 │ 分隔条(32px) │ 远程面板 ────────────┤
│ 头部：图标 标题  隐藏/刷新/新建文件夹          │
│ 路径栏：⬆ + 可点击编辑的路径                   │
│ 列表：图标 名称 大小（目录蓝色）                │
└───────────────────────────────────────────────┘
```

- 折叠条：`文件管理` + 活动任务徽标，点击展开/收起
- 分隔条两个按钮：→ 上传（选中数徽标）、← 下载；忙碌时变 spinner
- 选择模型：单击单选、Cmd/Ctrl 多选切换、Shift 范围选择、右键选中并弹菜单
- 排序：目录优先，其次名称不区分大小写
- 隐藏文件开关：每个面板独立
- 路径栏：点击变输入框，Enter 提交、Esc 取消；⬆ 上级目录
- 右键菜单：`打开` / `重命名` / `新建文件夹` / `删除` / `复制路径`
- 状态：加载中 spinner、`空目录`、错误文本
- 传输队列每行：方向箭头 + 动作 + 文件名 + 进度条 + 百分比/状态 + 取消按钮

**传输流程**

- 预检冲突：与目标面板当前可见列表按**文件名**比较；有冲突时弹：
  - 文件夹冲突：`文件夹合并/覆盖确认` → `跳过现有文件` / `全部覆盖`
  - 文件冲突：`文件覆盖确认` → `逐个询问` / `全部覆盖`
  - 无冲突：`确认上传/下载` → `开始上传/下载`
- 目录传输（远端有 `tar`）：
  - 上传：本地 `tar` 打包（0→20%）→ SFTP 上传 tar（20→80%）→ 远端 `tar -k -xf` 或先 `rm -rf` 后 `tar -xf`（→100%）→ 清理临时文件
  - 下载：远端 `tar -cf` → 下载 tar → 本地解包
- 无 `tar` 时降级为逐文件 SFTP（预建目录）
- 冲突返回 `CONFLICT:<path>`；取消通过共享 HashSet 协作式检查（每 32KB 检查一次）
- 进度事件 `transfer-progress`，整数百分比变化时才推送

### 3.9 终端 cwd 同步

- 打开文件管理器：先用缓存路径加载，再 `get_remote_cwd` 回填真实 cwd
- 之后持续跟随（`/proc` → `lsof` → `pwd` 三级降级）

### 3.10 设置与个性化

- 后端可读写：字体大小/字体族/主题/透明度/语言/scrollback/显示隐藏文件/默认协议/光标样式/光标闪烁/铃声
- 应用缩放：`Cmd/Ctrl + +/-/0`，范围 0.8–1.4，默认 0.8，存 localStorage
- （Web 版缺少完整设置面板，egui 版补齐）

### 3.11 通知与对话框

- Toast：右下角，2s 自动消失，成功/错误/信息三种图标与边框色
- 统一确认/提示对话框：`app-dialog-*`，overlay 点击 = 取消
- 登录提示对话框：无凭据主机连接时弹出，用户名 + 密码，Enter 在用户名框跳到密码框

---

## 4. 视觉规范（Cosmic / Glassmorphism）

| 类别 | 值 |
|---|---|
| 主背景 | `#07162d` + 径向渐变（左上 `#0e3a72`、右下 `rgba(54,224,142,.22)`） |
| 面板 | `rgba(7,22,43,.86)` + `blur(12px)` + 1px `rgba(58,132,255,.3)` |
| 卡片 | `#0c1f38` |
| 输入 | `rgba(6,18,36,.82)` |
| 终端 | `#050b14` |
| 主色 | `#2f7dff`，hover `#6ab5ff`，active `#1c5fcc`，辅色 `#57d8b2` |
| 文本 | 主 `#e7eff9`、次 `#a8bdd1`、弱 `#7d93a9` |
| 状态 | 成功 `#57e3a5`、错误 `#e0575c`、警告 `#f0a040` |
| 圆角 | 4 / 8 / 12 / 16 px，胶囊 999px |
| 字号 | 12 / 13 / 14 / 16 / 20 px |
| 光晕 | `0 0 12px rgba(72,161,255,.4)` |
| 背景星点 | 200 颗，半径 0.3–1.8，透明度 `0.3+0.7·|sin(t·speed+phase)|` |

关键尺寸：侧边栏 200px（折叠 48px）、标签条 38px、会话标签 30px（100–200px 宽）、面板头部 26px、路径栏 24px、文件行 22px、折叠条 28px、内容区 260px。

---

## 5. egui 重写映射

| Web 侧 | egui 侧 |
|---|---|
| xterm.js | `vt100::Parser` + 自绘单元格网格（`src/term.rs`） |
| ssh2 (libssh2) | `russh`（纯 Rust，ring 后端） |
| ssh2 SFTP + SCP | `russh-sftp` |
| rusqlite | rusqlite（同一份 schema） |
| RSA-OAEP + AES-GCM 信封 | 同格式 `ttenc:v1:`，可直接读原库 |
| Tauri `invoke` | `SessionManager` 上的方法 + `AppEvent` 事件总线 |
| `Channel<String>` | reader 任务直接写入共享 `vt100::Parser` |
| Zustand store | `AppState` + `actions.rs` |
| CSS 主题 | `theme.rs` 常量 + 自绘 widget（`widgets.rs`） |
| React 组件 | `ui/*.rs` 各模块 |

详见 `spec-backend.md`（后端逐命令规格）与 `spec-filemanager.md`（文件管理器逐交互规格）。
