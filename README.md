# Vibe Usage

Windows 应用，自动追踪 AI 编程工具的 Token 用量和费用。App 常驻系统托盘；数据同步到 [vibecafe.ai/usage](https://vibecafe.ai/usage)。

功能与视觉均与 macOS 版 1:1 对齐（差异清单见 [docs/PARITY.md](docs/PARITY.md)）。

## 下载

从 [Releases](https://github.com/vibe-cafe/vibe-usage-windows/releases/latest) 下载 `VibeUsage-x.y.z-Windows-Setup.exe` 并运行（per-user 安装，无需管理员权限；缺少 WebView2 时安装器会自动下载）。

安装包由 Release workflow 通过 SignPath `Release` 策略提交 Authenticode 签名。Windows 仍可能因为新证书或下载量低显示 SmartScreen 声誉提示；如出现「Windows 已保护你的电脑」，点「更多信息」→「仍要运行」。

## 配置

1. 打开 Vibe Usage，点击「登录并链接数据」
2. 浏览器自动打开 vibecafe.ai 审批页面 — 登录后确认验证码与 app 一致
3. 点击「确认链接」 — app 自动拿到 Key 并开始同步

配置与 CLI 共享 `%USERPROFILE%\.vibe-usage\config.json`，可与 `npx @vibe-cafe/vibe-usage` 共存。

## 功能

- 系统托盘常驻，点击托盘图标打开用量面板
- 后台每 30 分钟自动同步数据，也可手动「更新数据」
- 弹出窗口查看费用、总 Token、缓存 Token、趋势图表
- **订阅配额监控**：可分别显示 Codex / Claude Code 的 5 小时 / 7 天 token 配额，悬停查看消耗 vs 时间对比
- 支持今天 / 24H / 7D / 30D / 90D / 自定义日期，以及终端 / 工具 / 模型 / 项目筛选
- 可在托盘图标显示今日费用和 Token 数
- 内置 [@vibe-cafe/vibe-usage](https://github.com/vibe-cafe/vibe-usage) CLI 与 Node 运行时，开箱即用，无需安装 Node.js
- 支持开机自启动、单实例、应用内检查更新

## 系统要求

- Windows 10 21H2+ / Windows 11，x64
- 无其他前置依赖（CLI 与 Node 22 运行时随应用捆绑）

## 从源码构建

```powershell
git clone https://github.com/vibe-cafe/vibe-usage-windows.git
cd vibe-usage-windows

# 首次：安装工具链 (Node 22 / Rust 1.88 / VS Build Tools)
pwsh -File scripts/setup-windows-build-env.ps1

pnpm install
pnpm run release:windows       # 产出 VibeUsage-<version>-Windows-Setup.exe + latest.json
```

代码签名构建可通过环境变量提供证书：

- `WINDOWS_CODESIGN_PFX_BASE64` + `WINDOWS_CODESIGN_PFX_PASSWORD`：Base64 编码的 PFX 证书及密码
- `WINDOWS_CODESIGN_CERT_THUMBPRINT`：已安装到证书库的代码签名证书 thumbprint
- `WINDOWS_CODESIGN_TIMESTAMP_URL`：可选，默认 `http://timestamp.digicert.com`

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
  ├─ tray / panel        托盘 + 标准主窗口（显示/聚焦/隐藏到托盘）
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
