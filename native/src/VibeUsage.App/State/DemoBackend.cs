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
/// The in-memory fake backend with made-up data: the quota readings of the
/// Mac panel renderer's scene (<see cref="DemoScenes"/>; 131a by default:
/// Claude, Codex, Cursor, Grok) and today's usage ($41.20 and 65.5M).
/// 「刷新额度」 pretends to read for a moment; dragging a round button saves
/// the order in memory. Nothing here touches the network, the disk or
/// credentials. Until the vertical slice (#94) wires
/// <see cref="NativeBackend"/>, the shell always runs on this; afterwards CI
/// screenshots keep using it.
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
internal sealed class DemoData
{
    private readonly DemoScene scene;
    private readonly DateTimeOffset now;
    private readonly DemoReadings readings;
    private readonly object gate = new();
    private DateTimeOffset lastSync;
    private string[] order;
    private bool refreshing;

    public const double TotalCost = 41.20;
    public const long TotalTokens = 65_500_000;

    public DemoData(DemoScenario scenario, DateTimeOffset now)
    {
        scene = DemoScenes.For(scenario);
        this.now = now;
        readings = new DemoReadings(now);
        lastSync = now.AddMinutes(-26);
        order = scene.Subscriptions;
    }

    private string[] Enabled => scene.Subscriptions;

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
                lock (gate)
                {
                    string[] reading = refreshing ? Enabled : scene.Refreshing;
                    return Element(Cache(reading));
                }
            case "get_rate_limits":
                StartRefresh(fake);
                return Element(Quotas());
            case "get_quota_products":
                return Element(Products());
            case "update_setting":
                return Element(UpdateSetting(fake, args));
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

    private JsonObject Cache(string[] reading) => new()
    {
        ["snapshots"] = Quotas(),
        ["refreshing"] = reading.Length > 0,
        ["providers"] = Strings(reading),
    };

    /// <summary>「刷新额度」: every enabled subscription reads for a moment, then the same numbers come back.</summary>
    private void StartRefresh(FakeBackend fake)
    {
        lock (gate)
        {
            if (refreshing) return;
            refreshing = true;
        }
        Publish(fake, Cache(Enabled));
        _ = FinishRefresh(fake);
    }

    private async Task FinishRefresh(FakeBackend fake)
    {
        await Task.Delay(2500).ConfigureAwait(false);
        lock (gate) refreshing = false;
        Publish(fake, Cache([]));
    }

    private static void Publish(FakeBackend fake, JsonObject cache)
    {
        try
        {
            fake.Publish(new BackendEvent.QuotasChanged(Typed(cache, b => b.GetCachedRateLimitsAsync())));
        }
        catch (ObjectDisposedException)
        {
            // The app quit during the pretend read.
        }
    }

    /// <summary>The product catalogue: all eight, detected unless the scene says otherwise.</summary>
    private JsonArray Products()
    {
        (string Id, string Name)[] catalog =
        [
            ("codex", "Codex"), ("claudeCode", "Claude"), ("cursor", "Cursor"), ("kimi-code", "Kimi Code"),
            ("grok", "Grok"), ("opencode-go", "OpenCode Go"), ("zcode", "ZCode"), ("antigravity", "Antigravity / AGY"),
        ];
        return new JsonArray(catalog.Select(p => (JsonNode)new JsonObject
        {
            ["provider"] = p.Id,
            ["displayName"] = p.Name,
            ["availability"] = "ready",
            ["isDetected"] = !scene.NotDetected.Contains(p.Id),
        }).ToArray());
    }

    /// <summary>The only setting the panel writes is the subscriptions' order (dragging a round button); kept in memory.</summary>
    private JsonObject UpdateSetting(FakeBackend fake, JsonElement args)
    {
        if (args.TryGetProperty("change", out var change) && change.TryGetProperty("quotaProductOrder", out var list))
        {
            lock (gate) order = list.EnumerateArray().Select(e => e.GetString() ?? "").Where(e => e.Length > 0).ToArray();
        }
        var settings = Settings();
        try
        {
            fake.Publish(new BackendEvent.SettingsChanged(Typed(settings, b => b.GetSettingsAsync())));
        }
        catch (ObjectDisposedException)
        {
        }
        return settings;
    }

    /// <summary>A command answer as the typed record the events carry, read through the backend library's own contract.</summary>
    private static T Typed<T>(JsonObject json, Func<FakeBackend, Task<T>> read)
    {
        using var parser = new FakeBackend(new SynchronizationContext());
        var element = Element(json);
        parser.Respond = (_, _) => Task.FromResult(element);
        return read(parser).GetAwaiter().GetResult();
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
        ["quotaProductOrder"] = Strings(order),
        ["quotaSelectedTabId"] = null,
        ["quotaSelectionInitialized"] = true,
        ["zCodeQuotaRegion"] = "bigModel",
        ["egressMode"] = "auto",
        ["lastSuccessfulSyncAt"] = lastSync.ToUnixTimeMilliseconds(),
    };

    /// <summary>The scene's readings of the enabled subscriptions.</summary>
    private JsonArray Quotas()
    {
        var all = readings.For(scene);
        return new JsonArray(Enabled.Select(id => (JsonNode)all[id].DeepClone()).ToArray());
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
