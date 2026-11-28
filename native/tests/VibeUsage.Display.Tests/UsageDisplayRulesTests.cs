using System.Globalization;

namespace VibeUsage.Display.Tests;

public class UsageDisplayRulesTests
{
    internal static readonly TimeZoneInfo Zone = TimeZoneInfo.FindSystemTimeZoneById("Asia/Singapore");
    internal static DateTimeOffset D(string iso) => DateTimeOffset.Parse(iso, CultureInfo.InvariantCulture);
    internal static readonly DateTimeOffset Now = D("2026-11-27T07:42Z");
    internal static UsageBucket B(string time, long input = 1, double cost = 0, string source = "codex", string model = "m", string host = "mac") =>
        new(D(time), source, model, host, input, EstimatedCost: cost);
    internal static UsageDisplaySnapshot Snap(IEnumerable<UsageBucket> data, UsageRange? range = null,
        DateTimeOffset? now = null, TimeZoneInfo? zone = null) => new(data, range ?? UsageRange.Today, now ?? Now, zone ?? Zone);

    [Fact]
    public void todayStartsAtFirstLocalUsageAndCountsBothBucketFormats()
    {
        var old = B("2026-11-26T17:00Z", 120) with { CachedInputTokens = 40, OutputTokens = 20, ReasoningOutputTokens = 5 };
        var modern = old with { BucketStart = D("2026-11-26T17:30Z"), InputTokens = 100, CacheCreation5mTokens = 7, CacheCreation1hTokens = 13, EstimatedCost = 1.25 };
        var s = Snap([modern, old, B("2026-11-26T15:30Z", 999), B("2026-11-26T19:00Z", 30, 2), B("2026-11-26T19:50Z", 999)], now: D("2026-11-26T19:40Z"));
        Assert.Equal([370L, 0, 30], s.Series.Select(a => a.TotalTokens));
        Assert.Equal([1.25, 0, 2], s.Series.Select(a => a.Cost));
        Assert.Equal(new(270, 50, 80, 3.25), s.Total);
        Assert.Equal([17, 18, 19], s.Grid.Slots.Select(slot => slot.Start.UtcDateTime.Hour));
        Assert.Equal(370, s.PeakTokens);
        Assert.Equal(2, s.PeakCost);
        Assert.Equal([80d, 270, 50], s.TokenComposition.Listed.Select(r => r.Value));
    }

    [Fact]
    public void dailySeriesCrossesAMidnightDSTChangeWithoutLosingTheFollowingDay()
    {
        var s = Snap([B("2026-09-06T04:00Z", 5), B("2026-09-07T03:00Z", 7)], new(Period.SevenDays),
            D("2026-09-09T12:00Z"), TimeZoneInfo.FindSystemTimeZoneById("America/Santiago"));
        Assert.Equal([0L, 0, 0, 5, 7, 0, 0], s.Series.Select(a => a.TotalTokens));
    }

    [Fact]
    public void clearingTheNewPanelFiltersKeepsThePeriodAndClassicProjectSelection()
    {
        // #85 decision 1: only the three new-panel dimensions exist on Windows.
        var state = new UsagePanelState { Range = new(Period.ThirtyDays), Filters = new(["cursor"], ["Model"], ["mac"]) };
        state.ClearFilters();
        Assert.Equal(Period.ThirtyDays, state.Range.Period);
        Assert.Empty(state.Filters.Sources);
        Assert.Empty(state.Filters.Models);
        Assert.Empty(state.Filters.Hostnames);
    }

    [Fact]
    public void ninetyLocalDaysStartWithSixDaysAndEndWithSevenIncludingToday()
    {
        var s = Snap([B("2026-08-28T16:00Z",999),B("2026-08-29T16:00Z"),B("2026-09-03T16:00Z",2),
            B("2026-09-04T16:00Z",4),B("2026-11-20T16:00Z",8),B("2026-11-26T16:00Z",16)], new(Period.NinetyDays));
        Assert.Equal([3L, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 24], s.Series.Select(a => a.TotalTokens));
        Assert.Equal(D("2026-08-29T16:00Z"), s.Grid.Slots[0].Start);
        Assert.Equal(D("2026-09-04T16:00Z"), s.Grid.Slots[1].Start);
        Assert.Equal(new(D("2026-11-20T16:00Z"), Now), s.Grid.Slots[^1]);
    }

    [Fact]
    public void rolledUpFirstHourStillCountsInTheRollingDay()
    {
        var s = Snap([B("2026-11-26T10:00Z", 11), B("2026-11-26T10:05Z", 99), B("2026-11-27T10:00Z", 3)],
            new(Period.RollingDay), D("2026-11-27T10:15Z"));
        Assert.Equal(D("2026-11-26T10:15Z"), s.Grid.Slots[0].Start);
        Assert.Equal(11, s.Series[0].TotalTokens);
        Assert.Equal(3, s.Series[^1].TotalTokens);
        Assert.Equal(14, s.Total.TotalTokens);
    }

    [Fact]
    public void singleCustomDaySplitsTheEntireLocalDayIntoHours()
    {
        var range = new UsageRange(new(2026, 11, 18), new(2026, 11, 18));
        var s = Snap([B("2026-11-17T15:30Z"), B("2026-11-17T16:00Z", 2), B("2026-11-18T15:30Z", 4), B("2026-11-18T16:00Z", 8)], range);
        Assert.Equal(24, s.Series.Count);
        Assert.Equal((2L, 4L, 6L), (s.Series[0].TotalTokens, s.Series[^1].TotalTokens, s.Total.TotalTokens));
        Assert.Equal(D("2026-11-18T16:00Z"), s.Grid.Slots[^1].End);
        Assert.Equal(D("2026-11-18T15:59:59.999Z"), range.InclusiveCustomEnd(Zone));
    }

    [Theory]
    [InlineData(2, 2, 1)]
    [InlineData(60, 60, 1)]
    [InlineData(61, 9, 5)]
    [InlineData(63, 9, 7)]
    [InlineData(64, 10, 1)]
    public void customRangesSwitchToWeeksAfterSixtyLocalDaysCountingBackFromTheirEnd(int days, int count, int firstLength)
    {
        var end = new DateOnly(2026, 11, 17);
        var start = end.AddDays(1 - days);
        var s = Snap([B("2026-11-16T16:00Z", 2)], new(end, start));
        Assert.Equal(count, s.Series.Count);
        Assert.Equal(TimeSpan.FromDays(firstLength), s.Grid.Slots[0].End - s.Grid.Slots[0].Start);
        Assert.Equal(TimeSpan.FromDays(days <= 60 ? 1 : 7), s.Grid.Slots[^1].End - s.Grid.Slots[^1].Start);
        Assert.Equal(2, s.Series[^1].TotalTokens);
    }

    [Fact]
    public void aSingleCustomDayIncludesTheServersFirstPartialUTCHour()
    {
        var s = Snap([B("2026-11-17T18:00Z", 3), B("2026-11-18T18:00Z", 4)], new(new(2026, 11, 18), new(2026, 11, 18)),
            zone: TimeZoneInfo.FindSystemTimeZoneById("Asia/Kathmandu"));
        Assert.Equal(25, s.Series.Count);
        Assert.Equal(D("2026-11-17T18:15Z"), s.Grid.Slots[0].Start);
        Assert.Equal(D("2026-11-18T18:15Z"), s.Grid.Slots[^1].End);
        Assert.Equal((3L, 4L), (s.Series[0].TotalTokens, s.Series[^1].TotalTokens));
    }

    [Fact]
    public void subscriptionTemporarilyReplacesOnlyTheManualToolSelection()
    {
        const string stamp = "2026-11-27T02:00Z";
        UsageBucket[] data = [B(stamp,1,model:"alias1"),B(stamp,2,source:"opencode",model:"alias1"),B(stamp,4,source:"opencode",model:"alias2"),
            B(stamp,8,source:"opencode",model:"other"),B(stamp,16,source:"opencode",model:"alias2",host:"other")];
        var state = new UsagePanelState { Filters = new(["codex"], ["Model"], ["mac"]) };
        UsageDisplaySnapshot Read(params Provider[] enabled) => new(data, state.Range, Now, Zone, state.EffectiveFilters(enabled),
            new UsageNames
            {
                Models = new Dictionary<string, string> { ["alias1"] = "Model", ["alias2"] = "Model" },
                Sources = new Dictionary<string, string> { ["opencode"] = "OpenCode" }
            });
        Provider[] both = [Provider.Codex, Provider.OpenCodeGo];
        Assert.Equal(1, Read(both).Total.TotalTokens);
        state.SelectedSubscription = Provider.OpenCodeGo;
        var detail = Read(both);
        Assert.Equal(6, detail.Total.TotalTokens);
        foreach (var (d, name) in new[] { (Dimension.Model, "Model"), (Dimension.Tool, "OpenCode"), (Dimension.Terminal, "mac") })
            Assert.Equal(name, Assert.Single(detail.Ranking(d, Metric.Tokens).Top).Name);
        Assert.Equal(1, Read(Provider.OpenCodeGo).Total.TotalTokens);
        Assert.Equal(1, Read(Provider.Cursor, Provider.Codex).Total.TotalTokens);
        state.SelectedSubscription = null;
        Assert.Equal(1, Read(both).Total.TotalTokens);
    }

    [Fact]
    public void everyProductOverridesTheToolByItsUsageSource()
    {
        var state = new UsagePanelState();
        var providers = Enum.GetValues<Provider>();
        string[] expected = ["codex", "claude-code", "cursor", "kimi-code", "grok", "opencode", "zcode", "antigravity"];
        foreach (var (p, source) in providers.Zip(expected))
        {
            state.SelectedSubscription = p;
            Assert.Equal(source, Assert.Single(state.EffectiveFilters(providers).Sources));
        }
    }

    [Fact]
    public void rankingUsesEachCardsMetricAndKeepsEveryRemainingRow()
    {
        (string m, long input, double cost)[] entries = [("A", 50, 0), ("B", 40, 0), ("C", 30, 3), ("D", 20, 4), ("E", 15, 5)];
        var s = Snap(entries.Reverse().Select(e => B("2026-11-27T02:00Z", e.input, e.cost, model: e.m)));
        var tokens = s.Ranking(Dimension.Model, Metric.Tokens);
        Assert.Equal(["A", "B", "C"], tokens.Top.Select(r => r.Name));
        Assert.Equal(["D", "E"], tokens.Remainder.Select(r => r.Name));
        Assert.Equal((35d, 155d, 50d / 155), (tokens.RemainderValue, tokens.Total, tokens.Top[0].Share));
        var cost = s.Ranking(Dimension.Model, Metric.Cost);
        Assert.Equal(["E", "D", "C"], cost.Top.Select(r => r.Name));
        Assert.Equal(["A", "B"], cost.Remainder.Select(r => r.Name));
        Assert.Equal([RankColor.Blue, RankColor.Teal, RankColor.Magenta], cost.Top.Select(r => r.Color));
        Assert.Equal(5d / 12, cost.Top[0].Share);
        Assert.All(cost.Remainder, r => Assert.Equal(RankColor.Gray, r.Color));
        Assert.Equal(155, Assert.Single(s.Ranking(Dimension.Tool, Metric.Tokens).Top).Value);
    }

    [Fact]
    public void tappingTheHeroSwitchesOnlyItsOwnDisplayUnit()
    {
        var state = new UsagePanelState();
        Assert.False(state.UsesCny);
        state.ToggleHeroUnit();
        Assert.False(state.UsesChineseTokens);
        state.Hero = Hero.Token;
        state.ToggleHeroUnit();
        state.Hero = Hero.ActiveTime;
        state.ToggleHeroUnit();
        Assert.True(state.UsesCny && state.UsesChineseTokens);
        Assert.Equal(["1200万", "9999", "6550万", "1510万", "1亿", "2.1亿"], new long[] { 12000000, 9999, 65500000, 15100000, 99999999, 210000000 }
            .Select(n => RecentUsageNumber.Tokens(n, state.UsesChineseTokens)));
        state.PanelDidOpen();
        state.ToggleHeroUnit();
        Assert.False(state.UsesCny);
    }

    [Fact]
    public void presentationChoicesPreserveEffectiveFiltersAndLastForThisAppSession()
    {
        var s = new UsagePanelState { Filters = new(["codex"], ["Model"]), Range = new(Period.ThirtyDays), DetailsExpanded = true };
        s.ToggleHeroUnit();
        s.Hero = Hero.Token;
        s.ToggleHeroUnit();
        s[Dimension.Terminal] = Metric.Cost;
        s.SelectedSubscription = Provider.OpenCodeGo;
        s.PanelDidOpen();
        Assert.Same(s.Filters, s.EffectiveFilters([Provider.Codex, Provider.OpenCodeGo]));
        Assert.Equal(Period.ThirtyDays, s.Range.Period);
        Assert.True(s.UsesCny && s.UsesChineseTokens);
        Assert.Equal(Metric.Cost, s[Dimension.Terminal]);
        var relaunched = new UsagePanelState();
        Assert.Equal(Period.Today, relaunched.Range.Period);
        Assert.False(relaunched.UsesCny || relaunched.UsesChineseTokens);
        Assert.Equal(Metric.Tokens, relaunched[Dimension.Terminal]);
    }

    [Theory]
    [InlineData(Period.Today)]
    [InlineData(Period.RollingDay)]
    public void loadedTodaySnapshotExpiresAtTheNextLocalMidnight(Period period)
    {
        var range = new UsageRange(period);
        var before = D("2026-11-27T15:59:59Z");
        var key = UsageReadKey.For(1, range, before, Zone);
        var loaded = new LoadedUsage(key, Snap([B("2026-11-27T02:00Z", 5)], range, before));
        var keeper = new UsageFiguresKeeper();
        Assert.Equal(FiguresState.Current, keeper.Resolve(key, loaded, false, true, before, Zone).State);
        var after = before.AddSeconds(1);
        // An open panel can tick past midnight before its caller constructs a new selection.
        Assert.Null(keeper.Resolve(key, loaded, true, true, after, Zone).Snapshot);
        var next = UsageReadKey.For(1, range, after, Zone);
        Assert.Equal(FiguresState.Loading, keeper.Resolve(next, loaded, true, true, after, Zone).State);
    }

    [Fact]
    public void snapshotUsesOnlyTheResponseForTheSelectedRangeAndCustomDates()
    {
        var range = new UsageRange(new(2026, 11, 20), new(2026, 11, 27));
        var key = UsageReadKey.For(1, range, Now, Zone);
        var loaded = new LoadedUsage(key, Snap([B("2026-11-27T02:00Z")], range));
        var keeper = new UsageFiguresKeeper();
        foreach (var other in new UsageRange[] { new(Period.RollingDay), UsageRange.Today, new(new(2026, 11, 21), new(2026, 11, 27)) })
            Assert.Null(keeper.Resolve(UsageReadKey.For(1, other, Now, Zone), loaded, false, true, Now, Zone).Snapshot);
        Assert.Same(loaded.Snapshot, keeper.Resolve(key, loaded, false, true, Now, Zone).Snapshot);
    }

    [Fact]
    public void openingResetsOnlyTheSubscriptionHeroAndExpansion()
    {
        var s = new UsagePanelState { SelectedSubscription = Provider.Codex, Hero = Hero.ActiveTime, DetailsExpanded = true, Range = new(Period.SevenDays) };
        s.PanelDidOpen();
        Assert.Equal((null, Hero.Cost, false, Period.SevenDays), (s.SelectedSubscription, s.Hero, s.DetailsExpanded, s.Range.Period));
    }
}
