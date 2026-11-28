namespace VibeUsage.Display;

public sealed record UsageBucket(DateTimeOffset BucketStart, string Source, string Model, string Hostname,
    long InputTokens = 0, long OutputTokens = 0, long CachedInputTokens = 0, long ReasoningOutputTokens = 0,
    long? CacheCreation5mTokens = null, long? CacheCreation1hTokens = null, double? EstimatedCost = null)
{
    internal UsageAmount Amount => new(InputTokens + (CacheCreation5mTokens ?? 0) + (CacheCreation1hTokens ?? 0),
        OutputTokens + ReasoningOutputTokens, CachedInputTokens, EstimatedCost ?? 0);
}

public readonly record struct UsageAmount(long Input, long Output, long Cached, double Cost)
{
    public long TotalTokens => Input + Output + Cached;
    public static UsageAmount operator +(UsageAmount a, UsageAmount b) =>
        new(a.Input + b.Input, a.Output + b.Output, a.Cached + b.Cached, a.Cost + b.Cost);
}

public sealed class UsageDisplaySnapshot
{
    public UsagePlotGrid Grid { get; }
    public UsagePlotGrid ActivityGrid { get; }
    public UsageAmount Total { get; }
    public IReadOnlyList<UsageAmount> Series { get; }
    public long PeakTokens { get; }
    public double PeakCost { get; }
    public UsageRanking TokenComposition { get; }
    private readonly Dictionary<(Dimension, Metric), UsageRanking> rankings = [];

    public UsageDisplaySnapshot(IEnumerable<UsageBucket> buckets, UsageRange range, DateTimeOffset now, TimeZoneInfo zone,
        UsageFilters? filters = null, UsageNames? names = null)
    {
        filters ??= new();
        var grid = new UsagePlotGrid(range, now, zone);
        var amounts = new UsageAmount[grid.Slots.Count];
        int? first = null, firstActivity = null;
        var groups = new Dictionary<Dimension, Dictionary<string, UsageAmount>>();
        foreach (var b in buckets)
        {
            if (!filters.IncludesActivity(b)) continue;
            var slot = grid.Index(b.BucketStart);
            if (slot < 0) continue;
            firstActivity = Math.Min(firstActivity ?? slot, slot);
            var model = names?.ModelName(b.Model) ?? b.Model;
            if (filters.Models.Count > 0 && !filters.Models.Contains(model)) continue;
            first = Math.Min(first ?? slot, slot);
            var amount = b.Amount;
            amounts[slot] += amount;
            Total += amount;
            void Add(Dimension d, string name)
            {
                if (!groups.TryGetValue(d, out var names)) groups[d] = names = [];
                if (name.Length == 0) name = "未知";
                names[name] = names.GetValueOrDefault(name) + amount;
            }
            Add(Dimension.Model, model);
            Add(Dimension.Terminal, b.Hostname);
            Add(Dimension.Tool, names?.ToolName(b.Source) ?? b.Source);
        }

        Grid = grid.ForUsage(first);
        ActivityGrid = firstActivity == null ? grid : firstActivity == first ? Grid : grid.ForUsage(firstActivity);
        Series = amounts[(amounts.Length - Grid.Slots.Count)..];
        PeakTokens = Series.Select(a => a.TotalTokens).DefaultIfEmpty().Max();
        PeakCost = Series.Select(a => a.Cost).DefaultIfEmpty().Max();
        TokenComposition = first == null ? UsageRanking.Empty : UsageRanking.Ordered(
            [("缓存", Total.Cached), ("输入", Total.Input), ("输出", Total.Output)]);
        foreach (var (dimension, amountsByName) in groups)
        {
            rankings[(dimension, Metric.Cost)] = UsageRanking.Ranked(amountsByName.Select(p => (p.Key, p.Value.Cost)));
            rankings[(dimension, Metric.Tokens)] = UsageRanking.Ranked(amountsByName.Select(p => (p.Key, (double)p.Value.TotalTokens)));
        }
    }

    public UsageRanking Ranking(Dimension dimension, Metric metric) =>
        rankings.GetValueOrDefault((dimension, metric), UsageRanking.Empty);
}

// The caller increments AccountGeneration on relink, server change or reset. No credentials enter the display library.
public sealed record UsageReadKey(long AccountGeneration, UsageRange Range, string TimeZoneId, DateOnly? LocalDay)
{
    public static UsageReadKey For(long accountGeneration, UsageRange range, DateTimeOffset now, TimeZoneInfo zone) =>
        new(accountGeneration, range, zone.Id, range.Period is Period.Today or Period.RollingDay ? UsageRange.LocalDate(now, zone) : null);
}

public enum FiguresState { Loading, Current, Faded, Unavailable }
public sealed record UsageFigures(FiguresState State, UsageDisplaySnapshot? Snapshot);
public sealed record LoadedUsage(UsageReadKey Key, UsageDisplaySnapshot Snapshot);

public sealed class UsageFiguresKeeper
{
    private LoadedUsage? retained;

    public UsageFigures Resolve(UsageReadKey selection, LoadedUsage? loaded, bool pending, bool hasFinishedRead,
        DateTimeOffset now, TimeZoneInfo zone)
    {
        var today = UsageRange.LocalDate(now, zone);
        bool CanShow(UsageReadKey key) => key.AccountGeneration == selection.AccountGeneration
            && key.TimeZoneId == zone.Id && (key.LocalDay == null || key.LocalDay == today);
        if (retained != null && !CanShow(retained.Key)) retained = null;
        if (loaded?.Key == selection && CanShow(selection))
        {
            retained = loaded;
            return new(FiguresState.Current, loaded.Snapshot);
        }
        if (pending && retained != null) return new(FiguresState.Faded, retained.Snapshot);
        if (pending || !hasFinishedRead) return new(FiguresState.Loading, null);
        retained = null;
        return new(FiguresState.Unavailable, null);
    }
}
