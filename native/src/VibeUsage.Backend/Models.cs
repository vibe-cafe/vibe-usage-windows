using System.Text.Json.Serialization;

namespace VibeUsage.Backend;

public sealed record HostOptions(string RoamingConfig, string LocalData, string Resources, string Version, bool Autostart);
public sealed record BackendError(string Code, string Message);
public sealed class BackendException(BackendError error) : Exception(error.Message)
{
    public string Code { get; } = error.Code;
}

public enum Provider
{
    [JsonStringEnumMemberName("codex")] Codex,
    [JsonStringEnumMemberName("claudeCode")] ClaudeCode,
    [JsonStringEnumMemberName("cursor")] Cursor,
    [JsonStringEnumMemberName("kimi-code")] KimiCode,
    [JsonStringEnumMemberName("grok")] Grok,
    [JsonStringEnumMemberName("opencode-go")] OpenCodeGo,
    [JsonStringEnumMemberName("zcode")] ZCode,
    [JsonStringEnumMemberName("antigravity")] Antigravity
}
public enum ZCodeRegion
{
    [JsonStringEnumMemberName("bigModel")] BigModel,
    [JsonStringEnumMemberName("zAI")] ZAi
}
public enum RootSource
{
    [JsonStringEnumMemberName("codex")] Codex,
    [JsonStringEnumMemberName("grok")] Grok,
    [JsonStringEnumMemberName("antigravity")] Antigravity
}
public enum EgressMode
{
    [JsonStringEnumMemberName("auto")] Auto,
    [JsonStringEnumMemberName("pause")] Pause,
    [JsonStringEnumMemberName("allow")] Allow
}

public sealed record AppStatus(bool Configured, string? Hostname, string ApiUrl, string Version,
    bool IsDev, bool RuntimeAvailable, bool TestDiagnosticsAvailable, bool UpdatesAvailable,
    string? ApiKeyDisplay = null);
public sealed record AppSettings(bool ShowCostInTray, bool ShowTokensInTray, bool CodexRateLimitEnabled,
    bool ClaudeRateLimitEnabled, Provider[] SelectedQuotaProductIds, Provider[] QuotaProductOrder,
    Provider? QuotaSelectedTabId, bool QuotaSelectionInitialized, ZCodeRegion ZCodeQuotaRegion,
    EgressMode EgressMode, ulong? LastSuccessfulSyncAt = null);
public sealed record DeviceCode(string UserCode);
public sealed record DeviceLinkState(string Status, string? Message = null);
public sealed record CursorLogin(string State, bool? Supported, string? Email = null, string? Error = null);
public sealed record CredentialStatus(bool BigModelConfigured, bool ZAiConfigured);
public sealed record SyncState(string Status, string? Message = null, ulong? LastSyncAt = null);
public sealed record SyncReason(string Code, string? Stage = null, int? Status = null, int? ExitCode = null);
public sealed record SyncRecord(ulong StartedAt, string Trigger, string Outcome, ulong DurationMs,
    SyncReason? Reason = null, string? CliFallback = null);
public sealed record UpdateInfo(string Version, string Url, string? Notes = null, string? Sha256 = null);
public sealed record ExtraRoots(string[]? Codex = null, string[]? Grok = null, string[]? Antigravity = null);

[JsonPolymorphic(TypeDiscriminatorPropertyName = "kind")]
[JsonDerivedType(typeof(UsageQuery.Days), "days")]
[JsonDerivedType(typeof(UsageQuery.From), "from")]
[JsonDerivedType(typeof(UsageQuery.Custom), "custom")]
public abstract record UsageQuery
{
    public sealed record Days([property: JsonPropertyName("days")] uint Count) : UsageQuery;
    public sealed record From(string FromIso) : UsageQuery;
    public sealed record Custom(string FromIso, string ToIso, string Tz) : UsageQuery;
}
public sealed record UsageBucket(string Source, string Model, string Project, string Hostname,
    string BucketStart, long InputTokens, long OutputTokens, long CachedInputTokens,
    long ReasoningOutputTokens, long TotalTokens, double? EstimatedCost = null,
    long? CacheCreationInputTokens = null, long? CacheCreation5mTokens = null, long? CacheCreation1hTokens = null);
public sealed record UsageSession(string Source, string Project, string Hostname, string FirstMessageAt,
    string LastMessageAt, double DurationSeconds, double ActiveSeconds, long MessageCount, long UserMessageCount);
public sealed record UsageFamily(string Key, string Label, string Provider);
public sealed record UsageNames(Dictionary<string,string> Sources, Dictionary<string,string> Models,
    Dictionary<string,string> ModelFamilies, UsageFamily[] Families);
public sealed record UsageData(UsageBucket[] Buckets, bool HasAnyData, UsageSession[]? Sessions = null,
    UsageNames? Names = null);
public sealed record UsageSnapshot(UsageQuery Query, UsageData Data);
public sealed record UsageError(UsageQuery Query, string Error);

public sealed record QuotaProduct(Provider Provider, string DisplayName, string Availability, bool IsDetected);
public sealed record QuotaStatus(string Kind, string? Message = null);
public sealed record QuotaFailure(string Code, int? Status = null, string? Region = null);
public sealed record QuotaWindow(double Utilization, double? ResetsAt = null, double? WindowDuration = null);
public sealed record MeterPart(string Label, double Utilization);
public sealed record QuotaMeter(string Id, string Label, double Utilization, double? ResetsAt = null,
    double? WindowDuration = null, MeterPart[]? Breakdown = null);
public sealed record Quota(Provider Provider, QuotaStatus Status, QuotaWindow? FiveHour = null,
    QuotaWindow? SevenDay = null, QuotaMeter[]? Meters = null, QuotaMeter[]? ModelScopedMeters = null,
    string? SourceLabel = null, string? Source = null, QuotaFailure? LiveFailure = null,
    string? EmptyReason = null, string? PlanLabel = null, double? DataAsOf = null,
    double? FetchedAt = null, bool FiveHourNotEnforced = false, ulong? ResetCreditsCount = null,
    string? SignIn = null);
public sealed record QuotaCache(Quota[] Snapshots, bool Refreshing, Provider[] Providers);
