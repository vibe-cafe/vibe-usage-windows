using static VibeUsage.Display.Tests.UsageDisplayRulesTests;

namespace VibeUsage.Display.Tests;

public class RecentUsageCardTests
{
    private readonly UsageFiguresKeeper keeper = new();
    private static readonly UsageReadKey Today = UsageReadKey.For(1, UsageRange.Today, Now, Zone);
    private static readonly UsageReadKey Week = UsageReadKey.For(1, new(Period.SevenDays), Now, Zone);
    private static LoadedUsage Load(UsageReadKey key, double cost) => new(key, Snap([B("2026-11-27T02:00Z", cost: cost)], key.Range));
    private UsageFigures Show(UsageReadKey key, LoadedUsage? loaded, bool pending) => keeper.Resolve(key, loaded, pending, true, Now, Zone);

    [Fact]
    public void aPeriodWhoseReadFailsAtOnceStopsFadingAndShowsDashes()
    {
        Assert.Equal(5, Show(Today, Load(Today, 5), false).Snapshot!.Total.Cost);
        Assert.Equal(FiguresState.Faded, Show(Week, null, true).State);
        var failed = Show(Week, null, false);
        Assert.Equal(FiguresState.Unavailable, failed.State);
        Assert.Equal("—", new RecentUsageFigures(failed.Snapshot?.Total, null, new()).Text(RecentRow.Cost));
        Assert.Equal(FiguresState.Loading, Show(Week, null, true).State);
    }

    [Fact]
    public void aRefreshWithAsManyBucketsButOtherFiguresRedraws()
    {
        Assert.Equal(5, Show(Week, Load(Week, 5), false).Snapshot!.Total.Cost);
        Assert.Equal(9, Show(Week, Load(Week, 9), false).Snapshot!.Total.Cost);
    }

    [Fact]
    public void aCachedPeriodShowsItsOwnFiguresThroughoutARefresh()
    {
        var cached = Load(Week, 7);
        Show(Week, cached, false);
        Show(Today, Load(Today, 1), false);
        var refresh = Show(Week, cached, true);
        Assert.Equal((FiguresState.Current, 7d), (refresh.State, refresh.Snapshot!.Total.Cost));
        Assert.Equal(8, Show(Week, Load(Week, 8), false).Snapshot!.Total.Cost);
    }

    [Fact]
    public void subscriptionActivityHeroShowsTheSelectedToolsTimeAfterLoading()
    {
        var old = new ActivityReadKey(Today, new(["claude-code"]));
        var selected = new ActivityReadKey(Today, new(["codex"]));
        LoadedActivity Reading(ActivityReadKey key, int minutes) => new(key, new(new(minutes * 60, minutes * 120, 2), [], Now.AddYears(-1), Now, true, Zone));
        var earlier = Reading(old, 9);
        RecentUsageFigures Figures(LoadedActivity loaded) => new(null, loaded.For(selected)?.Summary, new());
        Assert.Equal("—", Figures(earlier).Text(RecentRow.ActiveTime));
        Assert.Equal("—", Figures(earlier).Text(RecentRow.WorkTime));
        var fresh = Figures(Reading(selected, 17));
        Assert.Equal("17m", fresh.Text(RecentRow.ActiveTime));
        Assert.Equal("34m", fresh.Text(RecentRow.WorkTime));
    }
}
