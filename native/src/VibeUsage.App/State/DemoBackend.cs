using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using VibeUsage.App.Shell;
using VibeUsage.Backend;

namespace VibeUsage.App.State;

/// <summary>
/// The in-memory fake backend with made-up data in the shape of the Mac
/// design's sample (Codex, Claude, Cursor, Grok; $41.20 and 65.5M today).
/// Nothing here touches the network, the disk or credentials. Until the
/// vertical slice (#94) wires <see cref="NativeBackend"/>, the shell always
/// runs on this; afterwards CI screenshots keep using it.
/// </summary>
internal static class DemoBackend
{
    public static FakeBackend Create(DemoScenario scenario, SynchronizationContext context)
    {
        var fake = new FakeBackend(context);
        var data = new DemoData(scenario, DateTimeOffset.Now);
        fake.Respond = (name, args) => Task.FromResult(data.Answer(fake, name, args));
        return fake;
    }
}

/// <summary>Answers for the commands the shell sends; the rest fall back to the shared contract samples.</summary>
internal sealed class DemoData(DemoScenario scenario, DateTimeOffset now)
{
    private const double Hour = 3600, Day = 86400;
    private DateTimeOffset lastSync = now.AddMinutes(-26);

    public static readonly string[] Subscriptions = ["codex", "claudeCode", "cursor", "grok"];
    public const double TotalCost = 41.20;
    public const long TotalTokens = 65_500_000;

    private string[] Enabled => scenario == DemoScenario.Single ? ["claudeCode"] : Subscriptions;

    public JsonElement Answer(FakeBackend fake, string name, JsonElement args)
    {
        switch (name)
        {
            case "get_app_status":
                return Element(new JsonObject
                {
                    ["configured"] = true,
                    ["hostname"] = "demo",
                    ["apiUrl"] = "https://example.invalid",
                    ["version"] = "0.8.0",
                    ["isDev"] = false,
                    ["runtimeAvailable"] = true,
                    ["testDiagnosticsAvailable"] = false,
                    ["updatesAvailable"] = true,
                });
            case "get_settings":
                return Element(Settings());
            case "get_cached_rate_limits":
                return Element(new JsonObject { ["snapshots"] = Quotas(), ["refreshing"] = false, ["providers"] = new JsonArray() });
            case "get_rate_limits":
                return Element(Quotas());
            case "cached_usage":
                return Element(new JsonObject { ["query"] = TodayQuery(), ["data"] = Usage() });
            case "fetch_usage":
                return Element(Usage());
            case "get_sync_state":
                return Element(SyncState("idle"));
            case "get_update_info":
                return Element(null);
            case "trigger_sync":
                fake.Publish(new BackendEvent.SyncChanged(new SyncState("syncing")));
                _ = FinishSync(fake);
                return Element(null);
            case "panel_opened":
            case "settings_shown":
            case "open_external":
                return Element(null);
            default:
                return Fixture(name);
        }
    }

    private async Task FinishSync(FakeBackend fake)
    {
        await Task.Delay(1500).ConfigureAwait(false);
        lastSync = DateTimeOffset.Now;
        try
        {
            fake.Publish(new BackendEvent.SyncChanged(new SyncState("success", null, (ulong)lastSync.ToUnixTimeMilliseconds())));
        }
        catch (ObjectDisposedException)
        {
            // The app quit during the pretend sync.
        }
    }

    private JsonObject SyncState(string status) => new()
    {
        ["status"] = status,
        ["lastSyncAt"] = lastSync.ToUnixTimeMilliseconds(),
    };

    private JsonObject Settings() => new()
    {
        ["showCostInTray"] = true,
        ["showTokensInTray"] = false,
        ["codexRateLimitEnabled"] = true,
        ["claudeRateLimitEnabled"] = true,
        ["selectedQuotaProductIds"] = Strings(Enabled),
        ["quotaProductOrder"] = Strings(Enabled),
        ["quotaSelectedTabId"] = null,
        ["quotaSelectionInitialized"] = true,
        ["zCodeQuotaRegion"] = "bigModel",
        ["egressMode"] = "auto",
        ["lastSuccessfulSyncAt"] = lastSync.ToUnixTimeMilliseconds(),
    };

    /// <summary>
    /// The design's sample: Codex 5h 12% / 7d 79% running 17% ahead, Claude
    /// 5h 36% / 7d 31% 8% ahead, Cursor's month 41% on pace, Grok's week 88%
    /// with 17% to spare. A window used <c>u</c>% and running <c>lag</c>
    /// points ahead of time has <c>(u − lag)</c>% of its length behind it.
    /// </summary>
    private JsonArray Quotas()
    {
        var all = new Dictionary<string, JsonObject>
        {
            ["codex"] = Quota("codex", fiveHour: Window(88, 17, 5 * Hour), sevenDay: Window(21, -9, 7 * Day)),
            ["claudeCode"] = Quota("claudeCode", fiveHour: Window(64, 8, 5 * Hour), sevenDay: Window(69, 4, 7 * Day)),
            ["cursor"] = Quota("cursor", meters: [Meter("plan", "Monthly", Window(59, 2, 30 * Day))]),
            ["grok"] = Quota("grok", sevenDay: Window(12, -17, 7 * Day)),
        };
        var list = new JsonArray();
        foreach (var id in Enabled) list.Add((JsonNode)all[id]);
        return list;
    }

    private JsonObject Quota(string provider, JsonObject? fiveHour = null, JsonObject? sevenDay = null, JsonObject[]? meters = null)
    {
        var q = new JsonObject
        {
            ["provider"] = provider,
            ["status"] = new JsonObject { ["kind"] = "ok" },
            ["fetchedAt"] = now.ToUnixTimeMilliseconds() / 1000.0,
        };
        if (fiveHour != null) q["fiveHour"] = fiveHour;
        if (sevenDay != null) q["sevenDay"] = sevenDay;
        if (meters != null) q["meters"] = new JsonArray(meters.Select(m => (JsonNode)m).ToArray());
        return q;
    }

    private JsonObject Window(double used, double lag, double length)
    {
        double elapsed = (used - lag) / 100 * length;
        return new JsonObject
        {
            ["utilization"] = used,
            ["resetsAt"] = now.ToUnixTimeMilliseconds() / 1000.0 + (length - elapsed),
            ["windowDuration"] = length,
        };
    }

    private static JsonObject Meter(string id, string label, JsonObject window)
    {
        var m = new JsonObject { ["id"] = id, ["label"] = label };
        foreach (var (key, value) in window) m[key] = value?.DeepClone();
        return m;
    }

    private JsonObject TodayQuery()
    {
        var midnight = new DateTimeOffset(now.Date, now.Offset);
        return new JsonObject
        {
            ["kind"] = "from",
            ["fromIso"] = midnight.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", CultureInfo.InvariantCulture),
        };
    }

    /// <summary>
    /// Today's usage, spread over the last hours (never before local
    /// midnight), summing to the design's $41.20 and 65.5M tokens.
    /// </summary>
    private JsonObject Usage()
    {
        (string Source, string Model, string Host, double Share)[] mix =
        [
            ("claude-code", "claude-opus-4-5", "DESKTOP-VIBE", 0.34),
            ("claude-code", "claude-sonnet-4-5", "WORK-LAPTOP", 0.12),
            ("codex", "gpt-5.1-codex", "DESKTOP-VIBE", 0.26),
            ("codex", "gpt-5.1-codex-mini", "WORK-LAPTOP", 0.08),
            ("cursor", "composer-1", "DESKTOP-VIBE", 0.14),
            ("grok", "grok-code-fast-1", "WORK-LAPTOP", 0.06),
        ];
        double[] hourWeights = [0.10, 0.18, 0.16, 0.06, 0.22, 0.28];
        var midnight = new DateTimeOffset(now.Date, now.Offset);
        var currentHour = new DateTimeOffset(now.Year, now.Month, now.Day, now.Hour, 0, 0, now.Offset);
        var buckets = new JsonArray();
        long tokensLeft = TotalTokens;
        long centsLeft = (long)Math.Round(TotalCost * 100);
        int count = mix.Length * hourWeights.Length, index = 0;
        foreach (var (source, model, host, share) in mix)
        {
            for (int h = 0; h < hourWeights.Length; h++)
            {
                index++;
                var start = currentHour.AddHours(h - (hourWeights.Length - 1));
                if (start < midnight) start = midnight;
                double weight = share * hourWeights[h];
                long tokens = index == count ? tokensLeft : (long)Math.Round(TotalTokens * weight);
                long cents = index == count ? centsLeft : (long)Math.Round(TotalCost * 100 * weight);
                tokensLeft -= tokens;
                centsLeft -= cents;
                long cached = tokens * 70 / 100, output = tokens * 5 / 100, input = tokens - cached - output;
                buckets.Add((JsonNode)new JsonObject
                {
                    ["source"] = source,
                    ["model"] = model,
                    ["project"] = "demo",
                    ["hostname"] = host,
                    ["bucketStart"] = start.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", CultureInfo.InvariantCulture),
                    ["inputTokens"] = input,
                    ["outputTokens"] = output,
                    ["cachedInputTokens"] = cached,
                    ["reasoningOutputTokens"] = 0,
                    ["totalTokens"] = tokens,
                    ["estimatedCost"] = cents / 100.0,
                });
            }
        }
        return new JsonObject { ["buckets"] = buckets, ["hasAnyData"] = true };
    }

    private static JsonArray Strings(IEnumerable<string> values) => new(values.Select(v => (JsonNode)JsonValue.Create(v)!).ToArray());

    private static JsonElement Element(JsonNode? node)
    {
        using var doc = JsonDocument.Parse(node?.ToJsonString() ?? "null");
        return doc.RootElement.Clone();
    }

    /// <summary>The shared contract sample's success value (embedded in the backend library).</summary>
    private static JsonElement Fixture(string name)
    {
        using Stream? stream = typeof(FakeBackend).Assembly.GetManifestResourceStream($"Fixtures.{name}.json");
        if (stream == null) throw new BackendException(new("unknownCommand", "后端不支持这个命令"));
        using var doc = JsonDocument.Parse(stream);
        return doc.RootElement.GetProperty("value").Clone();
    }
}
