# 原生宿主接口 v1

C 签名见 [vibe_ffi.h](vibe_ffi.h)。Windows x64 加载 `vibe_ffi.dll`，macOS 加载 `libvibe_ffi.dylib`。命令名沿用 Rust 名称，JSON 字段采用 camelCase。[commands](commands/) 中每个文件以命令命名，包含一个参数对象和对应的成功值。[events](events/) 保存完整事件信封。样例均为合成数据。

## 调用和内存

1. 每个应用进程初始化一个后端。传入接口版本、回调和包含 `roamingConfig`、`localData`、`resources`（三个绝对路径）、`version`、`autostart` 的 JSON。版本不符会在启动任何服务前返回 `versionMismatch`。正式版和外测版在编译期区分，外测启用 `external-test-diagnostics`。
2. 宿主为每次调用分配大于零、递增的 64 位请求号，在提交前登记接收者。多个提交线程应共用同一把提交锁。接受时立即返回同一请求号和空错误指针。拒绝时返回零和错误字符串，宿主直接处理。每个已接受请求恰好收到一个结果，任务可以交错完成。JSON 不合法、未知命令和任务 panic 都通过该请求的结果返回。
3. 输入字符串由宿主分配，调用期间有效。Rust 为每个非空的同步错误和每个回调信封分配独立的 UTF-8 C 字符串。宿主复制或解码后，在 finally 中将原指针交给 `vibe_free` 一次。结果中字符串可以包含 JSON 转义的 NUL；`vibe_free(NULL)` 可直接调用。
4. 回调在后台线程运行，初始化返回前也可能收到事件。回调只复制消息、归还字符串并投递到宿主队列。宿主保持回调有效直至关闭返回。进入其他 DLL 操作前应退出回调栈，尤其是收到 `request-exit` 时应投递退出动作。
5. 关闭停止接收命令，取消在途任务，交付结果，然后等待后端和回调退出。每次关闭返回第一次关闭的结果，之后的调用返回 `closed`。后端的子进程监督器按应用进程管理，正式关闭后，下一次启动使用新进程。

成功、失败和事件的信封示例：

```json
{"kind":"result","requestId":12,"value":{"status":"idle"}}
```
```json
{"kind":"result","requestId":13,"error":{"code":"cancelled","message":"请求已取消"}}
```
```json
{"kind":"event","name":"update-cleared","payload":null}
```

同步错误直接返回同形状的 `{code,message}`。无返回值的命令成功时带 `"value":null`。接口层不记录命令参数、凭据或回调正文。

| 错误码 | 宿主收到它的条件 |
|---|---|
| `versionMismatch`、`alreadyOpen` | 接口版本不匹配，或已成功初始化 |
| `invalidRequestId`、`closed` | 请求号没有递增，或当前无法接收命令 |
| `invalidArguments`、`unknownCommand` | 参数格式无效，或命令未注册 |
| `backend` | 服务返回失败，`message` 携带可显示的原因 |
| `cancelled`、`panic` | 在途任务被关闭取消，或后端发生 panic |
| `internal`、`shutdown` | 创建运行时或编码失败，或后端未能确认子进程已退出 |

## 状态订阅

C# 外壳共用一个 `IBackend`。创建 `NativeBackend` 时传入界面线程的 `SynchronizationContext`，或在已有该上下文的界面线程创建。结果以 Task 返回，事件在同一队列上按顺序转交。窗口关闭时释放它的订阅，应用退出时释放后端。

`WatchUpdatesAsync` 同时处理发现新版和撤回，`WatchQuotasAsync` 同时处理额度快照和刷新状态。两者先挂事件再读当前值；读回之前已交付了新事件，就丢弃旧快照。其他状态共用 `WatchAsync`，传入当前值读取方法和对应的事件选择器。

```csharp
using var updates = await backend.WatchUpdatesAsync(showUpdate);
using var quotas = await backend.WatchQuotasAsync(showQuotas);
```

`rate-limits-updated` 携带 `snapshots`、`refreshing` 和 `providers`。开始、结束事件里的 `providers` 都是本轮实际到期或强制重读的产品，可为零个、一个或多个。读取当前值 `get_cached_rate_limits` 时，此列表表示此刻在读的产品，结束后为空。开机等待代理期间也可有 `refreshing=true` 和空列表。接口沿用后端现有刷新策略。

## 无界面开发和验证

`FakeBackend` 默认从共享样例返回合成结果。可以用 `Publish` 推送类型化事件，或用 `Respond` 模拟特定命令的等待、失败及连续响应。设置按项修改、登录项开关及已推送的状态保存在实例内存中。

正式库使用 `cargo build --release -p vibe-ffi`。真库冒烟使用相同的分发、线程、编解码和 C ABI，以 `--features smoke` 编译。冒烟握手号为 `0x80000001`，与生产 DLL 相遇会在初始化前失败。该构建仅接收本机测试用的读取、托盘费用显示开关以及合成的 panic、挂起命令；初始化不执行发现、访问凭据或启动联网任务。该库仅用于测试。

在仓库根目录构建并运行 macOS 冒烟：

```sh
cargo test --locked -p vibe-ffi
cargo build --locked --release -p vibe-ffi --features smoke
DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 DOTNET_CLI_HOME="$PWD/.display-cli" NUGET_PACKAGES="$PWD/.display-packages" ~/.dotnet/dotnet test native/tests/VibeUsage.Backend.Tests -c Release -p:SmokeLibrary="$PWD/target/release/libvibe_ffi.dylib"
```

Windows CI 向同一个测试项目传入 Release 生成的 `vibe_ffi.dll`。不加载 DLL 的契约与假后端测试可用 `--filter 'Category!=Smoke'` 单独运行。
