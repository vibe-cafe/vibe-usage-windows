using System;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using VibeUsage.App.Panel;
using VibeUsage.App.Shell;
using VibeUsage.App.State;
using VibeUsage.Backend;
using VibeUsage.Display;
using Xunit;
using DisplayProvider = VibeUsage.Display.Provider;

namespace VibeUsage.App.Tests;

public class SyncLabelTests
{
    private static readonly DateTimeOffset Now = new(2026, 11, 28, 15, 0, 0, TimeSpan.FromHours(8));

    [Fact]
    public void SyncedSetsTheNumberApart()
    {
        Assert.Equal(new SyncLabel.Synced("26", " 分钟前同步"), SyncLabel.Make("idle", null, Now.AddMinutes(-26), Now));
        Assert.Equal(new SyncLabel.Synced("3", " 小时前同步"), SyncLabel.Make("success", null, Now.AddHours(-3.5), Now));
        Assert.Equal(new SyncLabel.Synced(null, "刚刚同步"), SyncLabel.Make("idle", null, Now.AddSeconds(-20), Now));
    }

    [Fact]
    public void NeverSyncedAndSyncing()
    {
        Assert.Equal(new SyncLabel.Never(), SyncLabel.Make("idle", null, null, Now));
        Assert.Equal(new SyncLabel.Syncing(), SyncLabel.Make("syncing", null, Now.AddMinutes(-5), Now));
    }

    [Fact]
    public void ErrorKeepsTheFirstLineOfTheReason()
    {
        var label = SyncLabel.Make("error", "同步失败: 网络连接超时\n详细信息", Now.AddMinutes(-5), Now);
        Assert.Equal(new SyncLabel.Failed("网络连接超时", "同步失败: 网络连接超时\n详细信息"), label);
    }
}

public class PathMarkupTests
{
    [Fact]
    public void ReadsCompactSvgWithPackedArcFlags()
    {
        var d = PathMarkup.Parse("M9.205 8.658v-2.26a.797.797 0 00-.856 0l-5.97 3.473zm1 1h2");
        Assert.False(d.EvenOdd);
        Assert.Equal(2, d.Figures.Count);
        var first = d.Figures[0];
        Assert.True(first.Closed);
        Assert.Equal(new PathPoint(9.205, 8.658), first.Start);
        var arc = Assert.IsType<PathSegmentData.Arc>(first.Segments[1]);
        Assert.Equal((0.797, 0.797, false, false), (arc.RadiusX, arc.RadiusY, arc.LargeArc, arc.Clockwise));
        Assert.Equal(9.205 - 0.856, arc.To.X, 6);
        // After z the current point is the figure's start; m is relative to it.
        Assert.Equal(new PathPoint(10.205, 9.658), d.Figures[1].Start);
        Assert.Equal(new PathPoint(12.205, 9.658), d.Figures[1].Segments[0].To);
    }

    [Fact]
    public void ReadsXamlMarkupWithFillRule()
    {
        var d = PathMarkup.Parse("F0 M 0 0 L 10 0 10 10 H 0 V 5 C 1 2 3 4 5 6 S 7 8 9 10 Q 1 1 2 2 Z");
        Assert.True(d.EvenOdd);
        Assert.Equal(7, d.Figures[0].Segments.Count); // the second L pair repeats the line
    }

    [Theory]
    [InlineData(DisplayProvider.Codex)]
    [InlineData(DisplayProvider.ClaudeCode)]
    [InlineData(DisplayProvider.Cursor)]
    [InlineData(DisplayProvider.KimiCode)]
    [InlineData(DisplayProvider.ZCode)]
    [InlineData(DisplayProvider.Grok)]
    [InlineData(DisplayProvider.OpenCodeGo)]
    [InlineData(DisplayProvider.Antigravity)]
    public void EveryProviderMarkParses(DisplayProvider provider)
    {
        var d = PathMarkup.Parse(ProviderMarks.Markup(provider));
        Assert.NotEmpty(d.Figures);
        Assert.All(d.Figures.SelectMany(f => f.Segments), s => Assert.InRange(s.To.X, -0.5, 24.5));
    }
}

public class DemoDataTests
{
    private static async Task<(FakeBackend Backend, AppSettings Settings, QuotaCache Quotas, UsageSnapshot Usage)> Load(DemoScenario scenario)
    {
        var backend = DemoBackend.Create(scenario, new SynchronizationContext());
        var settings = await backend.GetSettingsAsync();
        var quotas = await backend.GetCachedRateLimitsAsync();
        var usage = await backend.CachedUsageAsync();
        return (backend, settings, quotas, usage!);
    }

    [Fact]
    public async Task FourSubscriptionsReadLikeTheMacDesign()
    {
        var (backend, settings, quotas, _) = await Load(DemoScenario.Subscriptions);
        using var _b = backend;
        Assert.Equal([Backend.Provider.Codex, Backend.Provider.ClaudeCode, Backend.Provider.Cursor, Backend.Provider.Grok],
            settings.SelectedQuotaProductIds);
        var snapshots = quotas.Snapshots.Select(DisplayMapping.Quota).ToArray();
        var enabled = settings.SelectedQuotaProductIds.Select(DisplayMapping.Provider).ToArray();
        var display = new QuotaPanelDisplay(snapshots, enabled, null, DateTimeOffset.Now);
        Assert.Equal(4, display.Overview.Total);
        Assert.Equal((1, 1), (display.Overview.NearlyEmpty, display.Overview.Fast));
        Assert.Equal(["超额 17%", "超额 8%", "进度正常", "余量 17%"], display.Overview.Rows.Select(r => r.Comment ?? "").ToArray());
        var codex = display.Overview.Rows[0];
        Assert.Equal(["5h", "7d"], codex.Rows.Select(m => m.Label).ToArray());
        Assert.Equal([12.0, 79.0], codex.Rows.Select(m => Math.Round(m.Remaining!.Value)).ToArray());
        Assert.Equal("mo", display.Overview.Rows[2].Rows[0].Label);
    }

    [Fact]
    public async Task OneSubscriptionOpensOnIt()
    {
        var (backend, settings, quotas, _) = await Load(DemoScenario.Single);
        using var _b = backend;
        var display = new QuotaPanelDisplay(quotas.Snapshots.Select(DisplayMapping.Quota).ToArray(),
            settings.SelectedQuotaProductIds.Select(DisplayMapping.Provider).ToArray(), null, DateTimeOffset.Now);
        Assert.Empty(display.Buttons);
        Assert.Equal(DisplayProvider.ClaudeCode, display.Detail?.Provider);
    }

    [Fact]
    public async Task TodayAddsUpToTheDesignsFigures()
    {
        var (backend, _, _, usage) = await Load(DemoScenario.Subscriptions);
        using var _b = backend;
        var snapshot = new UsageDisplaySnapshot(DisplayMapping.Buckets(usage.Data), UsageRange.Today, DateTimeOffset.Now, TimeZoneInfo.Local);
        Assert.Equal("$41.20", RecentUsageNumber.Cost(snapshot.Total.Cost));
        Assert.Equal("65.5M", RecentUsageNumber.Tokens(snapshot.Total.TotalTokens));
        Assert.Equal(3, snapshot.Ranking(Dimension.Tool, Metric.Tokens).Top.Count);
        Assert.Equal(2, snapshot.Ranking(Dimension.Terminal, Metric.Tokens).Listed.Count);
    }

    [Fact]
    public async Task SyncGoesThroughSyncingToSuccess()
    {
        var backend = DemoBackend.Create(DemoScenario.Subscriptions, new SynchronizationContext());
        using var _b = backend;
        var seen = new System.Collections.Concurrent.ConcurrentQueue<string>();
        var done = new TaskCompletionSource();
        backend.Subscribe(e =>
        {
            if (e is not BackendEvent.SyncChanged s) return;
            seen.Enqueue(s.Value.Status);
            if (s.Value.Status == "success") done.TrySetResult();
        });
        await backend.TriggerSyncAsync();
        await done.Task.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.Equal(["syncing", "success"], seen.ToArray());
    }

    [Fact]
    public async Task OtherCommandsFallBackToTheContractSamples()
    {
        var backend = DemoBackend.Create(DemoScenario.Subscriptions, new SynchronizationContext());
        using var _b = backend;
        Assert.False(await backend.GetLaunchAtLoginAsync());
        Assert.NotEmpty(await backend.GetQuotaProductsAsync());
    }
}
