namespace VibeUsage.Display;

public enum Provider { Codex, ClaudeCode, Cursor, KimiCode, Grok, OpenCodeGo, ZCode, Antigravity }

public static class ProviderNames
{
    public static string DisplayName(this Provider p) => p switch
    {
        Provider.ClaudeCode => "Claude",
        Provider.KimiCode => "Kimi Code",
        Provider.OpenCodeGo => "OpenCode Go",
        Provider.Antigravity => "Antigravity / AGY",
        _ => p.ToString()
    };
    public static string OverviewName(this Provider p) => p == Provider.Antigravity ? "Antigravity" : p.DisplayName();
    public static string UsageSource(this Provider p) => p switch
    {
        Provider.ClaudeCode => "claude-code",
        Provider.KimiCode => "kimi-code",
        Provider.OpenCodeGo => "opencode",
        _ => p.ToString().ToLowerInvariant()
    };
}

public enum QuotaStatus { Ok, NoData, Unauthorized, RetryableError, Error, Disabled }
public enum EmptyReason { LimitReached, NoWindow, NotEntitled, SessionWithoutPlanLimits, PeriodEnded, NoLocalRecord, NonessentialTrafficDisabled }
public enum QuotaSource { Live, LocalLog, Cache, DesktopRecord }
public enum CursorSignIn { Required, Expired }
public enum ZCodeRegion { BigModel, Zai }
public enum Failure
{
    ClaudeBinaryNotFound, ClaudeLaunchFailed, ClaudeTimedOut, ClaudeNoResponse, ClaudeNoWindows, ClaudeBlockedByPolicy,
    DesktopRecordStale, DesktopMultipleInstalls, DesktopMixedOrganizations, DesktopRecordUnreadable,
    CredentialsNotFound, CredentialsRejected, CredentialsExpired, CredentialsUnreadable, CredentialsEncrypted,
    LoginRefreshFailed, ProgramNotFound, ProgramOutdated, ProgramNoAnswer, LocalRecordUnreadable,
    EndpointTimedOut, EndpointUnreachable, EndpointHttpStatus, EndpointUnrecognized, CredentialStoreUnavailable,
    CliRuntimeMissing, CliTimedOut, CliFailed, CliUnrecognizedOutput, CliSchemaMismatch, CliNoResult, CliReportedFailure,
    EgressRegionUnsupported, EgressUnconfirmed, ProxyNotDetected, ProxyUnsupported
}

public sealed record QuotaWindow(double Utilization, DateTimeOffset? ResetsAt = null, double? WindowSeconds = null);
public sealed record MeterPart(string Label, double Utilization);
public sealed record QuotaMeter(string Id, string Label, QuotaWindow Window, params MeterPart[] Breakdown);
public sealed record ExtraUsage(bool IsEnabled, double Spend, double Limit);

public sealed record QuotaSnapshot(Provider Provider, QuotaStatus Status = QuotaStatus.Ok)
{
    public IReadOnlyList<QuotaMeter> Meters { get; init; } = [];
    public IReadOnlyList<QuotaMeter> ModelScopedMeters { get; init; } = [];
    public QuotaWindow? FiveHour { get; init; }
    public QuotaWindow? SevenDay { get; init; }
    public QuotaWindow? SevenDayOpus { get; init; }
    public QuotaWindow? SevenDaySonnet { get; init; }
    public ExtraUsage? ExtraUsage { get; init; }
    public string? PlanLabel { get; init; }
    public int? ResetCreditsCount { get; init; }
    public DateTimeOffset? FetchedAt { get; init; }
    public DateTimeOffset? DataAsOf { get; init; }
    public QuotaSource? Source { get; init; }
    public string? SourceLabel { get; init; }
    public EmptyReason? EmptyReason { get; init; }
    public Failure? LiveFailure { get; init; }
    public int? HttpStatus { get; init; }
    public string? EgressRegion { get; init; }
    public CursorSignIn? SignIn { get; init; }
}
