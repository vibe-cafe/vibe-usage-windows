using System.Text.Json;

namespace VibeUsage.Backend;

public abstract record BackendEvent
{
    public sealed record DeviceLink(DeviceLinkState Value) : BackendEvent;
    public sealed record SyncChanged(SyncState Value) : BackendEvent;
    public sealed record UsageChanged(UsageSnapshot Value) : BackendEvent;
    public sealed record UsageFailed(UsageError Value) : BackendEvent;
    public sealed record SettingsChanged(AppSettings Value) : BackendEvent;
    public sealed record QuotasChanged(QuotaCache Value) : BackendEvent;
    public sealed record CursorLoginUpdated(CursorLogin Value) : BackendEvent;
    public sealed record CursorLoginChanged : BackendEvent;
    public sealed record UpdateAvailable(UpdateInfo Value) : BackendEvent;
    public sealed record UpdateCleared : BackendEvent;
    public sealed record ExitRequested : BackendEvent;

    internal static BackendEvent Parse(string name, JsonElement payload) => name switch
    {
        "device-link" => new DeviceLink(Wire.Read<DeviceLinkState>(payload)),
        "sync-state" => new SyncChanged(Wire.Read<SyncState>(payload)),
        "usage-updated" => new UsageChanged(Wire.Read<UsageSnapshot>(payload)),
        "usage-error" => new UsageFailed(Wire.Read<UsageError>(payload)),
        "settings-updated" => new SettingsChanged(Wire.Read<AppSettings>(payload)),
        "rate-limits-updated" => new QuotasChanged(Wire.Read<QuotaCache>(payload)),
        "cursor-login-updated" => new CursorLoginUpdated(Wire.Read<CursorLogin>(payload)),
        "cursor-login-changed" when payload.ValueKind == JsonValueKind.Null => new CursorLoginChanged(),
        "update-available" => new UpdateAvailable(Wire.Read<UpdateInfo>(payload)),
        "update-cleared" when payload.ValueKind == JsonValueKind.Null => new UpdateCleared(),
        "request-exit" when payload.ValueKind == JsonValueKind.Null => new ExitRequested(),
        _ => throw new JsonException("后端事件无效")
    };
}
