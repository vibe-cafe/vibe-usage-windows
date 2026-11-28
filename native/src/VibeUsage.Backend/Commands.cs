using System.Text.Json;

namespace VibeUsage.Backend;

public abstract partial class BackendClient : IBackend
{
    internal abstract Task<JsonElement> SendAsync(string name, JsonElement args);

    private async Task<T> Read<T>(string name, JsonElement args) =>
        Wire.Read<T>(await SendAsync(name, args).ConfigureAwait(false)) ?? throw new JsonException("后端结果缺少数据");

    private async Task<T?> ReadOptional<T>(string name, JsonElement args) where T : class =>
        Wire.Read<T?>(await SendAsync(name, args).ConfigureAwait(false));

    private async Task Run(string name, JsonElement args)
    {
        if ((await SendAsync(name, args).ConfigureAwait(false)).ValueKind != JsonValueKind.Null)
            throw new JsonException("后端命令应返回空值");
    }

    public Task<AppStatus> GetAppStatusAsync() =>
        Read<AppStatus>("get_app_status", Wire.Empty);

    public Task<UsageData> FetchUsageAsync(UsageQuery query) =>
        Read<UsageData>("fetch_usage", Wire.Element(new QueryArgs(query)));

    public Task<UsageSnapshot?> CachedUsageAsync() =>
        ReadOptional<UsageSnapshot>("cached_usage", Wire.Empty);

    public Task PanelOpenedAsync(UsageQuery query) =>
        Run("panel_opened", Wire.Element(new QueryArgs(query)));

    public Task SettingsShownAsync() =>
        Run("settings_shown", Wire.Empty);

    public Task<DeviceCode> StartDeviceLinkAsync() =>
        Read<DeviceCode>("start_device_link", Wire.Empty);

    public Task CancelDeviceLinkAsync() =>
        Run("cancel_device_link", Wire.Empty);

    public Task TriggerSyncAsync() =>
        Run("trigger_sync", Wire.Empty);

    public Task<string> CollectDiagnosticsAsync() =>
        Read<string>("collect_diagnostics", Wire.Empty);

    public Task<SyncState> GetSyncStateAsync() =>
        Read<SyncState>("get_sync_state", Wire.Empty);

    public Task<Quota[]> GetRateLimitsAsync(bool force) =>
        Read<Quota[]>("get_rate_limits", Wire.Element(new RefreshArgs(force)));

    public Task<QuotaCache> GetCachedRateLimitsAsync() =>
        Read<QuotaCache>("get_cached_rate_limits", Wire.Empty);

    public Task<QuotaProduct[]> GetQuotaProductsAsync() =>
        Read<QuotaProduct[]>("get_quota_products", Wire.Empty);

    public Task<Quota[]> SetQuotaProductSelectedAsync(Provider provider, bool selected) =>
        Read<Quota[]>("set_quota_product_selected", Wire.Element(new SelectionArgs(provider, selected)));

    public Task<CredentialStatus> GetZCodeCredentialStatusAsync() =>
        Read<CredentialStatus>("get_zcode_credential_status", Wire.Empty);

    public Task<Quota[]> SetZCodeQuotaRegionAsync(ZCodeRegion region) =>
        Read<Quota[]>("set_zcode_quota_region", Wire.Element(new RegionArgs(region)));

    public Task<Quota[]> SetZCodeApiKeyAsync(ZCodeRegion region, string? apiKey) =>
        Read<Quota[]>("set_zcode_api_key", Wire.Element(new KeyArgs(region, apiKey)));

    public Task<CursorLogin> GetCursorLoginAsync() =>
        Read<CursorLogin>("get_cursor_login", Wire.Empty);

    public Task<CursorLogin> StartCursorLoginAsync() =>
        Read<CursorLogin>("start_cursor_login", Wire.Empty);

    public Task<CursorLogin> CancelCursorLoginAsync() =>
        Read<CursorLogin>("cancel_cursor_login", Wire.Empty);

    public Task<CursorLogin> LogoutCursorAsync() =>
        Read<CursorLogin>("logout_cursor", Wire.Empty);

    public Task<AppSettings> GetSettingsAsync() =>
        Read<AppSettings>("get_settings", Wire.Empty);

    public Task<AppSettings> UpdateSettingAsync(SettingChange change) =>
        Read<AppSettings>("update_setting", Wire.Element(new ChangeArgs(change.Fields)));

    public Task<SyncRecord[]> GetSyncHistoryAsync() =>
        Read<SyncRecord[]>("get_sync_history", Wire.Empty);

    public Task<bool> GetLaunchAtLoginAsync() =>
        Read<bool>("get_launch_at_login", Wire.Empty);

    public Task SetLaunchAtLoginAsync(bool enabled) =>
        Run("set_launch_at_login", Wire.Element(new LaunchArgs(enabled)));

    public Task<ExtraRoots> GetExtraRootsAsync() =>
        Read<ExtraRoots>("get_extra_roots", Wire.Empty);

    public Task AddExtraRootAsync(RootSource source, string path) =>
        Run("add_extra_root", Wire.Element(new RootArgs(source, path)));

    public Task RemoveExtraRootAsync(RootSource source, string path) =>
        Run("remove_extra_root", Wire.Element(new RootArgs(source, path)));

    public Task ResetConfigAsync() =>
        Run("reset_config", Wire.Empty);

    public Task OpenExternalAsync(string url) =>
        Run("open_external", Wire.Element(new UrlArgs(url)));

    public Task ExportTestDiagnosticsAsync(string destination) =>
        Run("export_test_diagnostics", Wire.Element(new DestinationArgs(destination)));

    public Task<UpdateInfo?> CheckForUpdateAsync() =>
        ReadOptional<UpdateInfo>("check_for_update", Wire.Empty);

    public Task<UpdateInfo?> GetUpdateInfoAsync() =>
        ReadOptional<UpdateInfo>("get_update_info", Wire.Empty);

    public Task InstallUpdateAsync() =>
        Run("install_update", Wire.Empty);
}
