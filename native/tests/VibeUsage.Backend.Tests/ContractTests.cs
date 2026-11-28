using System.Text.Json;

namespace VibeUsage.Backend.Tests;

public class ContractTests
{
    private static string Contract(string folder) => Path.Combine(AppContext.BaseDirectory, "contract", folder);
    private static JsonElement Load(string path)
    {
        using var doc = JsonDocument.Parse(File.ReadAllBytes(path));
        return doc.RootElement.Clone();
    }

    private sealed class Probe(string expectedName, JsonElement sample) : BackendClient(null)
    {
        internal override Task<JsonElement> SendAsync(string name, JsonElement args)
        {
            Assert.Equal(expectedName, name);
            Assert.True(JsonElement.DeepEquals(sample.GetProperty("args"), args), $"{name} 参数不符");
            return Task.FromResult(sample.GetProperty("value"));
        }
        protected override void CloseTransport() { }
    }

    // Optional wire fields omitted by Rust may be present with their defaults
    // after a .NET round trip. Every field actually in a fixture must survive.
    private static void CheckFields(JsonElement expected, JsonElement actual)
    {
        Assert.Equal(expected.ValueKind, actual.ValueKind);
        switch (expected.ValueKind)
        {
            case JsonValueKind.Object:
                foreach (var field in expected.EnumerateObject())
                    CheckFields(field.Value, actual.GetProperty(field.Name));
                break;
            case JsonValueKind.Array:
                Assert.Equal(expected.GetArrayLength(), actual.GetArrayLength());
                for (int i = 0; i < expected.GetArrayLength(); i++) CheckFields(expected[i], actual[i]);
                break;
            default: Assert.True(JsonElement.DeepEquals(expected, actual)); break;
        }
    }

    private static async Task<JsonElement> Result<T>(Task<T> result) => Wire.Element(await result);
    private static async Task<JsonElement> Result(Task result)
    {
        await result;
        return Wire.Element<UpdateInfo?>(null);
    }

    public static IEnumerable<object[]> Commands() => Directory.GetFiles(Contract("commands"), "*.json")
        .Select(path => new object[] { Path.GetFileNameWithoutExtension(path) });

    [Theory]
    [MemberData(nameof(Commands))]
    public async Task Typed_method_writes_arguments_and_reads_result(string name)
    {
        var sample = Load(Path.Combine(Contract("commands"), name + ".json"));
        using IBackend app = new Probe(name, sample);
        var result = await (name switch
        {
            "get_app_status" => Result(app.GetAppStatusAsync()),
            "fetch_usage" => Result(app.FetchUsageAsync(new UsageQuery.Days(1))),
            "cached_usage" => Result(app.CachedUsageAsync()),
            "panel_opened" => Result(app.PanelOpenedAsync(new UsageQuery.Days(1))),
            "settings_shown" => Result(app.SettingsShownAsync()),
            "start_device_link" => Result(app.StartDeviceLinkAsync()),
            "cancel_device_link" => Result(app.CancelDeviceLinkAsync()),
            "trigger_sync" => Result(app.TriggerSyncAsync()),
            "collect_diagnostics" => Result(app.CollectDiagnosticsAsync()),
            "get_sync_state" => Result(app.GetSyncStateAsync()),
            "get_rate_limits" => Result(app.GetRateLimitsAsync(true)),
            "get_cached_rate_limits" => Result(app.GetCachedRateLimitsAsync()),
            "get_quota_products" => Result(app.GetQuotaProductsAsync()),
            "set_quota_product_selected" => Result(app.SetQuotaProductSelectedAsync(Provider.OpenCodeGo, true)),
            "get_zcode_credential_status" => Result(app.GetZCodeCredentialStatusAsync()),
            "set_zcode_quota_region" => Result(app.SetZCodeQuotaRegionAsync(ZCodeRegion.ZAi)),
            "set_zcode_api_key" => Result(app.SetZCodeApiKeyAsync(ZCodeRegion.BigModel, "synthetic-only")),
            "get_cursor_login" => Result(app.GetCursorLoginAsync()),
            "start_cursor_login" => Result(app.StartCursorLoginAsync()),
            "cancel_cursor_login" => Result(app.CancelCursorLoginAsync()),
            "logout_cursor" => Result(app.LogoutCursorAsync()),
            "get_settings" => Result(app.GetSettingsAsync()),
            "update_setting" => Result(app.UpdateSettingAsync(SettingChange.ShowCostInTray(false))),
            "get_sync_history" => Result(app.GetSyncHistoryAsync()),
            "get_launch_at_login" => Result(app.GetLaunchAtLoginAsync()),
            "set_launch_at_login" => Result(app.SetLaunchAtLoginAsync(true)),
            "get_extra_roots" => Result(app.GetExtraRootsAsync()),
            "add_extra_root" => Result(app.AddExtraRootAsync(RootSource.Codex, "fixture/codex")),
            "remove_extra_root" => Result(app.RemoveExtraRootAsync(RootSource.Codex, "fixture/codex")),
            "reset_config" => Result(app.ResetConfigAsync()),
            "open_external" => Result(app.OpenExternalAsync("https://example.invalid")),
            "export_test_diagnostics" => Result(app.ExportTestDiagnosticsAsync("fixture/report.txt")),
            "check_for_update" => Result(app.CheckForUpdateAsync()),
            "get_update_info" => Result(app.GetUpdateInfoAsync()),
            "install_update" => Result(app.InstallUpdateAsync()),
            _ => throw new InvalidDataException(name)
        });
        CheckFields(sample.GetProperty("value"), result);
    }

    [Fact]
    public void Every_event_decodes_its_typed_payload()
    {
        var types = new HashSet<Type>();
        foreach (var path in Directory.GetFiles(Contract("events"), "*.json"))
        {
            var sample = Load(path);
            var payload = sample.GetProperty("payload");
            var evt = BackendEvent.Parse(sample.GetProperty("name").GetString()!, payload);
            var encoded = evt switch
            {
                BackendEvent.DeviceLink item => Wire.Element(item.Value),
                BackendEvent.SyncChanged item => Wire.Element(item.Value),
                BackendEvent.UsageChanged item => Wire.Element(item.Value),
                BackendEvent.UsageFailed item => Wire.Element(item.Value),
                BackendEvent.SettingsChanged item => Wire.Element(item.Value),
                BackendEvent.QuotasChanged item => Wire.Element(item.Value),
                BackendEvent.CursorLoginUpdated item => Wire.Element(item.Value),
                BackendEvent.CursorLoginChanged => Wire.Element<UpdateInfo?>(null),
                BackendEvent.UpdateAvailable item => Wire.Element(item.Value),
                BackendEvent.UpdateCleared => Wire.Element<UpdateInfo?>(null),
                BackendEvent.ExitRequested => Wire.Element<UpdateInfo?>(null),
                _ => throw new InvalidDataException()
            };
            CheckFields(payload, encoded);
            Assert.True(types.Add(evt.GetType()));
        }
        Assert.Equal(typeof(BackendEvent).GetNestedTypes().Length, types.Count);
    }
}
