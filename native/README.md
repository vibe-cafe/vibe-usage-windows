# Windows 原生库

`src/VibeUsage.App` 是原生外壳（WinUI 3），见下文「原生外壳」。`src/VibeUsage.Backend` 提供原生后端的 C# 封装和可独立使用的假后端。命令与事件的共用样例、C 头文件、内存所有权、订阅顺序和真 DLL 测试命令见[接口契约](contract/README.md)。

## 面板展示规则

`src/VibeUsage.Display` 是面板和托盘可共用的 .NET 10 纯计算库。它接收额度快照、用量桶、服务端活跃时长、选中的筛选、时区和当前时刻。库只依赖 .NET 基础类库，可以在 macOS 和 Windows 上运行。

## 构建与测试

在仓库根目录运行以下命令。本机首次还原 xUnit 测试依赖需要访问 NuGet。

```sh
DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_CLI_HOME="$(pwd)/.display-cli" NUGET_PACKAGES="$(pwd)/.display-packages" "$HOME/.dotnet/dotnet" build native/src/VibeUsage.Display -c Release
DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_CLI_HOME="$(pwd)/.display-cli" NUGET_PACKAGES="$(pwd)/.display-packages" "$HOME/.dotnet/dotnet" test native/tests/VibeUsage.Display.Tests -c Release
```

上面是 SDK 装在 `~/.dotnet`、不在 PATH 时的写法；SDK 在 PATH 上时直接用 `dotnet`。Windows CI 的独立作业 `native-display` 执行 `dotnet test native/tests/VibeUsage.Display.Tests -c Release`。本机临时产物是仓库根目录的 `.display-cli/`、`.display-packages/` 和两个项目各自的 `bin/`、`obj/`。

## 与 Mac 对齐

按 `vibe-usage-app@7645d4401f8133bc1b1d08b5363afc4ceb9db924` 核对。xUnit 方法沿用对应 Swift 用例名，可以在两端按名称查找。输入全部来自合成样例。

| 调用入口 | 输出和约定 |
|---|---|
| `QuotaPanelDisplay` | 传入设置中的启用顺序、选中订阅、探测与刷新状态。概览含排序、告警计数和折叠汇总。详情给出双环窗口、中心、评语、前三行、其余额度和读取动作。额度百分数和期望剩余量均为 0–100，时间长度单位为秒。 |
| `UsagePanelState` | 每次真正打开时调用 `PanelDidOpen()`。周期初始为今天。手选周期、手选筛选、单位和分布指标在本次运行期间保留。多订阅详情临时覆盖工具，所有用量卡共用 `EffectiveFilters`。托盘传同一周期和空筛选。 |
| `UsageDisplaySnapshot` | 传入当前周期读到的用量、有效筛选、时区和服务端的命名表。一次聚合得到总数、曲线和四张分布卡的数据。模型按显示名匹配，活跃时长的时间轴保留模型筛选前的覆盖范围。 |
| `RecentUsageFigures`、`RecentUsageChart` | 用服务端去重的活跃时长和 AI 工时、快照、主指标及单位生成数字、坐标轴、峰值和每格悬停内容。曲线的横坐标为 0–1。 |
| `UsageReadKey`、`ActivityReadKey`、`UsageFiguresKeeper` | 调用方在换账号、换服务器、重置配置时递增账号代次。用量键包含代次、周期和时区，今天及 24H 还包含当地日期。活跃时长键再加生效的工具和终端。将当前周期最近一次成功的快照传给 keeper，并告知是否等待读取，即可区分当前数字、变淡的旧数字、加载和读取失败。 |

用量缓存保存该周期返回的完整用量桶。切换模型时用同一批桶和新筛选重建 `UsageDisplaySnapshot`。活跃时长由服务端单独汇总，`LoadedActivity.For` 只接受与当前条件相符的结果。

Windows 按总票 #85 决定 1 只提供新版，筛选和分布的维度为终端、工具、模型。Claude 桌面端记录沿用 Windows 的四种失败原因、来源说明和记录时间，凭据管理器读取失败及 ZCode 两个区域沿用 Windows 文案。外壳负责触发读取、预取、保存设置、展示和交互。

## 原生外壳

`src/VibeUsage.App` 是 WinUI 3 托盘程序（Windows App SDK、.NET 10），自包含、免安装、完整裁剪（#86 的结论，不用预编译成本机代码），只出 x64。主程序名 `vibe-usage-app.exe`。目前接的是内存里的假后端（`State/DemoBackend.cs`，Mac 设计稿的样例数），不联网、不读写凭据和用户数据；#94 换成真后端。

外壳只能在 Windows 上编译，改动靠 GitHub Actions 的 `Native app` 工作流（`.github/workflows/native-app.yml`，Windows 2025 机器）验证：

1. 跑 `tests/VibeUsage.App.Tests`（外壳的纯逻辑：打开和收起的规则、托盘重试、定位、点击竞态、启动参数、同步文字、图形路径、假数据），这一步在 macOS 上也能跑：`dotnet test native/tests/VibeUsage.App.Tests -c Release`（SDK 不在 PATH 时照上文的写法）。
2. `dotnet publish native/src/VibeUsage.App/VibeUsage.App.csproj -c Release -r win-x64 -p:Platform=x64`，出现裁剪警告或禁用接口警告就失败。
3. 另一台机器下载产物，`ci/screenshot.ps1` 用 PrintWindow 存原尺寸面板截图，`ci/tray-smoke.ps1` 真实点击托盘图标，从 App 自己的日志判每一项；切到 150% 后（`ci/dpi-icon.ps1` 同时检查托盘图标重载）再各跑一轮。截图、日志和逐项结果在产物 `native-ui-results` 里。

这些脚本会往运行机注入模拟的鼠标键盘、改主题和缩放，只在 Actions 上跑，不要在真机上运行。

外壳的规矩：

- 不写联网代码。`BannedSymbols.txt` 把下面这些列为编译错误：.NET 的 HTTP（`HttpClient` 及其处理器、`HttpRequestMessage`、`WebRequest`/`HttpWebRequest`/`FtpWebRequest`、`WebClient`、`HttpListener`）、套接字和监听（`Socket`、`TcpClient`、`TcpListener`、`UdpClient`、`NetworkStream`、`ClientWebSocket`、QUIC）、`SslStream`、`SmtpClient`、`Dns`、`Ping`；WinRT 的 `Windows.Web.Http.HttpClient`、各类套接字和监听、后台上传下载、联合源和 AtomPub 客户端；`WebView2`；会去拉地址的图片源（`BitmapImage`、`SvgImageSource`）；启动外部程序或网址的 `Process`、`Launcher`、`RemoteLauncher`。打开网页交给后端的 `open_external`。这是编译期的补充手段，按类型名拦截，拦不住列表外的写法，也不能代替运行时的联网实测。
- 不用键盘鼠标输入类接口（全局热键、低级钩子、`GetAsyncKeyState`、模拟按键）。关面板只靠窗口失活和进程外的无障碍事件钩子：前台变化；只看任务栏进程的鼠标捕获和焦点变化（托盘打开后任务栏本来就是前台，点它的空白处不会换前台）；启动时系统没给前台的话，再看留在前面的那个程序的鼠标捕获。钩子在面板开着时才挂，收起就摘。
- 手动启动、再次启动、键盘打开时面板拿键盘。Windows 只让刚被用户启动的进程在启动早期拿前台，WinUI 初始化会把这次机会用掉，所以 `Program.Main` 在初始化之前先对自己调用 `AllowSetForegroundWindow`，预热窗口也直接以激活方式显示。
- 不用靠反射的数据绑定和运行时加载的 XAML 字符串；界面在代码里搭，JSON 走源码生成。
- 日志只记窗口类名，不记任何窗口标题、输入或设置值。默认在 `%LOCALAPPDATA%\ai.vibecafe.vibe-usage.windows\logs\shell.log`，超过 1 MB 时启动换到 `shell.old.log`。

启动参数：`--autostart` 是开机自启登记带的参数，只进托盘；已在运行时再收到它就直接退出，不弹面板。其余以 `--test-` 开头的参数只给 CI 用：`--test-log-dir=`、`--test-pin`（不自动收起）、`--test-expand`、`--test-scroll-end`、`--test-demo=single`、`--test-rect-file=`、`--test-icon-file=`、`--test-tray-add-failures=`。
