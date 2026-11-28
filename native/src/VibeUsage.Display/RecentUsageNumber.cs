using System.Globalization;
using System.Text.RegularExpressions;

namespace VibeUsage.Display;

public enum NumberRole { Symbol, Integer, Fraction, Unit }
public sealed record NumberPart(string Text, NumberRole Role);

public static class RecentUsageNumber
{
    private static readonly CultureInfo Invariant = CultureInfo.InvariantCulture;
    public static IReadOnlyList<NumberPart> Parts(string formatted, bool dropCents = false)
    {
        if (!formatted.Any(char.IsAsciiDigit)) return [new(formatted, NumberRole.Integer)];
        var parts = new List<NumberPart>();
        foreach (Match match in Regex.Matches(formatted, @"[0-9][0-9,]*|\.[0-9]+|[^0-9. ]+"))
        {
            string value = match.Value;
            if (char.IsAsciiDigit(value[0])) parts.Add(new(value, NumberRole.Integer));
            else if (value[0] == '.') parts.Add(new(value[1..], NumberRole.Fraction));
            else parts.Add(new(value, parts.Count == 0 ? NumberRole.Symbol : NumberRole.Unit));
        }
        int i = parts.FindIndex(p => p.Role == NumberRole.Integer);
        if (dropCents && i >= 0 && i + 1 < parts.Count && parts[i + 1].Role == NumberRole.Fraction
            && long.TryParse(parts[i].Text, NumberStyles.None, Invariant, out var whole))
        {
            var fraction = parts[i + 1].Text;
            if (whole >= 100 || whole == 0 && fraction.All(c => c == '0'))
            {
                parts[i] = new((whole + (fraction[0] >= '5' ? 1 : 0)).ToString(Invariant), NumberRole.Integer);
                parts.RemoveAt(i + 1);
            }
        }
        return parts;
    }

    public static string Duration(long seconds)
    {
        if (seconds <= 0) return "0m";
        if (seconds < 60) return $"{seconds}s";
        var minutes = seconds % 3600 / 60;
        var hours = seconds / 3600;
        if (hours == 0) return $"{minutes}m";
        return minutes == 0 ? $"{hours}h" : $"{hours}h {minutes:00}m";
    }

    public static string Cost(double usd, bool cny = false)
    {
        var amount = cny ? usd * 7 : usd;
        return (cny ? "¥" : "$") + amount.ToString(amount is > 0 and < 0.01 ? "F4" : "F2", Invariant);
    }

    public static string Tokens(long count, bool chinese = false)
    {
        if (count < 0) return "—";
        if (count < (chinese ? 10000 : 1000)) return count.ToString(Invariant);
        // Round the count before choosing its unit so 999,950 reads 1M.
        var scale = (decimal)Math.Pow(10, Math.Floor(Math.Log10(count)) - 2);
        var rounded = Math.Round(count / scale, MidpointRounding.AwayFromZero) * scale;
        var (unit, suffix) = chinese
            ? rounded >= 100_000_000 ? (100_000_000, "亿") : (10_000, "万")
            : rounded switch { >= 1_000_000_000 => (1_000_000_000, "B"), >= 1_000_000 => (1_000_000, "M"), _ => (1000, "K") };
        return (rounded / unit).ToString("0.###", Invariant) + suffix;
    }
}

public enum RecentRow { Cost, Token, ActiveTime, WorkTime }

public sealed record RecentUsageFigures(UsageAmount? Total, ActivityTime? Activity, UsagePanelState Units)
{
    public string Text(RecentRow row) => row switch
    {
        RecentRow.Cost when Total is { } a => RecentUsageNumber.Cost(a.Cost, Units.UsesCny),
        RecentRow.Token when Total is { } a => RecentUsageNumber.Tokens(a.TotalTokens, Units.UsesChineseTokens),
        RecentRow.ActiveTime when Activity != null => RecentUsageNumber.Duration(Activity.ActiveSeconds),
        RecentRow.WorkTime when Activity != null => RecentUsageNumber.Duration(Activity.WorkSeconds),
        _ => "—"
    };
    public static IReadOnlyList<RecentRow> Rows(Hero hero) => Enum.GetValues<RecentRow>().Where(r => r != (RecentRow)hero).ToArray();
    public string? ExactText(Hero hero) => hero switch
    {
        Hero.Cost when Total is { } a => (Units.UsesCny ? "¥" : "$") +
            (a.Cost * (Units.UsesCny ? 7 : 1)).ToString("F4", CultureInfo.InvariantCulture),
        Hero.Token when Total is { } a => a.TotalTokens.ToString("N0", CultureInfo.InvariantCulture),
        _ => null
    };
    public string? UnitHint(Hero hero) => hero switch
    {
        Hero.Cost => Units.UsesCny ? "点一下换成美元" : "点一下换成人民币",
        Hero.Token => Units.UsesChineseTokens ? "点一下换成 M、B" : "点一下换成万、亿",
        _ => null
    };
    public static string Label(Hero hero) => hero switch { Hero.Cost => "预估费用", Hero.Token => "总 Token", _ => "活跃时长" };
    public static string RowLabel(RecentRow row) => row switch
    {
        RecentRow.Cost => "费用",
        RecentRow.Token => "Token",
        RecentRow.ActiveTime => "活跃",
        _ => "AI 工时"
    };
}
