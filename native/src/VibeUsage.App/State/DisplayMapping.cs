using System;
using System.Globalization;
using System.Linq;
using B = VibeUsage.Backend;
using D = VibeUsage.Display;

namespace VibeUsage.App.State;

/// <summary>
/// Backend contract records → the display library's inputs. The skeleton maps
/// what its placeholders draw (status, windows, meters, plan, times); the
/// quota card (#95) adds source, failure, empty reason and sign-in.
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
