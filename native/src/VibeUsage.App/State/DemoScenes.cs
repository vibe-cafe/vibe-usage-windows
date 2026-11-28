using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json.Nodes;
using VibeUsage.App.Shell;
using VibeUsage.Display;

namespace VibeUsage.App.State;

/// <summary>Which quota readings a scene shows (Mac <c>PanelQuotas</c>, plus the Windows-only states).</summary>
public enum DemoQuotas
{
    Design, Normal, CodexRedClaudeNormal, ClaudeFastCodexNormal, FiveCalm, Special, FableAlert, ClaudeExtra,
    CursorLoading, CursorReadFailed, ClaudeLoginExpired, CursorNotEntitled, ClaudeStale, Dots,
    CursorSignIn, CursorSignInExpired, CodexPausedKept, CodexPausedEmpty, ClaudePolicy, ClaudePolicyKept,
    ClaudeNonSubscription, CodexExhaustedNoWindow, GrokExpired, ClaudeWeeklyFable, ClaudePro, AgyCrowded,
    /// <summary>One provider replaced by <see cref="DemoScene.Reading"/> (the Windows state scenes).</summary>
    Custom,
}

/// <summary>
/// One state of the panel for the CI screenshots, named after the Mac panel
/// renderer's scene (<c>PanelSnapshotTests.swift</c>) so the two images can
/// be put side by side. <c>win-*</c> scenes are states the Mac renderer does
/// not draw: Windows-only reasons and every row of
/// docs/SUBSCRIPTION_QUOTAS.md 「空态判定」.
/// </summary>
public sealed record DemoScene(string Id, string Title)
{
    /// <summary>Enabled subscriptions in Settings order (Mac 131a: Claude, Codex, Cursor, Grok).</summary>
    public string[] Subscriptions { get; init; } = ["claudeCode", "codex", "cursor", "grok"];
    public DemoQuotas Quotas { get; init; } = DemoQuotas.Design;
    /// <summary>For <see cref="DemoQuotas.Custom"/>: the provider shown and its reading.</summary>
    public (string Provider, Func<DemoReadings, JsonObject> Read)? Reading { get; init; }
    public Provider? Selected { get; init; }
    public string[] Refreshing { get; init; } = [];
    /// <summary>Providers the app did not detect (「未检测到安装」).</summary>
    public string[] NotDetected { get; init; } = [];
    public Provider? HoveredOverviewRow { get; init; }
    public string? HoveredDetailRow { get; init; }
    /// <summary>「另有 N 项」 unfolded.</summary>
    public bool Expanded { get; init; }
}

public static class DemoScenes
{
    public static DemoScene? Find(DemoScenario scenario) => All.FirstOrDefault(s => s.Id == scenario.Id);

    public static DemoScene For(DemoScenario scenario) => Find(scenario) ?? All[0];

    private static readonly string[] Eight = ["claudeCode", "codex", "antigravity", "kimi-code", "cursor", "grok", "opencode-go", "zcode"];

    /// <summary>A Windows state: the provider among Claude, Codex and Grok (or in Cursor's place), its detail open.</summary>
    private static DemoScene State(string id, string title, string provider, Func<DemoReadings, JsonObject> read, bool detail = true)
    {
        string[] subs = provider is "claudeCode" or "codex" or "cursor" or "grok"
            ? ["claudeCode", "codex", "cursor", "grok"]
            : ["claudeCode", "codex", provider, "grok"];
        return new DemoScene(id, title)
        {
            Subscriptions = subs,
            Quotas = DemoQuotas.Custom,
            Reading = (provider, read),
            Selected = detail ? DisplayProvider(provider) : null,
        };
    }

    internal static Provider DisplayProvider(string id) => id switch
    {
        "codex" => Provider.Codex,
        "claudeCode" => Provider.ClaudeCode,
        "cursor" => Provider.Cursor,
        "kimi-code" => Provider.KimiCode,
        "grok" => Provider.Grok,
        "opencode-go" => Provider.OpenCodeGo,
        "zcode" => Provider.ZCode,
        _ => Provider.Antigravity,
    };

    public static readonly DemoScene[] All =
    [
        // 订阅配额 (Mac #109), one scene per drawing of 131–133.
        new("131a", "概览：4 个订阅，两家要留意"),
        new("131b", "点 Codex 的圆钮") { Selected = Provider.Codex },
        new("131c", "点 Claude：第三行是 Fable") { Selected = Provider.ClaudeCode },
        new("131d", "点 Cursor：只有一个额度") { Selected = Provider.Cursor },
        new("131e", "概览：都正常") { Quotas = DemoQuotas.Normal },
        new("131f", "概览里鼠标停在 Claude 那一行") { HoveredOverviewRow = Provider.ClaudeCode },
        new("132a", "概览：2 个订阅") { Subscriptions = ["codex", "claudeCode"], Quotas = DemoQuotas.CodexRedClaudeNormal },
        new("132b", "概览：3 个订阅") { Subscriptions = ["claudeCode", "codex", "cursor"], Quotas = DemoQuotas.ClaudeFastCodexNormal },
        new("132c", "概览：5 个订阅，其余收成一行") { Subscriptions = ["claudeCode", "codex", "cursor", "kimi-code", "grok"], Quotas = DemoQuotas.FiveCalm },
        new("132d", "概览：8 个订阅") { Subscriptions = Eight },
        new("132e", "概览：用完了、要登录、读不到、不走订阅") { Subscriptions = ["codex", "claudeCode", "cursor", "kimi-code"], Quotas = DemoQuotas.Special },
        new("132c-agy", "概览：5 个订阅，Antigravity 名字最长、两个 100%") { Subscriptions = ["claudeCode", "codex", "antigravity", "cursor", "grok"], Quotas = DemoQuotas.AgyCrowded },
        new("132f", "还没有订阅") { Subscriptions = [] },
        new("132g", "只有一个订阅：没有概览那一排") { Subscriptions = ["claudeCode"] },
        new("132h", "Fable 快用完：概览里多一个红色的 Fable") { Quotas = DemoQuotas.FableAlert },
        new("132i", "点进 Claude：中间是最紧的 Fable") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.FableAlert },
        new("132j", "三个额度：OpenCode Go") { Subscriptions = ["claudeCode", "codex", "cursor", "opencode-go"], Selected = Provider.OpenCodeGo },
        new("132k", "四个额度：另有 1 项") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudeExtra },
        new("132l", "点另有 1 项：全部列出") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudeExtra, Expanded = true },
        new("132m", "点进一家：还在读") { Selected = Provider.Cursor, Refreshing = ["cursor"], Quotas = DemoQuotas.CursorLoading },
        new("132n", "点进一家：读取失败，可以重试") { Selected = Provider.Cursor, Quotas = DemoQuotas.CursorReadFailed },
        new("132o", "点进一家：要重新登录") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudeLoginExpired },
        new("132p", "点进一家：未订阅") { Selected = Provider.Cursor, Quotas = DemoQuotas.CursorNotEntitled },
        new("132q", "读到的是旧数据：来源和原因") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudeStale },
        new("132r", "鼠标停在 7d 那一行") { Selected = Provider.Codex, HoveredDetailRow = "7d" },
        new("132s", "刷新额度（悬停是系统提示）"),
        new("132t", "刷新额度：读取中转圈，数字留着") { Refreshing = ["claudeCode"] },
        new("132u", "只看一家时刷新：只读这一家") { Selected = Provider.Codex, Refreshing = ["codex"] },
        new("133a", "概览和圆钮：平时"),
        new("133b", "点了 Codex：圆钮亮起来，概览灯灭") { Selected = Provider.Codex },
        new("133c", "圆钮悬停（系统提示：名字、套餐、状态）"),
        new("133d", "读不到数据：圆钮右上角白点") { Quotas = DemoQuotas.Dots },
        new("133e", "拖动圆钮（实际窗口验收）"),
        new("133f", "8 个订阅：一排放得下") { Subscriptions = Eight, Selected = Provider.Antigravity },
        new("133g", "齿轮（系统提示：打开订阅配额设置）"),

        // Every row of the Mac special-state table (#98), plus Claude without 5h and Claude Pro.
        new("state-loading-overview", "读取中：概览行灰块、圆钮无圈") { Refreshing = ["cursor"], Quotas = DemoQuotas.CursorLoading },
        new("state-cursor-signin-overview", "Cursor 需要登录：概览写原因、白点") { Quotas = DemoQuotas.CursorSignIn },
        new("state-cursor-signin-detail", "Cursor 需要登录：点了到设置的登录行") { Selected = Provider.Cursor, Quotas = DemoQuotas.CursorSignIn },
        new("state-cursor-expired-detail", "Cursor 登录已失效") { Selected = Provider.Cursor, Quotas = DemoQuotas.CursorSignInExpired },
        new("state-readfailed-overview", "读取失败：概览写原因、白点") { Quotas = DemoQuotas.CursorReadFailed },
        new("state-paused-kept-overview", "已暂停读取（有旧数字）：照常画、白点") { Quotas = DemoQuotas.CodexPausedKept },
        new("state-paused-kept-detail", "已暂停读取（有旧数字）：底下写原因和来源") { Selected = Provider.Codex, Quotas = DemoQuotas.CodexPausedKept },
        new("state-paused-empty-overview", "已暂停读取（没有数字）") { Quotas = DemoQuotas.CodexPausedEmpty },
        new("state-paused-empty-detail", "已暂停读取（没有数字）：一句原因") { Selected = Provider.Codex, Quotas = DemoQuotas.CodexPausedEmpty },
        new("state-policy-overview", "组织策略：概览写原因、白点") { Quotas = DemoQuotas.ClaudePolicy },
        new("state-policy-detail", "组织策略：一句原因") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudePolicy },
        new("state-policy-kept-detail", "组织策略：桌面端记录照常画") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudePolicyKept },
        new("state-nonsub-overview", "未使用订阅登录：概览、无圈") { Quotas = DemoQuotas.ClaudeNonSubscription },
        new("state-nonsub-detail", "未使用订阅登录：一句原因") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudeNonSubscription },
        new("state-exhausted-detail", "已用完：照常画") { Subscriptions = ["codex", "claudeCode", "cursor", "kimi-code"], Selected = Provider.Codex, Quotas = DemoQuotas.Special },
        new("state-exhausted-nowindow-overview", "已用完（没有窗口）：红色已用完") { Quotas = DemoQuotas.CodexExhaustedNoWindow },
        new("state-exhausted-nowindow-detail", "已用完（没有窗口）") { Selected = Provider.Codex, Quotas = DemoQuotas.CodexExhaustedNoWindow },
        new("state-expired-overview", "已过重置时间：不算要留意") { Quotas = DemoQuotas.GrokExpired },
        new("state-expired-detail", "已过重置时间：写已重置、等下次读取") { Selected = Provider.Grok, Quotas = DemoQuotas.GrokExpired },
        new("claude-no5h-fable-overview", "Claude 没有 5h、有 Fable：概览") { Quotas = DemoQuotas.ClaudeWeeklyFable },
        new("claude-no5h-fable-detail", "Claude 没有 5h、有 Fable：只画外圈") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudeWeeklyFable },
        new("claude-pro-detail", "Claude Pro：没有 Fable") { Selected = Provider.ClaudeCode, Quotas = DemoQuotas.ClaudePro },
        new("claude-pro-single", "只开 Claude Pro 一家") { Subscriptions = ["claudeCode"], Quotas = DemoQuotas.ClaudePro },

        // docs/SUBSCRIPTION_QUOTAS.md 「空态判定（不猜）」, row by row.
        State("win-empty-nowindow", "没有生效的额度窗口", "codex", b => b.Empty("codex", reason: "noWindow")),
        State("win-empty-notentitled", "未订阅 OpenCode Go", "opencode-go", b => b.Empty("opencode-go", reason: "notEntitled")),
        State("win-nonsub-kimi", "当前登录方式不含订阅额度", "kimi-code", b => b.Empty("kimi-code", reason: "sessionWithoutPlanLimits")),
        State("win-period-grok", "账期已结束 · 用一次 Grok 后更新", "grok", b => b.Empty("grok", reason: "periodEnded")),
        State("win-period-agy", "额度已重置 · 下次读取时更新", "antigravity", b => b.Empty("antigravity", reason: "periodEnded")),
        State("win-period-kimi", "上个周期已结束", "kimi-code", b => b.Empty("kimi-code", reason: "periodEnded")),
        State("win-norecord-grok", "暂无记录 · 用一次 Grok 后更新", "grok", b => b.Empty("grok", reason: "noLocalRecord")),
        State("win-norecord-agy", "用过 Antigravity 后显示", "antigravity", b => b.Empty("antigravity", reason: "noLocalRecord")),
        State("win-norecord-codex", "本机暂无记录", "codex", b => b.Empty("codex", reason: "noLocalRecord")),
        State("win-never-read", "尚未读取", "cursor", b => b.Bare("cursor")),
        State("win-unknown", "没读到额度，原因未知", "zcode", b => b.Empty("zcode")),
        State("win-not-installed", "未检测到安装", "kimi-code", b => b.Empty("kimi-code")) with { NotDetected = ["kimi-code"] },
        State("win-disabled", "订阅配额未启用", "codex", b => b.Empty("codex", status: "disabled")),

        // Sign-in and keys.
        State("win-zcode-needs-key", "ZCode 没配 Key：请在设置中配置 BigModel API Key", "zcode", b => b.Empty("zcode", status: "unauthorized", failure: "credentialsNotFound")),
        State("win-zcode-key-invalid", "ZCode Key 无效", "zcode", b => b.Empty("zcode", status: "unauthorized", failure: "credentialsRejected")),
        State("win-kimi-login", "请先登录 Kimi Code", "kimi-code", b => b.Empty("kimi-code", status: "unauthorized", failure: "credentialsNotFound")),
        State("win-agy-login", "请在 Antigravity 中登录", "antigravity", b => b.Empty("antigravity", status: "unauthorized", failure: "credentialsRejected")),
        State("win-opencode-missing", "OpenCode Go：在 OpenCode 里登录后自动显示", "opencode-go", b => b.Empty("opencode-go", failure: "credentialsNotFound")),
        State("win-opencode-expired", "OpenCode Go：打开 OpenCode 后自动显示", "opencode-go", b => b.Empty("opencode-go", failure: "credentialsExpired")),
        State("win-cursor-encrypted", "Cursor 登录信息已加密", "cursor", b => b.Empty("cursor", failure: "credentialsEncrypted")),
        State("win-cursor-refresh-failed", "Cursor 登录续期失败（可重试）", "cursor", b => b.Empty("cursor", status: "retryableError", failure: "loginRefreshFailed")),
        State("win-credential-store", "无法读取 Windows 凭据管理器", "zcode", b => b.Empty("zcode", status: "retryableError", failure: "credentialStoreUnavailable")),

        // Programs and reads.
        State("win-agy-not-installed", "请安装 agy 或 Antigravity", "antigravity", b => b.Empty("antigravity", failure: "programNotFound")),
        State("win-grok-local", "Grok 没有返回额度：本地日志的数", "grok", b => b.Local("grok")),
        State("win-node-missing", "缺少 Node.js（可重试）", "kimi-code", b => b.Empty("kimi-code", status: "retryableError", failure: "cliRuntimeMissing")),
        State("win-http-502", "接口报错 502（可重试）", "codex", b => b.Empty("codex", status: "retryableError", failure: "endpointHttpStatus", httpStatus: 502)),
        State("win-claude-not-found", "未找到 Claude Code", "claudeCode", b => b.Empty("claudeCode", failure: "claudeBinaryNotFound")),

        // Claude Desktop's record (Windows only).
        State("win-desktop-stale", "Claude 桌面端记录已过期", "claudeCode", b => b.Empty("claudeCode", failure: "desktopRecordStale", source: "desktopRecord")),
        State("win-desktop-multiple", "装了多份 Claude 桌面端", "claudeCode", b => b.Empty("claudeCode", failure: "desktopMultipleInstalls", source: "desktopRecord")),
        State("win-desktop-orgs", "Claude 桌面端记录涉及多个组织", "claudeCode", b => b.Empty("claudeCode", failure: "desktopMixedOrganizations", source: "desktopRecord")),
        State("win-desktop-unreadable", "Claude 桌面端记录无法解析", "claudeCode", b => b.Empty("claudeCode", failure: "desktopRecordUnreadable", source: "desktopRecord")),

        // The pauses (network-privacy rule 3).
        State("win-pause-hk", "网络出口在香港，已暂停读取", "codex", b => b.Empty("codex", failure: "egressRegionUnsupported", region: "HK")),
        State("win-pause-unconfirmed", "无法确认网络出口，已暂停读取", "claudeCode", b => b.Empty("claudeCode", failure: "egressUnconfirmed")),
        State("win-pause-noproxy", "未检测到代理，已暂停读取", "codex", b => b.Empty("codex", failure: "proxyNotDetected")),
        State("win-pause-proxy-unsupported", "代理无法照着走，已暂停读取", "cursor", b => b.Empty("cursor", failure: "proxyUnsupported")),
    ];
}

/// <summary>
/// The readings behind the scenes, after the Mac renderer's sample numbers
/// (131a: Codex 5h 12% / 7d 79%, Claude 5h 36% / 7d 31% / Fable 58%,
/// Cursor mo 41%, Grok 7d 88%, …). Windows are placed relative to the
/// clock, so pace, forecasts and resets read the same every time.
/// </summary>
public sealed class DemoReadings(DateTimeOffset now)
{
    private const double Hour = 3600;
    private double Now => now.ToUnixTimeMilliseconds() / 1000.0;

    /// <summary><paramref name="remaining"/> percent left; <paramref name="due"/> percent of the window still to go.</summary>
    public JsonObject Window(double remaining, double hours, double due) => new()
    {
        ["utilization"] = 100 - remaining,
        ["resetsAt"] = Now + hours * Hour * due / 100,
        ["windowDuration"] = hours * Hour,
    };

    public JsonObject Snapshot(string provider, JsonObject? fiveHour, JsonObject? sevenDay, string? plan = null)
    {
        var q = Bare(provider);
        q["status"] = new JsonObject { ["kind"] = "ok" };
        q["source"] = "live";
        q["fetchedAt"] = Now;
        if (fiveHour != null) q["fiveHour"] = fiveHour;
        if (sevenDay != null) q["sevenDay"] = sevenDay;
        if (plan != null) q["planLabel"] = plan;
        return q;
    }

    /// <summary>A snapshot never read: no time, no reason.</summary>
    public JsonObject Bare(string provider) => new()
    {
        ["provider"] = provider,
        ["status"] = new JsonObject { ["kind"] = "noData" },
    };

    public JsonObject Empty(string provider, string status = "noData", string? reason = null, string? failure = null,
        string? source = null, int? httpStatus = null, string? region = null)
    {
        var q = Bare(provider);
        q["status"] = new JsonObject { ["kind"] = status };
        q["fetchedAt"] = Now;
        if (reason != null) q["emptyReason"] = reason;
        if (source != null) q["source"] = source;
        if (failure != null)
        {
            var f = new JsonObject { ["code"] = failure };
            if (httpStatus != null) f["status"] = httpStatus;
            if (region != null) f["region"] = region;
            q["liveFailure"] = f;
        }
        return q;
    }

    /// <summary>Grok's week from its own log because the live read gave nothing (Windows: 「来源：本地日志 · 没有返回额度」).</summary>
    public JsonObject Local(string provider)
    {
        var q = Snapshot(provider, null, Window(88, 168, 71));
        q["source"] = "localLog";
        q["liveFailure"] = new JsonObject { ["code"] = "programNoAnswer" };
        return q;
    }

    private static JsonObject Meter(string id, string label, JsonObject window)
    {
        var m = new JsonObject { ["id"] = id, ["label"] = label };
        foreach (var (key, value) in window) m[key] = value?.DeepClone();
        return m;
    }

    private JsonObject Fable(double remaining, double due) => Meter("fable", "Fable", Window(remaining, 168, due));

    private static JsonArray Array(params JsonObject[] items) => new(items.Select(i => (JsonNode)i).ToArray());

    private JsonObject Codex()
    {
        var q = Snapshot("codex", Window(12, 5, 29), Window(79, 168, 53), "Plus");
        q["resetCreditsCount"] = 2;
        return q;
    }

    private JsonObject CodexNormal() => Snapshot("codex", Window(62, 5, 29), Window(79, 168, 53), "Plus");

    private JsonObject Claude()
    {
        var q = Snapshot("claudeCode", Window(36, 5, 16), Window(31, 168, 39), "Max");
        q["modelScopedMeters"] = Array(Fable(58, 39));
        return q;
    }

    private JsonObject ClaudeNormal() => Snapshot("claudeCode", Window(36, 5, 16), Window(53, 168, 39), "Max");

    private Dictionary<string, JsonObject> Design()
    {
        var cursor = Bare("cursor");
        cursor["status"] = new JsonObject { ["kind"] = "ok" };
        cursor["meters"] = Array(Meter("plan", "Month", Window(41, 720, 42)));
        cursor["planLabel"] = "Pro";
        cursor["source"] = "live";
        cursor["fetchedAt"] = Now;
        var opencode = Bare("opencode-go");
        opencode["status"] = new JsonObject { ["kind"] = "ok" };
        opencode["meters"] = Array(
            Meter("rolling", "5h", Window(64, 5, 16)),
            Meter("weekly", "Weekly", Window(48, 168, 42)),
            new JsonObject { ["id"] = "monthly", ["label"] = "Monthly", ["utilization"] = 78 });
        opencode["source"] = "live";
        opencode["fetchedAt"] = Now;
        return new()
        {
            ["codex"] = Codex(),
            ["claudeCode"] = Claude(),
            ["cursor"] = cursor,
            ["grok"] = Snapshot("grok", null, Window(88, 168, 71)),
            ["antigravity"] = Snapshot("antigravity", Window(9, 5, 22), Window(70, 168, 62)),
            ["kimi-code"] = Snapshot("kimi-code", Window(71, 5, 34), Window(40, 168, 52)),
            ["opencode-go"] = opencode,
            ["zcode"] = Snapshot("zcode", null, Window(72, 168, 67)),
        };
    }

    /// <summary>Every provider's reading for a scene (only the enabled ones are sent).</summary>
    public Dictionary<string, JsonObject> For(DemoScene scene)
    {
        var all = Design();
        void Set(JsonObject q) => all[(string)q["provider"]!] = q;
        foreach (var q in all.Values) q["fetchedAt"] ??= Now;
        switch (scene.Quotas)
        {
            case DemoQuotas.Normal:
                Set(ClaudeNormal());
                Set(CodexNormal());
                break;
            case DemoQuotas.CodexRedClaudeNormal:
                Set(ClaudeNormal());
                break;
            case DemoQuotas.ClaudeFastCodexNormal:
                Set(CodexNormal());
                break;
            case DemoQuotas.FiveCalm:
                Set(Snapshot("kimi-code", Window(71, 5, 34), Window(60, 168, 52)));
                break;
            case DemoQuotas.Special:
                Set(Snapshot("codex", Window(0, 5, 40), Window(79, 168, 53)));
                Set(Empty("claudeCode", status: "unauthorized", failure: "credentialsNotFound"));
                Set(Empty("cursor", status: "retryableError", failure: "endpointTimedOut"));
                Set(Empty("kimi-code", reason: "sessionWithoutPlanLimits"));
                break;
            case DemoQuotas.FableAlert:
            {
                var claude = Snapshot("claudeCode", Window(36, 5, 16), Window(53, 168, 39), "Max");
                claude["modelScopedMeters"] = Array(Fable(9, 61));
                Set(claude);
                break;
            }
            case DemoQuotas.ClaudeExtra:
            {
                // The Mac's pay-as-you-go spend (18 of 100) is a fourth row 「额外 82%」.
                // Windows' Claude has no such field; the same row comes from an on-demand meter.
                var claude = Claude();
                claude["meters"] = Array(new JsonObject { ["id"] = "on-demand", ["label"] = "额外", ["utilization"] = 18 });
                Set(claude);
                break;
            }
            case DemoQuotas.CursorLoading:
                Set(Bare("cursor"));
                break;
            case DemoQuotas.CursorReadFailed:
            {
                var cursor = Empty("cursor", status: "retryableError", failure: "endpointTimedOut");
                cursor["planLabel"] = "Pro";
                Set(cursor);
                break;
            }
            case DemoQuotas.ClaudeLoginExpired:
            {
                var claude = Empty("claudeCode", status: "unauthorized", failure: "credentialsRejected");
                claude["planLabel"] = "Max";
                Set(claude);
                break;
            }
            case DemoQuotas.CursorNotEntitled:
                Set(Empty("cursor", reason: "notEntitled"));
                break;
            case DemoQuotas.ClaudeStale:
            {
                var claude = Claude();
                claude.Remove("modelScopedMeters");
                claude["source"] = "cache";
                claude["liveFailure"] = new JsonObject { ["code"] = "claudeTimedOut" };
                Set(claude);
                break;
            }
            case DemoQuotas.Dots:
                Set(Empty("claudeCode", status: "unauthorized", failure: "credentialsNotFound"));
                Set(Empty("cursor", status: "retryableError", failure: "endpointTimedOut"));
                break;
            case DemoQuotas.CursorSignIn:
            {
                var cursor = Empty("cursor");
                cursor["signIn"] = "required";
                Set(cursor);
                break;
            }
            case DemoQuotas.CursorSignInExpired:
            {
                var cursor = Empty("cursor");
                cursor["signIn"] = "expired";
                Set(cursor);
                break;
            }
            case DemoQuotas.CodexPausedKept:
            {
                var codex = Codex();
                codex["source"] = "cache";
                codex["liveFailure"] = new JsonObject { ["code"] = "egressRegionUnsupported", ["region"] = "CN" };
                Set(codex);
                break;
            }
            case DemoQuotas.CodexPausedEmpty:
                Set(Empty("codex", failure: "egressRegionUnsupported", region: "CN"));
                break;
            case DemoQuotas.ClaudePolicy:
                Set(Empty("claudeCode", reason: "nonessentialTrafficDisabled"));
                break;
            case DemoQuotas.ClaudePolicyKept:
            {
                // Windows reads Claude Desktop's record (no network) and shows it as such:
                // the record's note and time, no policy line (the Mac adds the policy reason).
                var claude = Snapshot("claudeCode", new JsonObject { ["utilization"] = 64 }, new JsonObject { ["utilization"] = 69 }, "Max");
                claude["source"] = "desktopRecord";
                claude["dataAsOf"] = Now - Hour;
                Set(claude);
                break;
            }
            case DemoQuotas.ClaudeNonSubscription:
                Set(Empty("claudeCode", reason: "sessionWithoutPlanLimits"));
                break;
            case DemoQuotas.CodexExhaustedNoWindow:
                Set(Empty("codex", reason: "limitReached"));
                break;
            case DemoQuotas.GrokExpired:
                Set(Snapshot("grok", null, new JsonObject { ["utilization"] = 40, ["resetsAt"] = Now - Hour, ["windowDuration"] = 168 * Hour }));
                break;
            case DemoQuotas.ClaudeWeeklyFable:
            {
                var claude = Snapshot("claudeCode", null, Window(62, 168, 55), "Max");
                claude["modelScopedMeters"] = Array(Fable(41, 55));
                Set(claude);
                break;
            }
            case DemoQuotas.ClaudePro:
                Set(Snapshot("claudeCode", Window(58, 5, 40), Window(74, 168, 60), "Pro"));
                break;
            case DemoQuotas.AgyCrowded:
                Set(Snapshot("codex", null, Window(53, 168, 64), "Pro"));
                Set(Snapshot("claudeCode", Window(99, 5, 88), Window(39, 168, 21), "Max"));
                Set(Snapshot("antigravity", new JsonObject { ["utilization"] = 0 }, Window(100, 168, 91)));
                break;
            case DemoQuotas.Custom when scene.Reading is { } reading:
                Set(reading.Read(this));
                break;
        }
        return all;
    }
}
