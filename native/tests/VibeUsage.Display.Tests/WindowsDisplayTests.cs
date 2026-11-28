using System.Globalization;
using static VibeUsage.Display.Tests.QuotaPanelDisplayTests;

namespace VibeUsage.Display.Tests;

public class WindowsDisplayTests
{
    [Theory]
    [InlineData(Failure.DesktopRecordStale, "Claude 桌面端记录已过期")]
    [InlineData(Failure.DesktopMultipleInstalls, "装了多份 Claude 桌面端，账号不明确")]
    [InlineData(Failure.DesktopMixedOrganizations, "Claude 桌面端记录涉及多个组织，账号不明确")]
    [InlineData(Failure.DesktopRecordUnreadable, "Claude 桌面端记录无法解析")]
    public void desktopRecordNamesOnlyTheReportedFailure(Failure failure, string text)
    {
        var c = Card(new(Provider.ClaudeCode, QuotaStatus.NoData) { Source = QuotaSource.DesktopRecord, LiveFailure = failure });
        Assert.Equal(text, c.DetailTitle);
        Assert.True(c.ShowsWhiteDot);
        Assert.Empty(c.Rows);
        Assert.Null(c.RecordTime);
    }

    [Fact]
    public void desktopRecordShowsTheProductionTimeInTheViewersZone()
    {
        var s = new QuotaSnapshot(Provider.ClaudeCode)
        {
            FiveHour = new(50),
            DataAsOf = Now.AddHours(-1),
            FetchedAt = Now,
            Source = QuotaSource.DesktopRecord,
            SourceLabel = "Claude 桌面历史额度 · 非实时 · 非本机独占用量"
        };
        var c = Card(s);
        var zone = TimeZoneInfo.FindSystemTimeZoneById("Asia/Singapore");
        var culture = CultureInfo.GetCultureInfo("zh-CN");
        Assert.Equal("记录时间 " + TimeZoneInfo.ConvertTime(Now.AddHours(-1), zone).ToString("G", culture), c.RecordTimeText(zone, culture));
        Assert.Equal(s.SourceLabel, c.SourceNote);
        Assert.Equal("Claude 桌面额度记录 · 非本机独占用量", Card(s with { SourceLabel = null }).SourceNote);
        Assert.Null(Card(s with { Source = QuotaSource.Cache }).RecordTimeText(zone, culture));
        Assert.Null(Card(s with { DataAsOf = null }).RecordTime);
    }

    [Fact]
    public void desktopModelAllowancesNeverTakeTheMiddle()
    {
        var s = new QuotaSnapshot(Provider.ClaudeCode)
        {
            Source = QuotaSource.DesktopRecord,
            Meters = [new("desktop-so", "Opus", new(98)), new("desktop-sn", "Sonnet", new(35))]
        };
        var c = Card(s);
        Assert.Equal(2, c.ButtonRemaining);
        Assert.Null(c.RingCenter);
        Assert.Equal("Opus", c.AdditionalAlert!.Label);
    }

    [Theory]
    [InlineData(ZCodeRegion.BigModel, "请在设置中配置 BigModel API Key")]
    [InlineData(ZCodeRegion.Zai, "请在设置中配置 Z.ai API Key")]
    public void missingZCodeKeyNamesItsRegion(ZCodeRegion region, string expected)
    {
        var s = new QuotaSnapshot(Provider.ZCode, QuotaStatus.Unauthorized) { LiveFailure = Failure.CredentialsNotFound };
        var c = new QuotaPanelDisplay([s], [Provider.ZCode], null, Now, region: region).Detail!;
        Assert.Equal(("需 Key", expected), (c.Comment, c.DetailTitle));
        Assert.Equal("API Key 无效，请在设置中更换", Card(s with { LiveFailure = Failure.CredentialsRejected }).DetailTitle);
    }

    [Fact]
    public void credentialStoreFailureIsNotAnExpiredKey()
    {
        var c = Card(new(Provider.ZCode, QuotaStatus.RetryableError) { LiveFailure = Failure.CredentialStoreUnavailable });
        Assert.Equal(("读取失败", "无法读取 Windows 凭据管理器", QuotaAction.Retry), (c.Comment, c.DetailReason, c.Action));
    }

    [Fact]
    public void cursorSignInTakesPrecedenceOverEncryptedCredentialFailure()
    {
        var c = Card(new(Provider.Cursor, QuotaStatus.NoData) { SignIn = CursorSignIn.Required, LiveFailure = Failure.CredentialsEncrypted });
        Assert.Equal(("需要登录 Cursor", QuotaAction.CursorSettings), (c.DetailTitle, c.Action));
    }

    [Fact]
    public void onlyExhaustionColorsTheRoundButton()
    {
        var s = new QuotaSnapshot(Provider.Codex) { FiveHour = new(99) };
        Assert.Equal((PaceTone.Red, ButtonRing.NeutralArc), (Card(s).Tone, Card(s).ButtonRing));
        Assert.Equal(ButtonRing.NeutralArc, Card(s with { FiveHour = Window(45, 168, 60) }).ButtonRing);
        Assert.Equal(ButtonRing.RedStartPoint, Card(s with { FiveHour = new(100) }).ButtonRing);
        Assert.Equal(ButtonRing.Empty, Card(s with { FiveHour = new(100, Now) }).ButtonRing);
        var half = Card(s with { FiveHour = new(50) }).Outer!;
        Assert.Equal(31, half.LitTicks(QuotaFormat.OverviewTicks));
        Assert.Equal(48, half.LitTicks(QuotaFormat.RingTicks));
    }
}
