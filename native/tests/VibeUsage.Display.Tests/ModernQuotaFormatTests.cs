using static VibeUsage.Display.Tests.QuotaPanelDisplayTests;

namespace VibeUsage.Display.Tests;

public class ModernQuotaFormatTests
{
    private static QuotaPanelMeter Meter(QuotaWindow w, string label = "7d", params MeterPart[] split) =>
        Card(new(Provider.Cursor) { Meters = [new("plan", label, w, split)] }).Rows[0];

    [Fact]
    public void aSliverLeftIsNeverWrittenAsZero()
    {
        Assert.Equal(["0", "<1", "<1", "1", "12", "100"], new[] { 0, 0.4, 0.99, 1, 12.4, 99.6 }.Select(QuotaFormat.Percent));
    }

    [Fact]
    public void onlyTheOverviewRowShortensAntigravity()
    {
        Assert.Equal("Antigravity", Provider.Antigravity.OverviewName());
        Assert.Equal("Antigravity / AGY", Provider.Antigravity.DisplayName());
        foreach (var p in Enum.GetValues<Provider>().Where(p => p != Provider.Antigravity))
            Assert.Equal(p.DisplayName(), p.OverviewName());
    }

    [Fact]
    public void forecastsReadAsTheDesignWritesThem()
    {
        Assert.Equal("够用到重置", new Forecast(null).Text);
        foreach (var (sec, text) in new[] { (1800d, "30 分钟"), (20, "1 分钟"), (10800, "3 小时"), (23.7 * 3600, "1 天"), (46 * 3600, "2 天") })
            Assert.Equal($"约 {text}后用完", new Forecast(sec).Text);
    }

    [Fact]
    public void durationsRoundToTheirLastUnit()
    {
        Assert.Equal(["3 天 17 小时", "3 天", "2 小时 5 分钟", "1 小时", "12 分钟", "1 分钟"],
            new double[] { 89 * 3600 - 60, 72 * 3600, 7520, 3600, 720, 5 }.Select(QuotaFormat.Duration));
    }

    [Fact]
    public void resetIsAClockTimeWithinADayAndDaysAfterThat()
    {
        Assert.Equal(Now.AddHours(2).ToString("HH:mm"), QuotaFormat.Reset(Meter(new(50, Now.AddHours(2))), TimeZoneInfo.Utc));
        Assert.Equal("4天后", QuotaFormat.Reset(Meter(new(50, Now.AddHours(89))), TimeZoneInfo.Utc));
        Assert.Null(QuotaFormat.Reset(Meter(new(50)), TimeZoneInfo.Utc));
    }

    [Fact]
    public void tooltipNamesTheWindowAndOnlyWhatIsKnown()
    {
        var tip = QuotaFormat.Tooltip(Meter(new(21, Now.AddHours(89), 168 * 3600)))!;
        Assert.Equal("7 天窗口", tip.Title);
        Assert.Equal([new("已使用", "21%"), new("已过去", "3 天 7 小时"), new("离重置", "3 天 17 小时")], tip.Lines);
        var month = QuotaFormat.Tooltip(Meter(new(78), "Monthly"))!;
        Assert.Equal("mo", month.Title);
        Assert.Equal(new("已使用", "78%"), Assert.Single(month.Lines));
    }

    [Fact]
    public void aReadingPastItsResetShowsNoTooltipNotEvenItsSplit()
    {
        MeterPart[] split = [new("Cursor 模型", 4), new("第三方模型", 88)];
        var m = Meter(new(48, Now.AddDays(9)), "Month", split);
        Assert.Contains(new("第三方模型", "88%"), QuotaFormat.Tooltip(m)!.Lines);
        var expired = Meter(new(48, Now), "Month", split);
        Assert.Null(QuotaFormat.Tooltip(expired));
        Assert.Equal("已重置，等下次读取", expired.Message);
    }

    [Theory]
    [InlineData(5, PaceTone.Red)]
    [InlineData(70, PaceTone.Neutral)]
    public void aLoneAlertingTeamPoolIsWrittenOnce(int left, PaceTone tone)
    {
        var c = Card(new(Provider.Cursor) { Meters = [new("team", "Team", Window(left, 720, 33))] });
        Assert.Equal(new("team", "Team", left, tone, false), Assert.Single(c.OverviewChips));
    }

    [Fact]
    public void overviewChipsPutTheAlertBeforeTheRingWindows()
    {
        var c = Card(new(Provider.ClaudeCode)
        {
            FiveHour = Window(36, 5, 16),
            SevenDay = Window(53, 168, 39),
            ModelScopedMeters = [new("f", "Fable", Window(9, 168, 61))]
        });
        Assert.Equal(["Fable", "5h", "7d"], c.OverviewChips.Select(r => r.Label));
        Assert.Equal([false, true, true], c.OverviewChips.Select(r => r.IsColumn));
        Assert.Equal([PaceTone.Red, PaceTone.Neutral, PaceTone.Neutral], c.OverviewChips.Select(r => r.Tone));
    }
}
