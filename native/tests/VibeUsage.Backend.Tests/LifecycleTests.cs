using System.Collections.Concurrent;
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

            // A production library rejects the smoke version before startup.
            using var app = new NativeBackend(options, 0x80000001, new SynchronizationContext());
            var changed = new TaskCompletionSource<AppSettings>(TaskCreationOptions.RunContinuationsAsynchronously);
            using var events = app.Subscribe(e =>
            {
                if (e is BackendEvent.SettingsChanged settings) changed.TrySetResult(settings.Value);
            });
            Assert.True((await app.GetSettingsAsync()).ShowCostInTray);
            Assert.Equal("idle", (await app.GetSyncStateAsync()).Status);
            var saved = await app.UpdateSettingAsync(SettingChange.ShowCostInTray(false));
            Assert.False(saved.ShowCostInTray);
            Assert.False((await changed.Task.WaitAsync(TimeSpan.FromSeconds(5))).ShowCostInTray);
            Assert.False((await app.GetSettingsAsync()).ShowCostInTray);
            Assert.Equal("panic", (await Assert.ThrowsAsync<BackendException>(
                () => app.SendAsync("__panic", Wire.Empty))).Code);

            var pending = Enumerable.Range(0, 8).Select(_ => app.SendAsync("__pending", Wire.Empty)).ToArray();
            app.Dispose();
            foreach (var task in pending)
            {
                Assert.True(task.IsCompleted);
                Assert.Equal("cancelled", (await Assert.ThrowsAsync<BackendException>(() => task)).Code);
            }
            app.Dispose();
            Assert.Equal(0, NativeBackend.Native.Close());
        }
        finally { dir.Delete(true); }
    }
}
