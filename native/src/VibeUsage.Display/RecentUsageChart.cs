using System.Globalization;

namespace VibeUsage.Display;

public sealed record CurvePoint(double X, double Value, bool IsKnown = true);
public sealed record AxisTick(double X, string Label);
public sealed record SlotTooltip(string Period, string? Value, IReadOnlyList<DisplayLine> Rows);

public sealed class RecentUsageChart
{
    public IReadOnlyList<CurvePoint> Curve { get; }
    public IReadOnlyList<CurvePoint> HoverPoints { get; }
    public IReadOnlyList<SlotTooltip> Tooltips { get; }
    public IReadOnlyList<AxisTick> Ticks { get; }
    public bool HasKnownPoints => Curve.Count > 0;
    public double Peak { get; }
    public string PeakLabel { get; }
    public string StartLabel { get; }
    public string EndLabel { get; }
    public bool IsEmpty { get; }

    public RecentUsageChart(UsageDisplaySnapshot snapshot, Hero metric, UsagePanelState units, ActivityReport? activity = null)
    {
        var grid = metric == Hero.ActiveTime ? snapshot.ActivityGrid : snapshot.Grid;
        var slots = grid.Slots;
        var labels = new SlotLabels(grid);
        var times = metric == Hero.ActiveTime ? activity?.Series(grid) : null;
        IsEmpty = metric == Hero.ActiveTime ? activity != null && (activity.Summary?.ActiveSeconds ?? 0) == 0 : snapshot.Series.Count == 0;
        var span = slots.Count == 0 ? 1 : Math.Max(1, (slots[^1].End - slots[0].Start).TotalSeconds);
        double Position(DateTimeOffset date) => Math.Clamp((date - slots[0].Start).TotalSeconds / span, 0, 1);
        var hover = new List<CurvePoint>();
        var tips = new List<SlotTooltip>();
        for (var i = 0; i < slots.Count; i++)
        {
            var slot = slots[i];
            var x = i == slots.Count - 1 ? 1 : Position(slot.Start + (slot.End - slot.Start) / 2);
            var period = labels.Period(slot);
            if (metric == Hero.ActiveTime)
            {
                var time = times?[i];
                hover.Add(new(x, time?.ActiveSeconds ?? 0, time != null));
                var rows = new List<DisplayLine>();
                if (time != null)
                {
                    rows.Add(new("活跃", RecentUsageNumber.Duration(time.ActiveSeconds)));
                    if (time.WorkSeconds > 0) rows.Add(new("AI 工时", RecentUsageNumber.Duration(time.WorkSeconds)));
                    if (time.ParallelText is { } parallel) rows.Add(new("平均同时", parallel));
                }
                tips.Add(new(period, time == null ? "—" : null, rows));
            }
            else
            {
                var a = snapshot.Series[i];
                hover.Add(new(x, metric == Hero.Cost ? a.Cost : a.TotalTokens));
                string Tokens(long n) => RecentUsageNumber.Tokens(n, units.UsesChineseTokens);
                tips.Add(metric == Hero.Cost ? new(period, RecentUsageNumber.Cost(a.Cost, units.UsesCny), [])
                    : new(period, null, [new("总 Token", Tokens(a.TotalTokens)), new("输出", Tokens(a.Output)),
                        new("输入", Tokens(a.Input)), new("缓存", Tokens(a.Cached))]));
            }
        }
        HoverPoints = hover;
        Tooltips = tips;
        var curve = hover.Where(p => p.IsKnown).ToList();
        if (hover.FirstOrDefault() is { IsKnown: true, X: > 0 } first)
            curve.Insert(0, new(0, grid.Range.Period == Period.RollingDay ? first.Value : 0));
        Curve = curve;
        Peak = curve.Select(p => p.Value).DefaultIfEmpty().Max();
        PeakLabel = AxisLabel(Peak, metric, units);
        StartLabel = slots.Count > 0 ? labels.Start(slots[0]) : "";
        EndLabel = slots.Count > 0 ? labels.End(slots[^1]) : "";
        var ticks = new List<AxisTick>();
        if (slots.Count > 2)
        {
            int start = labels.Hourly && (slots[0].End - slots[0].Start).TotalSeconds < 3599 ? 1 : 0;
            double third = (slots.Count - start) / 3.0;
            int step = labels.Hourly ? new[] { 1, 2, 3, 4, 6, 8, 12 }.MinBy(n => Math.Abs(n - third)) : Math.Max(1, QuotaFormat.Round(third));
            foreach (int index in new[] { start + step, start + 2 * step }.Where(n => n < slots.Count))
            {
                var date = slots[index].Start;
                var x = Position(date);
                if (x is > 0.12 and < 0.85) ticks.Add(new(x, labels.Tick(date)));
            }
        }
        Ticks = ticks;
    }

    public static string AxisLabel(double value, Hero metric, UsagePanelState units)
    {
        var invariant = CultureInfo.InvariantCulture;
        if (metric == Hero.ActiveTime) return RecentUsageNumber.Duration((long)Math.Round(value));
        if (metric == Hero.Token)
        {
            var text = RecentUsageNumber.Tokens((long)Math.Round(value), units.UsesChineseTokens);
            if (!"KMB".Contains(text[^1])) return text;
            var n = double.Parse(text[..^1], invariant);
            return (n >= 10 ? Math.Round(n, MidpointRounding.AwayFromZero).ToString(invariant) : n.ToString("0.#", invariant)) + text[^1];
        }
        var cost = RecentUsageNumber.Cost(value, units.UsesCny);
        var amount = double.Parse(cost[1..], invariant);
        if (amount < 0.01) return cost;
        return cost[..1] + (amount >= 10 ? Math.Round(amount, MidpointRounding.AwayFromZero).ToString(invariant)
            : amount.ToString(amount >= 1 ? "0.#" : "0.##", invariant));
    }

    private sealed class SlotLabels(UsagePlotGrid grid)
    {
        public bool Hourly { get; } = grid.Slots.All(s => (s.End - s.Start).TotalSeconds <= 3601);
        private readonly bool weekly = grid.Slots.Any(s => (s.End - s.Start).TotalDays > 2);
        private DateTimeOffset Local(DateTimeOffset t) => TimeZoneInfo.ConvertTime(t, grid.Zone);
        private string Time(DateTimeOffset t) => Local(t).ToString("H:mm", CultureInfo.InvariantCulture);
        private string Day(DateTimeOffset t) => Local(t).ToString("M'/'d", CultureInfo.InvariantCulture);
        private string Prefix(DateTimeOffset t) => (Local(grid.Now).Date - Local(t).Date).Days switch
        {
            0 => "",
            1 => "昨天 ",
            _ => Day(t) + " "
        };
        public string Tick(DateTimeOffset t) => Hourly ? Time(t) : Day(t);
        public string Start(Slot s)
        {
            if (!Hourly) return Day(s.Start);
            var hour = Local(s.Start);
            return Prefix(hour) + $"{hour.Hour}:00";
        }
        private bool EndsDay(Slot s) => s.End > s.Start && Local(s.End).Date != Local(s.Start).Date
            && s.End == UsageRange.Midnight(UsageRange.LocalDate(s.End, grid.Zone), grid.Zone);
        public string End(Slot s)
        {
            if ((grid.Now - s.End).TotalSeconds < 1) return Hourly ? "现在" : "今天";
            return Hourly ? EndsDay(s) ? "24:00" : Time(s.End) : Day(s.End.AddSeconds(-1));
        }
        public string Period(Slot s)
        {
            if (Hourly) return Prefix(s.Start) + Time(s.Start) + "–" + (EndsDay(s) ? "24:00" : Time(s.End));
            var first = Day(s.Start);
            var last = Day(s.End.AddSeconds(-1));
            return weekly && first != last ? first + "–" + last : first;
        }
    }
}
