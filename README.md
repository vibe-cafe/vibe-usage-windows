# Vibe Usage for Windows

Windows 版 [Vibe Usage](https://github.com/vibe-cafe/vibe-usage-app) —— 自动追踪 AI 编程工具的 Token 用量和费用，常驻系统托盘，数据同步到 [vibecafe.ai/usage](https://vibecafe.ai/usage)。

功能与视觉与 macOS 版 1:1 对齐（差异清单见 [docs/PARITY.md](docs/PARITY.md)）。

## 功能

- 系统托盘常驻，点击弹出 520×620 用量面板（暗色主题，与 macOS 版完全一致）
- 内置 [@vibe-cafe/vibe-usage](https://github.com/vibe-cafe/vibe-usage) CLI 与 Node 运行时，**开箱即用，无需安装 Node.js**，后台每 30 分钟自动同步
- 面板展示预估费用 / 总 Token / 缓存 Token / 活跃时长、每小时/每日趋势图、终端/工具/模型/项目四维分布环形图
- 支持 今天 / 24H / 7D / 30D / 90D / 自定义 时间范围，终端 / 工具 / 模型 / 项目 多选筛选（模型按家族分组）
- **订阅配额监控**：本地读取 Codex / Claude 的 5 小时 / 7 天 Token 配额，悬停查看消耗 vs 时间对比
- 托盘图标可显示今日费用 / Token（数字渲染进图标，完整数值在悬停提示中）
- 浏览器设备链接登录（与 CLI / macOS 版同一套 `vibecafe.ai` 设备授权流程）
- 开机自启动、单实例、应用内检查更新

## 安装

从 [Releases](https://github.com/haoruilee/vibe-usage-app-windows/releases/latest) 下载 `VibeUsage-x.y.z-Windows-Setup.exe` 并运行（per-user 安装，无需管理员权限；缺少 WebView2 时安装器会自动下载）。

## 配置

1. 点击托盘图标打开面板，点「登录并链接数据」
2. 浏览器自动打开 vibecafe.ai 授权页 —— 登录后确认验证码与面板一致
3. 点「确认链接」—— 应用自动拿到 Key 并开始同步

配置与 CLI 共享 `%USERPROFILE%\.vibe-usage\config.json`，可与 `npx @vibe-cafe/vibe-usage` 共存。

## 系统要求

- Windows 10 21H2+ / Windows 11，x64
- 无其他前置依赖（CLI 与 Node 22 运行时随应用捆绑）

## 从源码构建

```powershell
# 首次: 安装工具链 (Node 22 / Rust 1.88 / VS Build Tools)
pwsh -File scripts/setup-windows-build-env.ps1

pnpm install
pnpm run release:windows       # 产出 VibeUsage-<version>-Windows-Setup.exe + latest.json
```

开发调试：

```powershell
node scripts/vendor-cli.mjs    # 准备内置 CLI（一次即可）
pnpm tauri dev
```

## 测试

```bash
pnpm test                # 前端单测（formatters/aggregate/modelFamilies，与 Swift 实现对拍）
cargo test --workspace   # Rust 单测（config/codex 配额/claude 配额/statusline hook/托盘字体渲染）
```

## 架构

```
前端 (React + Tailwind, WebView2)     ← 视觉 1:1 复刻 macOS SwiftUI 视图
  └─ invoke / events
Rust (Tauri 2)
  ├─ tray / panel        托盘 + 无边框弹窗（定位/动画/失焦关闭）
  ├─ api_client          GET /api/usage、设备链接 code/poll
  ├─ sync_engine         spawn node <内置CLI> sync（120s 超时、CREATE_NO_WINDOW）
  ├─ scheduler           30 分钟定时同步 + 24h 更新检查
  ├─ rate_limit          Codex rollout JSONL / Claude statusline 捕获文件
  ├─ statusline_hook     写入 ~/.claude/settings.json 的 Node 包装器（自愈/备份/还原）
  └─ updater             latest.json + SHA-256 校验 + NSIS 静默升级
内置资源
  ├─ resources/cli       vendored @vibe-cafe/vibe-usage（含 Windows 补丁, scripts/vendor-cli.mjs）
  └─ resources/node      node.exe 22 LTS（scripts/fetch-node.mjs, 构建时下载）
```

## 相关项目

- [vibe-usage-app](https://github.com/vibe-cafe/vibe-usage-app) — macOS 版（本项目的功能与视觉基准）
- [@vibe-cafe/vibe-usage](https://github.com/vibe-cafe/vibe-usage) — 命令行同步工具
- [vibecafe.ai/usage](https://vibecafe.ai/usage) — Web 仪表盘

## License

MIT
