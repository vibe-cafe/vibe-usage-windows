namespace VibeUsage.Backend;

public interface IBackend : IDisposable
{
    IDisposable Subscribe(Action<BackendEvent> receive);
    Task<IDisposable> WatchAsync<T>(Func<Task<T>> read,
        Func<BackendEvent, (bool Changed, T Value)> select, Action<T> receive);
    Task<AppStatus> GetAppStatusAsync();
    Task<UsageData> FetchUsageAsync(UsageQuery query);
    Task<UsageSnapshot?> CachedUsageAsync();
    Task PanelOpenedAsync(UsageQuery query);
    Task SettingsShownAsync();
    Task<DeviceCode> StartDeviceLinkAsync();
    Task CancelDeviceLinkAsync();
    Task TriggerSyncAsync();
    Task<string> CollectDiagnosticsAsync();
    Task<SyncState> GetSyncStateAsync();
    Task<Quota[]> GetRateLimitsAsync(bool force);
    Task<QuotaCache> GetCachedRateLimitsAsync();
    Task<QuotaProduct[]> GetQuotaProductsAsync();
    Task<Quota[]> SetQuotaProductSelectedAsync(Provider provider, bool selected);
    Task<CredentialStatus> GetZCodeCredentialStatusAsync();
    Task<Quota[]> SetZCodeQuotaRegionAsync(ZCodeRegion region);
    Task<Quota[]> SetZCodeApiKeyAsync(ZCodeRegion region, string? apiKey);
    Task<CursorLogin> GetCursorLoginAsync();
    Task<CursorLogin> StartCursorLoginAsync();
    Task<CursorLogin> CancelCursorLoginAsync();
    Task<CursorLogin> LogoutCursorAsync();
    Task<AppSettings> GetSettingsAsync();
    Task<AppSettings> UpdateSettingAsync(SettingChange change);
    Task<SyncRecord[]> GetSyncHistoryAsync();
    Task<bool> GetLaunchAtLoginAsync();
    Task SetLaunchAtLoginAsync(bool enabled);
    Task<ExtraRoots> GetExtraRootsAsync();
    Task AddExtraRootAsync(RootSource source, string path);
    Task RemoveExtraRootAsync(RootSource source, string path);
    Task ResetConfigAsync();
    Task OpenExternalAsync(string url);
    Task ExportTestDiagnosticsAsync(string destination);
    Task<UpdateInfo?> CheckForUpdateAsync();
    Task<UpdateInfo?> GetUpdateInfoAsync();
    Task InstallUpdateAsync();
}
