using System.Globalization;

namespace VibeUsage.Display;

public enum PaceTone { Neutral, Amber, Red }
public sealed record Forecast(double? RunsOutInSeconds)
{
    public string Text => RunsOutInSeconds is double eta ? $"约 {QuotaFormat.Roughly(eta)}后用完" : "够用到重置";
}

public sealed class QuotaPanelMeter
{
    public string Id { get; }
    public string Label { get; }
    public IReadOnlyList<MeterPart> Breakdown { get; }
    public DateTimeOffset? ResetsAt { get; }
    public double? WindowSeconds { get; }
    public double? SecondsLeft { get; }
    public double? Elapsed { get; }
    public double? UsedPercent { get; }
    public double? Remaining { get; }
    public double? ExpectedRemaining { get; }
    public Forecast? Prediction { get; }
    public PaceTone Tone { get; }
    public string? Message => Remaining == null ? "已重置，等下次读取" : null;
    internal double? PrimaryDuration { get; }
    internal bool IsExtra { get; }
    internal bool IsModel { get; }
    internal double? CommentLag { get; }

    private QuotaPanelMeter(QuotaMeter meter, DateTimeOffset now, double? primary = null, bool model = false, bool extra = false)
    {
        Id = meter.Id;
        Label = extra ? "额外" : meter.Label == "Month" ? "mo" : meter.Label;
        Breakdown = meter.Breakdown;
        PrimaryDuration = primary;
        IsModel = model;
        IsExtra = extra;
        var window = meter.Window;
        ResetsAt = window.ResetsAt;
        WindowSeconds = window.WindowSeconds is > 0 ? window.WindowSeconds : null;
        var left = (ResetsAt - now)?.TotalSeconds;
        if (left <= 0) return;
        SecondsLeft = left;
        UsedPercent = Math.Clamp(window.Utilization, 0, 100);
        var used = UsedPercent.Value;
        var remaining = 100 - used;
        Remaining = remaining;
        Tone = !extra && remaining < 10 ? PaceTone.Red : PaceTone.Neutral;
        if (WindowSeconds is not double duration || left is not double seconds) return;
        Elapsed = Math.Max(0, duration - seconds);
        if (extra) return;
        ExpectedRemaining = Math.Min(100, seconds * 100 / duration);
        var elapsed = Elapsed.Value;
        var lag = used - elapsed * 100 / duration;
        if (remaining > 0 && elapsed >= duration * 0.15)
        {
            var eta = used > 0 ? remaining * elapsed / used : double.PositiveInfinity;
            Prediction = new(lag > 0 ? eta : null);
            Tone = seconds - eta >= 900 && (remaining < 10 || eta <= 3600) ? PaceTone.Red
                : lag > 5 ? PaceTone.Amber : PaceTone.Neutral;
        }
        if (Tone == PaceTone.Red || Prediction != null) CommentLag = lag;
    }

    public int LitTicks(int count) => (int)Math.Floor((Remaining ?? 0) / 100 * count + 0.5);

    internal static QuotaPanelMeter[] Rows(QuotaSnapshot s, DateTimeOffset now)
    {
        // Windows Desktop records supply Opus and Sonnet in the generic meter list.
        bool DesktopModel(QuotaMeter m) => s.Provider == Provider.ClaudeCode && s.Source == QuotaSource.DesktopRecord
            && m.Id is "desktop-so" or "desktop-sn";
        var windows = s.Meters.Where(m => m.Id != "on-demand" && !DesktopModel(m)).ToList();
        if (s.FiveHour != null) windows.Add(new("5h", "5h", s.FiveHour));
        if (s.SevenDay != null) windows.Add(new("7d", "7d", s.SevenDay));
        var plan = windows.Select(Canonical)
            .OrderBy(w => w.Meter.Label is "5h" or "7d" ? 0 : 1)
            .ThenBy(w => w.Duration ?? double.PositiveInfinity).ToArray();
        var rows = plan.Take(2).Select(w => new QuotaPanelMeter(w.Meter, now, w.Duration)).ToList();
        rows.AddRange(s.ModelScopedMeters.Select(m => new QuotaPanelMeter(m, now, model: true)));
        var other = plan.Skip(2).Select(w => w.Meter);
        rows.AddRange(other.Where(m => m.Label != "Month").Select(m => new QuotaPanelMeter(m, now)));
        rows.AddRange(s.Meters.Where(DesktopModel).Select(m => new QuotaPanelMeter(m, now, model: true)));
        if (s.SevenDayOpus != null) rows.Add(new(new("opus", "Opus", s.SevenDayOpus), now, model: true));
        if (s.SevenDaySonnet != null) rows.Add(new(new("sonnet", "Sonnet", s.SevenDaySonnet), now, model: true));
        rows.AddRange(other.Where(m => m.Label == "Month").Select(m => new QuotaPanelMeter(m, now)));
        rows.AddRange(s.Meters.Where(m => m.Id == "on-demand").Select(m => new QuotaPanelMeter(m, now, extra: true)));
        if (s.ExtraUsage is { IsEnabled: true, Limit: > 0 } spend)
            rows.Add(new(new("extra", "额外", new(spend.Spend / spend.Limit * 100)), now, extra: true));
        return rows.ToArray();
    }

    private static (QuotaMeter Meter, double? Duration) Canonical(QuotaMeter m)
    {
        var label = string.Concat(m.Label.Where(c => !char.IsWhiteSpace(c))).ToLowerInvariant();
        label = label switch { "daily" or "day" => "1d", "weekly" or "week" or "1w" => "7d", "monthly" or "month" => "Month", _ => label };
        if (label == "Month") return (m with { Label = label }, m.Window.WindowSeconds ?? 30 * 86400);
        if (label.Length < 2 || !double.TryParse(label[..^1], NumberStyles.Float, CultureInfo.InvariantCulture, out var amount) || amount <= 0)
            return (m, null);
        int scale = label[^1] switch { 'm' => 60, 'h' => 3600, 'd' => 86400, 'w' => 604800, _ => 0 };
        return scale == 0 ? (m, null) : (m with { Label = label }, m.Window.WindowSeconds ?? amount * scale);
    }
}
