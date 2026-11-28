using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.Json.Serialization.Metadata;

namespace VibeUsage.Backend;

internal sealed record QueryArgs(UsageQuery Query);
internal sealed record RefreshArgs(bool Force);
internal sealed record SelectionArgs(Provider Provider, bool Selected);
internal sealed record RegionArgs(ZCodeRegion Region);
internal sealed record KeyArgs(ZCodeRegion Region, string? ApiKey);
internal sealed record ChangeArgs(Dictionary<string, JsonElement> Change);
internal sealed record LaunchArgs(bool Enabled);
internal sealed record RootArgs(RootSource Source, string Path);
internal sealed record UrlArgs(string Url);
internal sealed record DestinationArgs(string Destination);

[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    RespectRequiredConstructorParameters = true, RespectNullableAnnotations = true,
    AllowOutOfOrderMetadataProperties = true, UseStringEnumConverter = true)]
[JsonSerializable(typeof(QueryArgs))]
[JsonSerializable(typeof(RefreshArgs))]
[JsonSerializable(typeof(SelectionArgs))]
[JsonSerializable(typeof(RegionArgs))]
[JsonSerializable(typeof(KeyArgs))]
[JsonSerializable(typeof(ChangeArgs))]
[JsonSerializable(typeof(LaunchArgs))]
[JsonSerializable(typeof(RootArgs))]
[JsonSerializable(typeof(UrlArgs))]
[JsonSerializable(typeof(DestinationArgs))]
[JsonSerializable(typeof(AppStatus))]
[JsonSerializable(typeof(UsageData))]
[JsonSerializable(typeof(UsageSnapshot))]
[JsonSerializable(typeof(DeviceCode))]
[JsonSerializable(typeof(string))]
[JsonSerializable(typeof(SyncState))]
[JsonSerializable(typeof(Quota[]))]
[JsonSerializable(typeof(QuotaCache))]
[JsonSerializable(typeof(QuotaProduct[]))]
[JsonSerializable(typeof(CredentialStatus))]
[JsonSerializable(typeof(CursorLogin))]
[JsonSerializable(typeof(AppSettings))]
[JsonSerializable(typeof(SyncRecord[]))]
[JsonSerializable(typeof(bool))]
[JsonSerializable(typeof(ExtraRoots))]
[JsonSerializable(typeof(UpdateInfo))]
[JsonSerializable(typeof(DeviceLinkState))]
[JsonSerializable(typeof(UsageError))]
[JsonSerializable(typeof(HostOptions))]
[JsonSerializable(typeof(BackendError))]
[JsonSerializable(typeof(Provider?))]
[JsonSerializable(typeof(EgressMode))]
[JsonSerializable(typeof(Dictionary<string, JsonElement>))]
[JsonSerializable(typeof(Provider[]))]
[JsonSerializable(typeof(JsonElement))]
internal partial class ContractJson : JsonSerializerContext;

internal static class Wire
{
    internal static readonly JsonElement Empty = Element(new Dictionary<string, JsonElement>());
    internal static JsonTypeInfo<T> Type<T>() => (JsonTypeInfo<T>)ContractJson.Default.GetTypeInfo(typeof(T))!;
    internal static JsonElement Element<T>(T value) => JsonSerializer.SerializeToElement(value, Type<T>());
    internal static T Read<T>(JsonElement value) => value.Deserialize(Type<T>())!;
}

public sealed class SettingChange
{
    internal Dictionary<string, JsonElement> Fields { get; }
    private SettingChange(string name, JsonElement value) => Fields = new() { [name] = value };
    public static SettingChange ShowCostInTray(bool value) => new("showCostInTray", Wire.Element(value));
    public static SettingChange ShowTokensInTray(bool value) => new("showTokensInTray", Wire.Element(value));
    public static SettingChange EgressMode(EgressMode value) => new("egressMode", Wire.Element(value));
    public static SettingChange QuotaProductOrder(Provider[] value) => new("quotaProductOrder", Wire.Element(value));
    public static SettingChange QuotaSelectedTabId(Provider? value) => new("quotaSelectedTabId", Wire.Element(value));
}
