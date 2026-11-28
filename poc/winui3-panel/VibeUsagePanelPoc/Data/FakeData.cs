using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

namespace VibeUsage.PanelPoc.Data;

// Everything the sample shows is written here. No network, no files, no
// accounts: the point is the shape of the information and how it feels.

public enum Pace { Normal, Fast, Critical }

/// <param name="Label">Short window name: 5 小时, 7 天, 本月, Fable.</param>
/// <param name="Remaining">Remaining percentage, 0–100.</param>
/// <param name="Prediction">够用到重置 / 约 40 分钟后用完.</param>
/// <param name="Reset">15:18 重置 / 3 天后重置.</param>
public sealed record QuotaWindow(string Label, int Remaining, Pace Pace, string Prediction, string Reset);

public sealed record Subscription(string Id, string Name, string Plan, string Glyph,
    IReadOnlyList<QuotaWindow> Windows, double UsageShare)
{
    /// <summary>The window that decides how this subscription is doing.</summary>
    public QuotaWindow Binding => Windows.OrderByDescending(w => (int)w.Pace).ThenBy(w => w.Remaining).First();

    public Pace Pace => Binding.Pace;
}

public enum AxisAlign { Left, Center, Right }

public sealed record AxisLabel(int Index, string Text, AxisAlign Align);

/// <param name="Bars">Estimated cost per slot (dollars).</param>
/// <param name="SlotLabels">Tooltip text per slot.</param>
public sealed record UsageRange(string Label, double TokensM, string Active, string AiHours,
    double[] Bars, string[] SlotLabels, IReadOnlyList<AxisLabel> Axis)
{
    public double Cost => Math.Round(Bars.Sum(), 2);
}

public sealed record Share(string Name, double TokensM, double Cost);

public sealed record Dimension(string Label, bool HasCost, IReadOnlyList<Share> Items);

public static class FakeData
{
    public static readonly IReadOnlyList<Subscription> Subscriptions = new[]
    {
        new Subscription("codex", "Codex", "Plus", ProviderGlyphs.Codex, new[]
        {
            new QuotaWindow("5 小时", 12, Pace.Critical, "约 40 分钟后用完", "15:18 重置"),
            new QuotaWindow("7 天", 79, Pace.Normal, "够用到重置", "4 天后重置"),
        }, 0.45),
        new Subscription("claude", "Claude", "Max", ProviderGlyphs.Claude, new[]
        {
            new QuotaWindow("5 小时", 36, Pace.Normal, "够用到重置", "15:18 重置"),
            new QuotaWindow("7 天", 31, Pace.Fast, "约 2 天后用完", "3 天后重置"),
            new QuotaWindow("Fable", 58, Pace.Normal, "够用到重置", "3 天后重置"),
        }, 0.35),
        new Subscription("cursor", "Cursor", "Pro", ProviderGlyphs.Cursor, new[]
        {
            new QuotaWindow("本月", 41, Pace.Normal, "够用到重置", "12 天后重置"),
        }, 0.15),
        new Subscription("grok", "Grok", "SuperGrok", ProviderGlyphs.Grok, new[]
        {
            new QuotaWindow("7 天", 88, Pace.Normal, "够用到重置", "5 天后重置"),
        }, 0.05),
    };

    public static readonly IReadOnlyList<UsageRange> Ranges = BuildRanges();

    public static readonly IReadOnlyList<Dimension> Dimensions = new[]
    {
        new Dimension("模型", true, new[]
        {
            new Share("GPT-5.5", 36.0, 18.40),
            new Share("Opus 5.5", 14.4, 14.10),
            new Share("Gemini 3.8 Pro", 6.6, 3.20),
            new Share("其余 2 个", 8.5, 5.50),
        }),
        new Dimension("Token 构成", false, new[]
        {
            new Share("缓存读取", 62.5, 0),
            new Share("输入", 2.1, 0),
            new Share("输出", 0.9, 0),
        }),
        new Dimension("终端", true, new[]
        {
            new Share("DESKTOP-7Q2K", 53.1, 33.40),
            new Share("mac-mini", 12.4, 7.80),
        }),
        new Dimension("工具", true, new[]
        {
            new Share("Codex", 36.0, 18.40),
            new Share("Claude Code", 22.9, 16.40),
            new Share("Cursor", 6.6, 6.40),
        }),
    };

    public const string LastSync = "26 分钟前同步";

    public static string TrayTooltip
    {
        get
        {
            var urgent = Subscriptions.Where(s => s.Pace == Pace.Critical).Select(s => $"{s.Name} {s.Binding.Label}剩余 {s.Binding.Remaining}%");
            return $"Vibe Usage\n今天 {Money(Ranges[0].Cost)}" + (urgent.Any() ? " · " + string.Join("，", urgent) : string.Empty);
        }
    }

    public static string Money(double dollars) => "$" + dollars.ToString(dollars >= 1000 ? "#,0" : "0.00", CultureInfo.InvariantCulture);

    public static string Tokens(double millions) => millions >= 1000
        ? (millions / 1000).ToString("0.00", CultureInfo.InvariantCulture) + "B"
        : millions.ToString("0.0", CultureInfo.InvariantCulture) + "M";

    private static UsageRange[] BuildRanges()
    {
        // 今天: half-hour slots from the first activity (9:00) to now (16:30), as the Mac panel trims to activity.
        double[] today = { 0.6, 1.2, 2.3, 2.9, 3.4, 3.5, 3.3, 3.1, 0.4, 0.7, 3.2, 4.4, 4.7, 4.6, 1.9, 1.0 };
        string[] todayLabels = Enumerable.Range(0, today.Length)
            .Select(i => $"{9 + i / 2}:{(i % 2 == 0 ? "00" : "30")}").ToArray();

        // 过去 24 小时: hourly, 17:00 yesterday to 16:00 today.
        double[] day = { 3.1, 4.2, 2.0, 5.6, 3.9, 1.2, 0.4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1.8, 5.2, 6.9, 6.4, 1.1, 7.6, 9.3, 2.9 };
        string[] dayLabels = Enumerable.Range(0, 24).Select(i => $"{(17 + i) % 24}:00").ToArray();

        // 过去 7 天 (today is Saturday 11/28).
        double[] week = { 4.6, 31.0, 36.5, 28.9, 22.4, 33.1, 41.2 };
        string[] weekDays = { "周日", "周一", "周二", "周三", "周四", "周五", "今天" };
        string[] weekLabels = { "11/22 周日", "11/23 周一", "11/24 周二", "11/25 周三", "11/26 周四", "11/27 周五", "11/28 今天" };

        // 过去 30 天: deterministic pseudo-random, quieter on weekends.
        var start30 = new DateTime(2026, 10, 30);
        var rng = new Random(20261128);
        double[] month = Enumerable.Range(0, 30).Select(i =>
        {
            var d = start30.AddDays(i);
            bool weekend = d.DayOfWeek is DayOfWeek.Saturday or DayOfWeek.Sunday;
            return i == 29 ? 41.2 : Math.Round((weekend ? 4 : 18) + rng.NextDouble() * (weekend ? 10 : 24), 1);
        }).ToArray();
        string[] monthLabels = Enumerable.Range(0, 30).Select(i => start30.AddDays(i).ToString("M/d", CultureInfo.InvariantCulture)).ToArray();

        // 过去 90 天: weekly.
        var start90 = new DateTime(2026, 8, 30);
        double[] quarter = { 96, 118, 142, 131, 155, 170, 149, 183, 201, 176, 214, 188, 197.7 };
        string[] quarterLabels = Enumerable.Range(0, 13)
            .Select(i => $"{start90.AddDays(7 * i):M/d} 起一周").ToArray();

        return new[]
        {
            new UsageRange("今天", 65.5, "3 小时 12 分钟", "4 小时 28 分钟", today, todayLabels, new[]
            {
                new AxisLabel(0, "9:00", AxisAlign.Left), new AxisLabel(4, "11:00", AxisAlign.Left),
                new AxisLabel(8, "13:00", AxisAlign.Left), new AxisLabel(15, "现在", AxisAlign.Right),
            }),
            new UsageRange("过去 24 小时", 98.1, "5 小时 40 分钟", "7 小时 55 分钟", day, dayLabels, new[]
            {
                new AxisLabel(0, "昨天 17:00", AxisAlign.Left), new AxisLabel(9, "2:00", AxisAlign.Left),
                new AxisLabel(16, "9:00", AxisAlign.Left), new AxisLabel(23, "现在", AxisAlign.Right),
            }),
            new UsageRange("过去 7 天", 312.4, "21 小时 5 分钟", "30 小时 12 分钟", week, weekLabels,
                weekDays.Select((t, i) => new AxisLabel(i, t, AxisAlign.Center)).ToArray()),
            new UsageRange("过去 30 天", 1214.0, "86 小时", "121 小时", month, monthLabels, new[]
            {
                new AxisLabel(0, "10/30", AxisAlign.Left), new AxisLabel(8, "11/7", AxisAlign.Left),
                new AxisLabel(16, "11/15", AxisAlign.Left), new AxisLabel(29, "今天", AxisAlign.Right),
            }),
            new UsageRange("过去 90 天", 3480.0, "241 小时", "352 小时", quarter, quarterLabels, new[]
            {
                new AxisLabel(0, "8/30", AxisAlign.Left), new AxisLabel(4, "9/27", AxisAlign.Left),
                new AxisLabel(8, "10/25", AxisAlign.Left), new AxisLabel(12, "本周", AxisAlign.Right),
            }),
        };
    }
}
