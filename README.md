# RescueForge - 轻量跨平台数据恢复工具

> RescueForge 是一款轻量、跨平台、高性能的数据恢复桌面应用，融合现代前后端技术，兼顾专业级恢复能力和消费级使用体验。

## ✨ 核心特性

- **极致轻量与跨平台**：安装包小于 15 MB，一套代码编译出 Windows、macOS、Linux 原生应用
- **Rust 驱动的高性能引擎**：利用 Rust 的零成本抽象和 `rayon` 数据并行，扫描速度提升 3-5 倍
- **现代化 UI 与实时反馈**：基于 Vue 3 的暗色科幻 Glassmorphism 界面，通过 Tauri Events 实时推送扫描进度
- **Sidecar 安全提权**：主进程永不提权，独立 Sidecar 进程通过 UAC/sudo 提权读取磁盘，杜绝前端 XSS 风险
- **多级扫描引擎**：快速扫描（元数据）→ 深度扫描（目录树）→ RAW 雕刻（魔术字节匹配，100+ 签名）
- **文件预览与恢复车**：内存级预览（图片/文档/音视频），拖入恢复车一键导出，SHA-256 完整性校验
- **分区修复向导**：丢失分区搜索、MBR/GPT 重建、引导扇区备份/修复
- **磁盘镜像**：支持 .img/.dd 格式，坏道跳过
- **深度恢复已删除文件**：按目录过滤扫描，支持 Shift+Delete 强制删除、回收站清空后残留数据恢复
- **自动更新**：内置检查更新，GitHub Actions 自动打包发布，签名校验安全升级

## 🏗️ 技术栈

| 层级 | 技术 | 说明 |
|------|------|------|
| 前端 | Vue 3 + TypeScript + Vite + Pinia + TailwindCSS | 响应式 UI，暗色 Glassmorphism 主题 |
| 图表 | ECharts + Canvas API | 磁盘热力图，进度可视化 |
| 桌面框架 | Tauri 2.x | 原生窗口，< 15MB 安装包 |
| 后端引擎 | Rust (Tokio + Rayon + rusqlite) | 磁盘 I/O，文件系统解析，多线程雕刻 |
| 数据存储 | SQLite (WAL 模式) | 扫描历史，文件签名库，用户设置 |

## 🚀 快速开始

### 环境要求

- **Node.js** >= 18
- **Rust** >= 1.75 (with `rustup`)
- **Windows**: Visual Studio Build Tools (MSVC)
- **Linux**: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`
- **macOS**: Xcode Command Line Tools

### 安装依赖

```bash
# 安装前端依赖
cd src-ui
pnpm install

# 安装 Rust 工具链
rustup target add wasm32-unknown-unknown
cargo install tauri-cli --version "^2"
```

### 开发模式

```bash
# 一键启动 (前端 + 后端热重载)
cargo tauri dev

# 或仅启动前端开发
cd src-ui
pnpm run dev
```

### 构建生产版本

```bash
# 构建前端
cd src-ui
pnpm run build

# 构建桌面应用安装包（版本号自动取自 src-ui/package.json）
cd src-tauri
cargo tauri build
```

> 打包产物位于 `src-tauri/target/release/bundle/`。

## 📦 版本管理

应用版本以 **`src-ui/package.json` 的 `version` 字段为唯一来源**，通过以下机制自动同步，无需多处手改：

| 位置 | 机制 |
|------|------|
| 打包版本 | `tauri.conf.json` 用 `"version": "../src-ui/package.json"` 引用 |
| 前端显示（状态栏/关于页） | 运行时 `getVersion()` 读后端，dev 回退编译期注入的 `__APP_VERSION__` |
| 自动更新 | 见下文「自动更新」 |

**发布新版本只需改一处：**

```bash
# 1. 修改 src-ui/package.json 的 version（如 0.1.0 -> 0.1.1）
# 2. 打标签并推送，触发 CI 自动打包
git tag v0.1.1
git push origin v0.1.1
```

## 🔄 自动更新

应用内置「检查更新」功能（设置页 → 关于 → 检查更新），依赖 Tauri 官方 updater 插件。
旧版本用户点击后自动下载最新安装包并安装。

### 首次配置：生成签名密钥

更新包需要签名校验，私钥用于 CI 打包时签名、公钥内嵌到应用用于验签。**私钥绝不能提交到仓库或泄露**。

```bash
# 生成签名密钥对（生成 .key 私钥文件和 .key.pub 公钥文件）
cargo tauri signer generate -w ~/.tauri/rescueforge.key
```

命令执行后会：
1. 提示输入私钥密码（可选但强烈建议设置，CI 需要用到）
2. 生成两个文件：
   - `~/.tauri/rescueforge.key` —— **私钥**（保密，勿提交）
   - `~/.tauri/rescueforge.key.pub` —— **公钥**（用于应用内验签）

随后完成两步配置：

**① 把公钥填入 `src-tauri/tauri.conf.json`**

打开 `~/.tauri/rescueforge.key.pub`，把内容填入 `plugins.updater.pubkey`：

```json
{
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://github.com/WeedFire/RescueForge/releases/latest/download/latest.json"
      ],
      "pubkey": "这里填 rescueforge.key.pub 文件里的内容"
    }
  }
}
```

**② 把私钥配置到 GitHub Secrets（CI 打包时用）**

进入仓库 `Settings → Secrets and variables → Actions`，新增 secret：

| Secret 名称 | 值 |
|-------------|-----|
| `TAURI_SIGNING_PRIVATE_KEY` | 私钥文件内容（见下方 base64 编码说明） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成密钥时设置的密码（未设密码可省略） |

私钥文件内容转 base64（供 `TAURI_SIGNING_PRIVATE_KEY` 使用）：

```bash
# Linux / macOS
base64 -w0 ~/.tauri/rescueforge.key
```

```powershell
# Windows PowerShell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("$env:USERPROFILE\.tauri\rescueforge.key"))
```

> ⚠️ 一旦丢失私钥或密码，将无法再为更新包签名，自动更新功能永久失效，请妥善备份。

### 更新流程（每次发布）

1. 改 `src-ui/package.json` 的 `version`
2. `git tag v<version> && git push origin v<version>`
3. CI 自动打包并发布 Release（含 `latest.json`）
4. 已安装用户点「检查更新」即可自动升级

## 🤖 GitHub Actions 自动打包

`.github/workflows/release.yml` 会在推送 `v*` 标签（或手动触发）时，自动构建并发布 Release：

- **4 个平台**：Windows（x64）、Linux（x64）、macOS（Apple Silicon + Intel）
- **发布产物**：各平台安装包 + 自动生成的 `latest.json`（供自动更新使用）
- **触发方式**：`git push origin v0.1.1`（打标签后推送），或 GitHub 网页 Actions 页手动 `Run workflow`

首次使用前需完成上面「自动更新」章节的密钥配置，否则 Release 构建会因缺少签名密钥失败。

## 📁 项目结构

```
RescueForge/
├── src-ui/                      # Vue 3 前端
│   ├── src/
│   │   ├── components/          # UI 组件 (layout/dashboard/scanning/results/partition)
│   │   ├── views/               # 页面 (Dashboard/Scanning/Results/PartitionRepair/Settings)
│   │   ├── stores/              # Pinia 状态管理 (disk/scan/file/settings/ui)
│   │   ├── composables/         # 组合式函数 (useTauriEvent/useScanControl/useTheme)
│   │   ├── api/                 # Tauri IPC 封装
│   │   ├── i18n/                # 国际化 (中文/英文)
│   │   ├── router/              # Vue Router 配置
│   │   └── types/               # TypeScript 类型定义
│   ├── package.json
│   └── vite.config.ts
├── src-tauri/                   # Tauri 主进程 (Rust)
│   ├── src/
│   │   ├── commands/            # Tauri 命令 (disk/scan/partition/settings)
│   │   ├── privilege.rs         # 跨平台权限检测与提权
│   │   ├── scheduler.rs         # Tokio 任务调度器
│   │   ├── database.rs          # SQLite 数据持久化
│   │   ├── sidecar.rs           # Sidecar 进程管理
│   │   ├── main.rs              # 程序入口
│   │   └── lib.rs               # 库入口
│   ├── Cargo.toml
│   └── tauri.conf.json
├── rescueforge-core/            # Rust 核心共享库
│   ├── src/
│   │   ├── types.rs             # 数据模型 (DiskInfo/ScanSession/RecoveredFile...)
│   │   ├── block_device.rs      # BlockDevice trait
│   │   ├── events.rs            # 事件类型 (ScanEvent/SidecarMessage...)
│   │   └── signatures.rs        # 100+ 文件签名库
│   └── Cargo.toml
├── disk-scanner/                # Sidecar 独立二进制 (提权磁盘扫描)
│   ├── src/
│   │   ├── main.rs              # Sidecar 入口 (JSON Lines IPC)
│   │   ├── raw_disk.rs          # 跨平台 Raw Disk I/O
│   │   ├── partition.rs         # MBR/GPT 分区解析
│   │   ├── smart.rs             # S.M.A.R.T. 检测
│   │   ├── ntfs.rs              # NTFS 文件系统解析
│   │   ├── fat.rs               # FAT32/exFAT 文件系统解析
│   │   ├── ext4.rs              # ext4 文件系统解析
│   │   ├── carving.rs           # RAW 文件雕刻引擎
│   │   ├── partition_rebuild.rs # 分区修复重建
│   │   ├── boot_sector.rs       # 引导扇区备份修复
│   │   ├── disk_image.rs        # 磁盘镜像 (.img/.dd)
│   │   └── ipc.rs               # IPC 协议工具
│   └── Cargo.toml
├── .github/workflows/release.yml # GitHub Actions 自动打包
├── Cargo.toml                   # Rust Workspace 配置
├── 设计文档.md                   # 详细设计文档
└── README.md
```

## 🔒 安全设计

- **Sidecar 隔离**：主进程保持普通用户权限，仅对独立编译的 `disk_scanner` 二进制提权
- **强制只读**：所有底层磁盘操作以只读方式打开，`BlockDevice` trait 仅暴露读取方法
- **IPC 安全**：Sidecar 通过 stdout JSON Lines 单向输出，不包含网络功能
- **CSP 策略**：前端严格内容安全策略，禁止内联脚本
- **二次确认**：所有不可逆操作（分区修复、引导扇区写入）强制弹出确认对话框

## 📝 开发阶段

- **Phase 1** ✅ 核心引擎与基础 UI：跨平台 Raw Disk I/O、MBR/GPT 解析、Vue 3 + Tauri 脚手架、S.M.A.R.T. 监测、任务调度
- **Phase 2** ✅ 快速扫描与文件恢复：NTFS/FAT/exFAT 解析、虚拟滚动文件树、文件预览、恢复车
- **Phase 3** ✅ RAW 雕刻与深度扫描：多线程文件雕刻引擎、100+ 签名、磁盘热力图、暂停/继续/取消
- **Phase 4** ✅ 分区修复与高级功能：分区重建、引导扇区修复、磁盘镜像、i18n、主题切换

## 📄 许可证

MIT License - 详见 LICENSE 文件
