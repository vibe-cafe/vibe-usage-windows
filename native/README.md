# Windows 原生面板展示规则

`src/VibeUsage.Display` 是面板和托盘可共用的 .NET 10 纯计算库。它接收额度快照、用量桶、服务端活跃时长、选中的筛选、时区和当前时刻。库只依赖 .NET 基础类库，可以在 macOS 和 Windows 上运行。

## 构建与测试

在仓库根目录运行以下命令。本机首次还原 xUnit 测试依赖需要访问 NuGet。

```sh
DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_CLI_HOME="$(pwd)/.display-cli" NUGET_PACKAGES="$(pwd)/.display-packages" /Users/hutong/.dotnet/dotnet build native/src/VibeUsage.Display -c Release
DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_CLI_HOME="$(pwd)/.display-cli" NUGET_PACKAGES="$(pwd)/.display-packages" /Users/hutong/.dotnet/dotnet test native/tests/VibeUsage.Display.Tests -c Release
```

其他机器安装 .NET 10 SDK 后使用 PATH 上的 `dotnet`。Windows CI 的独立作业 `native-display` 执行 `dotnet test native/tests/VibeUsage.Display.Tests -c Release`。本机临时产物是仓库根目录的 `.display-cli/`、`.display-packages/` 和两个项目各自的 `bin/`、`obj/`。

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
