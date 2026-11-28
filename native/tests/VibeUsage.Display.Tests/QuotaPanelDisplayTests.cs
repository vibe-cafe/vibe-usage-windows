using static VibeUsage.Display.Provider;
using static VibeUsage.Display.PaceTone;

namespace VibeUsage.Display.Tests;

public class QuotaPanelDisplayTests
{
    internal static readonly DateTimeOffset Now = DateTimeOffset.FromUnixTimeSeconds(1_800_000_000);
    internal static QuotaWindow Window(double remaining, double hours, double due) =>
        new(100 - remaining, Now.AddMilliseconds(Math.Round(hours * 3600 * due * 10)), hours * 3600);
    private static QuotaSnapshot S(Provider p, QuotaWindow? shortWindow = null, QuotaWindow? week = null) =>
        new(p) { FiveHour = shortWindow, SevenDay = week };
    private static QuotaMeter Fable(double remaining = 3, double due = 50) => new("fable", "Fable", Window(remaining, 168, due));
    internal static QuotaSubscriptionDisplay Card(QuotaSnapshot s) => new QuotaPanelDisplay([s], [s.Provider], null, Now).Detail!;
    private static string[] Labels(IEnumerable<QuotaPanelMeter> rows) => rows.Select(r => r.Label).ToArray();
    private static QuotaSnapshot[] Fixtures => [
        S(Codex, Window(12, 5, 29), Window(79, 168, 53)),
        S(ClaudeCode, Window(36, 5, 16), Window(31, 168, 39)),
        S(Cursor) with { Meters = [new("plan", "Month", Window(41, 720, 42))] },
        S(Antigravity, Window(9, 5, 22), Window(70, 168, 62)),
        S(KimiCode, Window(71, 5, 34), Window(40, 168, 52)),
        S(Grok, null, Window(88, 168, 71)), S(OpenCodeGo, Window(64, 5, 16), Window(48, 168, 42)),
        S(ZCode, null, Window(72, 168, 67))];

    [Fact]
    public void fiveSubscriptionsAsIn132c()
    {
        Provider[] order = [Cursor, Codex, ClaudeCode, KimiCode, Grok];
        var panel = new QuotaPanelDisplay([S(KimiCode, Window(71, 5, 34), Window(60, 168, 52)), .. Fixtures], order, null, Now);
        var o = panel.Overview;
        Assert.Equal(order, panel.Buttons.Select(b => b.Provider));
        Assert.Equal([Codex, ClaudeCode, Cursor], o.Rows.Select(r => r.Provider));
        Assert.Equal(["超额 17%", "超额 8%", "进度正常"], o.Rows.Select(r => r.Comment));
        Assert.Equal((5, 1, 1, Red, 42.5), (o.Total, o.NearlyEmpty, o.Fast, o.Tone, o.RowPitch));
        Assert.Equal([KimiCode, Grok], o.Remainder!.Providers);
        Assert.Equal("都正常", o.Remainder.Summary);
    }

    [Fact]
    public void eightSubscriptionsAsIn132d()
    {
        Provider[] order = [Cursor, KimiCode, Codex, Antigravity, Grok, ClaudeCode, OpenCodeGo, ZCode];
        var panel = new QuotaPanelDisplay(Fixtures, order, null, Now);
        var o = panel.Overview;
        Assert.Equal(order, panel.Buttons.Select(b => b.Provider));
        Assert.Equal([Codex, Antigravity, KimiCode], o.Rows.Select(r => r.Provider));
        Assert.Equal(["超额 17%", "超额 13%", "超额 12%"], o.Rows.Select(r => r.Comment));
        Assert.Equal((8, 2, 2, 4), (o.Total, o.NearlyEmpty, o.Fast, o.AttentionCount));
        Assert.Equal([ClaudeCode, Cursor, Grok, OpenCodeGo, ZCode], o.Remainder!.Providers);
        Assert.Equal(("1 个超额", Amber), (o.Remainder.Summary, o.Remainder.Tone));
    }

    [Fact]
    public void paceBoundariesAndForecastsUseUnroundedPercentages()
    {
        (double left, double hours, double due, PaceTone tone, string? comment, string? prediction)[] examples = [
            (45,20,50,Neutral,"进度正常","eta"), (55,20,50,Neutral,"进度正常","reset"),
            (44.9,20,50,Amber,"超额 5%","eta"), (56,20,50,Neutral,"余量 6%","reset"),
            (9,20,86,Red,"超额 77%",null), (10,20,86,Neutral,null,null),
            (9,20,85,Red,"超额 76%","eta"), (10,100,85,Amber,"超额 75%","eta"),
            (10,100,50,Amber,"超额 40%","eta"), (20,6,100.0/3,Red,"超额 13%","eta"),
            (49,2,50,Neutral,"进度正常","eta"), (19,5,20,Neutral,"进度正常","eta"),
            (9,100,10,Red,"超额 1%","eta"), (9,168,9.2,Red,"超额 1%","eta"),
            (7,8,25.0/480*100,Neutral,"进度正常","reset"), (7,5,7,Neutral,"进度正常","reset"),
            (6.8,5,6.8,Neutral,"进度正常","reset"), (15.8,100,20.8,Neutral,"进度正常","eta"),
            (0,20,99,Red,"已用完",null), (100,20,50,Neutral,"余量 50%","reset")];
        foreach (var (left, hours, due, tone, comment, prediction) in examples)
        {
            var card = Card(S(Codex, Window(left, hours, due)));
            var row = Assert.Single(card.Rows);
            Assert.Equal((tone, comment), (card.Tone, card.Comment));
            Assert.Equal(tone, row.Tone);
            Assert.Equal(comment == null ? Neutral : tone, card.CommentTone);
            if (prediction == "eta")
                Assert.Equal(left * hours * 3600 * (1 - due / 100) / (100 - left), row.Prediction!.RunsOutInSeconds!.Value, 3);
            else Assert.Equal(prediction == null ? null : new Forecast(null), row.Prediction);
        }
    }

    [Fact]
    public void bothForecastAlertsRequireAtLeastFifteenMinutesBeforeReset()
    {
        foreach (var (used, duration) in new[] { (80.0, 18000), (92.0, 180000) })
            foreach (int offset in new[] { -1, 0, 1 })
            {
                var left = duration - (duration - 900) * used / 100 + offset;
                var row = Card(S(Codex, new(used, Now.AddSeconds(left), duration))).Outer!;
                Assert.Equal(offset < 0 ? Neutral : Red, row.Tone);
                Assert.True(row.Prediction!.RunsOutInSeconds < left);
            }
    }

    [Fact]
    public void unavailableTimingAndResetNeverInventAPrediction()
    {
        var c = Card(S(KimiCode) with
        {
            Meters = [new("daily","Daily", new(99, Now.AddHours(1))),
            new("weekly","Weekly",new(90, WindowSeconds: 604800)), new("old","Model A",new(100,Now))]
        });
        Assert.Equal(("1d", 1d, Red), (c.Tightest!.Label, c.ButtonRemaining, c.Tone));
        Assert.Null(c.Comment);
        Assert.All(c.Rows, r => { Assert.Null(r.Prediction); Assert.Null(r.ExpectedRemaining); });
        var daily = Assert.Single(c.Rows, r => r.Id == "daily");
        Assert.Null(daily.WindowSeconds);
        Assert.Equal(3600, daily.SecondsLeft);
        Assert.Null(Assert.Single(c.Rows, r => r.Id == "weekly").Elapsed);
        Assert.Equal("已重置，等下次读取", c.Rows[2].Message);
        Assert.Equal("已用完", Card(S(Codex, new(100))).ButtonStatus);
    }

    [Fact]
    public void exhaustedModelOverridesThePaceCommentAndButtonEvenWithoutDuration()
    {
        foreach (double? duration in new double?[] { 604800, null })
        {
            var c = Card(S(ClaudeCode, Window(12, 5, 29)) with { ModelScopedMeters = [new("f", "Fable", new(100, Now.AddHours(1), duration))] });
            Assert.Equal(("已用完", "已用完", Red, ButtonRing.RedStartPoint), (c.Comment, c.ButtonStatus, c.CommentTone, c.ButtonRing));
        }
    }

    [Fact]
    public void hiddenFableGetsTheAlertWhenWeeklyIsEquallyUrgentButFurtherBehind()
    {
        var c = Card(S(ClaudeCode, Window(50, 5, 50), Window(8, 168, 50)) with { SevenDayOpus = Window(6, 168, 35), ModelScopedMeters = [Fable(3, 10)] });
        Assert.Equal(("7d", Red, "Fable", 3d, "超额 42%"), (c.Outer!.Label, c.Outer.Tone, c.AdditionalAlert!.Label, c.ButtonRemaining, c.Comment));
    }

    [Fact]
    public void claudeWeeklyWithFableHasOneRingAndAlertsOnFable()
    {
        var c = Card(S(ClaudeCode, null, Window(60, 168, 50)) with { ModelScopedMeters = [Fable()] });
        Assert.Equal(["7d", "Fable"], Labels(c.Rows));
        Assert.Null(c.Inner);
        Assert.Equal(("Fable", Red, 3d, 60d), (c.AdditionalAlert!.Label, c.Tone, c.ButtonRemaining, c.RingCenter!.Remaining));
    }

    [Fact]
    public void ringCenterIsTheTighterRingWindowAndNeverAModelAllowanceOrExtra()
    {
        var s = Fixtures[1] with { SevenDayOpus = Window(4, 168, 14), ModelScopedMeters = [Fable(9, 61)], ExtraUsage = new(true, 99, 100) };
        var c = Card(s);
        Assert.Equal(("Opus", 4d, "7d", 31d), (c.Tightest!.Label, c.ButtonRemaining, c.RingCenter!.Label, c.RingCenter.Remaining));
        Assert.Equal("5h", Card(s with { FiveHour = Window(12, 5, 29) }).RingCenter!.Label);
        var pool = Card(S(Cursor) with { Meters = [new("team", "Team", Window(5, 720, 33))] });
        Assert.Null(pool.Outer);
        Assert.Equal("Team", pool.RingCenter!.Label);
    }

    [Fact]
    public void expiredRingWindowsLeaveTheMiddleEmptyRatherThanShowFable()
    {
        var expired = new QuotaWindow(40, Now.AddMinutes(-1), 18000);
        var c = Card(S(ClaudeCode, expired, expired) with { ModelScopedMeters = [Fable()] });
        Assert.Null(c.Outer);
        Assert.Null(c.RingCenter);
        Assert.Equal("Fable", c.Tightest!.Label);
        Assert.Null(Card(S(ClaudeCode) with { SevenDayOpus = Window(30, 168, 50), ExtraUsage = new(true, 90, 100) }).RingCenter);
        Assert.Equal("Code review", Card(S(Codex) with { Meters = [new("r", "Code review", new(10))] }).RingCenter!.Label);
    }

    [Fact]
    public void claudeProWithoutModelAllowancesShowsOnlyItsTwoWindows()
    {
        var c = Card(Fixtures[1] with { PlanLabel = "Pro" });
        Assert.Equal("Claude Pro · 偏快", c.ButtonHelp);
        Assert.Equal(["5h", "7d"], Labels(c.Rows));
        Assert.Null(c.AdditionalAlert);
        Assert.Equal(("7d", "5h"), (c.Outer!.Label, c.Inner!.Label));
    }

    [Fact]
    public void claudeWeeklyAloneHasOneRingAndOneRow()
    {
        var c = Card(S(ClaudeCode, null, Window(60, 168, 50)));
        Assert.Equal("7d", Assert.Single(c.Rows).Label);
        Assert.Same(c.Rows[0], c.Outer);
        Assert.Null(c.Inner);
        Assert.Null(c.AdditionalAlert);
    }

    [Fact]
    public void codexFeatureQuotaNeverFillsAMissingRing()
    {
        var s = S(Codex, null, Window(8, 168, 50)) with { Meters = [new("review", "Code review", Window(5, 168, 50))] };
        var c = Card(s);
        Assert.Equal(["7d", "Code review"], Labels(c.Rows));
        Assert.Null(c.Inner);
        Assert.Equal("review", c.AdditionalAlert!.Id);
        var feature = Card(s with { SevenDay = null });
        Assert.Null(feature.Outer);
        Assert.Equal(5, feature.ButtonRemaining);
    }

    [Fact]
    public void multipleModelAllowancesUseTheirOwnTimingAndStaySeparateFromTheMainRings()
    {
        var c = Card(S(ClaudeCode, Window(36, 5, 16), Window(53, 168, 39)) with
        {
            SevenDayOpus = Window(4, 168, 14),
            SevenDaySonnet = Window(33, 168, 31),
            ExtraUsage = new(true, 100, 100),
            PlanLabel = "Max",
            ResetCreditsCount = 2,
            ModelScopedMeters = [new("a", "Model A", Window(9, 168, 61)), new("b", "Model B", new(98))]
        });
        Assert.Equal(["5h", "7d", "Model A", "Model B", "Opus", "Sonnet", "额外"], Labels(c.Rows));
        Assert.Equal(["5h", "7d", "Model B"], Labels(c.CompactRows));
        Assert.Equal(("另有 4 项", "超额 52%", "Model A", 2d, "Max", "重置券 ×2"),
            (c.FoldLabel, c.Comment, c.AdditionalAlert!.Label, c.ButtonRemaining, c.PlanLabel, c.CreditsText));
        Assert.Equal(Neutral, c.Rows[^1].Tone);
    }

    [Fact]
    public void calendarMonthCanBeTightestAndPayAsYouGoNeverOwnsARing()
    {
        var month = new QuotaMeter("m", "Monthly", new(78));
        var c = Card(S(OpenCodeGo, Window(64, 5, 16), Window(48, 168, 39)) with { Meters = [month] });
        Assert.Equal(["5h", "7d", "mo"], Labels(c.Rows));
        Assert.Equal("mo", c.Tightest!.Label);
        Assert.Equal(("7d", "5h"), (c.Outer!.Label, c.Inner!.Label));
        var cursor = Card(S(Cursor) with { Meters = [new("on-demand", "On-demand", new(100)), month] });
        Assert.Equal(("mo", 22d, Neutral), (cursor.Outer!.Label, cursor.ButtonRemaining, cursor.Tone));
        Assert.Null(cursor.Inner);
        Assert.Null(cursor.Outer.Prediction);
    }

    [Fact]
    public void countSelectionAndRefreshScopeFollowOnlyEnabledSubscriptions()
    {
        QuotaPanelDisplay Panel(Provider[] enabled, Provider? selected) => new(Fixtures, enabled, selected, Now);
        var empty = Panel([], Codex);
        Assert.True(empty.IsEmpty);
        Assert.Equal(("还没有要看的订阅", "去设置"), (empty.EmptyTitle, empty.EmptyActionLabel));
        Assert.Empty(empty.Buttons);
        Assert.Empty(empty.RefreshTargets);
        var single = Panel([Grok], null);
        Assert.Empty(single.Buttons);
        Assert.Null(single.UsageProvider);
        Assert.Equal(Grok, single.Detail!.Provider);
        Provider[] order = [Grok, ClaudeCode, Cursor, Codex];
        var overview = Panel(order, ZCode);
        Assert.Equal(order, overview.RefreshTargets);
        Assert.Equal([Codex, ClaudeCode, Grok, Cursor], overview.Overview.Rows.Select(r => r.Provider));
        Assert.Null(overview.Overview.Remainder);
        Assert.Equal(38.5, overview.Overview.RowPitch);
        var selected = Panel(order, Cursor);
        Assert.Equal(Cursor, selected.UsageProvider);
        Assert.Equal([Cursor], selected.RefreshTargets);
    }

    [Fact]
    public void exhaustedWithoutAWindowIsOneRedSubscriptionWithNoInventedMeter()
    {
        var s = new QuotaSnapshot(Codex, QuotaStatus.NoData) { EmptyReason = EmptyReason.LimitReached };
        var p = new QuotaPanelDisplay([s], [Codex, Cursor], Codex, Now);
        Assert.Equal((1, Red), (p.Overview.AttentionCount, p.Overview.Tone));
        Assert.Equal(("已用完", 0d, false), (p.Detail!.DetailTitle, p.Detail.ButtonRemaining, p.Detail.ShowsWhiteDot));
        Assert.Empty(p.Detail.Rows);
        Assert.Null(p.Detail.RingCenter);
    }

    [Fact]
    public void coldReadsShowSkeletonsAndWarmRefreshesKeepTheirNumbers()
    {
        var p = new QuotaPanelDisplay([Fixtures[0]], [Cursor, Codex], Cursor, Now, new HashSet<Provider> { Cursor, Codex });
        Assert.True(p.IsRefreshing);
        Assert.True(p.Detail!.ShowsSkeleton);
        Assert.Equal("正在读取…", p.Detail.DetailTitle);
        Assert.Null(p.Detail.ButtonRemaining);
        Assert.False(p.Detail.ShowsWhiteDot);
        Assert.Equal((ReadState.Normal, 12d), (p.Buttons[1].ReadState, p.Buttons[1].ButtonRemaining));
    }

    [Fact]
    public void emptyStatesSupplyReasonDotAndOnlyTheApplicableAction()
    {
        (Provider p, QuotaStatus status, Failure? failure, EmptyReason? empty, CursorSignIn? signIn, ReadState state, string comment, string title, bool dot, QuotaAction? action)[] cases = [
            (Cursor,QuotaStatus.NoData,null,null,CursorSignIn.Required,ReadState.NeedsLogin,"需登录","需要登录 Cursor",true,QuotaAction.CursorSettings),
            (Cursor,QuotaStatus.NoData,null,null,CursorSignIn.Expired,ReadState.LoginExpired,"登录已失效","Cursor 登录已失效",true,QuotaAction.CursorSettings),
            (ClaudeCode,QuotaStatus.Unauthorized,Failure.CredentialsRejected,null,null,ReadState.LoginExpired,"登录已失效","登录已失效，请重新登录",true,QuotaAction.Retry),
            (KimiCode,QuotaStatus.Unauthorized,Failure.CredentialsNotFound,null,null,ReadState.NeedsLogin,"需登录","请先登录 Kimi Code",true,QuotaAction.Retry),
            (ZCode,QuotaStatus.Unauthorized,Failure.CredentialsRejected,null,null,ReadState.LoginExpired,"Key 无效","API Key 无效，请在设置中更换",true,QuotaAction.Retry),
            (Antigravity,QuotaStatus.Unauthorized,Failure.CredentialsRejected,null,null,ReadState.NeedsLogin,"需登录","请在 Antigravity 中登录",true,QuotaAction.Retry),
            (OpenCodeGo,QuotaStatus.NoData,Failure.CredentialsExpired,null,null,ReadState.LoginExpired,"登录已失效","打开 OpenCode 后自动显示",true,null),
            (Grok,QuotaStatus.RetryableError,Failure.EndpointTimedOut,null,null,ReadState.ReadFailed,"读取失败","暂时读不到额度",true,QuotaAction.Retry),
            (Codex,QuotaStatus.Error,null,null,null,ReadState.ReadFailed,"读取失败","暂时读不到额度",true,QuotaAction.Retry),
            (ClaudeCode,QuotaStatus.NoData,null,EmptyReason.NonessentialTrafficDisabled,null,ReadState.OrganizationPolicy,"组织策略","组织策略关闭了 Claude Code 非必要流量 · 无法读取",true,null),
            (ClaudeCode,QuotaStatus.NoData,null,EmptyReason.SessionWithoutPlanLimits,null,ReadState.NonSubscriptionLogin,"未使用订阅登录","Claude Code 未使用订阅登录",false,null),
            (Codex,QuotaStatus.NoData,null,EmptyReason.NoWindow,null,ReadState.Normal,"无窗口","没有生效的额度窗口",false,null)];
        foreach (var e in cases)
        {
            var c = Card(new(e.p, e.status) { EmptyReason = e.empty, LiveFailure = e.failure, SignIn = e.signIn, FetchedAt = Now });
            Assert.Equal((e.comment, e.state, e.title, e.dot, e.action), (c.Comment, c.ReadState, c.DetailTitle, c.ShowsWhiteDot, c.Action));
            Assert.Equal(e.comment, c.ButtonStatus);
            Assert.Equal(e.failure == Failure.EndpointTimedOut ? "读取超时" : null, c.DetailReason);
            Assert.Null(c.ButtonRemaining);
            Assert.Equal(Neutral, c.CommentTone);
        }
    }

    [Theory]
    [InlineData(Failure.EgressRegionUnsupported, "网络出口在中国大陆，已暂停读取")]
    [InlineData(Failure.EgressUnconfirmed, "无法确认网络出口，已暂停读取")]
    [InlineData(Failure.ProxyNotDetected, "未检测到代理，已暂停读取")]
    [InlineData(Failure.ProxyUnsupported, "代理无法照着走，已暂停读取")]
    public void everyNetworkPauseKeepsAvailableMetersAndExplainsItsRoute(Failure failure, string reason)
    {
        var s = new QuotaSnapshot(Codex, QuotaStatus.NoData) { LiveFailure = failure, EgressRegion = "CN" };
        var empty = Card(s);
        Assert.Equal((ReadState.Paused, reason, "已暂停读取", true), (empty.ReadState, empty.DetailTitle, empty.Comment, empty.ShowsWhiteDot));
        Assert.Null(empty.Action);
        Assert.Null(empty.ButtonRemaining);
        foreach (var left in new[] { 36, 0 })
        {
            var kept = Card(s with { Status = QuotaStatus.Ok, SevenDay = Window(left, 168, 48), Source = QuotaSource.Cache });
            Assert.Equal(left == 0 ? Red : Amber, kept.Tone);
            Assert.Equal((left, "读取已暂停", "来源：缓存 · " + reason, Neutral), ((int)kept.ButtonRemaining!, kept.ButtonStatus, kept.SourceNote, kept.CommentTone));
            Assert.Null(kept.DetailTitle);
        }
    }

    [Theory]
    [InlineData(QuotaSource.Live, "上次实时读取")]
    [InlineData(QuotaSource.LocalLog, "本地日志")]
    [InlineData(QuotaSource.Cache, "缓存")]
    [InlineData(QuotaSource.DesktopRecord, "Claude 桌面端记录")]
    public void nonLiveReadNamesOnlyItsKnownSourceAndReason(QuotaSource source, string name)
    {
        var s = Fixtures[0] with { Source = source };
        Assert.Null(Card(s).SourceNote);
        var kept = Card(s with { LiveFailure = Failure.EndpointTimedOut });
        Assert.Equal("来源：" + name + " · 读取超时", kept.SourceNote);
        Assert.False(kept.ShowsWhiteDot);
        Assert.Equal(12, kept.ButtonRemaining);
    }

    [Fact]
    public void policyKeepsTheDesktopReadingAndExplainsOnlyTheLiveRead()
    {
        var s = S(ClaudeCode, new(28)) with { Source = QuotaSource.DesktopRecord, LiveFailure = Failure.ClaudeBlockedByPolicy };
        var kept = Card(s);
        Assert.Equal((72d, "组织策略", false, "来源：Claude 桌面端记录 · 组织策略关闭了 Claude Code 非必要流量"),
            (kept.ButtonRemaining, kept.Comment, kept.ShowsWhiteDot, kept.SourceNote));
        var empty = Card(s with { Status = QuotaStatus.NoData });
        Assert.True(empty.ShowsWhiteDot);
        Assert.Empty(empty.Rows);
        Assert.Null(empty.Action);
        Assert.Equal("组织策略关闭了 Claude Code 非必要流量", empty.DetailTitle);
    }

    [Fact]
    public void hoverUsesTheReportedWindowAndThePanelClock()
    {
        var c = Card(S(Codex, Window(70, 4, 25), new(90, Now, 604800)));
        var current = c.Rows[0];
        Assert.Equal((14400d, 10800d, 30d, 3600d), (current.WindowSeconds, current.Elapsed, current.UsedPercent, current.SecondsLeft));
        var expired = c.Rows[1];
        Assert.Equal(604800, expired.WindowSeconds);
        Assert.Null(expired.UsedPercent);
        Assert.Null(expired.Elapsed);
    }

    [Fact]
    public void mixedWindowsKeepMainRowsFirstAndExpiredReadingsExplainThemselves()
    {
        var c = Card(S(OpenCodeGo) with
        {
            Meters = [new("m","Monthly",new(30)),new("s","1h",Window(100,1,50)),
            new("long","Weekly",Window(31,168,39)),new("session","5h",Window(36,5,16)),new("g","Model group",new(3))]
        });
        Assert.Equal(["5h", "7d", "1h", "Model group", "mo"], Labels(c.Rows));
        Assert.Equal("7d", c.Tightest!.Label);
        var expired = Card(S(Codex, new(100, Now.AddSeconds(-1))));
        Assert.Equal("已重置", expired.Comment);
        Assert.Null(expired.ButtonRemaining);
        Assert.Equal(Neutral, expired.Tone);
    }

    [Fact]
    public void aHiddenUnreadableSubscriptionIsNotSummarizedAsHealthy()
    {
        var missing = new QuotaSnapshot(OpenCodeGo, QuotaStatus.NoData) { LiveFailure = Failure.CredentialsNotFound };
        var p = new QuotaPanelDisplay([missing, .. Fixtures], [Codex, ClaudeCode, Cursor, Grok, OpenCodeGo], OpenCodeGo, Now);
        Assert.Equal([Grok, OpenCodeGo], p.Overview.Remainder!.Providers);
        Assert.Null(p.Overview.Remainder.Summary);
        Assert.True(p.Detail!.ShowsWhiteDot);
        Assert.Equal(2, p.Overview.AttentionCount);
    }

    [Theory]
    [InlineData(false, "2 个快用完")]
    [InlineData(true, "2 个快用完 · 1 个超额")]
    public void foldedSummaryCountsRedAndAmberSeparatelyEvenWithAnUnreadableIcon(bool amber, string expected)
    {
        Provider[] reds = [Codex, Cursor, KimiCode, ZCode, OpenCodeGo];
        var snapshots = reds.Select(p => S(p, new(99))).ToList();
        snapshots.Insert(0, new(Grok, QuotaStatus.NoData) { LiveFailure = Failure.CredentialsNotFound });
        if (amber) snapshots.Add(S(ClaudeCode, Window(45, 168, 60)));
        var p = new QuotaPanelDisplay(snapshots, snapshots.Select(s => s.Provider).ToArray(), null, Now);
        Assert.Equal((Red, expected), (p.Overview.Remainder!.Tone, p.Overview.Remainder.Summary));
        Assert.True(p.Buttons[0].ShowsWhiteDot);
    }

    [Fact]
    public void urgencyAndCommentCanComeFromDifferentWindows()
    {
        var c = Card(S(ClaudeCode, Window(45, 5, 65), Window(72, 168, 60)) with { ModelScopedMeters = [new("m", "Model", Window(8, 168, 9))] });
        Assert.Equal((Red, Amber, "Model", "超额 20%"), (c.Tone, c.Inner!.Tone, c.AdditionalAlert!.Label, c.Comment));
        Assert.Equal("超额 4%", Card(S(ClaudeCode, Window(10.5, 8, 13.5), Window(46, 168, 50))).Comment);
        Assert.Equal("超额 6%", Card(S(ClaudeCode, Window(20, 5, 90), Window(44, 168, 50))).Comment);
    }
}
