using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

namespace VibeUsage.PanelPoc.Data;

// Everything the sample shows is written here: the same numbers as the Mac
// new panel's renders (vibe-usage-app PanelSnapshotTests: PanelQuotas,
// RecentUsageFixtures), so the two can be compared item by item. No network,
// no files, no accounts.

public enum Tone { Neutral, Amber, Red }

/// <param name="Remaining">0–100 left.</param>
/// <param name="Expected">0–100 the window's time says should be left (the triangle / mark).</param>
public sealed record Meter(string Id, string Label, double? Remaining, double? Expected, Tone Tone,
    string? Prediction = null, string? Reset = null);

/// <param name="Outer">The longer window: ticks on the bar and the ring.</param>
/// <param name="Inner">The shorter window: the thin line / inner arc.</param>
/// <param name="Center">What the ring's centre shows.</param>
public sealed record Subscription(string Id, string Name, string? Plan, string Glyph, double GlyphRatio, double ButtonGlyphSide,
    Meter? Outer, Meter? Inner, Meter Center, IReadOnlyList<Meter> Rows, string Comment, Tone CommentTone,
    double ButtonRemaining, string ButtonStatus, double UsageShare, bool ShowsDot = false)
{
    public bool Attention => CommentTone != Tone.Neutral;
}

public sealed record CurvePoint(double X, double V);

public sealed record AxisTick(double X, string Label);

/// <param name="Curve">Points 0…1 across, values 0…1 of <paramref name="Peak"/>.</param>
public sealed record Period(string Chip, bool ChipMono, double Cost, double TokensM, string Active, string Ai,
    IReadOnlyList<CurvePoint> Curve, double Peak, string StartLabel, IReadOnlyList<AxisTick> Ticks, string EndLabel,
    IReadOnlyList<string> SlotLabels);

public sealed record RankRow(string Name, double TokensM, double Cost);

public sealed record DetailCardData(string Title, bool HasSwitch, bool NamesAreDynamic, bool FixedOrder, IReadOnlyList<RankRow> Rows);

public static class FakeData
{
    private static readonly IReadOnlyList<Meter> NoRows = Array.Empty<Meter>();

    private static Subscription Codex() => new("codex", "Codex", "Plus", ProviderGlyphs.Codex, 0.833, 14,
        Outer: new Meter("codex-7d", "7d", 79, 52, Tone.Neutral, "够用到重置", "4天后"),
        Inner: new Meter("codex-5h", "5h", 12, 29, Tone.Red, "约 40 分钟后用完", "15:18"),
        Center: new Meter("codex-5h", "5h", 12, 29, Tone.Red),
        Rows: new[]
        {
            new Meter("codex-5h", "5h", 12, 29, Tone.Red, "约 40 分钟后用完", "15:18"),
            new Meter("codex-7d", "7d", 79, 52, Tone.Neutral, "够用到重置", "4天后"),
        },
        Comment: "超额 17%", CommentTone: Tone.Red, ButtonRemaining: 12, ButtonStatus: "快用完", UsageShare: 0.45);

    private static Subscription Claude() => new("claude", "Claude", "Max", ProviderGlyphs.Claude, 0.833, 15,
        Outer: new Meter("claude-7d", "7d", 31, 39, Tone.Amber, "约 2 天后用完", "3天后"),
        Inner: new Meter("claude-5h", "5h", 36, 16, Tone.Neutral, "够用到重置", "15:18"),
        Center: new Meter("claude-7d", "7d", 31, 39, Tone.Amber),
        Rows: new[]
        {
            new Meter("claude-5h", "5h", 36, 16, Tone.Neutral, "够用到重置", "15:18"),
            new Meter("claude-7d", "7d", 31, 39, Tone.Amber, "约 2 天后用完", "3天后"),
            new Meter("claude-fable", "Fable", 58, 39, Tone.Neutral, "够用到重置", "3天后"),
        },
        Comment: "超额 8%", CommentTone: Tone.Amber, ButtonRemaining: 31, ButtonStatus: "偏快", UsageShare: 0.35);

    private static Subscription Cursor() => new("cursor", "Cursor", "Pro", ProviderGlyphs.Cursor, 0.833, 13,
        Outer: new Meter("cursor-mo", "mo", 41, 42, Tone.Neutral, "够用到重置", "12天后"),
        Inner: null,
        Center: new Meter("cursor-mo", "mo", 41, 42, Tone.Neutral),
        Rows: new[] { new Meter("cursor-mo", "mo", 41, 42, Tone.Neutral, "够用到重置", "12天后") },
        Comment: "进度正常", CommentTone: Tone.Neutral, ButtonRemaining: 41, ButtonStatus: "进度正常", UsageShare: 0.15);

    private static Subscription Grok() => new("grok", "Grok", "SuperGrok", ProviderGlyphs.Grok, 0.978, 14,
        Outer: new Meter("grok-7d", "7d", 88, 71, Tone.Neutral, "够用到重置", "5天后"),
        Inner: null,
        Center: new Meter("grok-7d", "7d", 88, 71, Tone.Neutral),
        Rows: new[] { new Meter("grok-7d", "7d", 88, 71, Tone.Neutral, "够用到重置", "5天后") },
        Comment: "余量 17%", CommentTone: Tone.Neutral, ButtonRemaining: 88, ButtonStatus: "余量充足", UsageShare: 0.05);

    /// <summary>Enabled subscriptions in the Settings order (the round buttons' order).</summary>
    public static readonly IReadOnlyList<Subscription> Subscriptions = new[] { Claude(), Codex(), Cursor(), Grok() };

    /// <summary>The overview's rows: red first, then amber, then the rest in order.</summary>
    public static IReadOnlyList<Subscription> OverviewRows(IReadOnlyList<Subscription> enabled) =>
        enabled.OrderByDescending(s => (int)s.CommentTone).ThenBy(s => Index(enabled, s)).ToList();

    private static int Index(IReadOnlyList<Subscription> list, Subscription s)
    {
        for (int i = 0; i < list.Count; i++) if (list[i].Id == s.Id) return i;
        return 0;
    }

    public const string LastSyncNumber = "26";
    public const string LastSyncText = " 分钟前同步";

    public static string TrayTooltip => "Vibe Usage 原生面板样品\n今天 $41.20 · Codex 5h 剩余 12%";

    // -- 近期消耗 ------------------------------------------------------------------------------------

    /// <summary>Today's curve, sampled from the Mac render (9:00 → now, peak $9.4).</summary>
    private static readonly CurvePoint[] TodayCurve =
    {
        new(0.0, 0.0), new(0.0247, 0.127), new(0.0494, 0.248), new(0.0741, 0.346), new(0.0988, 0.425),
        new(0.1236, 0.508), new(0.1483, 0.582), new(0.173, 0.642), new(0.1977, 0.684), new(0.2224, 0.702),
        new(0.2471, 0.698), new(0.2718, 0.693), new(0.2965, 0.689), new(0.3213, 0.679), new(0.346, 0.665),
        new(0.3707, 0.651), new(0.3954, 0.605), new(0.4201, 0.499), new(0.4448, 0.355), new(0.4695, 0.215),
        new(0.4942, 0.1), new(0.5189, 0.044), new(0.5437, 0.081), new(0.5684, 0.201), new(0.5931, 0.369),
        new(0.6178, 0.545), new(0.6425, 0.698), new(0.6672, 0.801), new(0.6919, 0.852), new(0.7166, 0.898),
        new(0.7414, 0.935), new(0.7661, 0.967), new(0.7908, 0.99), new(0.8155, 1.0), new(0.8402, 0.995),
        new(0.8649, 0.986), new(0.8896, 0.963), new(0.9143, 0.94), new(0.939, 0.907), new(0.9638, 0.879),
        new(0.9885, 0.861), new(1.0, 0.842),
    };

    private static CurvePoint[] Even(params double[] values) =>
        values.Select((v, i) => new CurvePoint(values.Length == 1 ? 1 : (double)i / (values.Length - 1), v)).ToArray();

    private static string[] TodaySlots()
    {
        // 9:00 to 15:42 ("now") in the render's clock.
        return TodayCurve.Select(p =>
        {
            var t = TimeSpan.FromHours(9) + TimeSpan.FromMinutes(p.X * 402);
            return $"{(int)t.TotalHours}:{t.Minutes:00}";
        }).ToArray();
    }

    public static readonly IReadOnlyList<Period> Periods = new[]
    {
        new Period("今天", false, 41.20, 65.5, "3h 12m", "4h 28m", TodayCurve, 9.4, "9:00",
            new[] { new AxisTick(0.298, "11:00"), new AxisTick(0.596, "13:00") }, "现在", TodaySlots()),
        new Period("24H", true, 58.70, 92.4, "5h 06m", "7h 41m",
            Even(0.31, 0.45, 0.22, 0.60, 0.41, 0.13, 0.04, 0, 0, 0, 0, 0, 0, 0, 0, 0.19, 0.55, 0.73, 0.68, 0.12, 0.81, 1.0, 0.86, 0.31),
            9.4, "昨天 16:00", new[] { new AxisTick(0.25, "22:00"), new AxisTick(0.5, "4:00"), new AxisTick(0.75, "10:00") }, "现在",
            Enumerable.Range(0, 24).Select(i => $"{(16 + i) % 24}:00").ToArray()),
        new Period("7D", true, 197.70, 312.0, "21h 05m", "30h 12m",
            Even(0.11, 0.75, 0.89, 0.70, 0.54, 0.80, 1.0), 41.2, "11/22",
            new[] { new AxisTick(1.0 / 3, "11/24"), new AxisTick(2.0 / 3, "11/26") }, "今天",
            new[] { "11/22", "11/23", "11/24", "11/25", "11/26", "11/27", "今天" }),
        new Period("30D", true, 1214.0, 1210.0, "86h 10m", "121h 32m",
            Even(0.42, 0.55, 0.61, 0.18, 0.12, 0.66, 0.72, 0.58, 0.69, 0.81, 0.22, 0.15, 0.63, 0.77, 0.70, 0.74, 0.92,
                0.25, 0.19, 0.71, 0.84, 0.79, 0.88, 0.95, 0.27, 0.11, 0.86, 0.90, 0.80, 1.0),
            41.2, "10/30", new[] { new AxisTick(10.0 / 29, "11/9"), new AxisTick(20.0 / 29, "11/19") }, "今天",
            Enumerable.Range(0, 30).Select(i => new DateTime(2026, 10, 30).AddDays(i).ToString("M/d", CultureInfo.InvariantCulture)).ToArray()),
        new Period("90D", true, 3480.0, 3480.0, "241h 18m", "352h 40m",
            Even(0.45, 0.55, 0.66, 0.61, 0.72, 0.79, 0.70, 0.86, 0.94, 0.82, 1.0, 0.88, 0.92), 214, "8/30",
            new[] { new AxisTick(4.0 / 12, "9/27"), new AxisTick(8.0 / 12, "10/25") }, "今天",
            Enumerable.Range(0, 13).Select(i => new DateTime(2026, 8, 30).AddDays(7 * i).ToString("M/d", CultureInfo.InvariantCulture) + " 起").ToArray()),
    };

    // -- 展开详情 -------------------------------------------------------------------------------------

    public static readonly IReadOnlyList<DetailCardData> DetailCards = new[]
    {
        new DetailCardData("模型分布", true, true, false, new[]
        {
            new RankRow("GPT-5.5", 29.1, 18.40), new RankRow("Opus 5.5", 11.6, 14.10),
            new RankRow("Gemini 3.8 Pro", 5.3, 3.20), new RankRow("GPT-5.5 mini", 4.4, 3.10),
            new RankRow("Kimi K2", 2.5, 2.40),
        }),
        new DetailCardData("Token 构成", false, false, true, new[]
        {
            new RankRow("缓存", 50.5, 0), new RankRow("输入", 1.7, 0), new RankRow("输出", 0.7, 0),
        }),
        new DetailCardData("终端分布", true, true, false, new[]
        {
            new RankRow("Studio-MacBook-Pro-14", 42.9, 33.40), new RankRow("mac-mini", 10.1, 7.80),
        }),
        new DetailCardData("工具分布", true, true, false, new[]
        {
            new RankRow("Codex", 29.1, 18.40), new RankRow("Claude Code", 18.5, 16.40), new RankRow("Cursor", 5.3, 6.40),
        }),
    };

    // -- formats (DetailsFormat, Formatters) ------------------------------------------------------

    public static string Cost(double dollars) =>
        "$" + dollars.ToString(dollars >= 1000 ? "#,0" : "0.00", CultureInfo.InvariantCulture);

    /// <summary>The ring centre and the big number: cents dropped from $100 up.</summary>
    public static string CostCenter(double dollars) =>
        "$" + (dollars >= 100 ? Math.Round(dollars).ToString("#,0", CultureInfo.InvariantCulture) : dollars.ToString("0.00", CultureInfo.InvariantCulture));

    /// <summary>A Token figure in the unit of the card's total: one decimal, none from 100 up.</summary>
    public static string Tokens(double millions, double totalMillions)
    {
        double value = millions, unitTotal = totalMillions;
        string suffix = "M";
        if (totalMillions >= 999.5) { value /= 1000; unitTotal /= 1000; suffix = "B"; }
        else if (totalMillions < 0.9995) { value *= 1000; unitTotal *= 1000; suffix = "K"; }
        return (Math.Abs(value) >= 100 ? value.ToString("0", CultureInfo.InvariantCulture) : value.ToString("0.0", CultureInfo.InvariantCulture)) + suffix;
    }

    /// <summary>Three significant digits, like Formatters.formatPanelTokens (65.5M, 312M, 1.21B).</summary>
    public static string PanelTokens(double millions)
    {
        if (millions >= 1000) return (millions / 1000).ToString(millions >= 10000 ? "0.0" : "0.00", CultureInfo.InvariantCulture) + "B";
        return millions.ToString(millions >= 100 ? "0" : millions >= 10 ? "0.0" : "0.00", CultureInfo.InvariantCulture) + "M";
    }

    public static string Share(double fraction) =>
        fraction > 0 && fraction < 0.01 ? "<1%" : Math.Round(fraction * 100).ToString("0", CultureInfo.InvariantCulture) + "%";

    /// <summary>The peak label above the curve: 「$9.4」, 「$41」, 「$214」.</summary>
    public static string Peak(double dollars) =>
        "$" + (dollars < 10 ? dollars.ToString("0.0", CultureInfo.InvariantCulture) : Math.Round(dollars).ToString("0", CultureInfo.InvariantCulture));
}
