using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace VibeUsage.Backend.Tests;

public class LifecycleTests
{
    private sealed class Pump : SynchronizationContext
    {
        private readonly ConcurrentQueue<Action> actions = new();
        public override void Post(SendOrPostCallback action, object? state) => actions.Enqueue(() => action(state));
        public void Drain() { while (actions.TryDequeue(out var action)) action(); }
    }

    [Fact]
    public async Task Update_withdrawn_while_initial_read_is_in_flight_stays_withdrawn()
    {
        var ui = new Pump();
        using var app = new FakeBackend(ui);
        var read = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        app.Respond = (name, _) =>
        {
            Assert.Equal("get_update_info", name);
            // Fire before the snapshot method even returns its incomplete task.
            app.Publish(new BackendEvent.UpdateCleared());
            return read.Task;
        };
        var values = new List<UpdateInfo?>();
        var observing = app.WatchUpdatesAsync(values.Add);
        read.SetResult(Wire.Element(new UpdateInfo("old", "https://example.invalid")));
        using var subscription = await observing;
        ui.Drain();
        Assert.Null(Assert.Single(values));
        subscription.Dispose();
        app.Publish(new BackendEvent.UpdateAvailable(new("later", "https://example.invalid")));
        ui.Drain();
        Assert.Single(values);
    }

    [Fact]
    public async Task Fake_keeps_local_settings_and_refresh_scope_and_discards_queued_events_on_close()
    {
        var ui = new Pump();
        using var fake = new FakeBackend(ui);
        IBackend app = fake;
        var events = new List<BackendEvent>();
        using var subscription = app.Subscribe(events.Add);
        await app.UpdateSettingAsync(SettingChange.QuotaSelectedTabId(Provider.KimiCode));
        Assert.Equal(Provider.KimiCode, (await app.GetSettingsAsync()).QuotaSelectedTabId);
        await app.UpdateSettingAsync(SettingChange.QuotaSelectedTabId(null));
        Assert.Null((await app.GetSettingsAsync()).QuotaSelectedTabId);
        await app.SetLaunchAtLoginAsync(true);
        Assert.True(await app.GetLaunchAtLoginAsync());
        fake.Publish(new BackendEvent.QuotasChanged(new([], true, [Provider.ClaudeCode])));
        var cached = await app.GetCachedRateLimitsAsync();
        Assert.True(cached.Refreshing);
        Assert.Equal(Provider.ClaudeCode, Assert.Single(cached.Providers));
        ui.Drain();
        Assert.IsType<BackendEvent.QuotasChanged>(events.Last());
        var count = events.Count;
        fake.Publish(new BackendEvent.UpdateCleared());
        app.Dispose();
        ui.Drain();
        Assert.Equal(count, events.Count);
        await Assert.ThrowsAsync<ObjectDisposedException>(app.GetSettingsAsync);
    }

    [Fact]
    public async Task Closing_fake_completes_a_pending_scripted_read()
    {
        using var fake = new FakeBackend();
        var read = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        fake.Respond = (_, _) => read.Task;
        var pending = fake.GetSettingsAsync();
        fake.Dispose();
        Assert.Equal("cancelled", (await Assert.ThrowsAsync<BackendException>(() => pending)).Code);
        read.SetResult(Wire.Empty);
    }

    [Fact]
    public void Dispose_records_unconfirmed_shutdown_once()
    {
        var failure = new BackendError("shutdown", "未能确认后台子进程已退出");
        int closes = 0;
        using var app = new NativeBackend(new(".", ".", ".", "test", false), 0,
            init: (_, _, _) => 0, close: () => { closes++; return failure; });
        app.Dispose();
        app.Dispose();
        Assert.Equal(failure, app.CloseError);
        Assert.Equal(1, closes);
    }

    [Fact]
    [Trait("Category", "Smoke")]
    public async Task Real_library_handshake_results_event_panic_and_close()
    {
        var dir = Directory.CreateDirectory(Path.Combine(AppContext.BaseDirectory, Guid.NewGuid().ToString("N")));
        try
        {
            var options = new HostOptions(dir.FullName, Path.Combine(dir.FullName, "data"),
                Path.Combine(dir.FullName, "resources"), "0.8.0", false);
            var mismatch = Assert.Throws<BackendException>(() => new NativeBackend(options, 0));
            Assert.Equal("versionMismatch", mismatch.Code);

            // Observe every native envelope before the client's pending.TryRemove.
            var envelopes = new ConcurrentQueue<string>();
            int callbacks = 0, active = 0, closeReturned = 0;
            var late = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            BackendError? reentrant = null;
            NativeBackend.Native.Callback? observer = null;
            nint Init(uint version, string json, NativeBackend.Native.Callback forward)
            {
                observer = pointer =>
                {
                    Interlocked.Increment(ref active);
                    try
                    {
                        if (Volatile.Read(ref closeReturned) != 0) late.TrySetResult();
                        envelopes.Enqueue(Marshal.PtrToStringUTF8(pointer)!);
                        if (Interlocked.Increment(ref callbacks) == 1)
                            reentrant = NativeBackend.ReadError(NativeBackend.Native.Close());
                        forward(pointer);
                    }
                    finally { Interlocked.Decrement(ref active); }
                };
                return NativeBackend.Native.Init(version, json, observer);
            }
            using var app = new NativeBackend(options, 0x80000001, new SynchronizationContext(), Init);
            var changed = new TaskCompletionSource<AppSettings>(TaskCreationOptions.RunContinuationsAsynchronously);
            using var events = app.Subscribe(e =>
            {
                if (e is BackendEvent.SettingsChanged settings) changed.TrySetResult(settings.Value);
            });
            Assert.True((await app.GetSettingsAsync()).ShowCostInTray);
            Assert.Equal("closeFromRuntime", reentrant?.Code);
            Assert.Equal("idle", (await app.GetSyncStateAsync()).Status);
            var saved = await app.UpdateSettingAsync(SettingChange.ShowCostInTray(false));
            Assert.False(saved.ShowCostInTray);
            Assert.False((await changed.Task.WaitAsync(TimeSpan.FromSeconds(5))).ShowCostInTray);
            Assert.False((await app.GetSettingsAsync()).ShowCostInTray);
            Assert.Equal("panic", (await Assert.ThrowsAsync<BackendException>(
                () => app.SendAsync("__panic", Wire.Empty))).Code);

            var pending = Enumerable.Range(0, 8).Select(_ => app.SendAsync("__pending", Wire.Empty)).ToArray();
            int admitted = 5 + pending.Length;
            app.Dispose();
            Volatile.Write(ref closeReturned, 1);
            Assert.Equal(0, Volatile.Read(ref active));
            Assert.Null(app.CloseError);
            foreach (var task in pending)
            {
                Assert.True(task.IsCompleted);
                Assert.Equal("cancelled", (await Assert.ThrowsAsync<BackendException>(() => task)).Code);
            }
            app.Dispose();
            Assert.Equal(0, NativeBackend.Native.Close());
            var refused = NativeBackend.Native.Call((ulong)admitted + 1, "get_settings", "{}");
            Assert.Equal(0UL, refused.RequestId);
            Assert.Equal("closed", NativeBackend.ReadError(refused.Error)?.Code);
            await Task.WhenAny(late.Task, Task.Delay(150));
            Assert.False(late.Task.IsCompleted, "关闭返回后仍收到原生回调");
            var ids = new List<ulong>();
            foreach (string envelope in envelopes)
            {
                using var doc = JsonDocument.Parse(envelope);
                if (doc.RootElement.GetProperty("kind").GetString() == "result")
                    ids.Add(doc.RootElement.GetProperty("requestId").GetUInt64());
            }
            // Five completed commands and eight cancelled at close, with no duplicates.
            Assert.Equal(Enumerable.Range(1, admitted).Select(id => (ulong)id), ids.Order());
            GC.KeepAlive(observer);
        }
        finally { dir.Delete(true); }
    }
}
