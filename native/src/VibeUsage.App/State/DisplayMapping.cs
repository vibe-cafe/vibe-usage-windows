using System;
using System.Globalization;
using System.Linq;
using B = VibeUsage.Backend;
using D = VibeUsage.Display;

namespace VibeUsage.App.State;

/// <summary>
/// Backend contract records → the display library's inputs: status, windows,
/// meters, plan, times, and what the card explains with (source, failure,
/// empty reason, Cursor sign-in). Codes the library does not know stay null,
/// so a newer backend never makes the card guess (docs/SUBSCRIPTION_QUOTAS.md
/// 「空态判定（不猜）」).
/// </summary>
internal static class DisplayMapping
{
    public static D.Provider Provider(B.Provider p) => p switch
    {
        B.Provider.Codex => D.Provider.Codex,
        B.Provider.ClaudeCode => D.Provider.ClaudeCode,
        B.Provider.Cursor => D.Provider.Cursor,
        B.Provider.KimiCode => D.Provider.KimiCode,
        B.Provider.Grok => D.Provider.Grok,
        B.Provider.OpenCodeGo => D.Provider.OpenCodeGo,
        B.Provider.ZCode => D.Provider.ZCode,
        _ => D.Provider.Antigravity,
    };

    public static B.Provider Backend(D.Provider p) => p switch
    {
        D.Provider.Codex => B.Provider.Codex,
        D.Provider.ClaudeCode => B.Provider.ClaudeCode,
        D.Provider.Cursor => B.Provider.Cursor,
        D.Provider.KimiCode => B.Provider.KimiCode,
        D.Provider.Grok => B.Provider.Grok,
        D.Provider.OpenCodeGo => B.Provider.OpenCodeGo,
        D.Provider.ZCode => B.Provider.ZCode,
        _ => B.Provider.Antigravity,
    };

    public static D.ZCodeRegion Region(B.ZCodeRegion r) => r == B.ZCodeRegion.ZAi ? D.ZCodeRegion.Zai : D.ZCodeRegion.BigModel;

    public static D.QuotaSnapshot Quota(B.Quota q) => new(Provider(q.Provider), Status(q.Status.Kind))
    {
        FiveHour = Window(q.FiveHour),
        SevenDay = Window(q.SevenDay),
        Meters = q.Meters?.Select(Meter).ToArray() ?? [],
        ModelScopedMeters = q.ModelScopedMeters?.Select(Meter).ToArray() ?? [],
        PlanLabel = q.PlanLabel,
        ResetCreditsCount = q.ResetCreditsCount is ulong n ? (int)Math.Min(n, int.MaxValue) : null,
        FetchedAt = Seconds(q.FetchedAt),
        DataAsOf = Seconds(q.DataAsOf),
        Source = Source(q.Source),
        SourceLabel = q.SourceLabel,
        EmptyReason = Empty(q.EmptyReason),
        LiveFailure = Failure(q.LiveFailure?.Code),
        HttpStatus = q.LiveFailure?.Status,
        EgressRegion = q.LiveFailure?.Region,
        SignIn = q.SignIn switch { "required" => D.CursorSignIn.Required, "expired" => D.CursorSignIn.Expired, _ => null },
    };

    private static D.QuotaStatus Status(string kind) => kind switch
    {
        "ok" => D.QuotaStatus.Ok,
        "disabled" => D.QuotaStatus.Disabled,
        "unauthorized" => D.QuotaStatus.Unauthorized,
        "retryableError" => D.QuotaStatus.RetryableError,
        "error" => D.QuotaStatus.Error,
        _ => D.QuotaStatus.NoData,
    };

    private static D.QuotaSource? Source(string? source) => source switch
    {
        "live" => D.QuotaSource.Live,
        "cache" => D.QuotaSource.Cache,
        "desktopRecord" => D.QuotaSource.DesktopRecord,
        "localLog" => D.QuotaSource.LocalLog,
        _ => null,
    };

    private static D.EmptyReason? Empty(string? reason) => reason switch
    {
        "limitReached" => D.EmptyReason.LimitReached,
        "noWindow" => D.EmptyReason.NoWindow,
        "notEntitled" => D.EmptyReason.NotEntitled,
        "sessionWithoutPlanLimits" => D.EmptyReason.SessionWithoutPlanLimits,
        "periodEnded" => D.EmptyReason.PeriodEnded,
        "noLocalRecord" => D.EmptyReason.NoLocalRecord,
        "nonessentialTrafficDisabled" => D.EmptyReason.NonessentialTrafficDisabled,
        _ => null,
    };

    /// <summary>The backend's failure codes (Rust <c>RateLimitLiveFailure</c>, camelCase) share the library's names.</summary>
    internal static D.Failure? Failure(string? code) =>
        code is { Length: > 0 } && char.IsLower(code[0]) && Enum.TryParse<D.Failure>(code, ignoreCase: true, out var f) ? f : null;

    private static D.QuotaWindow? Window(B.QuotaWindow? w) =>
        w == null ? null : new(w.Utilization, Seconds(w.ResetsAt), w.WindowDuration);

    private static D.QuotaMeter Meter(B.QuotaMeter m) =>
        new(m.Id, m.Label, new(m.Utilization, Seconds(m.ResetsAt), m.WindowDuration),
            m.Breakdown?.Select(p => new D.MeterPart(p.Label, p.Utilization)).ToArray() ?? []);

    private static DateTimeOffset? Seconds(double? epochSeconds) =>
        epochSeconds is double s ? DateTimeOffset.FromUnixTimeMilliseconds((long)Math.Round(s * 1000)) : null;

    /// <summary>Buckets whose start cannot be read are left out rather than guessed.</summary>
    public static D.UsageBucket[] Buckets(B.UsageData data) => data.Buckets
        .Select(b => DateTimeOffset.TryParse(b.BucketStart, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out var start)
            ? new D.UsageBucket(start, b.Source, b.Model, b.Hostname, b.InputTokens, b.OutputTokens, b.CachedInputTokens,
                b.ReasoningOutputTokens, b.CacheCreation5mTokens, b.CacheCreation1hTokens, b.EstimatedCost)
            : null)
        .OfType<D.UsageBucket>()
        .ToArray();
}
