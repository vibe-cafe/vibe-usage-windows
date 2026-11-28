using System.Globalization;

namespace VibeUsage.Display;

public enum RankColor { Blue, Teal, Magenta, Gray }
public sealed record RankingRow(string Name, double Value, double Share, RankColor Color);

public sealed class UsageRanking
{
    public static UsageRanking Empty { get; } = Ordered([]);
    public double Total { get; }
    public IReadOnlyList<RankingRow> Top { get; }
    public IReadOnlyList<RankingRow> Remainder { get; }
    public IReadOnlyList<RankingRow> Listed { get; }
    public IReadOnlyList<RankingRow> Collapsed { get; }
    public double RemainderValue { get; }
    public string? RemainderLabel => Collapsed.Count == 0 ? null : $"其余 {Collapsed.Count} 个";
    public IReadOnlyList<int?> RingTicks { get; }

    public static UsageRanking Ranked(IEnumerable<(string Name, double Value)> entries) =>
        Ordered(entries.OrderByDescending(e => e.Value).ThenBy(e => e.Name, StringComparer.Ordinal));
    public static UsageRanking Ordered(IEnumerable<(string Name, double Value)> entries) => new(entries.ToArray());

    private UsageRanking((string Name, double Value)[] entries)
    {
        Total = entries.Sum(e => e.Value);
        var rows = entries.Select((e, i) => new RankingRow(e.Name, e.Value, Total == 0 ? 0 : e.Value / Total,
            i switch { 0 => RankColor.Blue, 1 => RankColor.Teal, 2 => RankColor.Magenta, _ => RankColor.Gray })).ToArray();
        Top = rows.Take(3).ToArray();
        Remainder = rows.Skip(3).ToArray();
        RemainderValue = Remainder.Sum(e => e.Value);
        Listed = rows.Length == 4 ? rows : Top;
        Collapsed = rows.Length == 4 ? [] : Remainder;
        var segments = Top.Select(e => e.Value).ToList();
        if (Remainder.Count > 0) segments.Add(RemainderValue);
        RingTicks = TickOwners(segments);
    }

    private static int?[] TickOwners(List<double> segments)
    {
        const int count = 72;
        var owners = new int?[count];
        var total = segments.Sum();
        if (total == 0) return owners;
        var exact = segments.Select(v => v / total * count).ToArray();
        var runs = exact.Select(v => v > 0 ? Math.Max(1, (int)v) : 0).ToArray();
        var left = count - runs.Sum();
        foreach (var index in Enumerable.Range(0, runs.Length).Where(i => runs[i] > 0)
                     .OrderByDescending(i => exact[i] - Math.Floor(exact[i])).Take(Math.Max(0, left))) runs[index]++;
        while (left < 0)
        {
            runs[Array.IndexOf(runs, runs.Max())]--;
            left++;
        }
        var separated = runs.Count(r => r > 0) > 1;
        var start = 0;
        for (int i = 0; i < runs.Length; i++)
        {
            var lit = separated && runs[i] > 1 ? runs[i] - 1 : runs[i];
            Array.Fill<int?>(owners, i, start, lit);
            start += runs[i];
        }
        return owners;
    }
}

public static class DetailsFormat
{
    public static string Share(double share)
    {
        var percent = QuotaFormat.Round(share * 100);
        return percent == 0 && share > 0 ? "<1%" : $"{percent}%";
    }

    public static string Value(double value, Metric metric, double cardTotal)
    {
        if (metric == Metric.Cost) return RecentUsageNumber.Cost(value);
        var (divisor, suffix) = cardTotal switch
        {
            >= 999_500_000 => (1e9, "B"),
            >= 999_500 => (1e6, "M"),
            >= 999.5 => (1e3, "K"),
            _ => (1, "")
        };
        var scaled = value / divisor;
        if (divisor > 1 && scaled is > 0 and < 0.05) return "<0.1" + suffix;
        return scaled.ToString(divisor == 1 || scaled >= 99.95 ? "F0" : "F1", CultureInfo.InvariantCulture) + suffix;
    }

    public static string Center(double total, Metric metric)
    {
        var text = Value(total, metric, total);
        if (metric == Metric.Tokens) return text;
        var parts = RecentUsageNumber.Parts(text, dropCents: true);
        return parts.Any(p => p.Role == NumberRole.Fraction) ? text : string.Concat(parts.Select(p => p.Text));
    }
}
