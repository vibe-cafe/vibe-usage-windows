using static VibeUsage.Display.Tests.UsageDisplayRulesTests;

namespace VibeUsage.Display.Tests;

public class UsageActivityTests
{
    [Theory]
    [InlineData("Asia/Shanghai", 3, 3, "2026-11-02T16:00Z", "2026-11-03T15:59:59.999Z")]
    [InlineData("America/New_York", 3, 3, "2026-11-03T05:00Z", "2026-11-04T04:59:59.999Z")]
    [InlineData("America/New_York", 1, 1, "2026-11-01T04:00Z", "2026-11-02T04:59:59.999Z")]
    [InlineData("Asia/Shanghai", 3, 18, "2026-11-02T16:00Z", "2026-11-18T15:59:59.999Z")]
    public void customDatesCoverWholeLocalDaysForUsageAndActivity(string zoneId, int first, int last, string start, string end)
    {
        var zone = TimeZoneInfo.FindSystemTimeZoneById(zoneId);
        var range = new UsageRange(new(2026, 11, first), new(2026, 11, last));
        Assert.Equal(D(start), range.Bounds(Now, zone).Start);
        Assert.Equal(D(end), range.InclusiveCustomEnd(zone));
        var grid = new UsagePlotGrid(range, Now, zone);
        Assert.Equal(D(end).AddMilliseconds(1), grid.Slots[^1].End);
        Assert.Equal(first == last, range.Hourly);
    }

    [Fact]
    public void summaryAndTrendDistinguishMissingHistoryFromIdleTime()
    {
        var grid = new UsagePlotGrid(UsageRange.Today, Now, Zone);
        var start = grid.Slots[0].Start;
        ActivityReport Report(DateTimeOffset? first, long seconds) => new(new(seconds, seconds, seconds > 0 ? 1 : null), [], first, start, true, Zone);
        var partial = Report(start.AddHours(3), 120);
        Assert.Equal("平均同时 1 个 · 2026-11-27 之前的记录无法重算", partial.PanelHelp);
        Assert.Null(partial.Series(grid)[2]);
        Assert.Equal(0, partial.Series(grid)[3]!.ActiveSeconds);
        Assert.Null(Report(null, 0).Summary);
        Assert.Equal("更新后显示", Report(start.AddHours(3), 0).PanelHelp);
        Assert.Equal(0, Report(start.AddDays(-1), 0).Summary!.ActiveSeconds);
    }

    [Fact]
    public void dailyActivityBelongsToLocalMidnight()
    {
        var grid = new UsagePlotGrid(new(Period.SevenDays), Now, Zone);
        var report = new ActivityReport(new(75, 105, 1.4), [new(D("2026-11-26T16:00Z"), 75, 105)],
            Now.AddYears(-1), grid.Slots[0].Start, false, Zone);
        Assert.Equal([0L, 0, 0, 0, 0, 0, 75], report.Series(grid).Select(t => t!.ActiveSeconds));
    }

    [Fact]
    public void subscriptionActivityUsesEffectiveToolsAcrossPeriodsReopensAndSingleSubscription()
    {
        var state = new UsagePanelState { SelectedSubscription = Provider.Codex };
        Provider[] both = [Provider.Codex, Provider.ClaudeCode];
        ActivityReadKey Key(Provider[] enabled) => new(UsageReadKey.For(1, state.Range, Now, Zone), state.EffectiveFilters(enabled));
        var today = Key(both);
        Assert.Equal(["codex"], today.Sources);
        state.Range = new(Period.SevenDays);
        var week = Key(both);
        Assert.NotEqual(today, week);
        Assert.Equal(["codex"], week.Sources);
        state.PanelDidOpen();
        var overview = Key(both);
        Assert.Empty(overview.Sources);
        state.SelectedSubscription = Provider.Codex;
        Assert.Equal(overview, Key([Provider.Codex]));
    }

    [Fact]
    public void subscriptionActivityIgnoresLateDetailAfterReturningToOverview()
    {
        var key = UsageReadKey.For(1, UsageRange.Today, Now, Zone);
        var detail = new ActivityReadKey(key, new(["codex"]));
        var report = new ActivityReport(new(1020, 1020, 1), [], Now.AddYears(-1), Now, true, Zone);
        var loaded = new LoadedActivity(detail, report);
        Assert.Null(loaded.For(new(key, new())));
        Assert.Same(report, loaded.For(new(key, new(["codex"], ["new model"]))));
        Assert.Null(loaded.For(new(key with { AccountGeneration = 2 }, new(["codex"]))));
        var original = new ActivityReadKey(key, new(["a", "b"], ["A"], ["x", "y"]));
        var cache = new Dictionary<ActivityReadKey, int> { [original] = 1 };
        Assert.Equal(1, cache[new(key, new(["b", "a"], ["B"], ["y", "x"]))]);
    }
}
