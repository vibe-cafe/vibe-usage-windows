// Data models — 1:1 port of VibeUsage/Models/*.swift

/** Mirrors UsageBucket.swift */
export interface UsageBucket {
  source: string;
  model: string;
  project: string;
  hostname: string;
  bucketStart: string;
  inputTokens: number;
  outputTokens: number;
  /** Not yet emitted by the sync pipeline; optional so decoding keeps working once it appears. */
  cacheCreationInputTokens?: number | null;
  /** Cache writes are separate from ordinary input in current API responses. */
  cacheCreation5mTokens?: number | null;
  cacheCreation1hTokens?: number | null;
  cachedInputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
  estimatedCost?: number | null;
}

/** Cache-write TTL fields are disjoint from input; absent on legacy responses. */
export function cacheWriteTokens(b: UsageBucket): number {
  return (b.cacheCreation5mTokens ?? 0) + (b.cacheCreation1hTokens ?? 0);
}

/** All observed tokens, including cache reads and cache writes. */
export function computedTotal(b: UsageBucket): number {
  return b.inputTokens + b.outputTokens + b.reasoningOutputTokens + b.cachedInputTokens + cacheWriteTokens(b);
}

function localDateKey(value: string): string {
  const date = new Date(value);
  if (isNaN(date.getTime())) return value.slice(0, 10);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/** Local calendar day for grouping, matching the web dashboard's timezone semantics. */
export function bucketDayKey(b: UsageBucket): string {
  return localDateKey(b.bucketStart);
}

/** Hour string (yyyy-MM-ddTHH) for hourly grouping — UTC prefix(13) */
export function bucketHourKey(b: UsageBucket): string {
  return b.bucketStart.slice(0, 13);
}

export function bucketDate(b: UsageBucket): Date | null {
  const d = new Date(b.bucketStart);
  return isNaN(d.getTime()) ? null : d;
}

/** Mirrors UsageSession.swift */
export interface UsageSession {
  source: string;
  project: string;
  hostname: string;
  firstMessageAt: string;
  lastMessageAt: string;
  durationSeconds: number;
  activeSeconds: number;
  messageCount: number;
  userMessageCount: number;
}

export function sessionDayKey(s: UsageSession): string {
  return localDateKey(s.firstMessageAt);
}

export function sessionHourKey(s: UsageSession): string {
  return s.firstMessageAt.slice(0, 13);
}

export function sessionDate(s: UsageSession): Date | null {
  const d = new Date(s.firstMessageAt);
  return isNaN(d.getTime()) ? null : d;
}

export interface UsageResponse {
  buckets: UsageBucket[];
  sessions?: UsageSession[] | null;
  hasAnyData: boolean;
}

// ---------------------------------------------------------------------------
// Dashboard controls — 1:1 port of AppState.swift enums

export type ChartMode = "token" | "cost" | "activeTime";

export const CHART_MODE_LABELS: Record<ChartMode, string> = {
  token: "Token",
  cost: "费用",
  activeTime: "活跃",
};

export type TimeRange = "today" | "1D" | "7D" | "30D" | "90D" | "custom";

export const TIME_RANGE_ORDER: TimeRange[] = ["today", "1D", "7D", "30D", "90D", "custom"];

export function timeRangeLabel(r: TimeRange): string {
  switch (r) {
    case "today":
      return "今天";
    case "1D":
      return "24H";
    case "custom":
      return "自定义";
    default:
      return r;
  }
}

export function fixedDayCount(r: TimeRange): number {
  switch (r) {
    case "today":
    case "1D":
      return 1;
    case "7D":
      return 7;
    case "30D":
      return 30;
    case "90D":
      return 90;
    case "custom":
      return 7;
  }
}

/** Hour-granularity for today + rolling 24h; day-granularity for longer ranges. */
export function isHourly(r: TimeRange): boolean {
  return r === "today" || r === "1D";
}

/**
 * Inclusive lower bound on bucket/session timestamps when this range is
 * active. Only `.today` tightens the client-side window below what the API
 * returned (mirrors TimeRange.startCutoff).
 */
export function startCutoff(r: TimeRange, now: Date = new Date()): Date | null {
  if (r !== "today") return null;
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d;
}

/** Mirrors FilterState */
export interface FilterState {
  sources: Set<string>;
  models: Set<string>;
  projects: Set<string>;
  hostnames: Set<string>;
}

export function emptyFilters(): FilterState {
  return { sources: new Set(), models: new Set(), projects: new Set(), hostnames: new Set() };
}

export function filtersAreEmpty(f: FilterState): boolean {
  return (
    f.sources.size === 0 && f.models.size === 0 && f.projects.size === 0 && f.hostnames.size === 0
  );
}

// ---------------------------------------------------------------------------
// Rate limits — mirrors RateLimit.swift (serialized from Rust)

export interface RateLimitWindow {
  /** 0-100 */
  utilization: number;
  /** epoch seconds */
  resetsAt?: number | null;
  /** seconds; enables the elapsed-time bar */
  windowDuration?: number | null;
}

export type RateLimitProvider =
  | "codex"
  | "claudeCode"
  | "kimi-code"
  | "zcode"
  | "grok"
  | "cursor"
  | "opencode-go";

export interface RateLimitMeter {
  id: string;
  label: string;
  /** 0-100 */
  utilization: number;
  /** epoch seconds */
  resetsAt?: number | null;
  /** seconds; enables the elapsed-time bar */
  windowDuration?: number | null;
}

export type RateLimitStatus =
  | { kind: "ok" }
  | { kind: "noData" }
  | { kind: "disabled" }
  | { kind: "unauthorized" }
  | { kind: "retryableError" }
  | { kind: "error"; message: string };

/**
 * Why a source that *did* answer had no window to draw, when it can say
 * (mirrors RateLimit.swift's EmptyReason). Codex's live usage endpoint reports
 * enforced windows exhaustively and carries `allowed` / `limit_reached`, so an
 * answer without any window is a fact. Every other source cannot tell
 * "used up" apart from "nothing here", so it leaves this null and the card
 * stays neutral rather than guessing.
 */
export type RateLimitEmptyReason =
  | "limitReached"
  | "noWindow"
  /**
   * The endpoint answered, but the account does not own the subscription
   * (OpenCode Go returns 403 for a key without the Go plan). A retry cannot
   * change it, so the card must not offer one.
   */
  | "notEntitled"
  /**
   * Claude Code answered `rate_limits_available: false`: this session is
   * authenticated with an API key, Bedrock or Vertex, where plan windows
   * genuinely do not apply. Also a definitive answer — the card explains
   * instead of offering a retry that cannot produce windows.
   */
  | "sessionWithoutPlanLimits";

export interface ProviderRateLimit {
  sourceLabel?: string | null;
  provider: RateLimitProvider;
  /** Absent/null means "the source did not say" — never render a verdict. */
  emptyReason?: RateLimitEmptyReason | null;
  meters?: RateLimitMeter[];
  fiveHour?: RateLimitWindow | null;
  sevenDay?: RateLimitWindow | null;
  planLabel?: string | null;
  /** Epoch seconds when the provider produced the numbers. */
  dataAsOf?: number | null;
  /** Epoch seconds when Vibe Usage fetched the snapshot. */
  fetchedAt?: number | null;
  fiveHourNotEnforced?: boolean;
  resetCreditsCount?: number | null;
  status: RateLimitStatus;
}

export type QuotaProductAvailability = "ready" | "pendingProtocol";

export interface QuotaProduct {
  provider: RateLimitProvider;
  displayName: string;
  availability: QuotaProductAvailability;
  isDetected: boolean;
}

export type ZCodeQuotaRegion = "bigModel" | "zAI";

export interface ZCodeCredentialStatus {
  bigModelConfigured: boolean;
  zAiConfigured: boolean;
}

// ---------------------------------------------------------------------------
// Sync state — mirrors SyncStatus (pushed from Rust via `sync-state` event)

export interface SyncState {
  status: "idle" | "syncing" | "success" | "error";
  message?: string | null;
  /** epoch millis of last successful sync */
  lastSyncAt?: number | null;
}

// ---------------------------------------------------------------------------
// App status (from Rust `get_app_status`)

export interface AppStatus {
  configured: boolean;
  hostname?: string | null;
  apiUrl: string;
  version: string;
  isDev: boolean;
  runtimeAvailable: boolean;
  testDiagnosticsAvailable: boolean;
  updatesAvailable: boolean;
  apiKeyDisplay?: string | null;
}

/** App settings persisted by Rust (mirrors UserDefaults keys). */
export interface AppSettings {
  showCostInTray: boolean;
  showTokensInTray: boolean;
  codexRateLimitEnabled: boolean;
  claudeRateLimitEnabled: boolean;
  selectedQuotaProductIds: RateLimitProvider[];
  /**
   * Display order of every catalog product, enabled or not. The tab strip
   * renders enabled products first, so this decides the sequence *within* each
   * of those two groups (mirrors the macOS `quotaProductOrder` key).
   */
  quotaProductOrder: RateLimitProvider[];
  /**
   * The tab the user last looked at; used to restore the card row's scroll
   * position (mirrors the macOS `quotaSelectedTabId` key).
   */
  quotaSelectedTabId?: RateLimitProvider | null;
  quotaSelectionInitialized: boolean;
  zCodeQuotaRegion: ZCodeQuotaRegion;
}

export type ExtraRoots = Partial<Record<"codex" | "grok" | "antigravity", string[]>>;

export interface UpdateInfo {
  version: string;
  notes?: string | null;
  url: string;
}

/** Query range sent to Rust fetch_usage — mirrors UsageQueryRange */
export type UsageQuery =
  | { kind: "days"; days: number }
  | { kind: "from"; fromIso: string }
  | { kind: "custom"; fromDate: string; toDate: string };
