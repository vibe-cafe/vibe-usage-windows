using System.Text.Json;

namespace VibeUsage.Backend;

/// <summary>In-memory shell fixture. Override a command to drive delays or failures;
/// publish typed events to drive the same subscriptions as the native host.</summary>
public sealed class FakeBackend(SynchronizationContext? context = null) : BackendClient(context)
{
    private readonly object gate = new();
    private readonly Dictionary<string, JsonElement> current = [];
    private readonly CancellationTokenSource stopping = new();
    private bool closed;
    public Func<string, JsonElement, Task<JsonElement>>? Respond { get; set; }

    private JsonElement Result(string name)
    {
        if (current.TryGetValue(name, out var value)) return value;
        using var stream = typeof(FakeBackend).Assembly.GetManifestResourceStream($"Fixtures.{name}.json")
            ?? throw new BackendException(new("unknownCommand", "后端不支持这个命令"));
        using var doc = JsonDocument.Parse(stream);
        return doc.RootElement.GetProperty("value").Clone();
    }

    internal override Task<JsonElement> SendAsync(string name, JsonElement args)
    {
        lock (gate)
        {
            ObjectDisposedException.ThrowIf(closed, this);
            if (Respond is { } handler) return Scripted(handler, name, args, stopping.Token);
            switch (name)
            {
                case "update_setting":
                    var fields = Wire.Read<Dictionary<string, JsonElement>>(Result("get_settings"));
                    foreach (var field in args.GetProperty("change").EnumerateObject())
                        fields[field.Name] = field.Value;
                    var settings = Wire.Element(fields);
                    Publish(new BackendEvent.SettingsChanged(Wire.Read<AppSettings>(settings)));
                    return Task.FromResult(settings);
                case "set_launch_at_login":
                    current["get_launch_at_login"] = args.GetProperty("enabled");
                    break;
                case "start_cursor_login":
                case "cancel_cursor_login":
                case "logout_cursor":
                    Publish(new BackendEvent.CursorLoginUpdated(Wire.Read<CursorLogin>(Result(name))));
                    break;
                case "fetch_usage":
                    Publish(new BackendEvent.UsageChanged(new(Wire.Read<UsageQuery>(args.GetProperty("query")),
                        Wire.Read<UsageData>(Result(name)))));
                    break;
                case "get_rate_limits":
                    var cached = Wire.Read<QuotaCache>(Result("get_cached_rate_limits"));
                    var quotas = Wire.Read<Quota[]>(Result(name));
                    var providers = quotas.Select(q => q.Provider).ToArray();
                    Publish(new BackendEvent.QuotasChanged(new(cached.Snapshots, true, providers)));
                    Publish(new BackendEvent.QuotasChanged(new(quotas, false, providers)));
                    break;
            }
            return Task.FromResult(Result(name));
        }
    }

    public void Publish(BackendEvent value)
    {
        lock (gate)
        {
            ObjectDisposedException.ThrowIf(closed, this);
            switch (value)
            {
                case BackendEvent.SettingsChanged e: current["get_settings"] = Wire.Element(e.Value); break;
                case BackendEvent.QuotasChanged e:
                    current["get_cached_rate_limits"] = Wire.Element(e.Value.Refreshing ? e.Value : e.Value with { Providers = [] });
                    break;
                case BackendEvent.UpdateAvailable e: current["get_update_info"] = Wire.Element(e.Value); break;
                case BackendEvent.UpdateCleared: current["get_update_info"] = Wire.Element<UpdateInfo?>(null); break;
                case BackendEvent.SyncChanged e: current["get_sync_state"] = Wire.Element(e.Value); break;
                case BackendEvent.CursorLoginUpdated e: current["get_cursor_login"] = Wire.Element(e.Value); break;
                case BackendEvent.UsageChanged e: current["cached_usage"] = Wire.Element(e.Value); break;
            }
            Emit(value);
        }
    }

    private static async Task<JsonElement> Scripted(Func<string, JsonElement, Task<JsonElement>> handler,
        string name, JsonElement args, CancellationToken token)
    {
        try { return await handler(name, args).WaitAsync(token).ConfigureAwait(false); }
        catch (OperationCanceledException) when (token.IsCancellationRequested)
        {
            throw new BackendException(new("cancelled", "请求已取消"));
        }
    }

    protected override void CloseTransport()
    {
        lock (gate)
        {
            if (closed) return;
            closed = true;
            stopping.Cancel();
            stopping.Dispose();
        }
    }
}
