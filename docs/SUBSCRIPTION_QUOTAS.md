# 订阅配额实现与复测边界

## 用量项顺序

2026-09-19 已批准：通用时间窗口按周期从短到长排列。确定的通用别名使用紧凑标签（`Daily` → `1d`、`Weekly` / `1w` → `7d`），所以同时存在两个常用窗口时始终显示 `5h` 在上、`7d` 在下。产品专属、模型专属和功能额度随后保持各自原始相对顺序。前端也会对旧 CLI 快照应用此规则；没有 5h 窗口的产品直接从首个真实窗口开始。

Codex 付费套餐（Plus / Pro / Prolite / Business / Business ProLite）没有拿到 5h 窗口时，卡片仍保留 5h 这一行并说明原因，规则与 macOS 相同（维护者 2026-10-24 定，见 macOS `AGENTS.md`「"Not enforced" vs "no data" for the Codex 5h window」）：实时用量接口会列出所有生效的窗口，它没给 5h 就说明官方关掉了这个限额（`fiveHourNotEnforced`），显示「不限额」；OpenAI 对 Pro / Prolite 一直关着 5h，这两种套餐不论数据来自接口还是本地日志都显示「不限额」；其余情况数据来自本地会话日志，少了 5h 窗口可能只是近 5 小时没用，显示「近 5 小时无活动」。Business ProLite 不算 Pro 套餐，只在接口确认没有 5h 时显示「不限额」。实现在 `src/lib/quotaProducts.ts:codexFiveHourPlaceholder`。

用户可以选择任意数量的订阅产品，选择顺序即卡片顺序；面板为每个已选产品始终渲染一张卡片，多于两张时横向滚动，不折叠成一句通用提示。自动检测只影响首次推荐（首次启动会勾选全部已检测且已就绪的产品，Antigravity / AGY 除外：Google 表示不支持第三方读取额度，要用户自己打开；已保存的选择不变）；手动选择不受安装路径限制。

## 数据流和职责

- `crates/core/src/quota_product.rs`：应用侧产品目录，集中维护名称、可用性、CLI 路由、常规目录和命令检测。Windows 应用安装位置在同一模块适配。
- `commands.rs`：启动时初始化一次默认选择，后续产品查询不写设置；处理选择、区域和用户输入的凭据。
- `rate_limits.rs`：刷新调度和短期内存缓存。Codex/Claude 保留原读取实现（独立 Claude Code 的 `rate_limits_available:false` 给出 `sessionWithoutPlanLimits` 空态；桌面端自带程序的同一答复触发本地回退），其他已接入产品通过 `quota_cli.rs` 使用固定 CLI 的 JSON 协议；CLI 结果里的可选 `emptyReason` 会映射到 `RateLimitEmptyReason`，缺失或未知取值保持 `None`（向前兼容）。每张卡片的快照另带 `source`（实际来源）和 `liveFailure`（固定失败原因），见下文「失败原因和来源（不放原文）」。
- 前端从目录获取产品名称，使用共享卡片展示配额；品牌图形是与 macOS 同一份的 Lobe Icons 单色路径（八家都有，包括 Antigravity / AGY），图标行、卡片与设置页共用 `src/components/ProviderIcon.tsx`。类型定义保留编译期检查，产品特有的登录提示仍由 UI 处理。

## Claude 探测约束（与 macOS 逐项一致）

`src-tauri/src/services/claude_usage.rs` 启动 Claude Code 的参数与 macOS `ClaudeUsageProbe.processArguments` 相同，单元测试锁定：

- 查找程序与「已检测」用同一条规则（`crates/core/src/claude_code.rs`，2026-11-09 起）：依次是 `VIBE_USAGE_CLAUDE_BIN`、`~\.local\bin\claude.exe`、`~\.claude\local\claude.exe`、`%LOCALAPPDATA%\Programs\Claude Code\claude.exe`，再按 PATH 目录顺序、目录内按 PATHEXT 顺序找 `claude.*`（与 `quota_product::path_executables` 共用）。以前读取时只找 `claude.exe`，而 npm 安装只在 PATH 上放 `claude.cmd`，结果是「已检测」却永远读不到；macOS `discoverBinaries` 一直会查 PATH。`.cmd` / `.bat` 不经 `cmd.exe` 运行（探测参数里的 JSON 会被 cmd 改写），而是把 npm / pnpm / Yarn 生成的 shim 当文本读，取最后一个以 `%dp0%` 或 `%~dp0` 开头的带引号路径：是 `.exe` 就直接启动（npm 版 Claude Code 2.1.x 的 `bin\claude.exe` 即此类），是 `.js` 就用 shim 同目录的 `node.exe`、否则 PATH 上第一个 `node.exe` 运行（与 shim 自己的选择一致）；两处都没有时才退回 App 内置的 Node（fnm 这类 Node 管理器只在它挂钩的 shell 里把 `node` 放进 PATH，开机自启的 App 看不到），指向另一个 `.cmd` 时最多再跟一层。解析不了的 shim、无扩展名的 POSIX `claude`、`.ps1` 都跳过。PATH 之后才列桌面端自带的 Claude Code。去重后最多试 3 个候选，卡片显示最后一个候选的原因（与 macOS 一致）。
- 桌面端候选复用下述三个数据根，只在 `claude-code\<数字版本>` 下找 `claude.exe` 或 `<十六进制目录>\claude.exe`。按版本号各段数值排序，取最新的完整程序；所有来源共用启动参数、环境和出口判断。公开记录分别展示了 Roaming 的单层路径、MSIX 根目录迁移和新版的哈希层；三种根与两种布局的全部组合属于推断，尚待 Windows 真机确认。[Roaming 实例](https://github.com/anthropics/claude-code/issues/62690) · [MSIX 迁移](https://github.com/anthropics/claude-code/issues/103860) · [新版哈希目录实例](https://github.com/anthropics/claude-code/issues/107318)
- 面板先显示有效的 `~/.claude.json` 缓存，没有可用缓存时显示有效的桌面端记录。预览只发给本次刷新的面板，不占 60 秒实时刷新间隔。任一候选读到实时额度就采用。独立 Claude Code 明确报告「未使用订阅登录」时直接显示空态；桌面端自带程序的同一答复只说明该程序没有返回额度，按实时失败处理。启动失败、超时、暂停或没有程序时，先重新读 Claude Code 缓存，再读桌面端记录。组织策略禁止探测时，预览和最终结果都只允许使用有效的桌面端记录。
- Windows 真机验收包括：仅装桌面端时的版本目录与 `get_usage`、独立安装优先、过期或多组织记录与实时读取并存、组织策略阻止探测、SOCKS5 实际读到额度、PAC 混合路线和探测结束后的端口回收。
- 必须带 `--print`。Claude Code 2.1.246 起，`--input-format stream-json` 缺 `--print` 会直接报错退出，探测在 `initialize` 之前就失败（vibe-usage-app#35；Windows 到 #25 才补上）。
- 用户关掉的流量照样关着，额度照常读（网络隐私最高原则第 4、6 条，2026-11-24 起）。按 Claude Code 2.1.330 的代码和只记主机名的本机代理实测：`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` 会拦下 Claude Code 认为非必要的所有请求，查额度（`/api/oauth/usage`）也在内；开着时 `get_usage` 只用 Claude Code 自己存的 `cachedUsageUtilization` 快照回答（60 秒内直接复用，查不到时最多拿 1 小时内的旧数），而这个快照没有任何地方会刷新，用户自己的会话也不会。所以探测进程只清掉这个总开关，同时打开 Claude Code 所有能单独关闭的开关（`vibe_core::claude_nonessential::QUIET_ENVIRONMENT`：快速模式（顺带省掉组织快速模式状态请求）、遥测、错误报告、自动更新、bug/反馈命令和问卷、GrowthBook 与功能开关、vitals、模型目录、安装检查、官方插件市场自动安装、`DO_NOT_TRACK`），环境变量和进程内 `--settings`（`probe_settings()`，命令行设置的优先级高于用户的设置文件，Claude Code 启动后会重新读那些文件的 `env`）里各放一份。用 Claude Code 2.1.330、登录有效、设了代理变量时实测，只观察到连往 `api.anthropic.com` 的连接，按代码对应查额度一次（60 秒内有它自己的快照时不发），外加每次启动都会发、没有单独开关能关的 `/api/claude_cli/bootstrap`；同样条件下总开关开着时一个请求都没发。按代码，Claude Code 还可能在同一地址、用同一账号：没设代理变量时预连接（`HEAD /api/hello`）；账号资料缺失或超过一天时刷新（`/api/oauth/profile`）；Team / Enterprise 账号读取远端托管设置和组织策略限制（`/api/claude_code/settings`、`/api/claude_code/policy_limits`）；符合条件的 Pro / Max 账号（个人订阅也算）在邀请资格缓存缺失或超过一天时刷新；登录过期时向 `platform.claude.com` 续期。用户或组织显式开启的 OpenTelemetry 导出、显式开启的网关模型发现不在这组开关里，照原设置；自定义 `ANTHROPIC_BASE_URL` 时预连接发往它。Claude Code 配成 Bedrock / Vertex / Foundry 等第三方服务时，这类登录没有订阅额度（`rate_limits_available: false`，独立 Claude Code 的卡片显示「Claude Code 未使用订阅登录」），而 Claude Code 启动时为确认模型可用，可能用用户的云账号向一个或多个模型各发一个固定的 `.`（`max_tokens: 1`），可能产生少量用量；App 不去识别这种配置（要和 Claude Code 对这几个键的布尔解析、设置优先级完全一致太容易出错，误判会让订阅用户看不到额度），和 main 一样照常启动并在隐私说明里写明。试过但没采用：按主机名过滤的本机中转分不开这几次请求（同一个主机）；解开 TLS 的中转能分开，但要让用户的 Claude 登录令牌以明文经过我们的进程；总开关开着时本机没有新鲜的额度记录；`--bare` 根本不读登录。组织的托管设置优先级高于 `--settings`。托管设置里的 `env` 在 Claude Code 2.1.330 里按键合并（schema 原文「env keeps its own per-key union」，与 `managedSourcesBehavior` 无关），所以对这一个键，设了它的最高来源说了算：注册表 HKLM `SOFTWARE\Policies\ClaudeCode` 的 `Settings` 设了这个键就以它为准，否则看 `C:\Program Files\ClaudeCode\managed-settings.json` 加按顺序合并的 `managed-settings.d\*.json`（后面的文件设了这个键就以它为准，空字符串也算）；HKLM 和托管文件都没有策略时才看 HKCU。这样算出来总开关是开的（`is_forced_by_policy`，只用这一个键，其余内容在内存里随即丢弃），就不启动额度探测；有有效的桌面端记录时显示记录及组织策略原因，缺少有效记录时，卡片为 `NoData` + `nonessentialTrafficDisabled`（「组织策略关闭了 Claude Code 非必要流量 · 无法读取」），不显示旧数、没有重试，每次刷新都重新检查。claude.ai 后台下发的托管设置本机看不到，这种情况下探测照常运行，Claude Code 可能用自己的近期快照回答，也可能没有窗口。`managedSourcesBehavior`（含显式 `merge`）不影响这个键的判断，因为 `env` 默认就逐键合并；只有组织显式关掉逐键合并（`CLAUDE_CODE_DISABLE_ADMIN_ENV_UNION`）、或机器策略 JSON 损坏时，App 与 Claude Code 的判断可能不同。托管 `env` 的变量名和 Claude Code 一样不区分大小写。与 macOS `ClaudeNonessentialTraffic` 一致；#190 的通用暂停代码里叫 `vendorTrafficDisabled`。不传 `--bare`。
- `get_usage` 请求必须带 `skip_behaviors: true`（2026-11-06 经维护者按架构审批门批准）。不带时，Claude Code 会先扫描最近 7 天的本地对话记录，为 `/usage` 生成消耗分析，扫完才回答。卡片不使用这部分，而重度用户的这次扫描可能超过单个程序 8 秒的预算（#25）。配额与套餐字段在两种情况下来自同一路径。
- 套餐名照 `subscription_type` 显示，首字母大写。2026-10-30 起 Max 20x 改名 Ultra，但 Claude Code 对这类账号仍报 `max`，所以显示 Max；不按「没有 5 小时窗口」推断 Ultra，因为未验证的 Ultra 账号仍有 5 小时窗口。
- 窗口为 `null` 视为「没有该窗口」，不是坏数据。实时回答、`~/.claude.json` 缓存、Claude Desktop 记录（`crates/core/src/rate_limit/claude_desktop.rs`）三条路径都只跳过该窗口，保留其余窗口。Claude Desktop 记录里非空但不是数字的值仍判整条无效；实时回答和缓存遇到不合法的窗口只丢掉该窗口。
- Claude Desktop 记录 `plan-usage-history.json` 在三处查找：普通安装的 `%APPDATA%\Claude`；MSIX 包旧版的 `%LOCALAPPDATA%\Packages\Claude_<包后缀>\LocalCache\Roaming\Claude`；MSIX 包 2026-11 更新后写入的 `%LOCALAPPDATA%\Claude-Data\claude_<包后缀>`（记录直接在这一层，#27）。`Claude_` / `claude_` 前缀不区分大小写，后缀只接受字母和数字。判断「装了多份」按安装而不是按目录：同一个包后缀的新旧两个目录算同一份安装，两处都有记录时取修改时间较新的一份（同样新时取新目录）；只有包后缀不同、或普通安装与 MSIX 包同时有记录，才算多份安装、不猜账号。比较前先解析链接（`canonicalize`，失败时用原路径），用户把旧目录链接到新目录时同一个文件只算一次。

## Antigravity / AGY 探测约束（与 CLI 查找规则一致）

2026-11-19 接入（vibe-cafe/vibe-usage-windows#54），对齐 macOS 0.7.4（vibe-cafe/vibe-usage-app#53）。读取全部由 CLI 完成（`quota fetch --product antigravity`）。2026-11-24 按 Hogan 批准的新办法改过（工作区《交接笔记/隐私修复/Antigravity额度读取-封号风险与新做法》）：

- **用过才问 Google。** CLI 比较本机三个对话库（`~\.gemini\antigravity`、`antigravity-cli`、`antigravity-ide` 下的 `conversations`）里 `.db` / `.db-wal` / `.pb` 的文件名、大小、修改时间，不读内容。本机一条对话都没有就不问；上次读完之后没变化也不问，显示上次读到的数（记录在 `~\.vibe-usage\antigravity-quota.json`，不含任何令牌或账号）。过了重置时间的窗口：期间没用过就显示用了 0%，用过就先不显示，等下次读。确认需要登录时丢掉旧数。两次问之间至少 5 分钟；读失败后按 5、10、20、40、60 分钟退避，退避期间点「重试」不会马上重读。
- **问的顺序。** ① 官方的 `agy -p /usage --output-format json`（只给 1.1.11 及以后，版本检查结果按程序记住）；② 正在运行的桌面端自己的本机服务：一条 PowerShell 查本会话里路径带 antigravity 的 `language_server` 进程和它监听的端口，用它命令行里自己的本机 CSRF 令牌，只发到这个进程的 `127.0.0.1` 端口；③ 前两步都不行才临时启动桌面端自带的 `language_server`，版本号读安装目录 `resources\app\package.json`，没有就读 `resources\app.asar` 里的 `package.json`，读不到就不启动（不再写死 `2.33.0`）。
- **agy 子进程。** 只在我们启动的 agy 里设 `AGY_CLI_DISABLE_AUTO_UPDATE=true`；它的 PATH 最前面放什么都不做的 `rundll32.bat`，`BROWSER` 指向 `browser.bat`，静默登录失败时 agy 想打开浏览器会先碰到它们，CLI 随即结束 agy。agy 若不经这两条路、直接调系统接口打开浏览器，这一层拦不住；CLI 会在 agy 报「需要登录」或 `/usage` 超时（交互登录会等一分钟）后停用 agy 这一步，直到 agy 自己的对话库有变化或 agy 换了版本。Windows 上没有真机验证过。
- **App 这边。** AGY 卡片的重读间隔是 5 分钟（`rate_limits.rs` 的 `SLOW_MAX_AGE`，Grok 也用它），窗口可见时的 60 秒定时轮到 AGY 不满 5 分钟就不调 CLI；手动刷新照样调 CLI，问不问由 CLI 决定。设置里 AGY 那一行原有一行灰字「Google 表示不支持第三方读取额度」，维护者 2026-11-26 定从界面删掉（嫌防守性文字多）；这个事实只留在 `PRIVACY.md`。macOS 的同一行由 macOS 自己跟进。
- CLI 给这一轮最多 20 秒，在 App 30 秒的 CLI 总预算以内；它和 Kimi / ZCode / Grok / OpenCode Go 同一批读取，最慢的一个决定这一批什么时候回来（与 macOS 相同）。

- CLI 找 `agy.exe` 的顺序：`ANTIGRAVITY_CLI_PATH`（测试用）→ `~\.local\bin\agy.exe` → PATH 上的 `agy.exe` → 官方安装位置 `%LOCALAPPDATA%\agy\bin\agy.exe`。最后一项由 vibe-cafe/vibe-usage#176 加入（要等它发到 npm 才生效）；官方文档（antigravity.google/docs/getting-started）和安装脚本 `install.ps1` / `install.cmd` 都装到这里，要等 `agy install` 改了用户环境变量才进 PATH，而开机自启、一直挂在托盘的 App 看不到之后改的 PATH（App 把自己启动时的 PATH 传给 CLI）。CLI 发版前，agy 不在 App 的 PATH 上时卡片会显示「请安装 agy 或 Antigravity」。
- 代理（`src-tauri/src/system_proxy.rs`）：每次 CLI 运行都按当下的系统代理（手动代理、按协议分开的写法、例外列表、PAC 由 WinHTTP 求值）给出 `HTTPS_PROXY` / `HTTP_PROXY` 和 `NODE_USE_ENV_PROXY=1`；只开 SOCKS 时传 `socks5://` 并把开关设为 `0`；PAC 求值失败时，系统里同时开着手动代理就用手动代理（和 Windows 自己一样），否则指向一个关闭的本机端口，让请求失败而不是直连；App 自己的环境里已有代理变量（含 `ALL_PROXY`）时沿用它，不注入系统代理。Node 22.21+/24 在这种环境下连 `127.0.0.1` 也会交给代理，代理不在本机或 Clash 走全局模式时就读不到额度，所以 `NO_PROXY` 总会并进 `localhost,127.0.0.1,::1`（大小写两种写法合并，保留用户原有的条目，`*` 保持原样），同步、配额、诊断都经过 `sync_engine::cli_command` 这一处；CLI 发往 agy 的请求自己也不走代理（vibe-cafe/vibe-usage#176）。
- 只认 `agy.exe`，不认 `.cmd` / `.bat`：官方渠道只发 exe；Node 不经 shell 启动不了 `.cmd`；有报道说 Antigravity 桌面端也会注册 `agy` 这个命令名，那种启动器会打开桌面端，而不是读额度。
- App 的「已检测」（`quota_product.rs`）：`~\.gemini\antigravity-cli`、`~\.gemini\antigravity`、上面两个固定位置（`quota_product::antigravity_cli_paths`，与 CLI 的 `antigravityInstallPaths()` 是同一份列表，改一边要改另一边）、PATH 上的 `agy`（按 PATHEXT，和其他产品一样）、桌面端 `Programs\Antigravity\Antigravity.exe`（Antigravity 2.0 桌面端和 1.x IDE 共用）和 `Programs\Antigravity IDE\Antigravity IDE.exe`（2.x 独立 IDE），根目录是 `%LOCALAPPDATA%`、`%LOCALAPPDATA%\Programs` 和 Program Files。官方文档没写桌面端装在哪，这两个位置来自 Google AI 开发者论坛的用户报告，是推断。只装了桌面端或 IDE、没装 agy 也算「已检测」（macOS 认 `Antigravity.app` 也是这样）；首次启动不会自动勾上（2026-11-26 起 Antigravity 一律由用户自己打开），用户打开后卡片提示去装 agy。
- Antigravity / AGY 卡片显示一个模型组的 5h / 7d（优先 Gemini）。`programNotFound` 或旧 CLI 的 `missing_credentials` 显示「请安装 agy 或 Antigravity」，登录被拒显示「请在 Antigravity 中登录」。`programNoAnswer` 显示「没有返回额度」。有缓存时保留额度条，在下面写「来源：缓存 · 没有返回额度」。

## Grok 主动读取（2026-11-25 Hogan 批准，工作区《Grok主动读取-审批门方案》）

读取全部由 CLI 完成（`quota fetch --product grok`，vibe-cafe/vibe-usage 的 Grok 主动读取 PR，要等它发到 npm 才生效）：

- CLI 找到官方 `grok.exe`（`%USERPROFILE%\.grok\bin`、PATH；不认 `.cmd` / `.bat`）后，先过出口检测，再启动 `grok agent --no-leader stdio`，只发 `initialize` 和 `_x.ai/billing` 两条消息问当前账期，拿到就结束进程，整次最多 10 秒。不建会话、不发提示词、不耗额度；Grok 用它自己的登录问 xAI，App 和 CLI 都不读 Grok 的登录文件。子进程里设 `GROK_DISABLE_AUTOUPDATER=1`，去掉 ZCode 的两个 Key 变量，代理变量原样继承（`system_proxy.rs` 给 CLI 的那一套）。
- 结果：问到当前账期 → `source: live`；没登录 → `unauthorized` + `credentialsMissing`，不显示旧数；用 xAI API Key 登录 → 「当前登录方式不含订阅额度」；其他失败（账单服务出错、Grok 太旧、超时、没装）→ 照旧读 Grok 日志末尾里它上次查到的数（`source: local`），并带 `failureReason`（`programNoAnswer` / `programOutdated` / `timedOut` / `programNotFound`），来源行写明原因。本周还没用过时 Grok 不写百分比，按 0% 显示（和 Grok 自己的界面一致）。
- 出口检测：Grok 在 CLI 的 `EGRESS_GATED_PRODUCTS` 里，检测主机是 `cli-chat-proxy.grok.com` 自己。暂停时不启动 Grok：日志里有本账期的数就照常显示（CLI 结果是 `ok` + `source: local`，另带 `pauseReason`，来源行写明暂停原因），没有就是 `unsupported` + `pauseReason`，卡片一行原因，和 Antigravity 一样。
- App 这边：窗口可见时 Grok 最多 5 分钟读一次（`rate_limits.rs` 的 `SLOW_MAX_AGE`），手动刷新照样读。出口检测暂停时例外，下一次刷新就重读（`recheck_egress`，见下节），暂停期间 CLI 不启动 Grok，只重新判断出口。
- **真机待测**：`grok.exe` 的实际安装位置、能否直接启动、超时后进程树收尾（`taskkill /T /F`）。

## Cursor 探测约束（单独登录：2026-11-25 Hogan 批准，工作区《Cursor-加密凭据-方案》）

2026-11-25 接入，对齐 macOS（vibe-cafe/vibe-usage-app#53）；最初由外部贡献者在 vibe-cafe/vibe-usage-windows#23 实现，这里在最新 main 上只保留了 Cursor 部分。Cursor 3.27 起把本机登录加密（vibe-cafe/vibe-usage#144），不解密；同日 Hogan 批准「单独登录」：用户在设置里登录一次 Cursor，App 保存这份登录，CLI 用它读用量和额度（CLI vibe-cafe/vibe-usage#210，0.15.10 起）。读取和联网全部由 CLI 完成（`quota fetch --product cursor`，CLI 规则见 vibe-cafe/vibe-usage `AGENTS.md`「Subscription quota isolation」和 Cursor 单独登录的审批记录）：

- **读什么。** CLI 认登录的顺序：App 经标准输入递来的单独登录（`--cursor-login-stdin`）→ CLI 自己存的 `~/.vibe-usage/cursor-login.json`（只用命令行的用户）→ Cursor 自己 `%APPDATA%\Cursor\User\globalStorage\state.vscdb` 里的 `cursorAuth/accessToken`（只读这一行，不续期、不写 Cursor 的库）。在内存里拼成会话 Cookie，请求 `https://cursor.com/api/usage-summary`。不读浏览器 Cookie。App 自己不碰 Cursor 的任何文件；「已检测」只看 `~\.cursor`、`%APPDATA%\Cursor`、`Programs\Cursor\Cursor.exe` 和 PATH 上的 `cursor`。
- **单独登录（`services/cursor_login.rs`）。** 设置「订阅配额」里 Cursor 开关下面一行，四种状态：未登录「未登录」+「登录」；等待中「请在浏览器里确认；如果 Cursor 让你去邮箱确认，点完邮件里的链接再回来」+「取消」；已登录显示邮箱（读不到写「Cursor 账号」）+「退出」；已失效「登录已失效」+「重新登录」，旁边一个次要的「退出」（不用确认，直接删掉这条失效记录，CLI 回到读 Cursor 本机登录）。原有的灰字「以 Cursor 命令行的身份登录，只用来读用量」维护者 11-26 定从界面删掉（设置页不放解释性句子），这个事实只写在 `PRIVACY.md`。文案和 macOS 逐字一致，清单在 `src/lib/cursorLogin.ts`。
  - **登录**：App 启动 `login cursor --json`。CLI 先输出 `{"event":"open","url":…}`，App 核对地址确实是 `https://cursor.com/loginDeepControl`（https、主机就是 `cursor.com`、没有端口和用户名、路径一致，`is_login_url`），通过才用默认浏览器打开，否则停掉 CLI，显示「登录地址不对，已取消」。登录成功后 CLI 输出 `{"event":"credential",…}`，App 存进凭据管理器；失败时输出 `{"event":"error","code":…}`，App 只认固定代码（`timeout` / `network` / `proxy`，其余算「Cursor 没有完成登录，请重试」），不保留 `message`。「取消」直接结束这个 CLI 和它启动的一切（Job 对象）。App 自己再兜一层 6 分钟上限（CLI 最多等 5 分钟，启动器可能先查 npm）。等待期间每秒重新读一次系统给 `api2.cursor.sh` 和 `cursor.com` 的路线（手动代理和 PAC；App 自己有代理变量时不读），开了、关了或换了代理就停掉这次登录，显示「代理设置变了，请重新登录」（和 macOS 一样），免得 CLI 按旧路线继续轮询和读邮箱。原来走代理的主机，PAC 一时答不上来不算变化（CLI 仍用原来的代理，不会直连）；原来直连的主机答不上来就算变化（刚开了 PAC）。取舍：App 拦不住 CLI 已经发出的请求，开代理后到下一次检查之间（不到 1 秒）CLI 可能还按旧路线轮询一次；用户开了代理还要重新打开 Cursor 页面、点确认，这么短的时间里拿不到登录。PAC 的答案按主机缓存 60 秒（原有），只改 PAC 规则、不换地址时要等缓存过期才看得出。没有 Node.js 时显示「没有可用的 Node.js 或 Bun」。
  - **保存**：Windows 凭据管理器里 Vibe Usage 自己的一条 `ai.vibecafe.vibe-usage.windows/cursor/login`（`services/credential_store.rs`，和 ZCode Key 同一套读写，`windows-sys` 的 `Win32_Security_Credentials` 原本就有，只存取我们自己的条目）。只留 CLI 凭据里已知的字段，刷新令牌和访问令牌相同就不重复存。单条上限 2560 字节，超过时分成最多 4 条（`/2`…`/4`），主条目最后写、最先删，读到缺条就当作读不了。
  - **交给 CLI**：每次 `sync` 和 `quota fetch` 都加 `--cursor-login-stdin`，凭据从标准输入递过去，写完就关；不放进参数和环境变量。到期要续期时先续期，再读代理准备命令（续期可能要一分钟，代理要按启动时的来）；命令准备好之后、启动前一刻再核对一次登录，这期间点了「退出」或重新登录就按当下的来。续期命令自己也在准备好之后核对，期间退出就不续。App 没有单独登录时不加这个参数，CLI 照旧读 Cursor 本机登录（没被加密的用户体验不变）。
  - **续期**：离到期（凭据自带的 `expiresAt`，约登录后 60 天）不到 7 天时，在交给 CLI 之前先跑 `login cursor --refresh --json --cursor-login-stdin`：拿到新凭据就存好再用；Cursor 说结束（`{"event":"expired"}`）就判定失效；其他失败 6 小时内不再试，旧凭据没过期就照用。续期一次只跑一个（每续期一次，cursor.com 的 Sessions 里多一条）。
  - **被拒后**：CLI 对 App 递来的凭据不续期，被 Cursor 拒绝（或已过期）就报 `signIn: expired`（同步里是「Cursor 登录已失效…」那句提示，告警码 `cursor_login_expired`）。App 收到后续期一次：成功就存好，额度这次立刻用新凭据再读一次（同步等下一次）；续期被拒，或者刚续出来、还没被 Cursor 接受过（实时读到额度、同步里没有 Cursor 的提示）就在一小时内又被拒，判定失效，不再续（每续一次 cursor.com 多一条会话）；超过一小时的拒绝另算，照常续期一次；续期没完成（网络等）卡片显示「登录续期失败」。
  - **失效**：凭据管理器里只留失效时间和邮箱，令牌删掉。之后每次运行仍递这条不含令牌的记录，CLI 把它当作无法使用的登录：额度报 `signIn: expired`，同步跳过 Cursor，不发任何带登录的请求，也不退回去读 Cursor 本机登录（和 CLI 自己存的失效文件行为一致）。「重新登录」或「退出」后恢复。
  - **退出**：点「退出」先在同一行展开确认，只有一句：「退出 Cursor 登录？」「只删除 Vibe Usage 里的这份登录，Cursor 编辑器不受影响。」+「取消」「退出」（维护者 11-26 定，嫌原来那段字太多）。撤销办法（cursor.com 设置的 Sessions 里显示为「Desktop App」，每续期一次多一条，按时间认，别撤正在用的 Cursor 编辑器那条，最多 10 分钟生效）和「Cursor 本机的登录还能读时照旧读取」写在 `PRIVACY.md`，不放界面；CLI `logout cursor` 的终端提示不变。确认后删掉凭据管理器里的条目；那一刻已经在跑的一次同步、额度读取或续期会跑完（和 CLI 自己「同步中途退出」的取舍一样），之后的运行不再带这份登录。Cursor 没有公开的注销接口，官网那边的登录留到自然过期或用户自己撤销。退出后 CLI 回到读 Cursor 本机登录；本机登录的若是另一个账号，CLI 的防串账保护会让它等 App 最后一次同步满 7 天后才开始同步（CLI `APP_ACCOUNT_NOTICE`），额度照常读。
  - **CLI 支不支持**：设置窗口显示时（启动时预先加载的隐藏窗口不算），App 在后台跑一次 `help --all`，看有没有 `login cursor`；没有（CLI 早于 0.15.10）就不显示「登录」，已登录或已失效的仍能「退出」。支持的结论记到退出 App，不支持的一小时后再查，查失败下次打开设置再查。
  - **不进日志**：登录和续期命令的标准输出只在内存里解析，标准错误直接丢弃，不写 `sync.log`、诊断和同步记录。同步本身的输出照旧进 `sync.log`，CLI 保证不在里面打印凭据。
- **代理。** 和其他 CLI 产品一样经 `sync_engine::cli_command` 注入当下的系统代理（`system_proxy.rs`）。`cursor.com` 和 `api2.cursor.sh`（登录轮询、续期）都在开机代理探测和代理变量的主机清单里，PAC 对这两个主机给出的答案都会交给 CLI。CLI 照不了的代理（如只开 SOCKS4）返回 `unsupported` + `proxyUnsupported`，不发请求；登录时报 `proxy`，设置里显示「无法按你的代理设置连接 Cursor，未发送任何请求」。只靠 Windows「自动检测设置」（WPAD）下发代理、没有 PAC 地址和代理变量的网络，`system_proxy.rs` 目前认不出，会按直连（已知缺口，工作区《隐私修复/B块按第6条复查》记为后续；Windows 上的 Cursor 同步原来就走这条路，登录和续期也受影响）。Cursor 不在出口检测名单里（名单只有维护者点名的四家），登录和续期同样不检测，与 macOS 和 CLI 相同。
- **卡片。** 一行 `Month`（本计费周期，带时间进度条），按需额度显示为「额外」；Cursor 模型 / 第三方模型各用了多少只放在 `Month` 的悬停提示里（CLI 的 `breakdown`，标签映射 `quota_cli.rs:display_label`，与 macOS `QuotaCLIBridge.displayLabel` 相同）。套餐名在标题旁。悬停提示与 macOS `TooltipView` 一致：标题把 `5h` / `7d` / `Month` 写成「5 小时窗口 / 7 天窗口 / 月度窗口」，每行带「Token 用量」「时间」或「重置」小标签（对所有产品生效，PARITY 第 7 条）。
- **状态。** CLI 额度结果带 `signIn` 时（只随 `unsupported` 出现）：`required`（本机登录已加密）→「需要登录 Cursor」，`expired`（单独登录被 Cursor 结束）→「Cursor 登录已失效」；两种都没有重试按钮，点整张卡片打开设置并滚到 Cursor 这一行（`open_settings_window` 带 `cursorLogin`，设置窗口显示后取一次）；设置状态列显示「需登录 / 已失效」。只复述 CLI 报的 `signIn`，不猜。没有这个字段时照旧（旧 CLI；新 CLI 在本机根本没有 Cursor 登录时也不带）：本机登录已加密（`credentialsEncrypted`）→「登录信息已加密，暂时读不了」，没找到登录（`missing_credentials`）或 cursor.com 拒绝（`unauthorized`）→「请先登录 Cursor」/「登录已失效，请重新登录」+ 重试。`retryable_error` →「暂时无法读取订阅配额」+ 重试，CLI 有同一账号的缓存时显示旧数并标「来源：缓存 · 实时读取失败」；续期没完成 →「登录续期失败」+ 重试（`loginRefreshFailed`）。CLI 的额度缓存按账号（用户编号的单向哈希）分，续期换凭据不会让缓存失效。
- **刷新。** 与 Kimi / ZCode / Grok / OpenCode Go 同一批、同样 60 秒间隔（macOS 每个产品 60 秒冷却）。登录、退出或判定失效后，App 丢掉 Cursor 卡片的内存快照并通知面板重读；登录成功后马上同步一次，CLI 从上次成功同步的那周起往回补被跳过的用量（前提是 cursor.com 还留着记录）。首次启动且本机检测到 Cursor 时默认勾选（维护者 2026-11-08 定，见 CLI `AGENTS.md`）；已经初始化过选择的老用户不会被自动加上。
- **撤回。** CLI 撤掉单独登录时，新版 CLI 必须继续接受 `--cursor-login-stdin`（忽略内容即可）：App 有单独登录时每次都带这个参数，旧版 CLI 不认识它会让整次 `quota fetch` 失败（其他产品的卡片一起报错），`sync` 则会忽略它、改读 Cursor 本机登录。同样的情况也出现在登录后用户把 npm 源换成还停在 0.15.9 的镜像时（极少见，没有为它加检查）。App 撤回时隐藏登录行即可，凭据管理器里的条目留着无害。

## 出口检测（网络隐私最高原则第 3 条，2026-11-24）

`src-tauri/src/egress_check.rs`，与 macOS `EgressCheck` 同一套规则。Codex（`codex_usage::send`，每次尝试之前，因为每次尝试都重新取路线）和 Claude（`claude_usage::fetch`，策略检查之后、有可启动的 Claude Code 时）在带账号的请求发出前检查：这个地址的实时路线（`system_proxy::live_route`，含应用自己的代理变量和 PAC）是直连时，才用关掉所有代理、不跟随跳转的独立客户端访问 `https://<主机>/cdn-cgi/trace`，5 秒、最多读 4 KiB，只取 `loc=`。Codex 只查 `chatgpt.com` / `chat.openai.com`（`chatgpt_base_url` 指向别处是用户自选的中转，OpenAI 看到的是中转）；Claude 按最终交给它的环境判断：先生成启动 Claude Code 的代理变量（应用自己的，或系统代理加上 PAC 判为直连、补进 `NO_PROXY` 的主机），再叠加 Claude Code 自己 `settings.json` 里 `env` 的代理键（只读这几个键；文件可以是 UTF-8、带 BOM 的 UTF-8 或带 BOM 的小端 UTF-16，PowerShell 5.1 会写出后两种；和 Claude Code 一样不认大端 UTF-16）。Claude Code 只认 `HTTPS_PROXY` / `HTTP_PROXY`；`NO_PROXY` 和 `no_proxy` 合起来看。`.api.anthropic.com`、`*.anthropic.com` 这类写法各运行时读法不一（三轮审查对 Claude Code 的读法说法不同），所以两头稳妥：任何读法下都直连的主机（`*` 或主机本身）才发检测；只在某种读法下直连时，不放行也不检测，按「无法确认」暂停（选「暂停读取」时是「未检测到代理」，选「照常读取」时照旧读取）。不把要走代理的主机改成直连去检测。公司托管设置里下发的代理不读（少见，记在方案里）。

Claude 有主机须经 SOCKS 时，在探测开始时固定三个主机的路线。PAC 只为 Claude 使用 `WinHttpGetProxyForUrlEx` 的类型化首项，忽略后续 `DIRECT` 退路。中转绑定 `127.0.0.1:0`，用系统随机数生成每次独立的 Basic 凭据，只接受 `anthropic.com`、`claude.com`、`claude.ai` 及其子域的 443 CONNECT。SOCKS5 上游收到域名，由代理解析；HTTP(S) 上游收到 CONNECT；明确直连的主机按直连连接。其他放行域名使用探测开始时的第一个 SOCKS。上游失败或路线未解析时拒绝该隧道，结束或取消时关闭监听和连接。客户端和厂商之间的 TLS 在隧道内透传。[微软 WinHTTP 文档](https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpgetproxyforurlex)

出口判断使用子进程实际拿到的代理和 Claude 设置。全经代理时跳过匿名检测；中转本身要直连的主机也受三档出口设置约束。Claude 自己的 HTTP 代理设置仍可生效；若例外可能绕过已固定的代理，或无法安全启动中转，则暂停实时读取。

暂停的产品在下一次刷新时一定重读（不受 60 秒间隔限制），保留旧数时面板标签也亮琥珀点。`CN` / `HK` / `MO` →「网络出口在中国大陆 / 香港 / 澳门，已暂停读取」，没结果、`XX`、`T1` →「无法确认网络出口，已暂停读取」，设置「没检测到代理时」选「暂停读取」→「未检测到代理，已暂停读取」，不发检测；选「照常读取」就照旧直接读；默认「自动判断（推荐）」做上面的检测（`settings.json` 的 `egressMode`：`auto` / `pause` / `allow`，不认识的值按 `auto`）。拉起 CLI 时两个设置总是显式写成 `1` / `0`，用户为终端设的同名环境变量不会盖过设置页。这三种都是暂停不是失败：有本机来源的数（Codex 会话日志、Claude 缓存或桌面端记录）照常显示。桌面端记录沿用自己的来源行，其他来源写明暂停原因；没有就只显示这一行，没有重试按钮。结果按主机和网卡地址摘要缓存在内存里：支持地区 10 分钟，其他 1 分钟；同一时间只检测一次。Grok 和 Antigravity 由 CLI 检测（`pauseReason`，`quota_cli.rs` 映射成同样的原因），两个设置用环境变量转给它。 真机需验证 Windows 网卡摘要、TUN 切换以及手动代理和 PAC 分流；Claude 中转的验收条件见上文「Claude 探测约束」。

## 空态判定（不猜）

刷新在途显示「正在读取订阅配额…」。读取结束后优先显示数据源给出的 `emptyReason`，其次显示固定的 `liveFailure`。两者都没有时，首次读取前的占位显示「尚未读取」。有读取时间的快照按本机检测结果显示「没读到额度，原因未知」或「未检测到安装」。

| 原因 | 卡片 | 设置 |
|---|---|---|
| `limitReached`（Codex 接口报告已用满） | 额度已用满 · 等待重置 | 已用满 |
| `noWindow` | 没有生效的额度窗口 | 无窗口 |
| `notEntitled`（如 OpenCode Go 无订阅） | 未订阅 <产品名> | 未订阅 |
| `sessionWithoutPlanLimits`（Claude Code 返回 `rate_limits_available:false`：用了令牌（如 Magpie、中转站）、API Key、云厂商，或没登录；令牌优先于已存的 claude.ai 登录，订阅用户也会落到这里） | Claude「Claude Code 未使用订阅登录」，其他产品「当前登录方式不含订阅额度」 | Claude「未用订阅登录」，其他产品「不适用」 |
| `periodEnded` | Grok「账期已结束 · 用一次 Grok 后更新」，AGY「额度已重置 · 下次读取时更新」，其他产品「上个周期已结束」 | 待更新 |
| `noLocalRecord` | Grok「暂无记录 · 用一次 Grok 后更新」，AGY「用过 Antigravity 后显示」，其他产品「本机暂无记录」 | 无记录 |
| 应用本地的 `nonessentialTrafficDisabled` | 组织策略关闭了 Claude Code 非必要流量 · 无法读取 | 已禁止 |

Cursor 另有 CLI 的 `signIn`（`required` / `expired`），卡片显示「需要登录 Cursor」/「Cursor 登录已失效」，设置显示「需登录 / 已失效」，见上文「Cursor 探测约束」。

CLI 的六种空态代码见 `quota_cli.rs`。字段缺失、为 `null` 或代码未知时，桥接层保留 `None`。

CLI 是独立版本的依赖：应用不再内置 CLI 快照，每次都通过 `src-tauri/resources/cli-bootstrap.mjs` 运行 npm 上的 `latest`（见 `docs/PARITY.md`「CLI Windows 适配」）。应用 discovery 包含 Windows 安装位置等平台规则，不能把它与 CLI 的跨平台 discovery 当成完全等价。OpenCode Go 的配额由 CLI 读取，凭据也由 CLI 负责：2.x 凭据表的 `opencode-go`、`opencode`（OpenCode 账号登录或服务账号 Key）两项 → 1.x `auth.json` 的 `opencode-go`、`opencode` 两项（2026-11-24 起；此前只读 `opencode-go` 行和 `auth.json` 的 `opencode`，OpenCode 2.4 起的新用户和 1.x 按官方文档登录的用户都会被当成没登录）。CLI 0.13.1 起，没有 Go 订阅的 Key 在 403 时带机器可读的 `emptyReason: notEntitled`，桥接按可选字段解析，卡片显示「未订阅 OpenCode Go」。桥接对 OpenCode Go 单独处理两种登录状态（卡片文案与 macOS 一致）：CLI 报 `missing_credentials` → 无数据 + `credentialsNotFound`，卡片「在 OpenCode 里登录后自动显示」；`expired_credentials`（账号登录过期，CLI 不续期、不发请求）→ 无数据 + `credentialsExpired`，卡片「打开 OpenCode 后自动显示」；`unauthorized`（OpenCode 拒绝了登录）仍是「请在 OpenCode 中重新登录后重试」。其他 CLI 产品也区分缺少凭据和登录失效。Kimi 缺少登录显示「请先登录 Kimi Code」，失效显示「登录已失效，请重新登录」。ZCode 按区域提示配置 BigModel / Z.ai API Key，被拒时显示「API Key 无效，请在设置中更换」。

## 失败原因和来源（不放原文）

2026-11-09 起（一键诊断方案 1a），读取已经得出的失败原因不再只写进看不到的 `log::debug!`，而是作为固定代码带到卡片上。代码与 macOS `ProviderRateLimit.LiveFailure` 共用，文案在 `src/lib/quotaProducts.ts:quotaFailureText`，与 macOS 卡片一致。只传分类，不传任何原文：Claude Code 的 stderr、启动错误文本、HTTP 响应正文、CLI 的 `message` 都不保留。

- Claude 实时读取的固定原因是「未找到 Claude Code」「启动失败」「读取超时」「没有响应」「没有返回额度」。
- Claude 桌面端记录的四种原因（只有 Windows 有这四种；记录本身 macOS 也读。原先统一显示「桌面额度记录缺失、过期或账号不明确」）：Claude 桌面端记录已过期（最新样本超过 24 小时）/ 装了多份 Claude 桌面端，账号不明确（包后缀不同，或普通安装与 MSIX 包同时有记录；同一个包的新旧目录不算）/ Claude 桌面端记录涉及多个组织，账号不明确（同一份记录里 `org` 不一致；同一账号在个人组织和 Team 组织之间切换也会这样，所以只说组织不同，不断言换过账号）/ Claude 桌面端记录无法解析（读不了、超过 2 MB、格式或版本不符、窗口值非法、时间戳在未来）。
- 接口或凭据失败使用「读取超时」「连接失败」「接口报错 502」（无数字时为「接口报错」）「返回数据无法识别」「未找到登录信息」「登录已失效」。CLI 还能区分读不到登录信息、登录信息已加密、登录续期失败、找不到程序、版本较旧、程序没有返回额度和读不了本机记录。
- CLI 调用本身可能显示「缺少 Node.js」「读取超时」「配额 CLI 出错」「配额 CLI 输出无法识别」「配额 CLI 版本不兼容」「配额 CLI 没有返回结果」。Windows 凭据管理器读取失败沿用「无法读取 Windows 凭据管理器」。CLI 的 `failureReason` 只映射已知代码，`httpStatus` 只用于接口报错。

来源 `source` 的取值：实时读取（Claude Code 探测、Codex 接口、CLI 的 `live`）/ 缓存（`~/.claude.json` 的 `cachedUsageUtilization`、CLI 的 `cache`）/ Claude 桌面端记录 / 本地日志（Codex 会话 JSONL、CLI 的 `local`）。

实时读取成功时来源行留空。CLI 的 `ok` 带 `failureReason` 时保留额度条，并显示「来源：缓存 / 本地日志 · <原因>」。旧 CLI 从缓存作答而没有具体代码时显示「实时读取失败」。可重试的失败卡显示具体原因或「暂时读不到额度」，下方说明尝试的来源（实时读取、本地日志或缓存）。Claude 桌面端通常显示「Claude 桌面额度记录 · 非本机独占用量」，记录超过 15 分钟时显示「Claude 桌面历史额度 · 非实时 · 非本机独占用量」。普通探测失败原因留在诊断中；组织策略禁止实时读取且有有效记录时，来源行只显示「组织策略禁止实时读取，显示桌面记录」。

`proxyUnsupported` 显示「代理无法照着走，已暂停读取」。四种暂停均无重试按钮。有本地额度条时仍显示来源，桌面端记录之外的来源行也写明暂停原因。标签亮橙点，设置列显示「已暂停」。新增的显示原因沿用该结果原有的刷新间隔。

## 设置页状态列

设置窗口订阅 `rate-limits-updated` 后，通过只读命令 `get_cached_rate_limits` 取得面板内存快照与刷新状态。再次打开时读回当前缓存。面板刷新开始时事件载荷为 `null`，刷新结束后为面板收到的同一份快照列表。事件只更新设置窗口的显示。

产品已启用且刷新结束，才显示最近结果的短词，例如「未读取」「无数据」「需登录」「已失效」「需打开」「超时」。有额度条且未暂停，或产品关闭、正在刷新、没有快照时，显示「已检测 / 未检测到」。ZCode 平时按当前区域显示「待配置 / 已配置」，读取报告缺少或拒绝 Key 时显示「需 Key / Key 无效」。Cursor 的 CLI 要求登录时显示「需登录 / 已失效」；单独登录本身的状态在开关下面那一行。完整短词与卡片文案见 `src/lib/quotaProducts.ts`。

## 缓存和失败处理

ZCode 区域或 Key 改变后立即清除该产品内存快照，并递增请求代次。旧代次的请求即使后来成功，也不能写回缓存。修改凭据和取得请求上下文采用相同的 settings → cache 加锁顺序。通用设置入口改变区域时也会使缓存失效。

应用不再把刷新失败覆盖为旧的成功状态；CLI 自己负责有凭据隔离和过期检查的持久缓存。这样更换账号或跨过配额重置时间后，应用不会重新显示 CLI 已经拒绝的旧数值。产品仅按 60 秒进行常规刷新，外部工具自行切换账号仍需等下一次实际读取才能被感知。

Credential Manager 读取失败只影响 ZCode，其他产品仍可请求。CLI 响应必须完整对应请求集合；缺项、重复、意外产品、未知协议版本均不能作为成功响应缓存。UI 丢弃已被新设置或新刷新取代的异步返回值。

## Windows 新构建必须复测

1. 在两种区域都已配置的测试环境中，先读到 BigModel 配额，再切到 Z.ai 并模拟请求失败；不得显示 BigModel 数据。同一区域替换、删除 Key 也需复测。
2. 在旧请求尚未结束时切换区域/Key，确认旧返回不覆盖新上下文；同时验证设置窗与托盘面板的显示一致性。
3. 模拟 ZCode 凭据库读取失败，确认另一个已选产品仍能返回；验证失败后不会继续把过期配额显示为成功。
4. 使用固定 CLI 验证 Kimi/ZCode/Grok/OpenCode Go 的正常、无数据和失败状态，以及旧设置迁移、首次推荐、空选择和「选择第三项后仍保留前两项」；OpenCode Go 另需用没有 Go 订阅的 Key 复测 403 对应的未订阅空态。Antigravity / AGY 另需复测：agy 只装在默认位置 `%LOCALAPPDATA%\agy\bin`、不在 App 的 PATH 上时仍能读到 5h / 7d；agy 和桌面端都没装时显示「请安装 agy 或 Antigravity」，登录被拒时显示「请在 Antigravity 中登录」。2026-11-24 起另需复测：只装桌面端、桌面端开着时能读（第二步）；桌面端关着时能读（第三步，确认 `resources\app\package.json` 或 `resources\app.asar` 存在，否则这一步不会启动）；agy 登录失效时不弹浏览器；不用 Antigravity 时窗口一直开着，5 分钟后 `~\.vibe-usage\antigravity-quota.json` 的 `attemptAt` 不变。
5. Cursor：登录了 Cursor（明文凭据）时能读到 `Month`，悬停能看到 Cursor 模型 / 第三方模型；退出 Cursor 登录后卡片显示「请先登录 Cursor」、不显示旧额度；换账号后不显示前一个账号的数；凭据已加密（`cursor-safe-storage:`）时显示「登录信息已加密，暂时读不了」、没有重试按钮（要用带 vibe-cafe/vibe-usage#205、#207 的 CLI）；系统代理开着时请求走代理。2026-11-26 起另需复测单独登录（要 CLI 0.15.10，真实账号由维护者当次同意）：见 `docs/WINDOWS_ACCEPTANCE.md`「Cursor 单独登录」。
6. 重跑生命周期准入测试：关闭准入前子进程可运行，关闭后确有 Job 限额拒绝；继续验证取消、退出、无关 Node 存活、忙时立即卸载和升级。

本地 Rust 测试覆盖缓存失效、旧请求回写、请求/响应集合校验及凭据错误隔离；Mac 的通过不替代以上 Windows 实测。账户条件不满足记为 BLOCKED；ZCode Key 由用户在 GUI 输入。

## PR 范围

现有分支历史还包含 CLI 快照升级、外测打包、Windows 路径与退出修复。这些都不是“增加订阅位置”本身。保留既有已推送提交，通过后续独立提交审查配额修正和准入测试；不改写主分支或已推送历史。
