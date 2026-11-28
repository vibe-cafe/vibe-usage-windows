namespace VibeUsage.Display;

public enum ReadState { Normal, Loading, NeedsLogin, LoginExpired, ReadFailed, Paused, OrganizationPolicy, NonSubscriptionLogin }
public enum QuotaAction { Retry, CursorSettings }

internal sealed record QuotaReading(ReadState State, string? Title = null, string? Short = null,
    QuotaAction? Action = null, string? Reason = null)
{
    internal static QuotaReading Resolve(QuotaSnapshot s, bool refreshing, bool detected, ZCodeRegion region)
    {
        var reason = FailureText(s);
        if (s.LiveFailure == Failure.ClaudeBlockedByPolicy)
            return new(ReadState.OrganizationPolicy, reason, "组织策略");
        if (s.LiveFailure is Failure.ProxyUnsupported or Failure.ProxyNotDetected or Failure.EgressUnconfirmed or Failure.EgressRegionUnsupported)
            return new(ReadState.Paused, reason, "已暂停读取");
        if (s.Status != QuotaStatus.Ok && refreshing && s.EmptyReason != EmptyReason.LimitReached)
            return new(ReadState.Loading, "正在读取…");
        if (s.Provider == Provider.Cursor && s.Status is QuotaStatus.NoData or QuotaStatus.Unauthorized && s.SignIn is { } signIn)
            return signIn == CursorSignIn.Required
                ? new(ReadState.NeedsLogin, "需要登录 Cursor", "需登录", QuotaAction.CursorSettings)
                : new(ReadState.LoginExpired, "Cursor 登录已失效", "登录已失效", QuotaAction.CursorSettings);

        return s.Status switch
        {
            QuotaStatus.Ok => new(ReadState.Normal),
            QuotaStatus.Disabled => new(ReadState.Normal, "订阅配额未启用", "未启用"),
            QuotaStatus.RetryableError or QuotaStatus.Error => new(ReadState.ReadFailed, "暂时读不到额度", "读取失败", QuotaAction.Retry, reason),
            QuotaStatus.Unauthorized => Credential(s, AuthorizationText(s, region), QuotaAction.Retry),
            _ => s.EmptyReason switch
            {
                EmptyReason.NonessentialTrafficDisabled => new(ReadState.OrganizationPolicy, "组织策略关闭了 Claude Code 非必要流量 · 无法读取", "组织策略"),
                EmptyReason.SessionWithoutPlanLimits => new(ReadState.NonSubscriptionLogin,
                    s.Provider == Provider.ClaudeCode ? "Claude Code 未使用订阅登录" : "当前登录方式不含订阅额度", "未使用订阅登录"),
                EmptyReason.NoWindow => new(ReadState.Normal, "没有生效的额度窗口", "无窗口"),
                EmptyReason.NotEntitled => new(ReadState.Normal, $"未订阅 {s.Provider.DisplayName()}", "未订阅"),
                EmptyReason.LimitReached => new(ReadState.Normal, "已用完"),
                EmptyReason.PeriodEnded => new(ReadState.Normal, s.Provider switch
                {
                    Provider.Grok => "账期已结束 · 用一次 Grok 后更新",
                    Provider.Antigravity => "额度已重置 · 下次读取时更新",
                    _ => "上个周期已结束"
                }, "周期已结束"),
                EmptyReason.NoLocalRecord => new(ReadState.Normal, s.Provider switch
                {
                    Provider.Grok => "暂无记录 · 用一次 Grok 后更新",
                    Provider.Antigravity => "用过 Antigravity 后显示",
                    _ => "本机暂无记录"
                }, "无本机记录"),
                _ when reason != null => Credential(s, reason),
                _ when s.FetchedAt == null => new(ReadState.Normal, "尚未读取", "尚未读取"),
                _ when detected => new(ReadState.Normal, "没读到额度，原因未知", "无数据"),
                _ => new(ReadState.Normal, "未检测到安装", "未检测到安装")
            }
        };
    }

    private static QuotaReading Credential(QuotaSnapshot s, string title, QuotaAction? action = null) =>
        s.LiveFailure switch
        {
            Failure.CredentialsNotFound => new(ReadState.NeedsLogin, title, s.Provider == Provider.ZCode ? "需 Key" : "需登录", action),
            Failure.CredentialsRejected when s.Provider == Provider.Antigravity => new(ReadState.NeedsLogin, title, "需登录", action),
            Failure.CredentialsExpired or Failure.CredentialsRejected or null =>
                new(ReadState.LoginExpired, title, s.Provider == Provider.ZCode ? "Key 无效" : "登录已失效", action),
            _ => new(ReadState.ReadFailed, title, "读取失败", action)
        };

    private static string AuthorizationText(QuotaSnapshot s, ZCodeRegion region)
    {
        if (s.LiveFailure is not (null or Failure.CredentialsRejected or Failure.CredentialsNotFound))
            return FailureText(s)!;
        var missing = s.LiveFailure == Failure.CredentialsNotFound;
        return s.Provider switch
        {
            Provider.ZCode when missing => $"请在设置中配置 {(region == ZCodeRegion.BigModel ? "BigModel" : "Z.ai")} API Key",
            Provider.ZCode => "API Key 无效，请在设置中更换",
            Provider.Antigravity => "请在 Antigravity 中登录",
            Provider.OpenCodeGo => "请在 OpenCode 中重新登录后重试",
            Provider.KimiCode or Provider.Cursor when missing => $"请先登录 {s.Provider.DisplayName()}",
            _ => missing ? "请先登录" : "登录已失效，请重新登录"
        };
    }

    internal static string? SourceNote(QuotaSnapshot s)
    {
        if (FailureText(s) is not { } reason) return null;
        var label = s.Source switch
        {
            QuotaSource.Live => "上次实时读取",
            QuotaSource.Cache => "缓存",
            QuotaSource.DesktopRecord => "Claude 桌面端记录",
            QuotaSource.LocalLog => "本地日志",
            _ => null
        };
        return label == null ? reason : $"来源：{label} · {reason}";
    }

    private static string? FailureText(QuotaSnapshot s) => s.LiveFailure switch
    {
        null => null,
        Failure.ClaudeBinaryNotFound => "未找到 Claude Code",
        Failure.ClaudeLaunchFailed => "启动失败",
        Failure.ClaudeTimedOut or Failure.EndpointTimedOut or Failure.CliTimedOut => "读取超时",
        Failure.ClaudeNoResponse => "没有响应",
        Failure.ClaudeNoWindows or Failure.ProgramNoAnswer => "没有返回额度",
        Failure.ClaudeBlockedByPolicy => "组织策略关闭了 Claude Code 非必要流量",
        Failure.DesktopRecordStale => "Claude 桌面端记录已过期",
        Failure.DesktopMultipleInstalls => "装了多份 Claude 桌面端，账号不明确",
        Failure.DesktopMixedOrganizations => "Claude 桌面端记录涉及多个组织，账号不明确",
        Failure.DesktopRecordUnreadable => "Claude 桌面端记录无法解析",
        Failure.CredentialStoreUnavailable => "无法读取 Windows 凭据管理器",
        Failure.CredentialsNotFound when s.Provider == Provider.OpenCodeGo => "在 OpenCode 里登录后自动显示",
        Failure.CredentialsNotFound when s.Provider == Provider.Grok => "运行 grok login 后可实时读取",
        Failure.CredentialsNotFound => "未找到登录信息",
        Failure.CredentialsExpired when s.Provider == Provider.OpenCodeGo => "打开 OpenCode 后自动显示",
        Failure.CredentialsExpired or Failure.CredentialsRejected => "登录已失效",
        Failure.CredentialsUnreadable => "读不到登录信息",
        Failure.CredentialsEncrypted => "登录信息已加密，暂时读不了",
        Failure.LoginRefreshFailed => "登录续期失败",
        Failure.ProgramNotFound when s.Provider == Provider.Antigravity => "请安装 agy 或 Antigravity",
        Failure.ProgramNotFound when s.Provider == Provider.Grok => "未找到 Grok 命令行",
        Failure.ProgramNotFound => "未找到程序",
        Failure.ProgramOutdated => "版本较旧，升级后可实时读取",
        Failure.LocalRecordUnreadable => "读不了本机记录",
        Failure.EndpointUnreachable => "连接失败",
        Failure.EndpointHttpStatus => s.HttpStatus is int status ? $"接口报错 {status}" : "接口报错",
        Failure.EndpointUnrecognized => "返回数据无法识别",
        Failure.CliRuntimeMissing => "缺少 Node.js",
        Failure.CliFailed => "配额 CLI 出错",
        Failure.CliUnrecognizedOutput => "配额 CLI 输出无法识别",
        Failure.CliSchemaMismatch => "配额 CLI 版本不兼容",
        Failure.CliNoResult => "配额 CLI 没有返回结果",
        Failure.CliReportedFailure => "实时读取失败",
        Failure.EgressRegionUnsupported => "网络出口" + (s.EgressRegion switch
        {
            "CN" => "在中国大陆",
            "HK" => "在香港",
            "MO" => "在澳门",
            _ => "不在支持地区"
        }) + "，已暂停读取",
        Failure.EgressUnconfirmed => "无法确认网络出口，已暂停读取",
        Failure.ProxyNotDetected => "未检测到代理，已暂停读取",
        Failure.ProxyUnsupported => "代理无法照着走，已暂停读取",
        _ => "暂时读不到额度"
    };
}
