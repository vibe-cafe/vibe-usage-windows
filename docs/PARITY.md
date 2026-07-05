# macOS ↔ Windows 对齐说明 (Parity Notes)

本项目以 `vibe-usage-app`（macOS, SwiftUI, v0.5.1）为功能与视觉基准。本文档记录 1:1 对齐的映射关系与少数平台差异。

## 视觉常量（源自 Swift 源码，落在 `tailwind.config.cjs`）

| Token | 值 | Swift 来源 |
|---|---|---|
| 面板尺寸 | 520×620，圆角 12 | `MenuBarController.panelWidth/Height`, contentView cornerRadius |
| 背景 | `#0A0A0A` | `Color(white: 0.04)` |
| 卡片 | bg `#171717` / 边框 `#292929` / 圆角 4 | `Color(white: 0.09/0.16)` |
| 文字 | `#FFFFFF` / `#A1A1A1` / `#616161` / `#808080` | `.white`, `white: 0.63/0.38/0.5` |
| 费用绿 | `#33CC80` | `(0.2, 0.8, 0.5)` |
| 活跃蓝 | `#6199FF` | `(0.38, 0.6, 1.0)` |
| 更新链接蓝 | `#66B3FF` | `(0.4, 0.7, 1.0)` |
| 清除红 | `#FF6B6B` | `(1.0, 0.42, 0.42)` |
| 配额条三段色 | `#D9D9D9` / `#F59E0B` / `#F04545`（<70 / 70–90 / ≥90） | `ProgressBar.color(for:)` |
| Donut 色板 | `#3B82F6 #0FBA83 #F59E0B #F04545 #8C5CF5 #ED4D99`，其他 `#525252` | `DistributionChartsView` |
| 开启动画 | 220ms `cubic-bezier(0.22,1,0.36,1)`，scale 0.9→1 + y4 + 70% 淡入 | `animateOpen` |
| 关闭动画 | 140ms `cubic-bezier(0.5,0,0.9,0.4)`，scale→0.94 | `animateClose` |

聚合口径（`src/lib/aggregate.ts` ↔ Swift 各 View 的 filtered/chartData）：

- 总 Token = input + output + reasoning + cachedInput（`computedTotal`）
- 趋势图 Token 三段堆叠：output+reasoning（白 0.9）/ input（白 0.5）/ cached（白 0.24）
- 模型筛选**不作用于 sessions**（活跃时长）— macOS 既有行为，保持一致
- 「今天」与「24H」同拉 `days=1`，仅客户端 midnight cutoff 不同（菜单栏/托盘统计同样应用 cutoff）
- Donut Top6 + 其他；空维度值显示「未知」

## 组件映射

| macOS (Swift) | Windows (React/Rust) |
|---|---|
| `PopoverView` | `PopoverApp` + `DashboardView` |
| `RateLimitCardView` | `components/RateLimitCard.tsx` |
| `FilterTagsView` | `components/FilterTags.tsx` |
| `SummaryCardsView` | `components/SummaryCards.tsx` |
| `BarChartView` | `components/TrendChart.tsx`（自绘 div 堆叠条） |
| `DistributionChartsView` | `components/DistributionGrid.tsx`（自绘 SVG donut） |
| `SettingsView` (NSWindow 460×480) | `SettingsApp`（独立 WebView 窗口 460×620；Windows 保持首屏展示「关于」区域） |
| `AppState` | `state/AppStateContext.tsx` + Rust `AppCtx` |
| `APIClient` | `services/api_client.rs` |
| `SyncEngine`（npx/bun x，120s） | `services/sync_engine.rs`（内置 CLI + node，120s，CREATE_NO_WINDOW） |
| `SyncScheduler`（30 分钟） | `services/scheduler.rs` |
| `RuntimeDetector` | `crates/core/runtime.rs`（Windows 路径 + 捆绑 node 兜底） |
| `CodexRateLimitReader` | `crates/core/rate_limit/codex.rs` |
| `ClaudeRateLimitReader` | `crates/core/rate_limit/claude.rs` |
| `StatuslineHook`（bash 包装器） | `crates/core/statusline_hook.rs`（**Node 包装器**，Windows 无 bash） |
| `MenuBarController`（NSStatusItem + NSPanel） | `tray.rs` + `panel.rs`（托盘 + 无边框置顶窗口） |
| Sparkle | `services/updater.rs`（latest.json + SHA-256 + NSIS） |
| `SMAppService`（登录项） | `auto-launch` crate（HKCU Run 注册表键） |

## 平台差异（有意为之）

1. **托盘文本**：macOS 菜单栏支持图标旁文字；Windows 托盘不支持。Windows 始终使用高对比 32×32 图标，开启「显示费用/Token」时完整数值显示在托盘悬停提示中，避免小尺寸任务栏图标变得不可读。
2. **「在 Dock 中显示」**：Windows 无 Dock，省略此设置项。托盘右键菜单提供「打开面板/立即同步/设置/退出」（Windows 惯例，macOS 无右键菜单，属增强）。
3. **运行时**：macOS 版要求用户自装 Node/Bun 并用 `npx --yes` 每次联网解析；Windows 版捆绑打过补丁的 CLI 与 Node 22 运行时，离线可同步、版本可控（检测顺序：捆绑 node → 系统 node ≥22.5 → 系统 node ≥20 → bun）。
4. **statusline 包装器**：bash → Node 脚本（行为逐行对齐：rate_limits 摘取、原子写、原命令同 stdin 转发、备份/自愈/还原）。
5. **自更新**：Sparkle → 自研（GitHub Releases latest.json、SHA-256 校验、启动 NSIS 安装器）。UI 入口一致（footer「发现更新」+ 设置「检查更新」）。
6. **面板关闭按钮**：macOS footer「关闭」= 退出应用（`NSApplication.terminate`）；Windows 同义（退出到无进程）。托盘常驻由开机自启保证。
7. **配额悬停 tooltip**：交互与内容 1:1；Windows 使用 mouse enter/leave（无 NSTrackingArea 差异）。

## CLI Windows 补丁（vendored，见 `scripts/vendor-cli.mjs`）

上游 `@vibe-cafe/vibe-usage@0.9.8` 的四处 Windows 问题，vendor 时自动打补丁（补丁锚点丢失会构建失败，防止 CLI 升级后静默失效）：

1. `src/init.js` `openBrowser`：`execFile('start')` → `cmd /c start ""`（`start` 是 cmd 内建命令）
2. `src/parsers/codex.js` `extractProject`：`split('/')` → `split(/[\\/]/)`（Windows cwd 反斜杠）
3. `src/parsers/qwen-code.js`：同上
4. `src/parsers/opencode.js` / `amp.js`：增加 `%LOCALAPPDATA%` 数据目录探测（原 XDG 路径保底）

以上四处建议同步提交上游 PR；合并后 vendor 脚本的补丁会因锚点变化自动报错提醒移除。

## 共享文件契约（与 CLI / macOS 版一致）

| 文件 | 说明 |
|---|---|
| `%USERPROFILE%\.vibe-usage\config.json` | `apiKey` / `apiUrl` / `hostname`（camelCase，与 CLI 互写） |
| `%USERPROFILE%\.vibe-usage\state.json` | CLI 增量同步状态；「重置配置」时一并删除（修复 macOS/CLI 的 reset 不清 state 问题） |
| `%USERPROFILE%\.vibe-usage\claude-rate-limits.json` | statusline 包装器写入的配额快照 |
| `%USERPROFILE%\.vibe-usage\vibe-usage-statusline.js` | Node 包装器（macOS 为 .sh） |
| `%USERPROFILE%\.vibe-usage\statusline-original` / `settings.json.vibe-bak` | 原命令 sidecar / Claude 设置备份 |
| `%USERPROFILE%\.claude\settings.json` | statusline hook 安装点（尊重 `CLAUDE_CONFIG_DIR`） |
| `%USERPROFILE%\.codex\sessions\` | Codex 配额读取（只读） |
| `%APPDATA%\ai.vibecafe.vibe-usage.windows\settings.json` | 应用设置（对应 macOS UserDefaults） |

## 验收清单

见仓库根 `../VIBE-USAGE-WINDOWS-PLAN.md` §11（40+ 项逐条勾验）。
