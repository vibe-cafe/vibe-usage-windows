# macOS ↔ Windows 对齐说明 (Parity Notes)

本项目以 `vibe-usage-app`（macOS, SwiftUI）为功能与视觉基准。本文档记录 1:1 对齐的映射关系与少数平台差异，最近一次按 macOS 0.8.0（`5d786c6`）核对是 2026-11-25（Cursor 单独登录按同期的 macOS 实现和《Cursor-加密凭据-方案》第 3 节文案，2026-11-26）；文中写明「Windows 尚未跟进」的，是两端还没对齐的部分。

## 视觉常量（源自 Swift 源码，落在 `tailwind.config.cjs`）

| Token | 值 | Swift 来源 |
|---|---|---|
| 面板尺寸 | 520×620，圆角 12（Windows 为默认尺寸，窗口可调整，见「平台差异」11） | `MenuBarController.panelWidth/Height`, contentView cornerRadius |
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
| 标题栏 | 两个窗口都固定深色（`tauri.conf.json` 的 `theme: "Dark"`，设置窗口的恢复建窗代码同样设置），Windows 浅色主题下也不会出现一条白色标题栏 | 面板无边框深色；设置窗口 `NSAppearance(named: .darkAqua)` |
| 品牌图形 | 单色，开启用 `#E9E9E9`、未开启变灰（面板图标行 45% 不透明度，设置里 `#9C9C9C` 再乘 72%）；面板图标行、卡片标题、设置共用 | `ProviderGlyph`，`SettingsTheme.text` / `secondaryText` |

聚合口径（`src/lib/aggregate.ts` ↔ Swift 各 View 的 filtered/chartData）：

- 总 Token = input + output + reasoning + cachedInput + cacheCreation5mTokens + cacheCreation1hTokens（`computedTotal`，旧数据缺少的缓存写入项按 0 算）
- 趋势图 Token 三段堆叠为 output + reasoning（白 0.9）、input + 缓存写入（白 0.5）、缓存读取（白 0.24）
- 自定义日期按本地日历选整日。Windows 的用量和活跃时长来自同一份 `/api/usage` 响应，请求携带选择日期时的 IANA 时区，以及起始日零点至末日次日零点前 1 毫秒的 UTC ISO 时刻（macOS #119）。
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
| 长名字 `.truncationMode(.middle)` + `.help()`（分布图例/筛选选项，同前缀项目名区分信息在尾部） | `components/MiddleTruncateLabel.tsx`（head 截断 + tail 6 字符不收缩 + `title` 悬浮全名；两段都 `whitespace-pre`，否则拆分处的空格会被 flex 子元素吞掉，显示成「Gemini 4Carbon」） |
| `SettingsView`（NSWindow 700×460，宽度固定、可以拉高，深色，左侧边栏五页：通用 / 订阅配额 / 数据同步 / 数据目录（高级）/ 关于） | `SettingsApp`（独立 WebView 窗口，默认 460×620、可调整；仍是改版前的单页竖排：数据同步 → 订阅配额 → 隐私保护 → 常规 → 数据目录（高级）→ 测试诊断（仅测试构建）→ 关于 → 危险操作。侧边栏改版 Windows 尚未跟进） |
| `SettingsQuotaStatus.status` / `AppState.quotaWarningSnapshot` | `SettingsApp` + `lib/quotaProducts.ts:quotaProductStatusText`。启用且刷新结束后显示读取原因短词。有数但暂停时也显示「已暂停」。设置窗口只读面板内存快照，监听 `rate-limits-updated`。ZCode 行保留按区域的 Key 配置状态。 |
| `AppState`（面板和设置共用一份内存状态） | `state/AppStateContext.tsx` + Rust `AppCtx`。面板和设置是两个 WebView，各存一份设置，靠 `settings-updated` 事件跟上对方；设置窗口改开关时在最新那份上改，不会把面板刚拖好的图标顺序冲掉（#53） |
| `APIClient` | `services/api_client.rs` |
| `SyncEngine`（内置 Node 跑 `cli-bootstrap.mjs`，120s） | `services/sync_engine.rs`（内置 Node 跑同一份 `cli-bootstrap.mjs`，120s，CREATE_NO_WINDOW） |
| 设置中的隔离运行时目录（Codex / Grok / Antigravity） | 原生文件夹选择器 + 同一组 CLI `config roots/add-root/remove-root` 命令 |
| `SyncScheduler`（30 分钟） | `services/scheduler.rs` |
| 八产品订阅配额目录与任意数量选择 | Codex / Claude 原生适配；Kimi / ZCode / Grok / OpenCode Go / Antigravity / AGY 使用版本化 typed CLI bridge（OpenCode Go 的凭据读取由内置 CLI 负责：2.x 凭据表 → 1.x `auth.json` 回退；Antigravity 由 CLI 启动用户已登录的 `agy` 读取，Windows 额外查官方默认安装位置，见 `docs/SUBSCRIPTION_QUOTAS.md`「Antigravity / AGY 探测约束」）；Cursor 同样走 CLI（`quota fetch --product cursor`，与 macOS 0.7.4 起一致；有单独登录时凭据经标准输入递给 CLI，见 `docs/SUBSCRIPTION_QUOTAS.md`「Cursor 探测约束」） |
| `RateLimitCardView`：`QuotaTabStripView`（整目录图标 Tab：已开启的单色图形在前、未开启的变灰在后、可拖动排序、最右齿轮进设置、已启用但非 ok 加琥珀点）+ 一行两张 240px 卡片横滑，Tab 与横滑双向联动（点 Tab 滑过去、滑动时高亮跟随前缘卡片）；卡片只属于已启用产品，全部关闭时只剩图标行 | `components/RateLimitCard.tsx` + `components/QuotaTabStrip.tsx`：同一模型（HTML5 拖放排序、前缘卡片经滚动位置判定、`quotaTabOrder` 顺序与 `quotaProductOrder` 持久化、齿轮调 `open_settings_window`） |
| `RateLimitCardView.allRows` 的 Codex 5h 占位行（`fiveHourNotEnforced` 或 Pro / Prolite →「不限额」，否则「近 5 小时无活动」；维护者 2026-10-24 定）与 `expectsFiveHourWindow` | `lib/quotaProducts.ts:codexFiveHourPlaceholder` / `expectsCodexFiveHour`（同一规则；Windows 原先显示「官方当前未启用」，已对齐） |
| `RateLimitCardView.emptyStateText` / `authorizationText` | `lib/quotaProducts.ts:quotaEmptyStateText` / `quotaAuthorizationText`。六种 CLI 空态、登录缺失与失效、首次占位和未知原因的卡片文案逐字对齐。 |
| `ProviderRateLimit.Source` / `ProviderRateLimit.LiveFailure`（卡片的实际来源和固定失败原因，一键诊断 1a） | Rust `RateLimitSource` / `RateLimitLiveFailure`（同一组代码，另加 Windows 独有的桌面端记录四种原因和凭据管理器）+ `lib/quotaProducts.ts:quotaFailureText` / `quotaProvenanceText`（文案与显示规则一致，见 `docs/SUBSCRIPTION_QUOTAS.md`「失败原因和来源」） |
| `RateLimitCardView.header`（名称旁琥珀色「重置券 ×N」小胶囊；卡片底部不写「数据截至 N 分钟前」，维护者 2026-10-24 删掉，嫌吵） | `RateLimitCard.tsx` 的 `ProviderCard` 同样；Windows 只在数字来自 Claude 桌面端记录（可能落后于账号）时，在底部多写一行「记录时间」，macOS 没有这一行 |
| `ProviderGlyph` / `RateLimitCardView.ProviderIcon`（Lobe Icons 单色图形，八家都有，包括 Antigravity / AGY；绘制尺寸 `size × 18 / 22` 再乘各家的填充比例；设置表格里 22 的格子画 18，未开启再降到 72% 不透明度） | `components/ProviderIcon.tsx`（同一份 SVG 路径数据和比例，`fill-rule: evenodd`；Lobe Icons 的 MIT 声明在 `THIRD_PARTY_NOTICES.md`，和 macOS 一样随安装包分发） |
| ZCode 两区域 Key | BigModel / Z.ai 分开存入 Windows Credential Manager；只向明确选择的区域请求注入 |
| Cursor 单独登录（设置里 Cursor 一行：未登录 + 登录 / 等待（含邮箱确认提示）/ 邮箱 + 退出 / 已失效 + 重新登录（旁边次要的退出），不带说明文字；Vibe Usage 自己的钥匙串条目；每次启动 CLI 用 `--cursor-login-stdin` 递凭据；到期前 7 天 `login cursor --refresh`；卡片 `signIn` 两个空态点开设置。2026-11-25 维护者批准，macOS 由同期 PR 实现） | `services/cursor_login.rs`（登录、续期、被拒后续期一次、失效、退出、检查 CLI 支不支持）+ `services/credential_store.rs`（Credential Manager 读写，和 ZCode 共用；条目 `ai.vibecafe.vibe-usage.windows/cursor/login`，超过单条 2560 字节时分条）+ `lib/cursorLogin.ts`（文案与 macOS 逐字一致）+ `RateLimitCard.tsx`（`signIn` 空态，整张卡片打开设置的 Cursor 一行）。见 `docs/SUBSCRIPTION_QUOTAS.md`「Cursor 探测约束」 |
| `RuntimeDetector` | `crates/core/runtime.rs`（Windows 路径 + 捆绑 node 兜底） |
| `CodexRateLimitReader` | `crates/core/rate_limit/codex.rs` |
| Claude 模型周额度（macOS #118 的 `modelScopedMeters`） | 实时回复和 Claude Code 缓存共用原始 `limits` 解析。Windows 现有配额卡在通用窗口后显示来源给出的模型名，如 `7d`、`Fable`。只有 `7d` 时就显示一行。 |
| `ClaudeUsageProbe`（Claude Code stdio `get_usage` 探测）+ `ClaudeUsageCache` | `src-tauri/src/services/claude_usage.rs`（探测参数与请求体与 macOS 逐项一致，见 `docs/SUBSCRIPTION_QUOTAS.md`「Claude 探测约束」）+ `crates/core/claude_code.rs`（候选程序：与 `discoverBinaries` 一样查 PATH，npm 的 `claude.cmd` 解析到它调用的 `claude.exe` / `cli.js`，不经 cmd.exe；macOS 最后还会试 Claude Desktop 自带的 Claude Code，Windows 尚无此项）+ `crates/core/rate_limit/claude_desktop.rs`（Desktop 记录；macOS 也读桌面端的 `plan-usage-history.json`，Windows 另要处理多处安装位置：`%APPDATA%\Claude`、MSIX 包的 `Packages\Claude_<后缀>\LocalCache\Roaming\Claude` 和 2026-11 起的 `%LOCALAPPDATA%\Claude-Data\claude_<后缀>`，同一包后缀的新旧目录算一份安装，见 `docs/SUBSCRIPTION_QUOTAS.md`） |
| 旧 `StatuslineHook` | `crates/core/statusline_hook.rs` 仅用于安全退休本应用可证明归属的旧 hook |
| 设置「关于」→「诊断信息」：「复制诊断信息」最小本机摘要 + App 本机保留最近 10 次同步摘要（一键诊断 1b；维护者 2026-11-09 在架构审批门批准方案修订二：「可以开工，你派活吧。 1、2、3我都同意。」） | `services/diagnostics.rs`（白名单结构化字段）+ `services/cli_doctor.rs`（并入 CLI 的 `doctor --json`，方案 2′）+ `services/sync_history.rs`（`sync-history.json`）+ `lib/diagnostics.ts`（排版与兜底过滤）；字段、脱敏和存放位置见 `docs/DIAGNOSTICS.md`。macOS 同一功能由 vibe-usage-app 按同一方案实现（并入 `doctor --json` 那一步 macOS 尚未做） |
| `MenuBarController`（NSStatusItem + NSPanel） | `tray.rs` + `panel.rs`（托盘 + 标准主窗口，可调整大小）+ `window_geometry.rs`（再次打开时缩到屏幕放得下再居中） |
| Sparkle | `services/updater.rs`（latest.json + SHA-256 + NSIS 被动模式覆盖升级，见 `docs/WINDOWS_UPDATE.md`） |
| `SMAppService`（登录项） | `auto-launch` crate（HKCU Run 注册表键，值带 `--autostart`；带它启动时只进托盘、不弹主窗口（2026-11-26 维护者定，G02），点托盘图标再打开；设了代理（含 PAC 暂时取不到）、有同步或配额要读时，读配额和每一次同步（手动的也算）都等到代理就绪——应用自己的代理变量，或同步服务器、各厂商主机的系统路由（含 PAC）指向的端口能连上——最多等 2 分钟，等待期间配额卡显示「正在读取」；一个代理都没设就不等，四家厂商的读取改由出口检测把关，与 macOS `LoginLaunch` 对齐；旧注册值在启动时补上参数，不碰任务管理器的启用开关） |
| `EgressCheck`（没检测到代理时，先匿名检测出口国家再读 Codex、Claude；设置「隐私保护 → 没检测到代理时」三选一（自动判断（推荐）/ 暂停读取 / 照常读取），转给 CLI 检测 Antigravity） | `src-tauri/src/egress_check.rs`（同样的检测主机、地区、缓存时长和文案；网卡地址用 `GetAdaptersAddresses` 求摘要；设置在「隐私保护」分区，同样的弹出菜单（`MenuSelect`，自绘、对齐 macOS 原生弹出菜单），存为 `settings.json` 的 `egressMode`（开发时的两个旧字段读入时映射）；环境变量 `VIBE_USAGE_EGRESS_CHECK=1|0` / `VIBE_USAGE_REQUIRE_PROXY=1|0`） |
| `SystemProxy`（按网址解析系统代理，PAC 由系统求值，交给 CLI、Claude Code 和厂商请求） | `src-tauri/src/system_proxy.rs`（注册表手动代理含按协议分开的写法和例外列表、PAC 由 WinHTTP 求值；CLI 的主机清单含 Cursor 登录用的 `api2.cursor.sh`；reqwest 每次请求现解析；CLI 和 Claude 探测按同一份系统代理拿变量，Claude Code 只拿 HTTP 代理；系统把 Claude 的主机交给 SOCKS、Claude Code 又拿不到 HTTP 代理时，Windows 不启动 Claude Code、卡片暂停，Mac 用本机中转 `ClaudeSOCKSRelay` 照常读取，Windows 待做） |

## 平台差异（有意为之）

1. **托盘文本**：macOS 菜单栏支持图标旁文字；Windows 托盘不支持。Windows 始终使用高对比 32×32 图标，开启「显示费用/Token」时完整数值显示在托盘悬停提示中，避免小尺寸任务栏图标变得不可读。
2. **「在 Dock 中显示」**：Windows 无 Dock，省略此设置项。托盘右键菜单提供「打开面板/立即同步/设置/退出」（Windows 惯例，macOS 无右键菜单，属增强）。
3. **运行时**：两端都自带 Node，跑同一份 `cli-bootstrap.mjs`（macOS 的 `check-cli-bootstrap.sh` 保证两份逐字节相同），检测顺序也一样：捆绑 node → 系统 node ≥22.5 → 系统 node ≥20 → bun。只差捆绑的 Node 主版本（Windows 22，macOS 24），用户无感，以后两端一起升。
4. **Claude 配额**：使用受限的只读官方 CLI 探测，不安装 statusline；启动时只退休旧版本可证明归属的包装器。
5. **自更新**：Sparkle → 自研（GitHub Releases latest.json、SHA-256 校验、带 `/P /UPDATE /R` 启动 NSIS 安装器：只显示进度条、直接覆盖、装完重开）。UI 入口一致：有新版时标题旁出现蓝色「发现更新」小标，设置里有「检查更新」。macOS 由 Sparkle 先问一次「安装并重新启动」；Windows 点「发现更新」（或设置里查到新版后点「安装更新」）后不再询问，下载期间按钮显示进度。流程、旧版本过渡和撤回见 `docs/WINDOWS_UPDATE.md`。
6. **面板关闭按钮**：macOS footer「关闭」= 退出应用（`NSApplication.terminate`）；Windows 同义（退出到无进程）。托盘常驻由开机自启保证。
7. **配额悬停 tooltip**：交互与内容 1:1；Windows 使用 mouse enter/leave（无 NSTrackingArea 差异）。
8. **配额卡片数量**：与 0.6.1 相同，不设上限、不折叠；选择顺序即卡片顺序，多于两张时横向滚动。首次启动勾选全部「已检测且已就绪」的产品（ZCode 未配置 Key 时除外；Antigravity / AGY 一律不自动勾选，要用户自己打开，2026-11-26 起；已保存的选择不变；与 macOS 一致）。
9. **设置页「托盘」分组**：macOS 的「菜单栏」对应 Windows 的「托盘」，两项合并进「常规」，不单列分组。
10. **品牌图形**（已对齐，保留编号）：两端原先都用彩色官方资产，现在都换成 Lobe Icons 的单色图形（macOS 2026-11-20，Windows 随后跟进），见上方「视觉常量」和「组件映射」。
11. **窗口可调整大小**（vibe-cafe/vibe-usage-windows#51）：macOS 面板固定 520×620，设置窗口 700×460、宽度固定只能拉高。Windows 是带标题栏的普通窗口，固定 620 高在小屏或高缩放下放不下（例如 1366×768 + 125%，逻辑高度约 614，再扣掉任务栏和标题栏），所以两个窗口都允许拖动调整：
    - 主窗口默认 520×620，最小 520×360。宽度不低于设计宽度，否则汇总卡片的数字会被截断；高度不够时内容区滚动。放宽时配额卡片（两张各占一半）、筛选下拉的位置、趋势图横轴跟着伸展。不能最大化。
    - 设置窗口默认 460×620，最小 460×360，整窗滚动；内容列随宽度伸展到 640 后居中，默认宽度下仍是原来的 430 列。
    - 建窗时不超出屏幕可用区（`preventOverflow`）；从托盘再次打开（或打开设置）时，若窗口比所在屏幕的可用区大，先缩小再居中，标题栏不会被推出屏幕。
    - 主窗口大小在隐藏到托盘和退出时写入 `window-size.json`（见下方文件表），下次启动恢复；设置窗口不记。

## CLI Windows 适配（不再内置快照）

应用不再内置 CLI：`src-tauri/resources/cli-bootstrap.mjs` 在每次调用时解析 npm `latest`、校验 sha512 后缓存并用内置 Node 运行。解析或下载 `latest` 失败时改跑缓存里最新的完整版本（缓存为空时照旧失败），国内访问 npm 时断时续也不会让同步失败；macOS 现在也自带 Node、跑同一份脚本，这层退路两端一致。原先由 `scripts/vendor-cli.mjs` 打的那批 Windows 补丁已随 `windows-support` PR 进入上游（`src/init.js` 的 `cmd /c start`、`codex.js` / `qwen-code.js` 的反斜杠 cwd、`opencode-roots.js` 的 `%LOCALAPPDATA%\opencode`、`amp.js` 的 `%LOCALAPPDATA%\amp\threads`，以及 `config.json` / `state.json` 被误建成目录时的自愈）。

两处不属于"已上游"：

1. **`STATE_DIR` 回退（改由应用侧承担）**：`src/state.js` 只识别 `VIBE_USAGE_STATE_DIR`，不跟随 `VIBE_USAGE_CONFIG_DIR`；`src/config.js` 的 `CONFIG_DIR` 则原生读取。因此 `sync_engine.rs` 同时导出 `VIBE_USAGE_CONFIG_DIR` 与 `VIBE_USAGE_STATE_DIR`（同一个应用配置目录），以保持 Windows 版"状态与配置同目录"的既有语义——否则状态会落到 `~/.vibe-usage/state.json`，卸载也不会清理。
2. **Codex 持久化用量记录（已上游，待发版）**：`src/parsers/codex-usage-record.js` 与 `codex.js` / `codex-cache.js` / `codex-segments.js` 的接线原先只存在于内置副本。现已作为 vibe-cafe/vibe-usage#121 提到上游（`node --test` 528 passed / 0 failed，新增 12 个用例；与内置副本逐字节等价，只剩注释差异）。含该修复的版本发布后，`tests/codexUsageRecord.test.ts` 即应转绿——这条用例就是验收信号。它并非 Windows 专属逻辑（`token_usage_record` 是 Codex 的日志格式），此前放在客户端里，macOS 端跑同一解析器时同样少算。

## 共享文件契约（与 CLI / macOS 版一致）

| 文件 | 说明 |
|---|---|
| `%USERPROFILE%\.vibe-usage\config.json` | `apiKey` / `apiUrl` / `hostname`（camelCase，与 CLI 共享）。正式版默认地址为 `https://vibefriends.com`，开发版默认使用 `http://localhost:3000`。App 在内存中将旧生产地址映射到新站，自定义地址沿用原值。设备绑定、手动 Key 校验、面板用量、「详情」和「排行榜」都使用解析后的地址。绑定成功时，若服务端返回旧生产地址则保存新地址，未返回地址则保存本次绑定所用地址。 |
| `%USERPROFILE%\.vibe-usage\state.json` | CLI 增量同步状态；「重置配置」时一并删除（修复 macOS/CLI 的 reset 不清 state 问题） |
| `%USERPROFILE%\.vibe-usage\claude-rate-limits.json` | 旧版本 statusline 留下的抓取文件；当前版本不再读取（`crates/core/rate_limit/claude.rs` 的旧解析器已无调用方），也不安装 statusline |
| `%USERPROFILE%\.codex\sessions\` | Codex 配额读取（只读） |
| `%APPDATA%\ai.vibecafe.vibe-usage.windows\settings.json` | 应用设置（对应 macOS UserDefaults） |
| `%APPDATA%\ai.vibecafe.vibe-usage.windows\window-size.json` | 主窗口上次的大小（逻辑像素，`{"width","height"}`），只由 App 读写；从未调整过就不建这个文件，内容损坏或尺寸不合理时忽略、用默认大小。「重置配置」不删除（只是界面状态） |
| `%APPDATA%\ai.vibecafe.vibe-usage.windows\sync-history.json` | App 自己的最近 10 次同步摘要（只有类型化字段，见 `docs/DIAGNOSTICS.md`）；「重置配置」时删除 |

## 验收清单

真机验收清单见 `docs/WINDOWS_ACCEPTANCE.md`。
