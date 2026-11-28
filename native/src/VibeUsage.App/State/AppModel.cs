using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Threading.Tasks;
using VibeUsage.App.Shell;
using VibeUsage.Backend;
using VibeUsage.Display;
using DisplayProvider = VibeUsage.Display.Provider;

namespace VibeUsage.App.State;

/// <summary>
/// The shell's one application state (Mac <c>AppState</c>), shared by the
/// panel and later the settings window. Backend events arrive on the UI
/// thread; every subscription reads the current value after subscribing and
/// drops it when a newer event already arrived (<see cref="BackendWatches"/>).
/// The panel's navigation lives in <see cref="Panel"/>, not in the view, so
/// hiding the window keeps it and a real open resets it.
/// </summary>
internal sealed class AppModel : IDisposable
{
    private readonly IBackend backend;
    private readonly List<IDisposable> watches = [];
    private bool disposed;

    public AppStatus? Status { get; private set; }
    public AppSettings? Settings { get; private set; }
    public QuotaCache? Quotas { get; private set; }
    public UsageSnapshot? Usage { get; private set; }
    /// <summary>Products the backend found installed (「未检测到安装」 vs 「没读到额度，原因未知」); null until read.</summary>
    public IReadOnlySet<DisplayProvider>? Detected { get; private set; }
    public SyncState Sync { get; private set; } = new("idle");
    public UsagePanelState Panel { get; } = new();

    /// <summary>Something the panel draws changed.</summary>
    public event Action? Changed;

    /// <summary>The backend asked the host to quit (an update is being installed).</summary>
    public event Action? ExitRequested;

    public AppModel(IBackend backend) => this.backend = backend;

    public bool IsConfigured => Status?.Configured == true;

    public async Task StartAsync()
    {
        watches.Add(backend.Subscribe(e =>
        {
            if (e is BackendEvent.ExitRequested) ExitRequested?.Invoke();
        }));
        watches.Add(await backend.WatchQuotasAsync(q => Set(() => Quotas = q)));
        watches.Add(await backend.WatchAsync(backend.GetSyncStateAsync,
            e => e is BackendEvent.SyncChanged s ? (true, s.Value) : (false, Sync), s => Set(() => Sync = s)));
        watches.Add(await backend.WatchAsync<AppSettings?>(async () => await backend.GetSettingsAsync(),
            e => e is BackendEvent.SettingsChanged s ? (true, s.Value) : (false, null), s => Set(() => Settings = s)));
        watches.Add(await backend.WatchAsync(backend.CachedUsageAsync,
            e => e is BackendEvent.UsageChanged u ? (true, u.Value) : (false, null), u => Set(() => Usage = u)));
        Status = await backend.GetAppStatusAsync();
        Changed?.Invoke();
        ReadDetected();
    }

    /// <summary>The local product catalogue (no network): which products are installed.</summary>
    private async void ReadDetected()
    {
        try
        {
            var products = await backend.GetQuotaProductsAsync();
            Set(() => Detected = products.Where(p => p.IsDetected).Select(p => DisplayMapping.Provider(p.Provider)).ToHashSet());
        }
        catch (Exception ex)
        {
            ShellLog.Write($"get_quota_products failed: {(ex is BackendException b ? b.Code : ex.GetType().Name)}");
        }
    }

    private void Set(Action change)
    {
        if (disposed) return;
        change();
        Changed?.Invoke();
    }

    // -- what the panel draws ---------------------------------------------------------------

    /// <summary>Enabled subscriptions in the user's order (Mac <c>quotaCardProviders</c>).</summary>
    public IReadOnlyList<DisplayProvider> Enabled
    {
        get
        {
            if (Settings is not { } s) return [];
            var order = s.QuotaProductOrder.ToList();
            return s.SelectedQuotaProductIds
                .OrderBy(p => order.IndexOf(p) is int i && i >= 0 ? i : int.MaxValue)
                .Select(DisplayMapping.Provider)
                .ToArray();
        }
    }

    public QuotaPanelDisplay QuotaDisplay(DateTimeOffset now)
    {
        var snapshots = Quotas?.Snapshots.Select(DisplayMapping.Quota).ToArray() ?? [];
        var refreshing = Quotas is { Refreshing: true } q ? q.Providers.Select(DisplayMapping.Provider).ToHashSet() : null;
        var region = Settings is { } s ? DisplayMapping.Region(s.ZCodeQuotaRegion) : VibeUsage.Display.ZCodeRegion.BigModel;
        return new QuotaPanelDisplay(snapshots, Enabled, Panel.SelectedSubscription, now, refreshing, Detected, region);
    }

    /// <summary>Today's figures from the cached usage, or null before the first read.</summary>
    public UsageDisplaySnapshot? UsageDisplay(DateTimeOffset now)
    {
        if (Usage is not { } u) return null;
        return new UsageDisplaySnapshot(DisplayMapping.Buckets(u.Data), Panel.Range, now, TimeZoneInfo.Local,
            Panel.EffectiveFilters(Enabled));
    }

    public DateTimeOffset? LastSuccessfulSync =>
        (Sync.LastSyncAt ?? Settings?.LastSuccessfulSyncAt) is ulong ms ? DateTimeOffset.FromUnixTimeMilliseconds((long)ms) : null;

    // -- actions -----------------------------------------------------------------------------

    /// <summary>A real open of the panel: back to 概览, cost and collapsed (Mac <c>panelWillOpen</c>), then tell the backend.</summary>
    public void PanelOpening()
    {
        Panel.PanelDidOpen();
        Run(() => backend.PanelOpenedAsync(TodayQuery(DateTimeOffset.Now)), "panel_opened");
    }

    public void Select(DisplayProvider? provider)
    {
        Panel.SelectedSubscription = provider;
        Changed?.Invoke();
    }

    public void ToggleDetails()
    {
        Panel.DetailsExpanded = !Panel.DetailsExpanded;
        ShellLog.Write($"details {(Panel.DetailsExpanded ? "expanded" : "collapsed")}");
        Changed?.Invoke();
    }

    /// <summary>
    /// 「刷新额度」 and 「重试」: quotas only, never a usage sync (Mac
    /// <c>refreshQuotas</c>). Ignored while a read in the button's scope is
    /// running. The backend reads every enabled subscription: reading only
    /// the one on screen (Mac <c>refreshRateLimit(for:)</c>) is the backend's
    /// 「只刷一家」 (#95), which changes the requests and waits for the
    /// maintainer's approval.
    /// </summary>
    public void RefreshQuotas(QuotaPanelDisplay display)
    {
        if (display.IsRefreshing) return;
        ShellLog.Write($"quota refresh requested scope={(display.Detail?.Provider.ToString() ?? "all")}");
        Run(() => backend.GetRateLimitsAsync(force: true), "get_rate_limits");
    }

    /// <summary>
    /// Dragging a round button: <paramref name="provider"/> goes before
    /// <paramref name="before"/>, or after the last enabled subscription when
    /// null (Mac <c>moveQuotaProduct</c>), in the order Settings stores.
    /// </summary>
    public void MoveSubscription(DisplayProvider provider, DisplayProvider? before)
    {
        if (Settings is not { } s) return;
        var order = SubscriptionOrder.Move(s.QuotaProductOrder, Enabled.Select(DisplayMapping.Backend).ToArray(),
            DisplayMapping.Backend(provider), before is { } b ? DisplayMapping.Backend(b) : null);
        if (order == null) return;
        ShellLog.Write($"move {provider} before {(before?.ToString() ?? "end")}");
        Run(() => backend.UpdateSettingAsync(SettingChange.QuotaProductOrder(order)), "update_setting");
    }

    public void TriggerSync()
    {
        ShellLog.Write("sync requested");
        Run(backend.TriggerSyncAsync, "trigger_sync");
    }

    /// <summary>「网页版」「排行榜」: the backend checks the address and hands it to the default browser.</summary>
    public void OpenWeb(string path)
    {
        if (Status?.ApiUrl is not { Length: > 0 } server) return;
        ShellLog.Write($"open web {path}");
        Run(() => backend.OpenExternalAsync(server.TrimEnd('/') + "/" + path), "open_external");
    }

    public void SettingsShown() => Run(backend.SettingsShownAsync, "settings_shown");

    /// <summary>Same query the Tauri panel sends for 今天: from local midnight, in UTC.</summary>
    public static UsageQuery TodayQuery(DateTimeOffset now)
    {
        var midnight = new DateTimeOffset(now.Date, now.Offset);
        return new UsageQuery.From(midnight.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", CultureInfo.InvariantCulture));
    }

    private static async void Run(Func<Task> action, string what)
    {
        try
        {
            await action();
        }
        catch (Exception ex)
        {
            // Codes only: backend messages are for the UI and may name local paths.
            ShellLog.Write($"{what} failed: {(ex is BackendException b ? b.Code : ex.GetType().Name)}");
        }
    }

    /// <summary>Closes the backend: it stops accepting commands, ends its child processes (up to ~5 s) and returns.</summary>
    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        foreach (var w in watches) w.Dispose();
        watches.Clear();
        backend.Dispose();
    }
}
