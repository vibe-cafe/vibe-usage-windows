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
import { api, onPanelShown, onSyncState, onUpdateAvailable } from "../lib/api";
import {
  AppStatus,
  bucketDate,
  ChartMode,
  computedTotal,
  emptyFilters,
  FilterState,
  fixedDayCount,
  ProviderRateLimit,
  startCutoff,
  SyncState,
  TimeRange,
  UpdateInfo,
  UsageBucket,
  UsageQuery,
  UsageSession,
} from "../lib/types";
import { localDayKey } from "../lib/formatters";
import { invoke } from "@tauri-apps/api/core";

interface AppStateValue {
  status: AppStatus | null;
  configured: boolean;

  buckets: UsageBucket[];
  sessions: UsageSession[];
  hasAnyData: boolean;
  isLoadingData: boolean;
  hasLoadedUsageData: boolean;
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
  updateInfo: UpdateInfo | null;

  markConfigured: () => Promise<void>;
  fetchUsageData: () => Promise<void>;
  triggerSync: () => Promise<void>;
  refreshRateLimits: (force: boolean) => Promise<void>;
  enableClaudeRateLimit: () => Promise<void>;
  claudeRateLimitInstallError: string | null;
}

const AppStateContext = createContext<AppStateValue | null>(null);

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
  const [configured, setConfigured] = useState(false);

  const [buckets, setBuckets] = useState<UsageBucket[]>([]);
  const [sessions, setSessions] = useState<UsageSession[]>([]);
  const [hasAnyData, setHasAnyData] = useState(false);
  const [isLoadingData, setIsLoadingData] = useState(false);
  const [hasLoadedUsageData, setHasLoadedUsageData] = useState(false);

  const [timeRange, setTimeRangeRaw] = useState<TimeRange>("1D");
  const [customRangeFrom, setCustomRangeFrom] = useState<Date>(
    () => new Date(startOfToday().getTime() - 6 * 86400_000),
  );
  const [customRangeTo, setCustomRangeTo] = useState<Date>(startOfToday);
  const [chartMode, setChartMode] = useState<ChartMode>("token");
  const [filters, setFilters] = useState<FilterState>(emptyFilters);

  const [syncState, setSyncState] = useState<SyncState>({ status: "idle" });
  const [rateLimits, setRateLimits] = useState<ProviderRateLimit[]>([]);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [claudeRateLimitInstallError, setClaudeError] = useState<string | null>(null);

  const lastFetchTime = useRef<number | null>(null);
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
    if (!configuredRef.current || loadingRef.current) return;
    loadingRef.current = true;
    setIsLoadingData(true);
    try {
      const response = await api.fetchUsage(buildQuery());
      setBuckets(response.buckets);
      setSessions(response.sessions ?? []);
      setHasAnyData(response.hasAnyData);
    } catch (err) {
      // Silently fail — dashboard shows stale data or empty state (mirrors macOS).
      console.warn("Failed to fetch usage data:", err);
    } finally {
      lastFetchTime.current = Date.now();
      setHasLoadedUsageData(true);
      setIsLoadingData(false);
      loadingRef.current = false;
    }
  }, [buildQuery]);

  const fetchUsageDataIfNeeded = useCallback(async () => {
    if (lastFetchTime.current && Date.now() - lastFetchTime.current < 60_000) return;
    await fetchUsageData();
  }, [fetchUsageData]);

  const refreshRateLimits = useCallback(async (force: boolean) => {
    try {
      setRateLimits(await api.getRateLimits(force));
    } catch (err) {
      console.warn("rate limits:", err);
    }
  }, []);

  const enableClaudeRateLimit = useCallback(async () => {
    try {
      setClaudeError(null);
      setRateLimits(await api.enableClaudeRateLimit());
    } catch (err) {
      setClaudeError(String(err));
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
        setSyncState(await api.getSyncState());
        setRateLimits(await api.getRateLimits(false));
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
      onPanelShown(() => {
        // Config may have changed while hidden (relink / reset from settings).
        void api.getAppStatus().then((s) => {
          setStatus(s);
          setConfigured(s.configured);
          configuredRef.current = s.configured;
        });
        void fetchUsageDataIfNeeded();
        void refreshRateLimits(false);
      }),
    ];
    return () => {
      for (const p of subs) void p.then((un) => un());
    };
  }, [fetchUsageData, fetchUsageDataIfNeeded, refreshRateLimits]);

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
    configured,
    buckets,
    sessions,
    hasAnyData,
    isLoadingData,
    hasLoadedUsageData,
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
    updateInfo,
    markConfigured,
    fetchUsageData,
    triggerSync,
    refreshRateLimits,
    enableClaudeRateLimit,
    claudeRateLimitInstallError,
  };

  return <AppStateContext.Provider value={value}>{children}</AppStateContext.Provider>;
}
