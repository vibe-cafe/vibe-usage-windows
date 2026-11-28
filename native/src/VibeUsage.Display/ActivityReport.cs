using System.Globalization;

namespace VibeUsage.Display;

// The server deduplicates activity across the effective tools and terminals.
// Two independently selected sets in different click order share one cache entry.
public sealed record ActivityReadKey
{
    public UsageReadKey Usage { get; }
    public IReadOnlyList<string> Sources { get; }
    public IReadOnlyList<string> Hostnames { get; }

    public ActivityReadKey(UsageReadKey usage, UsageFilters effective)
    {
        Usage = usage;
        Sources = Array.AsReadOnly(effective.Sources.Order(StringComparer.Ordinal).ToArray());
        Hostnames = Array.AsReadOnly(effective.Hostnames.Order(StringComparer.Ordinal).ToArray());
    }

    public bool Equals(ActivityReadKey? other) => other != null && Usage == other.Usage
        && Sources.SequenceEqual(other.Sources) && Hostnames.SequenceEqual(other.Hostnames);

    public override int GetHashCode()
    {
        var hash = new HashCode();
        hash.Add(Usage);
        hash.Add(Sources.Count);
        foreach (var source in Sources) hash.Add(source);
        foreach (var host in Hostnames) hash.Add(host);
        return hash.ToHashCode();
    }
}

public sealed record LoadedActivity(ActivityReadKey Key, ActivityReport Report)
{
    public ActivityReport? For(ActivityReadKey selection) => Key == selection ? Report : null;
}

public sealed record ActivityTime(long ActiveSeconds, long WorkSeconds, double? Parallel)
{
    public string? ParallelText => Parallel is double p ? p.ToString("0.#", CultureInfo.InvariantCulture) + " 个" : null;
}
public sealed record ActivityPeriod(DateTimeOffset Start, long ActiveSeconds, long WorkSeconds);

public sealed class ActivityReport
{
    public ActivityTime? Summary { get; }
    public string PanelHelp { get; }
    public DateTimeOffset? FirstRecorded { get; }
    public bool Hourly { get; }
    private readonly IReadOnlyList<ActivityPeriod> periods;

    public ActivityReport(ActivityTime totals, IReadOnlyList<ActivityPeriod> periods, DateTimeOffset? firstRecorded,
        DateTimeOffset rangeStart, bool hourly, TimeZoneInfo zone, DateTimeOffset? since = null)
    {
        this.periods = periods;
        FirstRecorded = firstRecorded;
        Hourly = hourly;
        var partial = firstRecorded > (since ?? rangeStart);
        if (firstRecorded == null || partial && totals.WorkSeconds == 0)
        {
            PanelHelp = "更新后显示";
            return;
        }
        Summary = totals;
        var parts = new List<string>();
        if (totals.ParallelText is { } text) parts.Add("平均同时 " + text);
        if (partial) parts.Add(UsageRange.LocalDate(firstRecorded.Value, zone).ToString("yyyy-MM-dd", CultureInfo.InvariantCulture) + " 之前的记录无法重算");
        PanelHelp = string.Join(" · ", parts);
    }

    public ActivityTime?[] Series(UsagePlotGrid grid)
    {
        var result = new ActivityTime?[grid.Slots.Count];
        if (grid.Range.Hourly != Hourly || FirstRecorded == null) return result;
        var sums = new (long Active, long Work)[result.Length];
        foreach (var period in periods)
        {
            var i = grid.Index(period.Start);
            if (i < 0) continue;
            sums[i].Active += period.ActiveSeconds;
            sums[i].Work += period.WorkSeconds;
        }
        for (var i = 0; i < result.Length; i++)
        {
            if (grid.Slots[i].End <= FirstRecorded) continue;
            var (active, work) = sums[i];
            var concurrency = active > 0 ? Math.Max(1, Math.Round(10.0 * work / active, MidpointRounding.AwayFromZero) / 10) : (double?)null;
            result[i] = new(active, work, concurrency);
        }
        return result;
    }
}
