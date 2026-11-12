# 订阅配额实现与复测边界

## 用量项顺序

2026-09-19 已批准：通用时间窗口按周期从短到长排列。确定的通用别名使用紧凑标签（`Daily` → `1d`、`Weekly` / `1w` → `7d`），所以同时存在两个常用窗口时始终显示 `5h` 在上、`7d` 在下。产品专属、模型专属和功能额度随后保持各自原始相对顺序。前端也会对旧 CLI 快照应用此规则；没有 5h 窗口的产品直接从首个真实窗口开始。

Codex 付费套餐（Plus / Pro / Prolite / Business / Business ProLite）没有拿到 5h 窗口时，卡片仍保留 5h 这一行并说明原因，规则与 macOS 相同（维护者 2026-10-24 定，见 macOS `AGENTS.md`「"Not enforced" vs "no data" for the Codex 5h window」）：实时用量接口会列出所有生效的窗口，它没给 5h 就说明官方关掉了这个限额（`fiveHourNotEnforced`），显示「不限额」；OpenAI 对 Pro / Prolite 一直关着 5h，这两种套餐不论数据来自接口还是本地日志都显示「不限额」；其余情况数据来自本地会话日志，少了 5h 窗口可能只是近 5 小时没用，显示「近 5 小时无活动」。Business ProLite 不算 Pro 套餐，只在接口确认没有 5h 时显示「不限额」。实现在 `src/lib/quotaProducts.ts:codexFiveHourPlaceholder`。

用户可以选择任意数量的订阅产品，选择顺序即卡片顺序；面板为每个已选产品始终渲染一张卡片，多于两张时横向滚动，不折叠成一句通用提示。自动检测只影响首次推荐（首次启动会勾选全部已检测且已就绪的产品）；手动选择不受安装路径限制。Cursor 当前只展示待接入状态。

## 数据流和职责

- `crates/core/src/quota_product.rs`：应用侧产品目录，集中维护名称、可用性、CLI 路由、常规目录和命令检测。Windows 应用安装位置在同一模块适配。
- `commands.rs`：启动时初始化一次默认选择，后续产品查询不写设置；处理选择、区域和用户输入的凭据。
- `rate_limits.rs`：刷新调度和短期内存缓存。Codex/Claude 保留原读取实现（Claude 原生探测的 `rate_limits_available:false` 直接给出 `sessionWithoutPlanLimits` 空态原因），其他已接入产品通过 `quota_cli.rs` 使用固定 CLI 的 JSON 协议；CLI 结果里的可选 `emptyReason` 会映射到 `RateLimitEmptyReason`，缺失或未知取值保持 `None`（向前兼容）。每张卡片的快照另带 `source`（实际来源）和 `liveFailure`（固定失败原因），见下文「失败原因和来源（不放原文）」。
- 前端从目录获取产品名称，使用共享卡片展示配额；官方图标资产（七家，@2x）由 `src/assets/` 提供，卡片与设置页共用 `src/components/ProviderIcon.tsx`。类型定义保留编译期检查，产品特有的登录提示仍由 UI 处理。

## Claude 探测约束（与 macOS 逐项一致）

`src-tauri/src/services/claude_usage.rs` 启动 Claude Code 的参数与 macOS `ClaudeUsageProbe.processArguments` 相同，单元测试锁定：

- 查找程序与「已检测」用同一条规则（`crates/core/src/claude_code.rs`，2026-11-09 起）：依次是 `VIBE_USAGE_CLAUDE_BIN`、`~\.local\bin\claude.exe`、`~\.claude\local\claude.exe`、`%LOCALAPPDATA%\Programs\Claude Code\claude.exe`，再按 PATH 目录顺序、目录内按 PATHEXT 顺序找 `claude.*`（与 `quota_product::path_executables` 共用）。以前读取时只找 `claude.exe`，而 npm 安装只在 PATH 上放 `claude.cmd`，结果是「已检测」却永远读不到；macOS `discoverBinaries` 一直会查 PATH。`.cmd` / `.bat` 不经 `cmd.exe` 运行（探测参数里的 JSON 会被 cmd 改写），而是把 npm / pnpm / Yarn 生成的 shim 当文本读，取最后一个以 `%dp0%` 或 `%~dp0` 开头的带引号路径：是 `.exe` 就直接启动（npm 版 Claude Code 2.1.x 的 `bin\claude.exe` 即此类），是 `.js` 就用 shim 同目录的 `node.exe`、否则 PATH 上第一个 `node.exe` 运行（与 shim 自己的选择一致）；两处都没有时才退回 App 内置的 Node（fnm 这类 Node 管理器只在它挂钩的 shell 里把 `node` 放进 PATH，开机自启的 App 看不到），指向另一个 `.cmd` 时最多再跟一层。解析不了的 shim、无扩展名的 POSIX `claude`、`.ps1` 都跳过。去重后最多试 3 个候选，卡片显示最后一个候选的原因（与 macOS 一致）。
- 必须带 `--print`。Claude Code 2.1.246 起，`--input-format stream-json` 缺 `--print` 会直接报错退出，探测在 `initialize` 之前就失败（vibe-usage-app#35；Windows 到 #25 才补上）。
- 必须带进程内 `--settings '{"env":{"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC":""}}'`。只从子进程环境里删掉这个变量不够：Claude Code 启动后会重新读 `~/.claude/settings.json` 的 `env`，变量一旦生效，订阅账号的 `get_usage` 会回 `rate_limits: null`。值必须是空字符串，不能写 `"0"`；不写盘，也不传 `--bare`。
- `get_usage` 请求必须带 `skip_behaviors: true`（2026-11-06 经维护者按架构审批门批准）。不带时，Claude Code 会先扫描最近 7 天的本地对话记录，为 `/usage` 生成消耗分析，扫完才回答。卡片不使用这部分，而重度用户的这次扫描可能超过单个程序 8 秒的预算（#25）。配额与套餐字段在两种情况下来自同一路径。
- 套餐名照 `subscription_type` 显示，首字母大写。2026-10-30 起 Max 20x 改名 Ultra，但 Claude Code 对这类账号仍报 `max`，所以显示 Max；不按「没有 5 小时窗口」推断 Ultra，因为未验证的 Ultra 账号仍有 5 小时窗口。
- 窗口为 `null` 视为「没有该窗口」，不是坏数据。实时回答、`~/.claude.json` 缓存、Claude Desktop 记录（`crates/core/src/rate_limit/claude_desktop.rs`）三条路径都只跳过该窗口，保留其余窗口。Claude Desktop 记录里非空但不是数字的值仍判整条无效；实时回答和缓存遇到不合法的窗口只丢掉该窗口。
- Claude Desktop 记录 `plan-usage-history.json` 在三处查找：普通安装的 `%APPDATA%\Claude`；MSIX 包旧版的 `%LOCALAPPDATA%\Packages\Claude_<包后缀>\LocalCache\Roaming\Claude`；MSIX 包 2026-11 更新后写入的 `%LOCALAPPDATA%\Claude-Data\claude_<包后缀>`（记录直接在这一层，#27）。`Claude_` / `claude_` 前缀不区分大小写，后缀只接受字母和数字。判断「装了多份」按安装而不是按目录：同一个包后缀的新旧两个目录算同一份安装，两处都有记录时取修改时间较新的一份（同样新时取新目录）；只有包后缀不同、或普通安装与 MSIX 包同时有记录，才算多份安装、不猜账号。比较前先解析链接（`canonicalize`，失败时用原路径），用户把旧目录链接到新目录时同一个文件只算一次。

## 空态判定（不猜）

卡片的空态文案只复述数据源真正报告过的内容：刷新在途 →「正在读取订阅配额…」；Codex 实时用量接口的 `allowed:false` / `limit_reached:true` →「本期订阅配额已用满 · 等待额度重置」；同一接口应答但没有生效窗口 →「当前没有生效的额度窗口」；OpenCode Go 用量接口对没有 Go 订阅的 Key 返回 403 →「未订阅 OpenCode Go」；Claude Code 应答 `rate_limits_available:false`（API Key / Bedrock / Vertex 登录，配额窗口本就不适用）→「当前登录方式不含订阅额度（API Key / Bedrock / Vertex）」；读取本身得出了失败原因 → 直接显示该固定原因（见下一节，如「未找到 Claude Code 程序」「Claude 桌面端记录已过期」）；本机检测到产品、没有原因也尚无数据 →「暂未读取到订阅配额数据」；本机未安装/未登录 →「未检测到本机安装或登录」。`limit_reached` 与 `allowed` 都缺失时 `empty_reason` 保持 `None`（`RateLimitEmptyReason`）；CLI 桥只在 CLI 报告 `emptyReason` 时透传该字段（缺失/`null` 是当前快照的正常路径，未知取值丢弃，均保持 `None`），本地日志与缓存不填该字段，因此前端不会替任何来源断言「已用满」。

CLI 是独立版本的依赖：应用不再内置 CLI 快照，每次都通过 `src-tauri/resources/cli-bootstrap.mjs` 运行 npm 上的 `latest`（见 `docs/PARITY.md`「CLI Windows 适配」）。应用 discovery 包含 Windows 安装位置等平台规则，不能把它与 CLI 的跨平台 discovery 当成完全等价。OpenCode Go 的配额由 CLI 读取，凭据也由 CLI 负责（2.x 凭据表 → 1.x `auth.json` 回退）；CLI 0.13.1 起，没有 Go 订阅的 Key 在 403 时带机器可读的 `emptyReason: notEntitled`，桥接按可选字段解析，卡片显示「未订阅 OpenCode Go」。

## 失败原因和来源（不放原文）

2026-11-09 起（一键诊断方案 1a），读取已经得出的失败原因不再只写进看不到的 `log::debug!`，而是作为固定代码带到卡片上。代码与 macOS `ProviderRateLimit.LiveFailure` 共用，文案在 `src/lib/quotaProducts.ts:quotaFailureText`，与 macOS 卡片一致。只传分类，不传任何原文：Claude Code 的 stderr、启动错误文本、HTTP 响应正文、CLI 的 `message` 都不保留。

- Claude 实时读取：未找到 Claude Code 程序 / Claude 启动失败 / Claude 配额读取超时 / Claude 没有响应配额请求（进程退出、关闭输出或应答不成功）/ Claude 没有返回额度窗口（应答了，但 `rate_limits` 为空或没有可用窗口）。
- Claude 桌面端记录（仅 Windows，原先统一显示「桌面额度记录缺失、过期或账号不明确」）：Claude 桌面端记录已过期（最新样本超过 24 小时）/ 装了多份 Claude 桌面端，账号不明确（包后缀不同，或普通安装与 MSIX 包同时有记录；同一个包的新旧目录不算）/ Claude 桌面端记录涉及多个组织，账号不明确（同一份记录里 `org` 不一致；同一账号在个人组织和 Team 组织之间切换也会这样，所以只说组织不同，不断言换过账号）/ Claude 桌面端记录无法解析（读不了、超过 2 MB、格式或版本不符、窗口值非法、时间戳在未来）。
- Codex 实时接口：未找到 Codex 登录信息 / Codex 登录已失效 / Codex 配额接口请求超时 / Codex 配额接口连接失败 / Codex 配额接口返回错误 {状态码} / Codex 配额接口返回了无法识别的数据。
- 配额 CLI 整体失败：未找到 Node.js 或 Bun / 配额 CLI 读取超时 / 配额 CLI 运行失败 / 配额 CLI 返回了无法识别的数据 / 配额 CLI 协议版本不兼容 / 配额 CLI 没有返回这个产品。ZCode 读不了 Windows 凭据管理器时是「无法读取 Windows 凭据管理器」。CLI 单个产品以 `source: "cache"` 作答，说明它的实时读取已失败；具体原因只在 `message` 原文里，卡片只说「实时读取失败」。

来源 `source` 的取值：实时读取（Claude Code 探测、Codex 接口、CLI 的 `live`）/ 缓存（`~/.claude.json` 的 `cachedUsageUtilization`、CLI 的 `cache`）/ Claude 桌面端记录 / 本地日志（Codex 会话 JSONL、CLI 的 `local`）。

显示规则与 macOS 相同：实时读取成功时卡片不加说明行（维护者 2026-10-24 定过：每张卡挂一行说明太吵）；实时读取失败、但仍显示缓存或本地日志里的数值时，下面加一行小字「来源：<来源> · <原因>」；可重试卡片第一行是原因，第二行小字「来源：实时读取」，右侧是重试按钮；空态有原因时直接显示原因；需要重新登录的卡片文案不变；Claude 桌面端记录保留原有的小字「Claude 桌面额度记录 · 非本机独占用量」。Claude 实时读取失败且没有缓存时，卡片状态从「无数据」改为「可重试」（与 macOS 一致）；只有「未找到 Claude Code 程序」仍是空态，因为重试改变不了它。数据源先后顺序不变：桌面端记录 → 实时读取 → 缓存。

## 缓存和失败处理

ZCode 区域或 Key 改变后立即清除该产品内存快照，并递增请求代次。旧代次的请求即使后来成功，也不能写回缓存。修改凭据和取得请求上下文采用相同的 settings → cache 加锁顺序。通用设置入口改变区域时也会使缓存失效。

应用不再把刷新失败覆盖为旧的成功状态；CLI 自己负责有凭据隔离和过期检查的持久缓存。这样更换账号或跨过配额重置时间后，应用不会重新显示 CLI 已经拒绝的旧数值。产品仅按 60 秒进行常规刷新，外部工具自行切换账号仍需等下一次实际读取才能被感知。

Credential Manager 读取失败只影响 ZCode，其他产品仍可请求。CLI 响应必须完整对应请求集合；缺项、重复、意外产品、未知协议版本均不能作为成功响应缓存。UI 丢弃已被新设置或新刷新取代的异步返回值。

## Windows 新构建必须复测

1. 在两种区域都已配置的测试环境中，先读到 BigModel 配额，再切到 Z.ai 并模拟请求失败；不得显示 BigModel 数据。同一区域替换、删除 Key 也需复测。
2. 在旧请求尚未结束时切换区域/Key，确认旧返回不覆盖新上下文；同时验证设置窗与托盘面板的显示一致性。
3. 模拟 ZCode 凭据库读取失败，确认另一个已选产品仍能返回；验证失败后不会继续把过期配额显示为成功。
4. 使用固定 CLI 验证 Kimi/ZCode/Grok/OpenCode Go 的正常、无数据和失败状态，以及旧设置迁移、首次推荐、空选择和「选择第三项后仍保留前两项」；OpenCode Go 另需用没有 Go 订阅的 Key 复测 403 对应的未订阅空态。
5. 重跑生命周期准入测试：关闭准入前子进程可运行，关闭后确有 Job 限额拒绝；继续验证取消、退出、无关 Node 存活、忙时立即卸载和升级。

本地 Rust 测试覆盖缓存失效、旧请求回写、请求/响应集合校验及凭据错误隔离；Mac 的通过不替代以上 Windows 实测。账户条件不满足记为 BLOCKED；ZCode Key 由用户在 GUI 输入。

## PR 范围

现有分支历史还包含 CLI 快照升级、外测打包、Windows 路径与退出修复。这些都不是“增加订阅位置”本身。保留既有已推送提交，通过后续独立提交审查配额修正和准入测试；不改写主分支或已推送历史。
