using static VibeUsage.Display.Tests.UsageDisplayRulesTests;

namespace VibeUsage.Display.Tests;

public class ModernDetailsTests
{
    private static UsageRanking Ranking(params double[] values) =>
        UsageRanking.Ordered(values.Select((v, i) => (((char)('A' + i)).ToString(), v)));

    [Fact]
    public void ringRunsMatchTheDrawingOf135a()
    {
        var owners = Ranking(30.9, 16.2, 11.4, 7).RingTicks;
        Assert.Equal(72, owners.Count);
        Assert.Equal([33, 17, 11, 7], Enumerable.Range(0, 4).Select(i => owners.Count(x => x == i)));
        Assert.Equal([33, 51, 63, 71], Enumerable.Range(0, 72).Where(i => owners[i] == null));
        var composition = Ranking(62.2, 2.6, 0.7).RingTicks;
        Assert.Equal([67, 2, 1], Enumerable.Range(0, 3).Select(i => composition.Count(x => x == i)));
        Assert.All(Ranking(104).RingTicks, x => Assert.Equal(0, x));
    }

    [Fact]
    public void everyShareKeepsATickAndTheRingStaysWhole()
    {
        var owners = Ranking(1000, 1, 1, 0).RingTicks;
        Assert.Equal(72, owners.Count);
        Assert.Single(owners, x => x == 1);
        Assert.Single(owners, x => x == 2);
        Assert.DoesNotContain(3, owners);
        Assert.All(Ranking(0, 0, 0).RingTicks, Assert.Null);
        Assert.All(Ranking().RingTicks, Assert.Null);
    }

    [Fact]
    public void aLoneFourthRowIsListedAndTwoOrMoreCollapse()
    {
        var four = Ranking(4, 3, 2, 1);
        Assert.Equal(["A", "B", "C", "D"], four.Listed.Select(r => r.Name));
        Assert.Equal(RankColor.Gray, four.Listed[^1].Color);
        Assert.Empty(four.Collapsed);
        var five = Ranking(5, 4, 3, 2, 1);
        Assert.Equal(["A", "B", "C"], five.Listed.Select(r => r.Name));
        Assert.Equal(["D", "E"], five.Collapsed.Select(r => r.Name));
        Assert.Equal("其余 2 个", five.RemainderLabel);
        Assert.Equal(3, five.RemainderValue);
        Assert.Empty(Ranking(3, 2, 1).Collapsed);
    }

    [Fact]
    public void tokenCompositionKeepsCacheInputOutputWhicheverIsLargest()
    {
        var s = Snap([B("2026-11-27T02:00Z", 300) with { CachedInputTokens = 10, OutputTokens = 50 }]);
        var c = s.TokenComposition;
        Assert.Equal(["缓存", "输入", "输出"], c.Listed.Select(r => r.Name));
        Assert.Equal([RankColor.Blue, RankColor.Teal, RankColor.Magenta], c.Listed.Select(r => r.Color));
        Assert.Equal(360, c.Total);
    }

    [Fact]
    public void sharesReadAsWholePercentsAndATinyShareStaysVisible()
    {
        Assert.Equal(["47%", "100%", "<1%", "0%"], new[] { 30.9 / 65.5, 1, 0.004, 0 }.Select(DetailsFormat.Share));
    }

    [Fact]
    public void tokenFiguresTakeTheUnitOfTheCardsTotal()
    {
        (double value, double total, string text)[] cases = [(65500000,65500000,"65.5M"),(700000,65500000,"0.7M"),
            (104000000,104000000,"104M"),(98800000,104000000,"98.8M"),(1000000,104000000,"1.0M"),
            (20000,65500000,"<0.1M"),(1.5e9,2.1e9,"1.5B"),(999600,999600,"1.0M"),(48000,120000,"48.0K"),(850,850,"850"),(0,0,"0")];
        foreach (var (value, total, text) in cases) Assert.Equal(text, DetailsFormat.Value(value, Metric.Tokens, total));
        Assert.Equal("$41.20", DetailsFormat.Value(41.2, Metric.Cost, 41.2));
    }

    [Fact]
    public void theRingsCenterDropsCentsFromAHundredLikeTheBigNumber()
    {
        Assert.Equal(["$41.20", "$99.99", "$0.0042", "$100", "$269", "$999", "$1000", "$1000", "$71711", "$123457", "$1234568", "$0"],
            new[] { 41.2, 99.99, 0.0042, 100, 268.6, 999.49, 999.99, 1000, 71711.27, 123456.78, 1234567.89, 0 }.Select(c => DetailsFormat.Center(c, Metric.Cost)));
        Assert.Equal("$71711.27", DetailsFormat.Value(71711.27, Metric.Cost, 71711.27));
        Assert.Equal("65.5M", DetailsFormat.Center(65500000, Metric.Tokens));
    }

    [Fact]
    public void cardMetricsAreSeparateAndOutliveAnOpen()
    {
        var state = new UsagePanelState();
        Assert.Equal(Metric.Tokens, state[Dimension.Model]);
        state[Dimension.Model] = Metric.Cost;
        state.PanelDidOpen();
        Assert.Equal((Metric.Cost, Metric.Tokens, Metric.Tokens), (state[Dimension.Model], state[Dimension.Terminal], state[Dimension.Tool]));
    }

    [Fact]
    public void cardsCountWhatTheSelectedSubscriptionCounts()
    {
        var state = new UsagePanelState();
        Provider[] enabled = [Provider.Codex, Provider.Cursor];
        UsageBucket[] data = [B("2026-11-27T02:00Z", 100), B("2026-11-27T02:00Z", 200, source: "cursor")];
        string[] Tools() => new UsageDisplaySnapshot(data, state.Range, Now, Zone, state.EffectiveFilters(enabled))
            .Ranking(Dimension.Tool, Metric.Tokens).Top.Select(r => r.Name).ToArray();
        Assert.Equal(["cursor", "codex"], Tools());
        state.SelectedSubscription = Provider.Codex;
        state.Filters = new(["cursor"]);
        Assert.Equal(["codex"], Tools());
        state.SelectedSubscription = null;
        Assert.Equal(["cursor"], Tools());
    }

    [Fact]
    public void figuresRuleCoversEveryReadState()
    {
        var keeper = new UsageFiguresKeeper();
        var key = UsageReadKey.For(1, UsageRange.Today, Now, Zone);
        var other = UsageReadKey.For(1, new(Period.SevenDays), Now, Zone);
        var empty = new LoadedUsage(key, Snap([]));
        Assert.Equal(FiguresState.Loading, keeper.Resolve(key, null, false, false, Now, Zone).State);
        Assert.Equal(FiguresState.Loading, keeper.Resolve(key, null, true, true, Now, Zone).State);
        Assert.Equal(FiguresState.Current, keeper.Resolve(key, empty, true, true, Now, Zone).State);
        Assert.Equal(FiguresState.Faded, keeper.Resolve(other, null, true, true, Now, Zone).State);
        Assert.Equal(FiguresState.Unavailable, keeper.Resolve(other, null, false, true, Now, Zone).State);
        Assert.Equal(FiguresState.Loading, keeper.Resolve(other, null, true, true, Now, Zone).State);
    }

    [Fact]
    public void onlyASuccessfulReadOfThePeriodDrawsFiguresOrZero()
    {
        var keeper = new UsageFiguresKeeper();
        var key = UsageReadKey.For(1, UsageRange.Today, Now, Zone);
        var current = new LoadedUsage(key, Snap([B("2026-11-27T02:00Z", 30)]));
        Assert.Null(keeper.Resolve(key, null, false, true, Now, Zone).Snapshot);
        Assert.Equal(30, keeper.Resolve(key, current, false, true, Now, Zone).Snapshot!.Total.TotalTokens);
        var empty = current with { Snapshot = Snap([]) };
        Assert.Equal(0, keeper.Resolve(key, empty, false, true, Now, Zone).Snapshot!.Total.TotalTokens);
    }

    [Fact]
    public void relinkingDropsThePreviousAccountsFiguresBeforeTheNewOnesArrive()
    {
        var keeper = new UsageFiguresKeeper();
        var key = UsageReadKey.For(1, UsageRange.Today, Now, Zone);
        var old = new LoadedUsage(key, Snap([B("2026-11-27T02:00Z", 1, 16)]));
        Assert.Equal(16, keeper.Resolve(key, old, false, true, Now, Zone).Snapshot!.Total.Cost);
        var relinked = key with { AccountGeneration = 2 };
        Assert.Null(keeper.Resolve(relinked, old, true, false, Now, Zone).Snapshot);
        Assert.Null(keeper.Resolve(relinked, old, false, true, Now, Zone).Snapshot);
    }
}
