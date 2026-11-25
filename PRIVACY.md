# 隐私政策：Vibe Usage for Windows

最后更新：2026-11-24

本文说明 Vibe Usage Windows 版（下称「App」），以及它在你电脑上运行的命令行组件 `@vibe-cafe/vibe-usage`（下称「CLI」），会读取什么、保存什么、把什么发到哪里。本文按代码的实际行为编写；代码改变时会同步更新，历史版本见本仓库的提交记录。

## 简要说明

- **没有登录过，就不会上传任何用量数据。** 登录指在 App 里点「登录并链接数据」，或者以前在这台电脑上用 CLI 登录过。App 和 CLI 共用同一份登录配置，所以以前用 CLI 登录过的，App 会沿用这个登录，启动后自动同步。
- **上传的是统计数字，不是内容。** 上传的是按半小时汇总的 token 数、模型名、项目名、计算机名和会话时长等统计，不上传对话、提示词、代码、文件内容，也不上传任何 AI 工具的登录凭证。项目名默认不上传；你在网页上打开「上传项目名称」后，多数工具只传文件夹名，但 Kiro 命令行版目前传的是完整的工作目录路径，见第 2 节。
- **上传的用量默认会在 VibeCafé 网站上公开展示**：排行榜上显示你的昵称、token 数和费用，个人主页显示每天的 token 数热力图。两项都可以在网页设置里关闭，见第 2 节。
- **订阅配额卡用的是你自己的登录信息，只发给那个工具自己的服务。** 例如，Codex 的额度只向 OpenAI 查询，查到的结果只在本机显示，不上传。
- 用量数据默认上传到 VibeCafé 运营的 `https://vibefriends.com`（下称「VibeCafé 服务器」）。旧生产地址也会连接到该站。如果你在配置文件里指定了自定义服务器，数据会发到你指定的地址。

## 1. 不登录也会发生的网络请求

| 什么时候 | 发给谁 | 发送的内容 |
|---|---|---|
| App 启动时和之后每 24 小时 | GitHub（`github.com`） | 读取最新版本说明 `latest.json`。你点面板上的「发现更新」或设置里的「安装更新」后，从 GitHub 下载安装包，并用 SHA-256 校验文件。 |
| 需要运行 CLI 时（同步，或读取 Kimi Code、ZCode、Grok、OpenCode Go、Antigravity / AGY 的配额） | npm 官方源（`registry.npmjs.org`） | 查询 `@vibe-cafe/vibe-usage` 的最新版本，下载、校验后缓存在本机。连不上时改用本机缓存。 |
| 安装时，仅在电脑缺少 WebView2 时 | 微软 | 安装程序下载微软的 WebView2 引导程序。 |
| 订阅配额卡开启时 | 见下文 | 用你本机已有的登录信息查询额度。 |
| 开机自启、系统设了代理、有同步或配额要读时 | 系统代理设置里的 PAC 地址和代理端口 | 读取配额和同步之前（更新检查和面板读取 Vibe 用量不等），每 2 秒读一次系统代理设置（会取 PAC 文件；PAC 地址要求 Windows 登录时，按系统默认方式使用当前身份），并试连代理端口，确认代理已就绪，最多 2 分钟。试连只建立连接，不发送任何数据，不带任何账号。系统没设代理时不等。 |
| 要读 Codex、Claude 的额度、又没检测到代理时（「没检测到代理时」选「自动判断（推荐）」） | 该厂商地址的 Cloudflare 节点（Codex 是 `chatgpt.com`，Claude 是 Anthropic 的地址） | 出口检测：用同一条线路访问一次 `/cdn-cgi/trace`，不带任何账号、Cookie 和令牌，只读其中的国家代码。同一地址 10 分钟内最多一次，暂停中每分钟最多一次，换了网络就重新检测。Antigravity 由 CLI 用同样的办法检测（Google 的地址没有这个页面，用 `chatgpt.com` 代替；它要走代理而 Google 直连时改用 `api.anthropic.com`）。Cloudflare 会看到你的出网 IP，但看不到账号。 |

**跟着你的代理。** App 自己的请求，以及交给 CLI 和 Claude Code 的代理设置，都按你此刻的 Windows 系统代理（含 PAC）和代理环境变量来。PAC 解析不了时，系统里同时开着手动代理就用手动代理（和 Windows 自己的做法一样），否则请求直接失败，不会改走直连。Claude Code 只能使用 HTTP 代理：系统代理只设了 SOCKS 时，Claude Code 目前不经代理（已知缺口，待修）。

**出口不对就暂停。** 读 Codex、Claude、Antigravity 的额度要用你在这些厂商的账号。没检测到代理时（系统代理没开、PAC 让它直连，或者代理在 TUN、路由器上本机看不到），先做上表的出口检测：出口在中国大陆、香港、澳门，或者确认不了，就不发带账号的请求，卡片写明原因（例如「网络出口在中国大陆，已暂停读取」），下次读取时重新检测；出口在支持地区就照常读。Cursor、Kimi、ZCode、OpenCode Go 不做这个检测。在设置 →「隐私保护」→「没检测到代理时」里三选一：「自动判断（推荐）」就是上面的做法；「暂停读取」是没检测到代理时一律暂停，不发检测请求；「照常读取」是不检测，直接读取。

**订阅配额卡。** App 第一次启动时，会自动选中在本机检测到的工具，你可以随时在设置里关掉任意一张。检测本身只看本机有没有这些工具的程序和目录，不读取凭证、不联网。各工具的读取方式如下：

- **Codex**：读取 Codex 保存在本机的登录令牌（`CODEX_HOME` 或 `%USERPROFILE%\.codex\auth.json`），向 OpenAI（`chatgpt.com`）查询额度。查不到时，改为读取本机的 Codex 会话日志。
- **Claude**：在本机运行你已安装的 Claude Code，只问额度：App 不传你的任何提示词、不开放任何工具、不保存会话，由 Claude Code 自己向 Anthropic 查询。这个进程里，App 把遥测、错误报告、自动更新、反馈问卷、功能开关下载等 15 个单项开关全部设成关闭；只清掉「关闭非必要网络流量」总开关 `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`，因为它会连查额度一起拦下。用官方订阅登录、设了代理变量时（Claude Code 2.1.330）实测，只观察到连往 `api.anthropic.com` 的连接，按代码对应查额度（60 秒内有它自己的快照时不发）和每次启动都会发、没有单独开关能关的启动配置请求（具体请求路径来自代码核对）。按 Claude Code 的代码，它还可能：没设 `HTTP(S)_PROXY` 变量时向你配置的 API 地址预连接；账号资料缺失或超过一天时刷新；Team / Enterprise 账号读取远端托管设置和策略限制；符合条件的 Pro / Max 账号（个人订阅也算）在缓存缺失或超过一天时刷新邀请资格；登录过期时向自己的登录服务 `platform.claude.com` 续期。Claude Code 配成 Bedrock / Vertex 等第三方服务时（这类登录没有订阅额度），它启动时为确认模型可用，可能用你的云账号向一个或多个模型各发一个固定的「.」，每次最多 1 个输出 token，可能产生少量用量，和以前一样。组织在托管设置里声明了额外的插件市场时，它可能连接其配置的地址。你或公司自己开启的 OpenTelemetry 导出、网关模型发现照原设置，App 不改。这些设置只在这个查额度的进程里生效，不写入任何文件，你平时使用 Claude Code 时的设置不受影响。如果公司下发的托管设置强制打开了这个总开关（和 Claude Code 一样按键合并：注册表 HKLM `SOFTWARE\Policies\ClaudeCode` 设了这个键就以它为准，否则看 `managed-settings.json` 及 `managed-settings.d`，两处都没有策略时才看 HKCU），Claude Code 无法查额度，App 不会启动额度探测；Claude 桌面端的额度记录照常读取和显示，没有桌面端记录、原本要启动 Claude Code 时，卡片显示「组织策略关闭了 Claude Code 非必要流量 · 无法读取」；只从服务端（claude.ai 管理后台）下发的组织策略 App 看不到，这时仍会启动 Claude Code，由它自己执行策略，可能只回近期快照或没有额度窗口；判断时只使用这一个设置，托管设置在内存里解析，其余内容随即丢弃，不保存、不记录、不进诊断。App 还会读取 Claude 桌面端保存在本机的额度记录文件（不联网）。
- **Kimi Code**：读取 Kimi 保存在本机的登录凭证，向 Kimi（`api.kimi.com`）查询额度。凭证快过期时，会向 `auth.kimi.com` 续期，并把新凭证写回 Kimi 原来的凭证文件。
- **ZCode**：只使用你在设置里主动填写的 Coding Plan Key，按你选的区域，发给智谱 BigModel（`open.bigmodel.cn`）或 Z.ai（`api.z.ai`），不会两边都试。这个 Key 保存在 Windows 凭据管理器里。
- **Grok**：只读取 Grok 官方命令行工具在本机的日志末尾，不联网，不读取凭证。
- **OpenCode Go**：读取 OpenCode 保存在本机的登录（`%USERPROFILE%\.local\share\opencode`：OpenCode Go 或 OpenCode 的 API Key，或 OpenCode 账号登录的访问令牌及其选中的工作区），向 OpenCode（`opencode.ai`）查询额度。只读，不改写也不续期 OpenCode 的登录；账号登录过期时不发请求，打开一次 OpenCode 即自动续期。
- **Antigravity / AGY**：只在你用过 Antigravity 之后才向 Google 查询（判断方法是看本机 Antigravity 对话记录文件的大小和修改时间，不读内容；本机一条对话都没有时不查询），两次至少隔 5 分钟，读失败后最长隔 1 小时；没用过时显示上次查到的数，过了重置时间的窗口显示为已恢复。查询时依次用：你已安装、已登录的官方 agy 的 `agy -p /usage`；正在运行的 Antigravity 桌面端自己的本机服务（App 会查看本机进程列表，取这个服务自己的本机访问令牌 CSRF Token，只发到它在 `127.0.0.1` 上的端口）；前两种都不行时，临时启动桌面端自带的语言服务器，并如实报告桌面端的版本号。都不发送提示词，由 agy 或桌面端用它们自己的登录向 Google 查询；App 不读取它们保存的登录信息，读完就结束自己启动的进程。我们启动的 agy 里关闭了它的自动更新，并放了不做任何事的浏览器启动器，让它登录失败时无法经这些启动器打开浏览器登录页（它若直接调用系统接口打开浏览器，这一层拦不住；Windows 上也还没有真机验证）；agy 要求登录或卡住之后，直到你再用 agy 之前都不再运行它。你自己运行的 agy 不受影响。上次查到的额度保存在本机 `%USERPROFILE%\.vibe-usage\antigravity-quota.json`。Google 表示不支持第三方应用读取额度。

查询到的额度只显示在 App 里，并缓存在本机，不会上传到 VibeCafé 服务器。

## 2. 登录之后，上传什么

**链接设备时。** 你点「登录并链接数据」后，App 把应用名和这台电脑的计算机名发给所用的服务器（默认是 VibeCafé 服务器），并打开该服务器提供的审批页。确认后，App 拿到一个 API Key，用来上传和读取你的数据。这个 Key 存在和 CLI 共用的配置文件里（见第 4 节）；App 启动时只要发现这里已有 Key，不管它是 App 还是 CLI 写的，都会开始自动同步。

**每次同步时**（App 在托盘里时每 30 分钟一次，面板打开时约每分钟一次，也可以手动「更新数据」），CLI 会读取本机支持的 AI 编程工具的日志和数据库，在本机算出用量，只上传新增或有变化的部分：

- **用量统计**，每 30 分钟一条：工具名、模型名、项目名、计算机名、这半小时的开始时间，以及各类 token 数（输入、输出、缓存读取、缓存写入、推理和合计）。
- **会话统计**，每个会话一条：工具名、项目名、会话编号的哈希值（不能反推出原编号）、第一条和最后一条消息的时间、持续时长、活跃时长、消息数、你发出的消息数，以及你一天 24 个小时里各个小时发出了几条消息。
- **客户端信息**：CLI 版本、由哪个程序发起（Windows App）及其版本、Node.js 版本、操作系统类型（Windows）、计算机名，以及一个每次同步随机生成的编号。

**项目名默认不上传。** VibeCafé 账号的「上传项目名称」默认是关闭的，关闭时所有工具的项目名都上传为 `unknown`。你在 VibeCafé 网页的 Vibe Usage 设置页打开它之后，项目名是这样取的：

- 多数工具取你工作时所在文件夹的名字，只取最后一级，不含完整路径；
- Codex 的会话如果在 Git 仓库里，取远程仓库的「所有者/仓库名」；
- **Kiro 命令行版（kiro-cli）目前上传的是完整的工作目录路径**，例如 `C:\Users\你的用户名\projects\demo`。这个问题已记录在 [vibe-cafe/vibe-usage#155](https://github.com/vibe-cafe/vibe-usage/issues/155)，修复后会更新本文；
- 有的工具没有项目信息，记为 `unknown`。

**Cursor。** 如果这台电脑装了 Cursor 并已登录，同步时 CLI 会读取 Cursor 保存在本机的登录令牌，向 Cursor（`cursor.com`）读取你这个账号的用量记录，再按上面的格式汇总上传（计算机名一律记为 `cursor-cloud`，项目名记为 `unknown`）。Cursor 的令牌只发给 `cursor.com`，不会上传到 VibeCafé 服务器。

**Antigravity。** Antigravity 的旧版历史记录（`.pb` 文件），要靠正在运行的 Antigravity 语言服务器才能读取。同步时，CLI 会查看本机进程的命令行，找出这个语言服务器的本机访问令牌（CSRF Token），再访问本机 `127.0.0.1` 上的这个服务。这些请求只在你的电脑内部进行，不发到外网。

**公开展示。** 上传的用量默认会在 VibeCafé 网站上公开展示，不登录也能看到：

- **排行榜**：分过去 24 小时、7 天、30 天三个时间段。有总榜（按费用、按 token 数各一份；网页上显示前 20 名，公开接口最多返回前 100 名），也有按工具和按模型族分的榜（各列前 10 名）。每一条显示你的昵称（handle）、token 数和费用。在 VibeCafé 网页的 Vibe Usage 设置页关闭「在排行榜中展示」后，你仍会出现在榜上，但别人看到的昵称是打码的，token 数和费用只显示大致量级（例如「1M+」「$100+」）；你自己仍能看到完整数据。这个开关要等排行榜下次刷新才生效：24 小时榜约 1 小时内，7 天和 30 天榜最长约 4 小时，这段时间里别人仍可能看到原来的昵称和数字。
- **个人主页热力图**：你的个人主页会公开显示过去 365 天里每天的 token 数。在 VibeCafé 网页的账户设置页关闭「在个人主页展示 Vibe Usage 热力图」后，几分钟内别人就看不到了。

**读取数据。** App 面板上的费用和用量图表，是用你的 API Key 和你电脑的时区，从 VibeCafé 服务器读回来的。同步前，CLI 还会读取你账号里「上传项目名称」这一项设置。

## 3. 不会上传的内容

- 对话内容、提示词、AI 的回复、代码、文件内容、工具调用的参数和结果；
- 完整的文件路径（Kiro 命令行版的项目名例外，见第 2 节）；
- 任何 AI 工具的登录令牌、Cookie 或 API Key（它们只发给对应工具自己的服务，见第 1、2 节）；
- 订阅配额卡查到的额度。

## 4. 本机保存的内容

| 位置 | 内容 |
|---|---|
| `%USERPROFILE%\.vibe-usage\` | 和 CLI 共用的目录。`config.json`：API Key、服务器地址、计算机名和你在设置里添加的额外扫描目录，API Key 以明文保存在你的用户目录里。`state.json`：上传进度记录，包括每条已上传记录的索引（以明文的工具名、模型名、项目名、计算机名和时间段为键）和内容哈希、对应的服务器地址和 API Key 指纹、Cursor 账号的遮罩标识（只保留前后几个字符）和同步进度。`cli-cache\`：缓存的 CLI 版本。另有配额查询缓存、解析缓存和 CLI 自己的同步记录。 |
| `%APPDATA%\ai.vibecafe.vibe-usage.windows\` | `settings.json`（App 设置：选中的配额卡、托盘显示方式、ZCode 区域）和 `sync-history.json`（最近 10 次同步的摘要：时间、结果、耗时和固定的原因代码，不含路径、项目名和 Key）。 |
| `%LOCALAPPDATA%\ai.vibecafe.vibe-usage.windows\logs\` | `sync.log`：每次同步时 CLI 的完整输出，超过 512 KB 后转存为 `sync.log.1`。可能包含本机文件路径和工具名，只留在本机。 |
| Windows 凭据管理器 | 只存 ZCode Key（名称以 `ai.vibecafe.vibe-usage.windows/zcode/` 开头），不写入任何设置文件。 |
| 注册表 | 安装信息；如果你打开了「开机自启动」，还有一个名为 `Vibe Usage` 的启动项。 |

## 5. 诊断信息

设置「关于」里的「复制诊断信息」，只在你点击时才收集，结果复制到剪贴板，由你自己决定发给谁，App 不会自动上传。收集时，CLI 会测试一次能不能连上 VibeCafé 服务器和 npm 官方源。报告只含固定字段和错误代码，不含路径、Key、对话内容和任何原文，详见 [docs/DIAGNOSTICS.md](docs/DIAGNOSTICS.md)。

## 6. 你可以怎么控制

- **不想上传**：不要登录。已经登录的，在设置里点「重置配置」，会删除本机的 API Key 和上传记录，并停止自动同步。因为 App 和 CLI 共用登录配置，这样做也会让这台电脑上的 CLI 退出登录。
- **不想查询某个工具的额度**：在设置里取消选中它的配额卡。
- **不想上传项目名**：保持 VibeCafé 网页 Vibe Usage 设置页的「上传项目名称」关闭（默认就是关闭的）。
- **不想公开展示**：在 Vibe Usage 设置页关闭「在排行榜中展示」（关闭后仍在榜上，但昵称打码、数字只显示量级；要等排行榜下次刷新才生效，24 小时榜约 1 小时内，7 天和 30 天榜最长约 4 小时），在账户设置页关闭「在个人主页展示 Vibe Usage 热力图」（几分钟内生效）。
- **清空已上传的数据**：先停掉所有设备上的同步（在 App 里「重置配置」或卸载；用 CLI 同步的设备也要停掉），再在 VibeCafé 网页的 Vibe Usage 设置页点「清空全部云端数据」。这会删除这个账号下所有工具、所有机器、所有时间的用量记录和会话记录，不可撤销。不先停掉同步的话，之后产生的用量还会继续上传到这个账号。CLI 的 `reset` 命令会先删除云端数据，再从本机日志重新上传，不能用来清空。
- **卸载**：在 Windows「设置 → 应用」里卸载。卸载时勾选「删除应用程序数据」，会删除上表中 `%APPDATA%` 和 `%LOCALAPPDATA%` 下的 App 目录。`%USERPROFILE%\.vibe-usage\` 和 CLI 共用，卸载时不会删除；如需清除，先在 App 里「重置配置」，再手动删除这个文件夹。ZCode Key 可以先在设置里删除，或在 Windows 凭据管理器里删除。

## 7. 第三方服务

App 和 CLI 会连接上文提到的第三方服务。这些请求按各服务方自己的隐私政策处理：

- GitHub：[GitHub General Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)
- npm：[npm Privacy](https://docs.npmjs.com/policies/privacy)
- 微软（WebView2）：[Microsoft Privacy Statement](https://www.microsoft.com/en-us/privacy/privacystatement)
- OpenAI（Codex）：[OpenAI Privacy Policy](https://openai.com/policies/privacy-policy/)
- Anthropic（Claude Code）：[Anthropic Privacy Policy](https://www.anthropic.com/legal/privacy)，以及 [Claude Code 数据使用说明](https://code.claude.com/docs/en/data-usage)
- Kimi（Kimi Code）：[Kimi 隐私政策](https://www.kimi.com/user/agreement/userPrivacy?version=v2)
- 智谱 BigModel（ZCode 国内区）：[智谱开放平台隐私政策](https://docs.bigmodel.cn/cn/terms/privacy-policy)
- Z.ai（ZCode 海外区）：[Z.ai Privacy Policy](https://docs.z.ai/legal-agreement/privacy-policy)
- OpenCode（OpenCode Go）：[OpenCode Privacy Policy](https://opencode.ai/legal/privacy-policy)
- Cursor：[Cursor Privacy Policy](https://cursor.com/privacy)
- Google（Antigravity / AGY）：[Google 隐私权政策](https://policies.google.com/privacy)

上传到 VibeCafé 服务器的数据，由 VibeCafé 负责保存和处理。

## 8. 问题反馈

对本文有疑问，或发现 App 的实际行为和本文不一致，请在本仓库提交 [Issue](https://github.com/vibe-cafe/vibe-usage-windows/issues)。

---

## English summary

This summary is a translation of the Chinese text above, which is the authoritative version.

The production app uses `https://vibefriends.com` for device linking and usage data. If you configure a custom server, the app uses that address.

- **No usage data is uploaded unless this computer has been signed in.** Signing in means clicking 「登录并链接数据」 (sign in and link) in the app, or having signed in with the CLI on this computer before. The app and the CLI share one configuration file, so an existing CLI sign-in is reused and the app starts syncing automatically.
- **After sign-in**, each sync uploads, to the Vibe Usage server operated by VibeCafé: per-30-minute token counts by tool, model, project name and computer name; per-session statistics (tool, project name, a one-way hash of the session id, first/last message time, duration, active time, message counts, an hourly histogram of your prompts); and client metadata (CLI and app versions, Node.js version, OS, computer name, a random per-sync id).
- **Project names are not uploaded by default.** The account setting 「上传项目名称」 (upload project names) on the website's Vibe Usage settings page is off by default, and while it is off every tool uploads `unknown`. When you turn it on, project names are: for most tools, the last folder name of the working directory; for Codex sessions in a Git repository, `owner/repo` of the remote; **the Kiro CLI currently uploads the full working-directory path** (a known issue, tracked in [vibe-cafe/vibe-usage#155](https://github.com/vibe-cafe/vibe-usage/issues/155)); `unknown` where a tool has no project information.
- **Public display (on by default):** uploaded usage is shown publicly on the VibeCafé website without sign-in. Leaderboards cover the last 24 hours, 7 days and 30 days: overall boards by cost and by tokens (the web page shows the top 20; the public API returns up to the top 100) and boards per tool and per model family (top 10 each), each entry showing your handle, token count and cost. Turning off 「在排行榜中展示」 (show in leaderboard) on the Vibe Usage settings page keeps you on the boards but masks your handle and shows only rough magnitudes (for example "1M+", "$100+") to others; you still see your full data. The change takes effect at the next leaderboard refresh: within about an hour for the 24-hour boards and up to about four hours for the 7-day and 30-day boards, and others may still see your handle and numbers until then. Your public profile shows a heatmap of daily token counts for the past 365 days; turn off 「在个人主页展示 Vibe Usage 热力图」 (show the Vibe Usage heatmap on my profile) in account settings to hide it; this takes effect within a few minutes.
- **Cursor:** if Cursor is installed and signed in, the CLI reads Cursor's locally stored token and fetches your usage events from `cursor.com`; the token is sent only to `cursor.com`. **Antigravity:** to read legacy `.pb` history, the CLI reads the CSRF token of the running Antigravity language server from the local process list and calls that server on `127.0.0.1`; this traffic stays on your computer.
- **Never uploaded:** conversation content, prompts, replies, code, file contents, tool-call payloads, full paths (except Kiro CLI project names, above), any AI tool credentials, or the subscription quotas shown in the app.
- **Proxy:** the app's own requests, and the proxy it hands to the CLI and Claude Code, follow the current Windows system proxy (PAC included) and proxy environment variables; when a PAC cannot be resolved, a manual proxy that is also switched on is used (as Windows does), otherwise the request fails instead of going direct. Claude Code can only use an HTTP proxy: with a SOCKS-only system proxy it currently does not go through the proxy (a known gap, to be fixed). After a login launch with a sync or quota read to do and a system proxy set, quota reads and syncs first wait (update checks and the panel's Vibe usage fetch do not) while the app checks every 2 seconds, for at most 2 minutes, that the proxy is up: it reads the system proxy settings (fetching the PAC; a PAC host that asks for Windows sign-in gets the current Windows identity, as the system does by default) and opens a connection to the proxy port; that connection sends no data and no account. With no system proxy set, nothing waits.
- **Egress check:** reading Codex, Claude or Antigravity quotas uses your account with that vendor. When no proxy is detected for it (no system proxy, a PAC that sends it direct, or a proxy this computer cannot see such as TUN or one on the router), the app first sends, on the same route, one request to `/cdn-cgi/trace` on that vendor's Cloudflare edge (`chatgpt.com` for Codex, Anthropic's host for Claude; the CLI does the same for Antigravity through `chatgpt.com`, or `api.anthropic.com` when only that one goes direct like Google, since Google's hosts have no such page), with no account, cookie or token, and reads only the country code. With an egress in mainland China, Hong Kong or Macau, or none confirmed, nothing carrying the account is sent and the card says why; the next read checks again. At most once per host every 10 minutes (once a minute while paused), and again after a network change. Cloudflare sees your egress IP, but no account. Cursor, Kimi, ZCode and OpenCode Go are not checked. In Settings → 隐私保护 (privacy protection), 「没检测到代理时」 (when no proxy is detected) offers three options: 「自动判断（推荐）」 (decide automatically, recommended) is the check above; 「暂停读取」 (pause reading) pauses those reads whenever no proxy is detected, without a check; 「照常读取」 (read as usual) skips the check and reads directly as before.
- **Without signing in**, the app contacts: GitHub (update checks every 24 hours and installer downloads), the npm registry (to fetch and verify the CLI), Microsoft (WebView2 bootstrapper, only when missing during installation), and, for the subscription quota cards you keep enabled, each AI tool's own service, using credentials already on your computer: OpenAI for Codex; Anthropic through your local Claude Code; Kimi, which refreshes a token close to expiry via `auth.kimi.com` and writes it back to Kimi's own credential file; BigModel or Z.ai with the ZCode key you enter, stored in Windows Credential Manager; OpenCode, with the login OpenCode saved on your computer (an API key, or an OpenCode account sign-in's access token and the workspace it chose; read only, never rewritten or renewed — an expired sign-in sends nothing until OpenCode renews it); Google through your local, signed-in Antigravity (only after you used Antigravity on this computer, judged from the size and modification time of its local conversation files without reading them — never when there is no conversation at all — at most every 5 minutes and up to an hour apart after failures; otherwise the last reading is shown, with windows past their reset shown as recovered. It asks, in order, the official `agy -p /usage`, a running Antigravity desktop's own local service — the app reads that service's local CSRF token from the process list and sends it only to that process on `127.0.0.1` — and only then a short-lived copy of the desktop's language server started under the desktop's real version. No prompt is sent; `agy` or the desktop asks Google with its own login, which the app never reads, and processes the app starts are stopped afterwards. In the `agy` the app starts, self-update is turned off and do-nothing browser launchers are put first so a failed sign-in cannot open a browser page through them (not yet verified on a real Windows machine); after `agy` asks for sign-in or hangs, the app does not run it again until you use `agy` yourself. Your own `agy` is unaffected. The last reading is kept in `%USERPROFILE%\.vibe-usage\antigravity-quota.json`. Google says reading quota from third-party apps is not supported). Grok quotas are read from a local log without network access. Quota results stay on your computer. **Claude:** in the quota-check process, the app turns off 15 separate switches (telemetry, error reports, auto-update, feedback surveys, feature-flag downloads and the like); only the master switch `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` is cleared, because it would block the quota read itself. Measured with Claude Code 2.1.330, a subscription login and a proxy variable set, only connections to `api.anthropic.com` showed up, matching in the code the quota read (skipped while its own snapshot is under a minute old) plus the start-up configuration request Claude Code sends at every start, which no separate switch covers (request paths from reading the code). From Claude Code's code it may also: preconnect to your configured API address when no `HTTP(S)_PROXY` variable is set; refresh the account profile when its copy is missing or over a day old; read a Team / Enterprise organization's remote settings and policy limits; refresh an eligible Pro / Max account's invite eligibility (personal plans included) when the cached record is missing or a day old; and renew an expired login with its own login service (`platform.claude.com`). When Claude Code is set up for Bedrock, Vertex or another third-party provider (no plan quota), it may check model availability with your cloud account at start, as before: a fixed "." to one or more models, at most 1 output token each, which may cost a little usage. If your organization declares extra plugin marketplaces in managed settings, it may fetch their configured addresses. OpenTelemetry export or gateway model discovery that you or your organization turned on is left as configured. These settings apply to that process only and are never written to a file; your normal Claude Code settings are unchanged. If your organization's managed settings force that master switch on (merged key by key, as Claude Code does: the HKLM `SOFTWARE\Policies\ClaudeCode` registry value when it sets the key, else `managed-settings.json` with `managed-settings.d`, and HKCU only when neither holds a policy), Claude Code cannot read quotas, so the app does not start the quota probe; Claude Desktop's quota record is still read and shown, and when there is none and Claude Code would otherwise be started, the Claude card says why. A policy delivered only from the server (the claude.ai admin console) is not visible to the app; then Claude Code is started and applies it itself, possibly answering from a recent snapshot or without windows. Only that one setting is used: managed settings are parsed in memory and everything else in them is dropped, never kept, logged or put in diagnostics.
- **Stored locally:** `%USERPROFILE%\.vibe-usage\` (shared with the CLI: API key in plain text, server address, computer name, extra scan folders; `state.json` with an index of uploaded records keyed by plain-text tool, model, project name, computer name and time slot plus content hashes, the server address and API key fingerprint, a masked Cursor account label and sync progress; CLI cache and other caches), `%APPDATA%\ai.vibecafe.vibe-usage.windows\` (settings, last 10 sync summaries), `%LOCALAPPDATA%\ai.vibecafe.vibe-usage.windows\logs\` (CLI output log, capped at 512 KB), and Windows Credential Manager (ZCode key only).
- **Controls:** don't sign in, or use 「重置配置」 (reset configuration), which removes the API key and stops syncing, and also signs out the CLI on this computer; turn off individual quota cards; keep project-name upload off (the default); turn off leaderboard display and the profile heatmap on the website. To clear uploaded data, first stop syncing on every device, then use 「清空全部云端数据」 (clear all cloud data) on the website's Vibe Usage settings page; it deletes all usage and session records of the account and cannot be undone. The CLI's `reset` command deletes cloud data and then re-uploads from local logs, so it is not suitable for permanently clearing cloud data. Uninstall from Windows Settings; the shared `%USERPROFILE%\.vibe-usage\` folder is kept and can be deleted manually.
- **Third-party privacy policies:** listed in section 7 above.
- **Contact:** open an issue in this repository.
