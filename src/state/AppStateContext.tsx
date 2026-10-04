// Central app state — port of Models/AppState.swift.
// Owns dashboard data, filters/time range, sync status and rate limits.

import {
  createContext,
  ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { api, onPanelShown, onSettingsUpdated, onSyncState, onUpdateAvailable } from "../lib/api";
import {
  AppSettings,
  AppStatus,
  bucketDate,
  ChartMode,
  computedTotal,
  emptyFilters,
  FilterState,
  fixedDayCount,
  ProviderRateLimit,
  QuotaProduct,
  RateLimitProvider,
  startCutoff,
  SyncState,
  TimeRange,
  UpdateInfo,
  UsageBucket,
  UsageQuery,
  UsageSession,
  ZCodeCredentialStatus,
} from "../lib/types";
import { localDayKey } from "../lib/formatters";
import {
  moveQuotaProduct as computeQuotaProductOrder,
  quotaTabOrder as computeQuotaTabOrder,
} from "../lib/quotaProducts";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { startLiveRefresh } from "../lib/liveRefresh";

/** The dashboard/quota state every component reads through `useAppState`. */
export interface AppStateValue {
  status: AppStatus | null;
  settings: AppSettings;
  configured: boolean;

  buckets: UsageBucket[];
  sessions: UsageSession[];
  hasAnyData: boolean;
  isLoadingData: boolean;
  hasLoadedUsageData: boolean;
  usageError: string | null;
  usageUpdatedAt: number | null;
  isInitialDataLoad: boolean;
  isRefreshingData: boolean;

  timeRange: TimeRange;
  setTimeRange: (r: TimeRange) => void;
  customRangeFrom: Date;
  customRangeTo: Date;
  setCustomRangeFrom: (d: Date) => void;
  setCustomRangeTo: (d: Date) => void;
  visibleDayCount: number;
  normalizedCustomRange: { from: Date; to: Date };

  chartMode: ChartMode;
  setChartMode: (m: ChartMode) => void;
  filters: FilterState;
  setFilters: (f: FilterState) => void;

  syncState: SyncState;
  rateLimits: ProviderRateLimit[];
  quotaProducts: QuotaProduct[];
  /** Enabled products first, then the disabled ones (see `quotaTabOrder`). */
  quotaTabOrder: RateLimitProvider[];
  zCodeCredentialStatus: ZCodeCredentialStatus;
  isRefreshingRateLimits: boolean;
  quotaSelectionError: string | null;
  updateInfo: UpdateInfo | null;

  markConfigured: () => Promise<void>;
  fetchUsageData: () => Promise<void>;
  triggerSync: () => Promise<void>;
  refreshRateLimits: (force: boolean) => Promise<void>;
  setQuotaProductSelected: (provider: RateLimitProvider, selected: boolean) => Promise<void>;
  /** Remember which tab the user is looking at (view state only). */
  selectQuotaTab: (provider: RateLimitProvider) => void;
  /** Reorder the tab strip; `target === null` means "past the last tab". */
  moveQuotaProduct: (provider: RateLimitProvider, target: RateLimitProvider | null) => void;
  rediscoverQuotaProducts: () => Promise<void>;
}

const AppStateContext = createContext<AppStateValue | null>(null);

const DEFAULT_SETTINGS: AppSettings = {
  showCostInTray: true,
  showTokensInTray: false,
  codexRateLimitEnabled: true,
  claudeRateLimitEnabled: false,
  selectedQuotaProductIds: [],
  quotaProductOrder: [],
  quotaSelectedTabId: null,
  quotaSelectionInitialized: false,
  zCodeQuotaRegion: "bigModel",
};

/**
 * True when a settings patch changes nothing the tab strip persists. Keeping
 * this out of the update path stops a re-render from rewriting settings.json
 * on every card scroll.
 */
function sameQuotaViewState(left: AppSettings, right: AppSettings): boolean {
  const sameTab = left.quotaSelectedTabId === right.quotaSelectedTabId;
  const leftOrder = left.quotaProductOrder ?? [];
  const rightOrder = right.quotaProductOrder ?? [];
  const sameOrder =
    leftOrder.length === rightOrder.length &&
    leftOrder.every((provider, index) => provider === rightOrder[index]);
  return sameTab && sameOrder;
}

const EMPTY_ZCODE_CREDENTIAL_STATUS: ZCodeCredentialStatus = {
  bigModelConfigured: false,
  zAiConfigured: false,
};

export function useAppState(): AppStateValue {
  const v = useContext(AppStateContext);
  if (!v) throw new Error("useAppState outside provider");
  return v;
}

function startOfToday(): Date {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d;
}

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [configured, setConfigured] = useState(false);

  const [buckets, setBuckets] = useState<UsageBucket[]>([]);
  const [sessions, setSessions] = useState<UsageSession[]>([]);
  const [hasAnyData, setHasAnyData] = useState(false);
  const [isLoadingData, setIsLoadingData] = useState(false);
  const [hasLoadedUsageData, setHasLoadedUsageData] = useState(false);
  const [usageError, setUsageError] = useState<string | null>(null);
  const [usageUpdatedAt, setUsageUpdatedAt] = useState<number | null>(null);

  const [timeRange, setTimeRangeRaw] = useState<TimeRange>("1D");
  const [customRangeFrom, setCustomRangeFrom] = useState<Date>(
    () => new Date(startOfToday().getTime() - 6 * 86400_000),
  );
  const [customRangeTo, setCustomRangeTo] = useState<Date>(startOfToday);
  const [chartMode, setChartMode] = useState<ChartMode>("token");
  const [filters, setFilters] = useState<FilterState>(emptyFilters);

  const [syncState, setSyncState] = useState<SyncState>({ status: "idle" });
  const [rateLimits, setRateLimits] = useState<ProviderRateLimit[]>([]);
  const [quotaProducts, setQuotaProducts] = useState<QuotaProduct[]>([]);
  const [zCodeCredentialStatus, setZCodeCredentialStatus] =
    useState<ZCodeCredentialStatus>(EMPTY_ZCODE_CREDENTIAL_STATUS);
  const [isRefreshingRateLimits, setIsRefreshingRateLimits] = useState(false);
  const [quotaSelectionError, setQuotaSelectionError] = useState<string | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  // A settings change or newer refresh supersedes responses already in flight.
  const quotaRequest = useRef(0);

  const lastFetchTime = useRef<number | null>(null);
  const usageRequest = useRef(0);
  const loadingRef = useRef(false);
  // Refs so the panel-shown listener sees current values without re-subscribing.
  const rangeRef = useRef<{ timeRange: TimeRange; from: Date; to: Date }>({
    timeRange: "1D",
    from: customRangeFrom,
    to: customRangeTo,
  });
  rangeRef.current = { timeRange, from: customRangeFrom, to: customRangeTo };
  const configuredRef = useRef(false);
  configuredRef.current = configured;
  // The tab strip persists through the same settings object Rust owns, so the
  // latest value has to be readable from callbacks that do not re-subscribe.
  const settingsRef = useRef<AppSettings>(DEFAULT_SETTINGS);
  settingsRef.current = settings;

  const normalizedCustomRange = useMemo(() => {
    return customRangeFrom <= customRangeTo
      ? { from: customRangeFrom, to: customRangeTo }
      : { from: customRangeTo, to: customRangeFrom };
  }, [customRangeFrom, customRangeTo]);

  const visibleDayCount = useMemo(() => {
    if (timeRange !== "custom") return fixedDayCount(timeRange);
    const from = new Date(normalizedCustomRange.from);
    from.setHours(0, 0, 0, 0);
    const to = new Date(normalizedCustomRange.to);
    to.setHours(0, 0, 0, 0);
    const days = Math.round((to.getTime() - from.getTime()) / 86400_000);
    return Math.max(days + 1, 1);
  }, [timeRange, normalizedCustomRange]);

  const buildQuery = useCallback((): UsageQuery => {
    const { timeRange: r, from, to } = rangeRef.current;
    switch (r) {
      case "today": {
        const start = startOfToday();
        return { kind: "from", fromIso: start.toISOString() };
      }
      case "1D":
        return { kind: "days", days: 1 };
      case "7D":
        return { kind: "days", days: 7 };
      case "30D":
        return { kind: "days", days: 30 };
      case "90D":
        return { kind: "days", days: 90 };
      case "custom": {
        const lo = from <= to ? from : to;
        const hi = from <= to ? to : from;
        return { kind: "custom", fromDate: localDayKey(lo), toDate: localDayKey(hi) };
      }
    }
  }, []);

  const fetchUsageData = useCallback(async () => {
    if (!configuredRef.current) return;
    const request = ++usageRequest.current;
    loadingRef.current = true;
    setIsLoadingData(true);
    try {
      const response = await api.fetchUsage(buildQuery());
      if (request !== usageRequest.current) return;
      setUsageError(null);
      setUsageUpdatedAt(Date.now());
      lastFetchTime.current = Date.now();
      setBuckets(response.buckets);
      setSessions(response.sessions ?? []);
      setHasAnyData(response.hasAnyData);
    } catch (err) {
      if (request !== usageRequest.current) return;
      setUsageError(String(err));
      console.warn("Failed to fetch usage data:", err);
    } finally {
      if (request === usageRequest.current) {
        setHasLoadedUsageData(true);
        setIsLoadingData(false);
        loadingRef.current = false;
      }
    }
  }, [buildQuery]);

  const fetchUsageDataIfNeeded = useCallback(async () => {
    if (loadingRef.current) return;
    if (lastFetchTime.current && Date.now() - lastFetchTime.current < 60_000) return;
    await fetchUsageData();
  }, [fetchUsageData]);

  const refreshRateLimits = useCallback(async (force: boolean) => {
    const request = ++quotaRequest.current;
    setIsRefreshingRateLimits(true);
    try {
      const snapshots = await api.getRateLimits(force);
      if (request === quotaRequest.current) setRateLimits(snapshots);
    } catch (err) {
      console.warn("rate limits:", err);
    } finally {
      if (request === quotaRequest.current) setIsRefreshingRateLimits(false);
    }
  }, []);

  const setQuotaProductSelected = useCallback(
    async (provider: RateLimitProvider, selected: boolean) => {
      const request = ++quotaRequest.current;
      setQuotaSelectionError(null);
      setIsRefreshingRateLimits(true);
      try {
        const nextRateLimits = await api.setQuotaProductSelected(provider, selected);
        const nextSettings = await api.getSettings();
        if (request !== quotaRequest.current) return;
        setSettings(nextSettings);
        setRateLimits(nextRateLimits);
      } catch (err) {
        if (request === quotaRequest.current) setQuotaSelectionError(String(err));
      } finally {
        if (request === quotaRequest.current) setIsRefreshingRateLimits(false);
      }
    },
    [],
  );

  // Mirrors AppState.quotaTabOrder on macOS: enabled products first, then the
  // disabled ones, each group in the persisted order (catalog order for a
  // product the stored order does not mention yet).
  const quotaTabOrderValue = useMemo(
    () =>
      computeQuotaTabOrder(
        quotaProducts.map((product) => product.provider),
        settings.selectedQuotaProductIds,
        settings.quotaProductOrder ?? [],
      ),
    [quotaProducts, settings.selectedQuotaProductIds, settings.quotaProductOrder],
  );

  /**
   * Persist tab-strip view state (order, last tab). Deliberately separate from
   * `setQuotaProductSelected`: looking at or rearranging products never turns
   * monitoring on, so this must not trigger a quota refresh or touch the
   * rate-limit snapshots.
   */
  const persistQuotaViewState = useCallback(async (patch: Partial<AppSettings>) => {
    const current = settingsRef.current;
    const next = { ...current, ...patch };
    if (sameQuotaViewState(current, next)) return;
    settingsRef.current = next;
    setSettings(next);
    try {
      await api.setSettings(next);
    } catch (err) {
      setQuotaSelectionError(String(err));
    }
  }, []);

  const selectQuotaTab = useCallback(
    (provider: RateLimitProvider) => {
      void persistQuotaViewState({ quotaSelectedTabId: provider });
    },
    [persistQuotaViewState],
  );

  const moveQuotaProduct = useCallback(
    (provider: RateLimitProvider, target: RateLimitProvider | null) => {
      const order = quotaTabOrderValue;
      void persistQuotaViewState({
        quotaProductOrder: computeQuotaProductOrder(
          order,
          settingsRef.current.selectedQuotaProductIds,
          provider,
          target,
        ),
      });
    },
    [persistQuotaViewState, quotaTabOrderValue],
  );

  const rediscoverQuotaProducts = useCallback(async () => {
    try {
      setQuotaSelectionError(null);
      setQuotaProducts(await api.getQuotaProducts());
      setSettings(await api.getSettings());
    } catch (err) {
      setQuotaSelectionError(String(err));
    }
  }, []);

  const triggerSync = useCallback(async () => {
    try {
      await api.triggerSync();
    } catch (err) {
      console.warn("trigger sync:", err);
    }
  }, []);

  const setTimeRange = useCallback(
    (r: TimeRange) => {
      setTimeRangeRaw(r);
      rangeRef.current = { ...rangeRef.current, timeRange: r };
      // Range change → server refetch (custom waits for 应用 button).
      if (r !== "custom") {
        void fetchUsageData();
      }
    },
    [fetchUsageData],
  );

  const markConfigured = useCallback(async () => {
    setConfigured(true);
    configuredRef.current = true;
    const s = await api.getAppStatus();
    setStatus(s);
    await fetchUsageData();
  }, [fetchUsageData]);

  // Initialize once.
  useEffect(() => {
    let disposed = false;
    (async () => {
      try {
        const s = await api.getAppStatus();
        if (disposed) return;
        setStatus(s);
        setConfigured(s.configured);
        configuredRef.current = s.configured;
        // Backend startup has already initialized the default selection.
        const nextQuotaProducts = await api.getQuotaProducts();
        const request = quotaRequest.current;
        const [nextSettings, nextSyncState, nextRateLimits, nextZCodeStatus] = await Promise.all([
          api.getSettings(),
          api.getSyncState(),
          api.getRateLimits(false),
          api.getZCodeCredentialStatus().catch(() => EMPTY_ZCODE_CREDENTIAL_STATUS),
        ]);
        if (disposed) return;
        setQuotaProducts(nextQuotaProducts);
        if (request === quotaRequest.current) setSettings(nextSettings);
        setSyncState(nextSyncState);
        if (request === quotaRequest.current) setRateLimits(nextRateLimits);
        setZCodeCredentialStatus(nextZCodeStatus);
        if (s.configured) {
          await fetchUsageData();
        }
      } catch (err) {
        console.error("init failed:", err);
      }
    })();
    return () => {
      disposed = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Other devices can upload without a local sync event.
  useEffect(() => startLiveRefresh(async () => {
    await Promise.allSettled([
      loadingRef.current ? Promise.resolve() : fetchUsageData(),
      refreshRateLimits(false),
      api.getSyncState().then(setSyncState),
    ]);
  }, async () => {
    const window = getCurrentWindow();
    return await window.isVisible() && !await window.isMinimized();
  }), [fetchUsageData, refreshRateLimits]);

  // Event subscriptions.
  useEffect(() => {
    const subs = [
      onSyncState((s) => {
        setSyncState(s);
        // After a successful CLI sync the backend refreshed nothing else —
        // re-pull dashboard data (mirrors triggerSync → fetchUsageData).
        if (s.status === "success") {
          void fetchUsageData();
        }
      }),
      onUpdateAvailable((u) => setUpdateInfo(u)),
      onSettingsUpdated((nextSettings) => {
        setSettings(nextSettings);
        setRateLimits([]);
        void api.getZCodeCredentialStatus().then(setZCodeCredentialStatus).catch(() => {});
        void refreshRateLimits(false);
      }),
      onPanelShown(() => {
        // Config may have changed while hidden (relink / reset from settings).
        void api.getAppStatus().then((s) => {
          setStatus(s);
          setConfigured(s.configured);
          configuredRef.current = s.configured;
        });
        void api.getQuotaProducts().then((products) => {
          setQuotaProducts(products);
          void api.getSettings().then(setSettings);
        });
        void api.getZCodeCredentialStatus().then(setZCodeCredentialStatus).catch(() => {});
        void fetchUsageDataIfNeeded();
        void refreshRateLimits(false);
      }),
    ];
    return () => {
      for (const p of subs) void p.then((un) => un());
    };
  }, [fetchUsageData, fetchUsageDataIfNeeded, refreshRateLimits]);

  // The backend can begin its startup sync before WebView event listeners are
  // fully attached. Reconcile only while the UI believes a sync is active so
  // a missed success/idle event can never leave the footer spinning forever.
  useEffect(() => {
    if (syncState.status !== "syncing") return;
    let disposed = false;
    const timer = window.setInterval(() => {
      void api.getSyncState().then((next) => {
        if (disposed) return;
        setSyncState(next);
        if (next.status === "success") void fetchUsageData();
      });
    }, 1000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [fetchUsageData, syncState.status]);

  // Push tray stats (cost + tokens for the ACTIVE range, no filters) —
  // mirrors AppState.menuBarCost/menuBarTokens incl. the `.today` cutoff.
  useEffect(() => {
    if (!configured || buckets.length === 0) return;
    const cutoff = startCutoff(timeRange);
    let cost = 0;
    let tokens = 0;
    for (const b of buckets) {
      if (cutoff) {
        const d = bucketDate(b);
        if (d && d < cutoff) continue;
      }
      cost += b.estimatedCost ?? 0;
      tokens += computedTotal(b);
    }
    void invoke("update_tray_stats", { cost, tokens }).catch(() => {});
  }, [configured, buckets, timeRange]);

  const isInitialDataLoad = isLoadingData && !hasLoadedUsageData && buckets.length === 0;
  const isRefreshingData = isLoadingData && hasLoadedUsageData;

  const value: AppStateValue = {
    status,
    settings,
    configured,
    buckets,
    sessions,
    hasAnyData,
    isLoadingData,
    hasLoadedUsageData,
    usageError,
    usageUpdatedAt,
    isInitialDataLoad,
    isRefreshingData,
    timeRange,
    setTimeRange,
    customRangeFrom,
    customRangeTo,
    setCustomRangeFrom,
    setCustomRangeTo,
    visibleDayCount,
    normalizedCustomRange,
    chartMode,
    setChartMode,
    filters,
    setFilters,
    syncState,
    rateLimits,
    quotaProducts,
    quotaTabOrder: quotaTabOrderValue,
    zCodeCredentialStatus,
    isRefreshingRateLimits,
    quotaSelectionError,
    updateInfo,
    markConfigured,
    fetchUsageData,
    triggerSync,
    refreshRateLimits,
    setQuotaProductSelected,
    selectQuotaTab,
    moveQuotaProduct,
    rediscoverQuotaProducts,
  };

  return <AppStateContext.Provider value={value}>{children}</AppStateContext.Provider>;
}
