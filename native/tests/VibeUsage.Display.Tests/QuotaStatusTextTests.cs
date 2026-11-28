using static VibeUsage.Display.Tests.QuotaPanelDisplayTests;

namespace VibeUsage.Display.Tests;

public class QuotaStatusTextTests
{
    [Fact]
    public void noDataUsesTheReportedReasonBeforeLocalDetection()
    {
        (EmptyReason empty, Provider provider, string comment, string title)[] cases = [
            (EmptyReason.NotEntitled,Provider.OpenCodeGo,"未订阅","未订阅 OpenCode Go"),
            (EmptyReason.PeriodEnded,Provider.Grok,"周期已结束","账期已结束 · 用一次 Grok 后更新"),
            (EmptyReason.PeriodEnded,Provider.Antigravity,"周期已结束","额度已重置 · 下次读取时更新"),
            (EmptyReason.PeriodEnded,Provider.Codex,"周期已结束","上个周期已结束"),
            (EmptyReason.NoLocalRecord,Provider.Grok,"无本机记录","暂无记录 · 用一次 Grok 后更新"),
            (EmptyReason.NoLocalRecord,Provider.Antigravity,"无本机记录","用过 Antigravity 后显示"),
            (EmptyReason.NoLocalRecord,Provider.Cursor,"无本机记录","本机暂无记录"),
            (EmptyReason.SessionWithoutPlanLimits,Provider.Cursor,"未使用订阅登录","当前登录方式不含订阅额度")];
        foreach (var (empty, provider, comment, title) in cases)
        {
            var c = Card(new(provider, QuotaStatus.NoData) { EmptyReason = empty, LiveFailure = Failure.EndpointTimedOut });
            Assert.Equal((comment, title), (c.Comment, c.DetailTitle));
            Assert.Null(c.Action);
        }
        var cold = new QuotaSnapshot(Provider.Codex, QuotaStatus.NoData);
        Assert.Equal("尚未读取", Card(cold).DetailTitle);
        var read = cold with { FetchedAt = Now };
        Assert.Equal("未检测到安装", Card(read).DetailTitle);
        Assert.Equal("没读到额度，原因未知", new QuotaPanelDisplay([read], [Provider.Codex], null, Now, detected: new HashSet<Provider> { Provider.Codex }).Detail!.DetailTitle);
        Assert.Equal("订阅配额未启用", Card(cold with { Status = QuotaStatus.Disabled }).DetailTitle);
    }

    [Fact]
    public void readFailuresUseTheFixedCopyForTheReportedChannel()
    {
        (Failure failure, string reason)[] cases = [
            (Failure.ClaudeBinaryNotFound,"未找到 Claude Code"),(Failure.ClaudeLaunchFailed,"启动失败"),
            (Failure.ClaudeTimedOut,"读取超时"),(Failure.CliTimedOut,"读取超时"),(Failure.ClaudeNoResponse,"没有响应"),
            (Failure.ClaudeNoWindows,"没有返回额度"),(Failure.ProgramNoAnswer,"没有返回额度"),
            (Failure.CredentialsNotFound,"未找到登录信息"),(Failure.CredentialsRejected,"登录已失效"),
            (Failure.CredentialsUnreadable,"读不到登录信息"),(Failure.CredentialsEncrypted,"登录信息已加密，暂时读不了"),
            (Failure.LoginRefreshFailed,"登录续期失败"),(Failure.ProgramNotFound,"未找到程序"),(Failure.ProgramOutdated,"版本较旧，升级后可实时读取"),
            (Failure.LocalRecordUnreadable,"读不了本机记录"),(Failure.EndpointUnreachable,"连接失败"),
            (Failure.EndpointHttpStatus,"接口报错"),(Failure.EndpointUnrecognized,"返回数据无法识别"),
            (Failure.CliRuntimeMissing,"缺少 Node.js"),(Failure.CliFailed,"配额 CLI 出错"),
            (Failure.CliUnrecognizedOutput,"配额 CLI 输出无法识别"),(Failure.CliSchemaMismatch,"配额 CLI 版本不兼容"),
            (Failure.CliNoResult,"配额 CLI 没有返回结果"),(Failure.CliReportedFailure,"实时读取失败")];
        var read = new QuotaSnapshot(Provider.Codex, QuotaStatus.RetryableError);
        foreach (var (failure, reason) in cases)
            Assert.Equal(reason, Card(read with { LiveFailure = failure }).DetailReason);
        Assert.Equal("接口报错 502", Card(read with { LiveFailure = Failure.EndpointHttpStatus, HttpStatus = 502 }).DetailReason);
        foreach (var (p, f, text) in new[] { (Provider.Grok,Failure.CredentialsNotFound,"运行 grok login 后可实时读取"),
            (Provider.OpenCodeGo,Failure.CredentialsNotFound,"在 OpenCode 里登录后自动显示"),
            (Provider.Grok,Failure.ProgramNotFound,"未找到 Grok 命令行"),(Provider.Antigravity,Failure.ProgramNotFound,"请安装 agy 或 Antigravity") })
            Assert.Equal(text, Card(read with { Provider = p, LiveFailure = f }).DetailReason);
    }
}
