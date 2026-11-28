using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using VibeUsage.App.Shell;
using VibeUsage.App.State;
using VibeUsage.Backend;
using VibeUsage.Display;
using Xunit;
using B = VibeUsage.Backend;
using D = VibeUsage.Display;

namespace VibeUsage.App.Tests;

public class DisplayMappingTests
{
    private static D.QuotaSnapshot Map(string json) => DisplayMapping.Quota(Read(json));

    private static B.Quota Read(string json)
    {
        using var parser = new FakeBackend(new SynchronizationContext());
        using var doc = System.Text.Json.JsonDocument.Parse("[" + json + "]");
        var element = doc.RootElement.Clone();
        parser.Respond = (_, _) => Task.FromResult(element);
        return parser.GetRateLimitsAsync(false).GetAwaiter().GetResult()[0];
    }

    [Fact]
    public void CarriesWhatTheCardExplainsWith()
    {
        var s = Map("""
            {"provider":"codex","status":{"kind":"noData"},"source":"cache","emptyReason":"periodEnded",
             "liveFailure":{"code":"egressRegionUnsupported","region":"HK"},"signIn":"required"}
            """);
        Assert.Equal(D.QuotaSource.Cache, s.Source);
        Assert.Equal(D.EmptyReason.PeriodEnded, s.EmptyReason);
        Assert.Equal(D.Failure.EgressRegionUnsupported, s.LiveFailure);
        Assert.Equal("HK", s.EgressRegion);
        Assert.Equal(D.CursorSignIn.Required, s.SignIn);
        var http = Map("""{"provider":"codex","status":{"kind":"retryableError"},"liveFailure":{"code":"endpointHttpStatus","status":502}}""");
        Assert.Equal((D.Failure.EndpointHttpStatus, 502), (http.LiveFailure!.Value, http.HttpStatus!.Value));
    }

    [Theory]
    [InlineData("claudeBinaryNotFound", D.Failure.ClaudeBinaryNotFound)]
    [InlineData("desktopMixedOrganizations", D.Failure.DesktopMixedOrganizations)]
    [InlineData("credentialStoreUnavailable", D.Failure.CredentialStoreUnavailable)]
    [InlineData("cliRuntimeMissing", D.Failure.CliRuntimeMissing)]
    [InlineData("proxyUnsupported", D.Failure.ProxyUnsupported)]
    public void FailureCodesShareTheLibrarysNames(string code, D.Failure expected) =>
        Assert.Equal(expected, DisplayMapping.Failure(code));

    [Theory]
    [InlineData("somethingNew")]
    [InlineData("ProxyUnsupported")]
    [InlineData("")]
    public void UnknownCodesAreNotGuessed(string code) => Assert.Null(DisplayMapping.Failure(code));

    [Fact]
    public void UnknownReasonsSourcesAndSignInsStayNull()
    {
        var s = Map("""{"provider":"cursor","status":{"kind":"noData"},"source":"elsewhere","emptyReason":"later","signIn":"maybe"}""");
        Assert.Null(s.Source);
        Assert.Null(s.EmptyReason);
        Assert.Null(s.SignIn);
    }
}

public class DemoSceneTests
{
    private static async Task<(FakeBackend Backend, QuotaPanelDisplay Display)> Load(string id)
    {
        var scene = DemoScenes.For(new DemoScenario(id));
        var backend = DemoBackend.Create(new DemoScenario(id), new SynchronizationContext());
        var settings = await backend.GetSettingsAsync();
        var cache = await backend.GetCachedRateLimitsAsync();
        var products = await backend.GetQuotaProductsAsync();
        var enabled = settings.SelectedQuotaProductIds.Select(DisplayMapping.Provider).ToArray();
        var refreshing = cache.Refreshing ? cache.Providers.Select(DisplayMapping.Provider).ToHashSet() : null;
        var detected = products.Where(p => p.IsDetected).Select(p => DisplayMapping.Provider(p.Provider)).ToHashSet();
        var display = new QuotaPanelDisplay(cache.Snapshots.Select(DisplayMapping.Quota).ToArray(), enabled, scene.Selected,
            DateTimeOffset.Now, refreshing, detected);
        return (backend, display);
    }

    [Fact]
    public void EverySceneHasItsOwnId() =>
        Assert.Equal(DemoScenes.All.Count, DemoScenes.All.Select(s => s.Id).Distinct().Count());

    [Fact]
    public async Task EverySceneReadsThroughTheContract()
    {
        foreach (var scene in DemoScenes.All)
        {
            var (backend, display) = await Load(scene.Id);
            using var _ = backend;
            Assert.Equal(scene.Subscriptions.Length, display.Overview.Total);
            if (scene.Selected is { } selected && scene.Subscriptions.Length > 1) Assert.Equal(selected, display.Detail?.Provider);
        }
    }

    [Theory]
    [InlineData("win-zcode-needs-key", "请在设置中配置 BigModel API Key", "需 Key")]
    [InlineData("win-pause-hk", "网络出口在香港，已暂停读取", "已暂停读取")]
    [InlineData("win-not-installed", "未检测到安装", "未检测到安装")]
    [InlineData("win-unknown", "没读到额度，原因未知", "无数据")]
    [InlineData("win-never-read", "尚未读取", "尚未读取")]
    [InlineData("win-agy-not-installed", "请安装 agy 或 Antigravity", "读取失败")]
    [InlineData("win-desktop-orgs", "Claude 桌面端记录涉及多个组织，账号不明确", "读取失败")]
    [InlineData("win-period-grok", "账期已结束 · 用一次 Grok 后更新", "周期已结束")]
    [InlineData("state-cursor-signin-detail", "需要登录 Cursor", "需登录")]
    [InlineData("132m", "正在读取…", null)]
    public async Task WindowsStatesSayWhatTheDocsSay(string id, string title, string? comment)
    {
        var (backend, display) = await Load(id);
        using var _ = backend;
        Assert.Equal(title, display.Detail!.DetailTitle);
        Assert.Equal(comment, display.Detail.Comment);
    }

    [Fact]
    public async Task TheMacSampleReadsTheSameOnWindows()
    {
        var (backend, display) = await Load("131b");
        using var _ = backend;
        var codex = display.Detail!;
        Assert.Equal("Plus", codex.PlanLabel);
        Assert.Equal("重置券 ×2", codex.CreditsText);
        Assert.Equal(12, Math.Round(codex.RingCenter!.Remaining!.Value));
        Assert.Equal("5h", codex.RingCenter.Label);
        var (b2, claude) = await Load("132k");
        using var __ = b2;
        Assert.Equal("另有 1 项", claude.Detail!.FoldLabel);
        Assert.Equal("额外", claude.Detail.Rows[^1].Label);
    }

    [Fact]
    public async Task RefreshReadsForAMomentThenComesBack()
    {
        var backend = DemoBackend.Create(DemoScenario.Subscriptions, new SynchronizationContext());
        using var _ = backend;
        var seen = new System.Collections.Concurrent.ConcurrentQueue<bool>();
        var done = new TaskCompletionSource();
        backend.Subscribe(e =>
        {
            if (e is not BackendEvent.QuotasChanged q) return;
            seen.Enqueue(q.Value.Refreshing);
            if (!q.Value.Refreshing) done.TrySetResult();
        });
        await backend.GetRateLimitsAsync(true);
        Assert.True((await backend.GetCachedRateLimitsAsync()).Refreshing);
        await done.Task.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.Equal([true, false], seen.ToArray());
        Assert.False((await backend.GetCachedRateLimitsAsync()).Refreshing);
    }

    [Fact]
    public async Task DraggedOrderIsKeptInMemory()
    {
        var backend = DemoBackend.Create(DemoScenario.Subscriptions, new SynchronizationContext());
        using var _ = backend;
        var changed = new TaskCompletionSource<AppSettings>();
        backend.Subscribe(e => { if (e is BackendEvent.SettingsChanged s) changed.TrySetResult(s.Value); });
        B.Provider[] order = [B.Provider.Codex, B.Provider.ClaudeCode, B.Provider.Cursor, B.Provider.Grok];
        await backend.UpdateSettingAsync(SettingChange.QuotaProductOrder(order));
        Assert.Equal(order, (await changed.Task.WaitAsync(TimeSpan.FromSeconds(10))).QuotaProductOrder);
        Assert.Equal(order, (await backend.GetSettingsAsync()).QuotaProductOrder);
    }
}

public class SubscriptionOrderTests
{
    private static readonly B.Provider[] Stored = [B.Provider.ClaudeCode, B.Provider.Codex, B.Provider.Cursor, B.Provider.Grok, B.Provider.KimiCode];
    private static readonly B.Provider[] Enabled = [B.Provider.ClaudeCode, B.Provider.Codex, B.Provider.Cursor, B.Provider.Grok];

    [Fact]
    public void DroppedOnAButtonGoesBeforeIt() =>
        Assert.Equal([B.Provider.ClaudeCode, B.Provider.Grok, B.Provider.Codex, B.Provider.Cursor, B.Provider.KimiCode],
            SubscriptionOrder.Move(Stored, Enabled, B.Provider.Grok, B.Provider.Codex));

    [Fact]
    public void PastTheLastButtonGoesAfterTheLastEnabled() =>
        Assert.Equal([B.Provider.Codex, B.Provider.Cursor, B.Provider.Grok, B.Provider.ClaudeCode, B.Provider.KimiCode],
            SubscriptionOrder.Move(Stored, Enabled, B.Provider.ClaudeCode, null));

    [Fact]
    public void NoChangeAndDisabledProductsDoNothing()
    {
        Assert.Null(SubscriptionOrder.Move(Stored, Enabled, B.Provider.Codex, B.Provider.Codex));
        Assert.Null(SubscriptionOrder.Move(Stored, Enabled, B.Provider.ClaudeCode, B.Provider.Codex));
        Assert.Null(SubscriptionOrder.Move(Stored, Enabled, B.Provider.KimiCode, B.Provider.Codex));
        Assert.Null(SubscriptionOrder.Move(Stored, Enabled, B.Provider.Grok, null));
    }

    [Fact]
    public void EnabledProductsMissingFromTheStoredOrderJoinIt() =>
        Assert.Equal([B.Provider.Codex, B.Provider.ClaudeCode],
            SubscriptionOrder.Move([], [B.Provider.ClaudeCode, B.Provider.Codex], B.Provider.Codex, B.Provider.ClaudeCode));
}
