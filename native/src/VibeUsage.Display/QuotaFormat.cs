using System.Globalization;

namespace VibeUsage.Display;

public sealed record DisplayLine(string Label, string Value);
public sealed record MeterTooltip(string Title, IReadOnlyList<DisplayLine> Lines);

public static class QuotaFormat
{
    public const int OverviewTicks = 61;
    public const int RingTicks = 96;
    internal static int Round(double value) => (int)Math.Round(value, MidpointRounding.AwayFromZero);
    public static string Percent(double value) => value is > 0 and < 1 ? "<1" : Round(value).ToString(CultureInfo.InvariantCulture);

    internal static string Roughly(double seconds)
    {
        if (seconds < 3600) return $"{Math.Max(1, Round(seconds / 60))} 分钟";
        var hours = Round(seconds / 3600);
        return hours < 24 ? $"{hours} 小时" : $"{Math.Max(1, Round(seconds / 86400))} 天";
    }

    public static string Duration(double seconds)
    {
        if (seconds >= 86400 - 1800)
        {
            var hours = Round(seconds / 3600);
            return hours % 24 == 0 ? $"{hours / 24} 天" : $"{hours / 24} 天 {hours % 24} 小时";
        }
        var minutes = Math.Max(1, Round(seconds / 60));
        if (minutes < 60) return $"{minutes} 分钟";
        return minutes % 60 == 0 ? $"{minutes / 60} 小时" : $"{minutes / 60} 小时 {minutes % 60} 分钟";
    }

    public static string? Reset(QuotaPanelMeter m, TimeZoneInfo zone) => m.SecondsLeft switch
    {
        null => null,
        < 86400 => TimeZoneInfo.ConvertTime(m.ResetsAt!.Value, zone).ToString("HH:mm", CultureInfo.InvariantCulture),
        double left => $"{Round(left / 86400)}天后"
    };

    public static MeterTooltip? Tooltip(QuotaPanelMeter m)
    {
        if (m.UsedPercent is not double used) return null;
        var lines = new List<DisplayLine> { new("已使用", Percent(used) + "%") };
        if (m.Elapsed is double elapsed) lines.Add(new("已过去", Duration(elapsed)));
        if (m.SecondsLeft is double left) lines.Add(new("离重置", Duration(left)));
        lines.AddRange(m.Breakdown.Select(p => new DisplayLine(p.Label, Percent(p.Utilization) + "%")));
        var title = m.WindowSeconds switch
        {
            null => m.Label,
            double s when s >= 86400 && s % 86400 == 0 => $"{s / 86400} 天窗口",
            double s when s >= 3600 => $"{Round(s / 3600)} 小时窗口",
            double s => $"{Math.Max(1, Round(s / 60))} 分钟窗口"
        };
        return new(title, lines);
    }
}
