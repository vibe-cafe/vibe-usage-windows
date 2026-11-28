using static VibeUsage.Display.Tests.UsageDisplayRulesTests;

namespace VibeUsage.Display.Tests;

public class RecentUsageDisplayTests
{
    [Fact]
    public void panelStateIsOneSetOfUsageControls()
    {
        var state = new UsagePanelState { SelectedSubscription = Provider.Cursor, Hero = Hero.Token };
        state.ToggleHeroUnit();
        var quota = new QuotaPanelDisplay([], [Provider.Codex, Provider.Cursor], state.SelectedSubscription, Now);
        Assert.Equal(Provider.Cursor, quota.Detail!.Provider);
        Assert.Equal("cursor", Assert.Single(state.EffectiveFilters([Provider.Codex, Provider.Cursor]).Sources));
        state.PanelDidOpen();
        Assert.Equal(Hero.Cost, state.Hero);
        Assert.True(state.UsesChineseTokens);
        Assert.Empty(state.EffectiveFilters([Provider.Codex, Provider.Cursor]).Sources);
    }

    [Fact]
    public void bigNumberSplitsSignIntegerDecimalsAndUnits()
    {
        Assert.Equal([new("$", NumberRole.Symbol), new("41", NumberRole.Integer), new("20", NumberRole.Fraction)], RecentUsageNumber.Parts("$41.20", true));
        Assert.Equal([new("$", NumberRole.Symbol), new("269", NumberRole.Integer)], RecentUsageNumber.Parts("$268.60", true));
        Assert.Equal(["¥", "1880"], RecentUsageNumber.Parts("¥1880.20", true).Select(p => p.Text));
        Assert.Equal(["$", "0"], RecentUsageNumber.Parts("$0.00", true).Select(p => p.Text));
        Assert.Equal(new("0012", NumberRole.Fraction), RecentUsageNumber.Parts("$0.0012", true)[^1]);
        Assert.Equal([new("65", NumberRole.Integer), new("5", NumberRole.Fraction), new("M", NumberRole.Unit)], RecentUsageNumber.Parts("65.5M"));
        Assert.Equal(["6550", "万"], RecentUsageNumber.Parts("6550万").Select(p => p.Text));
        Assert.Equal(["3", "h", "12", "m"], RecentUsageNumber.Parts("3h 12m").Select(p => p.Text));
        Assert.Equal(new("1,234", NumberRole.Integer), Assert.Single(RecentUsageNumber.Parts("1,234")));
        Assert.Equal(new("—", NumberRole.Integer), Assert.Single(RecentUsageNumber.Parts("—")));
    }

    [Fact]
    public void figuresFollowThePanelUnitsAndTheYuanSignAsDrawn()
    {
        var units = new UsagePanelState();
        var f = new RecentUsageFigures(new(65500000, 0, 0, 41.2037), null, units);
        Assert.Equal("$41.20", f.Text(RecentRow.Cost));
        Assert.Equal("$41.2037", f.ExactText(Hero.Cost));
        Assert.Equal("点一下换成人民币", f.UnitHint(Hero.Cost));
        Assert.Equal("65.5M", f.Text(RecentRow.Token));
        Assert.Equal("65,500,000", f.ExactText(Hero.Token));
        Assert.Equal("—", f.Text(RecentRow.ActiveTime));
        Assert.Equal("—", f.Text(RecentRow.WorkTime));
        units.ToggleHeroUnit();
        f = f with { Activity = new(11520, 16128, 1.4) };
        Assert.Equal("¥288.43", f.Text(RecentRow.Cost));
        Assert.Equal("¥288.4259", f.ExactText(Hero.Cost));
        Assert.Equal("点一下换成美元", f.UnitHint(Hero.Cost));
        Assert.Equal("3h 12m", f.Text(RecentRow.ActiveTime));
        Assert.Equal("4h 28m", f.Text(RecentRow.WorkTime));
        Assert.Equal([RecentRow.Token, RecentRow.ActiveTime, RecentRow.WorkTime], RecentUsageFigures.Rows(Hero.Cost));
        Assert.Equal([RecentRow.Cost, RecentRow.ActiveTime, RecentRow.WorkTime], RecentUsageFigures.Rows(Hero.Token));
        Assert.Equal([RecentRow.Cost, RecentRow.Token, RecentRow.WorkTime], RecentUsageFigures.Rows(Hero.ActiveTime));
        Assert.Equal("AI 工时", RecentUsageFigures.RowLabel(RecentRow.WorkTime));
        Assert.Equal("0m", (f with { Activity = new(0, 0, null) }).Text(RecentRow.WorkTime));
    }

    [Fact]
    public void tokensUseThreeSignificantDigitsAndDurationsPadMinutes()
    {
        long[] counts = [1040000000, 421000000, 93100000, 210000, 65500000, 12000000, 999950, 999449999, 1234, 950, 0, 999949999];
        Assert.Equal(["1.04B", "421M", "93.1M", "210K", "65.5M", "12M", "1M", "999M", "1.23K", "950", "0", "1B"], counts.Select(n => RecentUsageNumber.Tokens(n)));
        Assert.Equal(["0m", "30s", "48m", "3h 12m", "19h 05m", "47h"], new long[] { 0, 30, 2880, 11520, 68700, 169200 }.Select(RecentUsageNumber.Duration));
    }

    [Fact]
    public void todayRunsFromTheFirstUsageHourToNowWithRoundTicks()
    {
        var first = B("2026-11-27T01:00Z");
        double[] costs = [3.2, 6.6, 6.1, 0.4, 7.6, 9.4, 7.9];
        var chart = new RecentUsageChart(Snap(costs.Select((cost, i) => first with { BucketStart = first.BucketStart.AddHours(i), EstimatedCost = cost })), Hero.Cost, new());
        Assert.Equal(("9:00", "现在", 9.4, "$9.4"), (chart.StartLabel, chart.EndLabel, chart.Peak, chart.PeakLabel));
        Assert.Equal(["11:00", "13:00"], chart.Ticks.Select(t => t.Label));
        Assert.Equal(2 / 6.7, chart.Ticks[0].X, 5);
        Assert.Equal(new(0, 0), chart.Curve[0]);
        Assert.Equal(8, chart.Curve.Count);
        Assert.Equal(0.5 / 6.7, chart.HoverPoints[0].X, 5);
        Assert.Equal(1, chart.HoverPoints[^1].X);
        Assert.Equal(("11:00–12:00", "$6.10"), (chart.Tooltips[2].Period, chart.Tooltips[2].Value));
        Assert.Equal(("15:00–15:42", "$7.90"), (chart.Tooltips[^1].Period, chart.Tooltips[^1].Value));
    }

    [Fact]
    public void rollingDayStartsAtItsFirstValueAndNamesYesterday()
    {
        var s = Snap([B("2026-11-26T07:00Z", cost: 0.6), B("2026-11-26T13:00Z", cost: 2.9), B("2026-11-27T07:00Z", cost: 8)], new(Period.RollingDay));
        var chart = new RecentUsageChart(s, Hero.Cost, new());
        Assert.Equal(("昨天 15:00", "现在"), (chart.StartLabel, chart.EndLabel));
        Assert.Equal(["0:00", "8:00"], chart.Ticks.Select(t => t.Label));
        Assert.Equal(new(0, 0.6), chart.Curve[0]);
        Assert.Equal(("昨天 21:00–22:00", "$2.90"), (chart.Tooltips[6].Period, chart.Tooltips[6].Value));
    }

    [Fact]
    public void daysEndTodayOrOnTheirLastDayAndWeeksShowTheirSpan()
    {
        RecentUsageChart Chart(string when, UsageRange r) => new(Snap([B(when, cost: 19)], r), Hero.Cost, new());
        var week = Chart("2026-11-26T16:00Z", new(Period.SevenDays));
        Assert.Equal(("11/21", "今天", "11/27"), (week.StartLabel, week.EndLabel, week.Tooltips[^1].Period));
        Assert.Equal(["11/23", "11/25"], week.Ticks.Select(t => t.Label));
        var custom = Chart("2026-11-10T16:00Z", new(new(2026, 11, 3), new(2026, 11, 18)));
        Assert.Equal(("11/3", "11/18"), (custom.StartLabel, custom.EndLabel));
        Assert.Equal(["11/8", "11/13"], custom.Ticks.Select(t => t.Label));
        var quarter = Chart("2026-11-20T16:00Z", new(Period.NinetyDays));
        Assert.Equal(("今天", "11/21–11/27"), (quarter.EndLabel, quarter.Tooltips[^1].Period));
    }

    [Fact]
    public void tokenHoverBreaksTheSlotDownAndPeakLabelsStayShort()
    {
        var b = B("2026-11-27T06:00Z", 1510000) with { OutputTokens = 1510000, CachedInputTokens = 12080000 };
        var units = new UsagePanelState { Hero = Hero.Token };
        var chart = new RecentUsageChart(Snap([b]), Hero.Token, units);
        Assert.Equal("15M", chart.PeakLabel);
        Assert.Equal([new("总 Token", "15.1M"), new("输出", "1.51M"), new("输入", "1.51M"), new("缓存", "12.1M")], chart.Tooltips[0].Rows);
        Assert.Equal(["1.5M", "1B", "950"], new double[] { 1510000, 1e9, 950 }.Select(n => RecentUsageChart.AxisLabel(n, Hero.Token, units)));
        Assert.Equal(["$63", "$5.2", "$0.5"], new[] { 63.2, 5.17, 0.5 }.Select(n => RecentUsageChart.AxisLabel(n, Hero.Cost, units)));
        units.ToggleHeroUnit();
        Assert.Equal("1510万", RecentUsageChart.AxisLabel(15100000, Hero.Token, units));
        Assert.Equal("48m", RecentUsageChart.AxisLabel(2880, Hero.ActiveTime, units));
    }

    [Fact]
    public void activityCurveKnowsUnrecordedFromIdleAndIgnoresTheOtherGranularity()
    {
        var s = Snap([B("2026-11-27T01:00Z")]);
        var first = D("2026-11-27T02:30Z");
        ActivityReport Report(bool hourly) => new(new(4200, 5880, 1.4),
            [new(D("2026-11-27T03:00Z"), 1200, 1800), new(D("2026-11-27T05:00Z"), 3000, 4080)],
            first, s.Grid.Slots[0].Start, hourly, Zone);
        var r = Report(true);
        Assert.Equal(new long?[] { null, 0, 1200, 0, 3000, 0, 0 }, r.Series(s.Grid).Select(t => t?.ActiveSeconds));
        var chart = new RecentUsageChart(s, Hero.ActiveTime, new(), r);
        Assert.Equal("50m", chart.PeakLabel);
        Assert.Equal("—", chart.Tooltips[0].Value);
        Assert.Equal([new("活跃", "20m"), new("AI 工时", "30m"), new("平均同时", "1.5 个")], chart.Tooltips[2].Rows);
        Assert.Equal(chart.HoverPoints[1], chart.Curve[0]);
        Assert.False(chart.HoverPoints[0].IsKnown);
        Assert.Equal(6, chart.Curve.Count);
        Assert.Empty(new RecentUsageChart(s, Hero.ActiveTime, new(), Report(false)).Curve);
        Assert.Empty(new RecentUsageChart(s, Hero.ActiveTime, new()).Curve);
    }

    [Fact]
    public void theEmptyStateFollowsTheBigNumberAndAModelFilterKeepsActiveTime()
    {
        var s = new UsageDisplaySnapshot([B("2026-11-25T02:00Z")], new(Period.SevenDays), Now, Zone, new(models: ["Other"]));
        var report = new ActivityReport(new(5400, 7200, 1.3), [new(D("2026-11-23T16:00Z"),1800,2400),
            new(D("2026-11-24T16:00Z"),3600,4800)], D("2026-01-01T00:00Z"), s.ActivityGrid.Slots[0].Start, false, Zone);
        Assert.True(new RecentUsageChart(s, Hero.Cost, new(), report).IsEmpty);
        Assert.True(new RecentUsageChart(s, Hero.Token, new(), report).IsEmpty);
        var active = new RecentUsageChart(s, Hero.ActiveTime, new(), report);
        Assert.False(active.IsEmpty);
        Assert.Equal([0d, 0, 0, 1800, 3600, 0, 0], active.HoverPoints.Select(p => p.Value));
        Assert.False(new RecentUsageChart(s, Hero.ActiveTime, new()).IsEmpty);
        var idle = new ActivityReport(new(0, 0, null), [], Now.AddYears(-1), Now, false, Zone);
        Assert.True(new RecentUsageChart(s, Hero.ActiveTime, new(), idle).IsEmpty);
    }

    [Fact]
    public void aModelUsedOnlyInTheAfternoonDoesNotCutTheMorningsActiveTime()
    {
        UsageBucket[] buckets = [B("2026-11-27T01:00Z", model: "A"), B("2026-11-27T07:00Z", model: "B")];
        var s = new UsageDisplaySnapshot(buckets, UsageRange.Today, Now, Zone, new(["codex"], ["B"]));
        Assert.Equal(buckets[1].BucketStart, s.Grid.Slots[0].Start);
        Assert.Equal(buckets[0].BucketStart, s.ActivityGrid.Slots[0].Start);
        var other = new UsageDisplaySnapshot(buckets, UsageRange.Today, Now, Zone, new(["cursor"]));
        Assert.Equal(D("2026-11-26T16:00Z"), other.ActivityGrid.Slots[0].Start);
    }

    [Fact]
    public void aSingleCustomDayAsksActivityByTheHourOnlyInTheNewPanel()
    {
        var first = new DateOnly(2026, 11, 18);
        Assert.True(new UsageRange(first, first).Hourly);
        Assert.False(new UsageRange(first, first.AddDays(1)).Hourly);
    }

    [Fact]
    public void filterChipsNameTheFirstAndCountTheRest()
    {
        var state = new UsagePanelState();
        Assert.Empty(state.Chips([]).Chips);
        state.Filters = new(["Cursor", "Codex"], ["Sonnet 5.5", "Opus 5.5", "Haiku 5.5"], ["Host 10", "Host 2"]);
        Assert.Equal(["Host 2 +1", "Codex +1", "Haiku 5.5 +2"], state.Chips([]).Chips.Select(c => c.Title));
        Assert.Equal("11/3–11/18", new UsageRange(new(2026, 11, 3), new(2026, 11, 18)).ChipLabel);
        Assert.Equal("11/18", new UsageRange(new(2026, 11, 18), new(2026, 11, 18)).ChipLabel);
        Assert.Equal(["今天", "24H", "7D", "30D", "90D"], UsageRange.QuickPeriods.Select(r => r.Label));
    }

    [Fact]
    public void chipsSkipTheToolInsideASubscriptionAndLockItsRow()
    {
        var state = new UsagePanelState { Filters = new(["cursor", "codex"], ["Opus 5.5"]), SelectedSubscription = Provider.OpenCodeGo };
        var chips = state.Chips([Provider.Codex, Provider.OpenCodeGo]);
        Assert.Equal("OpenCode Go", chips.LockedTool);
        Assert.Equal(new(Dimension.Model, "Opus 5.5"), Assert.Single(chips.Chips));
        var names = new UsageNames { Sources = new Dictionary<string, string> { ["opencode"] = "OpenCode" } };
        Assert.Equal("OpenCode", state.Chips([Provider.Codex, Provider.OpenCodeGo], names).LockedTool);
        Assert.Null(state.Chips([Provider.OpenCodeGo]).LockedTool);
    }

    [Fact]
    public void openingThePanelClosesTheFilterMenu()
    {
        var s = new UsagePanelState { FilterMenuOpen = true };
        s.PanelDidOpen();
        Assert.False(s.FilterMenuOpen);
        s.PanelDidOpen();
        Assert.Equal(2, s.Presentation);
    }

    [Fact]
    public void subscriptionActivityRestoresManualToolsAndReadsOnlyChangedConditions()
    {
        var state = new UsagePanelState { Filters = new(["cursor"], ["A"], ["pc"]) };
        Provider[] enabled = [Provider.Codex, Provider.Cursor];
        var original = state.EffectiveFilters(enabled);
        state.Filters = new(["cursor"], ["B"], ["pc"]);
        Assert.True(original.SameActivityConditions(state.EffectiveFilters(enabled)));
        state.SelectedSubscription = Provider.Codex;
        Assert.False(original.SameActivityConditions(state.EffectiveFilters(enabled)));
        state.PanelDidOpen();
        Assert.True(original.SameActivityConditions(state.EffectiveFilters(enabled)));
        state.Filters = new(["cursor"], ["B"], ["laptop"]);
        Assert.False(original.SameActivityConditions(state.EffectiveFilters(enabled)));
    }
}
