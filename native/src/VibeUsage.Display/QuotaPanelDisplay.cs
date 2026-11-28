using System.Globalization;

namespace VibeUsage.Display;

public enum ButtonRing { Empty, NeutralArc, RedStartPoint }
public sealed record OverviewChip(string Id, string Label, double Remaining, PaceTone Tone, bool IsColumn);

public sealed class QuotaSubscriptionDisplay
{
    public Provider Provider { get; }
    public string? PlanLabel { get; }
    public int? ResetCreditsCount { get; }
    public string? CreditsText => ResetCreditsCount is > 0 ? $"重置券 ×{ResetCreditsCount}" : null;
    public IReadOnlyList<QuotaPanelMeter> Rows { get; }
    public IReadOnlyList<QuotaPanelMeter> CompactRows { get; }
    public int FoldedCount => Rows.Count - CompactRows.Count;
    public string? FoldLabel => FoldedCount > 0 ? $"另有 {FoldedCount} 项" : null;
    public QuotaPanelMeter? Tightest { get; }
    public QuotaPanelMeter? AdditionalAlert { get; }
    public QuotaPanelMeter? Outer { get; }
    public QuotaPanelMeter? Inner { get; }
    public QuotaPanelMeter? RingCenter { get; }
    public PaceTone Tone { get; }
    public string? Comment { get; }
    public PaceTone CommentTone => ReadState == ReadState.Normal && Comment != null ? Tone : PaceTone.Neutral;
    public double? ButtonRemaining { get; }
    public ButtonRing ButtonRing => ButtonRemaining switch { null => ButtonRing.Empty, 0 => ButtonRing.RedStartPoint, _ => ButtonRing.NeutralArc };
    public string ButtonStatus { get; }
    public string ButtonHelp => (PlanLabel == null ? Provider.DisplayName() : $"{Provider.DisplayName()} {PlanLabel}") + " · " + ButtonStatus;
    public bool ShowsWhiteDot => ReadState switch
    {
        ReadState.OrganizationPolicy => Rows.Count == 0,
        ReadState.Paused or ReadState.NeedsLogin or ReadState.LoginExpired or ReadState.ReadFailed => true,
        _ => false
    };
    public ReadState ReadState { get; }
    public bool ShowsSkeleton => ReadState == ReadState.Loading;
    public string? DetailTitle { get; }
    public string? DetailReason { get; }
    public QuotaAction? Action { get; }
    public string? SourceNote { get; }
    public DateTimeOffset? RecordTime { get; }
    public string? RecordTimeText(TimeZoneInfo zone, CultureInfo culture) =>
        RecordTime is { } time ? "记录时间 " + TimeZoneInfo.ConvertTime(time, zone).ToString("G", culture) : null;

    public IReadOnlyList<OverviewChip> OverviewChips { get; }

    internal QuotaSubscriptionDisplay(QuotaSnapshot s, DateTimeOffset now, bool refreshing, bool detected, ZCodeRegion region)
    {
        Provider = s.Provider;
        PlanLabel = s.PlanLabel;
        ResetCreditsCount = s.ResetCreditsCount;
        var reading = QuotaReading.Resolve(s, refreshing, detected, region);
        ReadState = reading.State;
        var exhausted = s.EmptyReason == EmptyReason.LimitReached && reading.State == ReadState.Normal;
        Rows = s.Status == QuotaStatus.Ok && !exhausted ? QuotaPanelMeter.Rows(s, now) : [];
        DetailTitle = Rows.Count == 0 ? reading.Title : null;
        DetailReason = reading.Reason;
        Action = reading.Action;
        SourceNote = Rows.Count > 0 ? QuotaReading.SourceNote(s) : null;
        if (Rows.Count > 0 && s.Provider == Provider.ClaudeCode && s.Source == QuotaSource.DesktopRecord)
        {
            RecordTime = s.DataAsOf;
            SourceNote ??= s.SourceLabel ?? "Claude 桌面额度记录 · 非本机独占用量";
        }

        var plan = Rows.Where(m => !m.IsExtra).ToArray();
        var current = plan.Where(m => m.Remaining.HasValue).ToArray();
        Tightest = current.MinBy(m => m.Remaining);
        var rings = current.Where(m => m.PrimaryDuration.HasValue).OrderByDescending(m => m.PrimaryDuration).Take(2).ToArray();
        Outer = rings.FirstOrDefault();
        Inner = rings.ElementAtOrDefault(1);
        RingCenter = rings.MinBy(m => m.Remaining);
        if (RingCenter == null && !plan.Any(m => m.PrimaryDuration.HasValue))
            RingCenter = current.Where(m => !m.IsModel).MinBy(m => m.Remaining);
        CompactRows = Rows.Take(3).ToArray();
        if (Tightest != null && !CompactRows.Contains(Tightest))
            CompactRows = [.. Rows.Take(2), Tightest];

        ButtonRemaining = exhausted ? 0 : Tightest?.Remaining;
        Tone = exhausted ? PaceTone.Red : current.Select(m => m.Tone).DefaultIfEmpty().Max();
        AdditionalAlert = Tone == PaceTone.Neutral ? null : current.FirstOrDefault(m => m.PrimaryDuration == null && m.Tone == Tone);
        Comment = reading.Short ?? PaceComment(current.Max(m => m.CommentLag), plan.Length > 0 && current.Length == 0);
        ButtonStatus = ReadState == ReadState.Paused ? "读取已暂停" : reading.Short ?? (ShowsSkeleton ? "正在读取…"
            : ButtonRemaining == 0 ? "已用完" : Tone switch
            {
                PaceTone.Red => "快用完",
                PaceTone.Amber => "偏快",
                _ => Tightest == null ? Comment ?? "暂无订阅配额数据" : "正常"
            });
        var chips = new List<OverviewChip>();
        void AddChip(QuotaPanelMeter? m, PaceTone tone, bool column)
        {
            if (m?.Remaining is double rem) chips.Add(new(m.Id, m.Label, rem, tone, column));
        }
        AddChip(AdditionalAlert, Tone, false);
        AddChip(Inner, PaceTone.Neutral, true);
        AddChip(Outer, PaceTone.Neutral, true);
        if (Outer == null && Tightest?.Id != AdditionalAlert?.Id) AddChip(Tightest, PaceTone.Neutral, false);
        OverviewChips = chips;
    }

    private string? PaceComment(double? lag, bool reset)
    {
        if (ButtonRemaining == 0) return "已用完";
        if (lag is not double delta) return reset ? "已重置" : null;
        if (Tone == PaceTone.Red && delta <= 0) return null;
        if (Tone == PaceTone.Red || delta > 5) return $"超额 {Math.Max(1, QuotaFormat.Round(delta))}%";
        return delta < -5 ? $"余量 {QuotaFormat.Round(-delta)}%" : "进度正常";
    }
}

public sealed record OverviewRemainder(IReadOnlyList<Provider> Providers, string? Summary, PaceTone Tone)
{
    public string Label => $"其余 {Providers.Count} 个";
}

public sealed class QuotaOverviewDisplay
{
    public int Total { get; }
    public int NearlyEmpty { get; }
    public int Fast { get; }
    public int AttentionCount => NearlyEmpty + Fast;
    public PaceTone Tone => NearlyEmpty > 0 ? PaceTone.Red : Fast > 0 ? PaceTone.Amber : PaceTone.Neutral;
    public IReadOnlyList<QuotaSubscriptionDisplay> Rows { get; }
    public double RowPitch => Rows.Count == 4 ? 38.5 : 42.5;
    public OverviewRemainder? Remainder { get; }

    internal QuotaOverviewDisplay(QuotaSubscriptionDisplay[] subscriptions)
    {
        Total = subscriptions.Length;
        NearlyEmpty = subscriptions.Count(s => s.Tone == PaceTone.Red);
        Fast = subscriptions.Count(s => s.Tone == PaceTone.Amber);
        var ordered = subscriptions.OrderByDescending(s => s.Tone).ToArray();
        Rows = Total >= 5 ? ordered[..3] : ordered;
        if (Total < 5) return;
        var hidden = ordered[3..];
        var labels = new List<string>();
        var red = hidden.Count(s => s.Tone == PaceTone.Red);
        var amber = hidden.Count(s => s.Tone == PaceTone.Amber);
        if (red > 0) labels.Add($"{red} 个快用完");
        if (amber > 0) labels.Add($"{amber} 个超额");
        if (labels.Count == 0 && hidden.All(s => !s.ShowsWhiteDot && s.ButtonRemaining.HasValue)) labels.Add("都正常");
        Remainder = new(hidden.Select(s => s.Provider).ToArray(), labels.Count == 0 ? null : string.Join(" · ", labels), hidden[0].Tone);
    }
}

public sealed class QuotaPanelDisplay
{
    public IReadOnlyList<QuotaSubscriptionDisplay> Buttons { get; }
    public QuotaOverviewDisplay Overview { get; }
    public QuotaSubscriptionDisplay? Detail { get; }
    public Provider? UsageProvider => Buttons.Count > 0 ? Detail?.Provider : null;
    public IReadOnlyList<Provider> RefreshTargets { get; }
    public bool IsRefreshing { get; }
    public bool IsEmpty => Overview.Total == 0;
    public string? EmptyTitle => IsEmpty ? "还没有要看的订阅" : null;
    public string? EmptyActionLabel => IsEmpty ? "去设置" : null;

    public QuotaPanelDisplay(IReadOnlyList<QuotaSnapshot> snapshots, IReadOnlyList<Provider> enabled, Provider? selected,
        DateTimeOffset now, IReadOnlySet<Provider>? refreshing = null, IReadOnlySet<Provider>? detected = null,
        ZCodeRegion region = ZCodeRegion.BigModel)
    {
        var subscriptions = enabled.Select(p => new QuotaSubscriptionDisplay(
            snapshots.FirstOrDefault(s => s.Provider == p) ?? new(p, QuotaStatus.NoData), now,
            refreshing?.Contains(p) == true, detected?.Contains(p) == true, region)).ToArray();
        Buttons = enabled.Count > 1 ? subscriptions : [];
        Overview = new(subscriptions);
        Detail = enabled.Count == 1 ? subscriptions[0] : subscriptions.FirstOrDefault(s => s.Provider == selected);
        RefreshTargets = Detail == null ? enabled.ToArray() : [Detail.Provider];
        IsRefreshing = refreshing?.Overlaps(RefreshTargets) == true;
    }
}
