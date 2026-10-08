# Vibe Usage

Windows 应用，自动追踪 AI 编程工具的 Token 用量和费用。App 常驻系统托盘；数据同步到 [vibecafe.ai/usage](https://vibecafe.ai/usage)。

## 下载

从 [Releases](https://github.com/vibe-cafe/vibe-usage-windows/releases/latest) 下载 `VibeUsage-x.y.z-Windows-Setup.exe` 并运行（per-user 安装，无需管理员权限；缺少 WebView2 时安装器会自动下载）。

安装包由 Release workflow 通过 SignPath `Release` 策略提交 Authenticode 签名。Windows 仍可能因为新证书或下载量低显示 SmartScreen 声誉提示；如出现「Windows 已保护你的电脑」，点「更多信息」→「仍要运行」。

## 配置

本机订阅配额无需 Vibe Usage 账号即可使用。若还需要跨设备 Token/费用统计：

1. 打开 Vibe Usage，在订阅配额下方点击「登录并链接数据」
2. 浏览器自动打开 vibecafe.ai 审批页面 — 登录后确认验证码与 app 一致
3. 点击「确认链接」 — app 自动拿到 Key 并开始同步

配置与 CLI 共享 `%USERPROFILE%\.vibe-usage\config.json`，可与 `npx @vibe-cafe/vibe-usage` 共存。

## 功能

- 系统托盘常驻，点击托盘图标打开用量面板
- 后台每 30 分钟自动同步数据，也可手动「更新数据」
- 弹出窗口查看费用、总 Token、缓存 Token、趋势图表
- **订阅配额监控**：自动检测 Codex、Claude Code、Kimi Code、ZCode、Grok、Cursor 与 OpenCode Go；面板顶部是一排产品图标 Tab（可拖动排序，最右侧齿轮图标进设置）：已生效的排在前面且保持品牌彩色，未生效的置灰排在后面、点它直接进设置；已生效但取不到数据的产品，Tab 上会多一个琥珀色小点。下方是**已生效**产品的卡片，一行两张、横向滑动 —— 点 Tab 滑到对应产品，滑动时高亮的 Tab 也会跟着走；全部关掉时这一段只剩那排图标。Cursor 当前明确标记为待接入
- 未读到数据时不猜：只有数据源明确报告配额用尽才显示「已用满」，其余情况按来源分别显示「正在读取…」「当前没有生效的额度窗口」「暂未读取到订阅配额数据」或「未检测到本机安装或登录」
- Codex / Claude 使用只读原生适配；Kimi Code 使用官方 CLI 登录；Grok 只读官方 CLI 的结构化配额日志；ZCode 使用用户明确提供的 BigModel（国内）或 Z.ai（海外）Coding Plan Key
- ZCode Key 只保存在当前 Windows 用户的 Credential Manager 中，不写入设置文件、不回显，也不会跨区域试发
- 支持今天 / 24H / 7D / 30D / 90D / 自定义日期，以及终端 / 工具 / 模型 / 项目筛选
- 可在托盘图标显示今日费用和 Token 数
- 内置 [@vibe-cafe/vibe-usage](https://github.com/vibe-cafe/vibe-usage) CLI 与 Node 运行时，开箱即用，无需安装 Node.js
- 可在设置中为 Codex、Grok、Antigravity / AGY 添加多个 Multica 或其他隔离运行时目录；各工具默认目录仍会继续扫描
- 订阅配额读取对齐 macOS：Codex 优先读取实时官方用量、离线回退会话日志；Claude 使用无工具、无提示、无会话持久化的只读探测，不修改 Claude 状态栏配置
- 内置 CLI 是仓库中已审查并固定的快照（版本见 `package.json` 的 `vibeUsageCliVersion`）；发布构建直接打包该快照，不会在构建期从 npm 解析或下载新的 Vibe Usage CLI，用户机器也不会在运行时拉取或执行未随安装包验证的新代码。npm `latest` 仅在维护者显式执行 `node scripts/vendor-cli.mjs` 更新快照时用于选择候选 CLI 版本
- workflow_dispatch 生成的外测包可导出严格脱敏的配额诊断；正式 tag Release 不编译诊断实现，设置入口也不会显示
- 支持开机自启动、单实例、应用内检查更新

需要 pnpm 10.8 或更新的 10.x。仓库 `.pnpmfile.cjs` 在 Windows 将 virtual store 放到 `%LOCALAPPDATA%/vbu-pnpm-vstore/<checkout-path-hash>`；按规范化 checkout 路径隔离，而不是只按版本号隔离。其他平台保留 pnpm 默认目录。用户显式 `virtual-store-dir` 配置优先；不要让独立 checkout 显式共用同一目录。修改 hook 后应同步更新锁文件中的 pnpmfileChecksum，保留原锁定依赖。

Release 构建入口在编译阶段重映射用户目录、Cargo/Rustup 自定义目录和源码路径，并恢复调用者的 Rust flags。必须扫描安装后的程序确认无私有构建路径，不能只扫描压缩安装器。额外回归：`node --test scripts/test-pnpm-store.cjs`、`powershell -NoProfile -File scripts/test-windows-rust-paths.ps1`（也用 PowerShell 7 执行）。

Windows 开发时通过 `scripts/cargo-windows.ps1` 执行 Cargo 子命令；原生 `cargo test` 不会自动调用本项目脚本，在长路径下仍可能触发 MSVC LNK1104。

长路径源码目录构建时，脚本自动把 Cargo 输出放到 `%LOCALAPPDATA%\vbu-t\<工作区哈希>`；源码和 Vite 工作目录保持原路径。显式设置的 `CARGO_TARGET_DIR` 优先，安装包和签名验证都使用实际输出目录。Windows 修复基线及尚未完成的验收见 [任务单](docs/WINDOWS_ACCEPTANCE.md)。

## 系统要求

- Windows 10 21H2+ / Windows 11，x64
- 无其他前置依赖（CLI 与 Node 22 运行时随应用捆绑）

## 从源码构建

本地外测最简单的方式：解压源码包后双击 `BUILD-WINDOWS-EXTERNAL-TEST.cmd`。首次运行可能通过 winget 安装 Node 22、Rust 1.88、Visual C++ Build Tools 与 Windows SDK；完成后安装包会出现在源码包根目录。外测包使用独立的 “Vibe Usage Test” 应用身份，不会覆盖已安装的正式版。

外测包不启动正式版更新轮询，设置中明确显示不检查更新，后端也拒绝检查和安装更新命令。外测身份不等于账户数据全部隔离：Release 外测仍使用现有 Vibe Usage 账号配置和正式 ZCode 凭据目标，测试前请保留现有状态。完整任务单见 [Windows 原生验收](docs/WINDOWS_ACCEPTANCE.md)。

也可以手动运行：

```powershell
git clone https://github.com/vibe-cafe/vibe-usage-windows.git
cd vibe-usage-windows

# 首次：安装工具链 (Node 22 / Rust 1.88 / VS Build Tools)
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/setup-windows-build-env.ps1

pnpm install
pnpm run release:windows       # 产出 VibeUsage-<version>-Windows-Setup.exe + latest.json
pnpm run release:windows:test  # 产出带脱敏诊断的本地外测安装包，不生成发布清单
```

代码签名构建可通过环境变量提供证书：

- `WINDOWS_CODESIGN_PFX_BASE64` + `WINDOWS_CODESIGN_PFX_PASSWORD`：Base64 编码的 PFX 证书及密码
- `WINDOWS_CODESIGN_CERT_THUMBPRINT`：已安装到证书库的代码签名证书 thumbprint
- `WINDOWS_CODESIGN_TIMESTAMP_URL`：可选，默认 `http://timestamp.digicert.com`

开发调试：

```powershell
node scripts/check-version.mjs # 确认已内置的受测 CLI，不要在验收中替换为 npm latest
pnpm tauri dev
```

## 测试

```bash
pnpm test                # 前端单测（formatters/aggregate/modelFamilies，与 Swift 实现对拍）
powershell -NoProfile -File scripts/cargo-windows.ps1 test --workspace   # Rust 单测（配置迁移、产品发现/选择、配额桥、凭据边界等）
powershell -NoProfile -File scripts/cargo-windows.ps1 test --workspace --features external-test-diagnostics # 外测更新隔离
node scripts/test-vendored-cli.mjs --tests-from ../vibe-usage # 对实际内置 CLI 运行同版本的上游测试
```

最后一项需要完整的 CLI Git checkout，其版本必须与内置快照一致（来源记录在 `src-tauri/resources/cli/.vibe-usage-source.json`）；npm 包没有 `test/` 目录，直接在里面运行 `node --test` 得到 0 项不能作为验收通过。

当前“活跃时长”按会话累加 `activeSeconds`，并行会话会重复计时，Codex 单轮内也没有空闲截断；它不是人的实际使用时长。此轮不改变共享统计算法，跨端口径与历史数据处理另行评审。

## 架构

```
前端 (React + Tailwind, WebView2)     ← 视觉 1:1 复刻 macOS SwiftUI 视图
  └─ invoke / events
Rust (Tauri 2)
  ├─ tray / panel        托盘 + 标准主窗口（显示/聚焦/隐藏到托盘）
  ├─ api_client          GET /api/usage、设备链接 code/poll
  ├─ sync_engine         spawn node <内置CLI> sync（120s 超时、CREATE_NO_WINDOW）
  ├─ scheduler           面板可见时约 1 分钟、托盘后台 30 分钟同步 + 24h 更新检查
  ├─ rate_limits         Codex / Claude 原生读取 + Kimi / ZCode / Grok typed CLI bridge
  ├─ quota_product       只读本地发现 + 两项选择策略（Cursor 待接入）
  ├─ zcode_credentials   Windows Credential Manager 安全存储
  ├─ statusline_hook     仅安全退休旧版本能够证明归属的 Claude hook
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

## 自动刷新与跨设备统计

- 面板可见且未最小化时，本机日志约每分钟同步一次（15 秒调度检查；同步耗时另计）；隐藏到托盘后保持 30 分钟间隔。前端每分钟重新获取云端用量、同步状态及已选择的订阅配额，慢请求不会在定时器内重叠。
- 费用默认汇总当前账号、时间范围内的全部终端。设备/工具/模型/项目筛选仍然有效，界面显示统计范围、最近一次成功获取云端数据的时间及刷新失败提示。比较 Mac 与 Windows 时，应使用相同账号、时间范围、时区和筛选。
- Windows 的 Claude 桌面发现同时支持普通 Roaming 和打包应用的 LocalCache 目录；安装目录类型不代表软件下载渠道。
- Claude 桌面额度仅只读解析已有的 `plan-usage-history.json` v2，不访问桌面凭据或改动 Claude。多安装或多组织记录不猜测账号；记录超过 15 分钟标为历史非实时，超过 24 小时不显示百分比。它是账户额度观察值，不能转换为本机 Token、费用或实时余额。记录更新速度由 Claude 决定。

- Codex 同时识别持久化 `token_usage_record` 与旧 `token_count`：缺少旧事件时仍采集请求用量，两种事件同时存在时不重复计数。解析缓存版本升级后会自动重扫已有日志，无需修改原始日志或手动清空上传记录。
