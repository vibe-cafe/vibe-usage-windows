// Settings window — port of macOS `Views/SettingsView.swift` (classic theme):
// a sidebar of five pages, 通用 / 订阅配额 / 数据同步 / 数据目录（高级）/ 关于,
// the chosen page on the right. This file owns the window's state and talks
// to the backend; `settings/` draws the pages. macOS's 「在 Dock 中显示」 and
// the menu-bar preview have no Windows equivalent and are left out; where the
// Windows-only rows went is listed in docs/PARITY.md (SettingsView row).

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getName } from "@tauri-apps/api/app";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  api,
  observeCachedRateLimits,
  onCursorLogin,
  onDeviceLink,
  onSettingsShown,
  onSettingsUpdated,
  onSyncState,
  onUpdateAvailable,
  onUpdateCleared,
} from "./lib/api";
import { formatInvokeError } from "./lib/errors";
import { extraRootsLoadPatch } from "./lib/extraRoots";
import type {
  AppSettings,
  AppStatus,
  CursorLoginStatus,
  ExtraRoots,
  ProviderRateLimit,
  QuotaProduct,
  RateLimitProvider,
  SyncState,
  UpdateInfo,
  ZCodeCredentialStatus,
  ZCodeQuotaRegion,
} from "./lib/types";
import {
  isZCodeConfigured,
  moveQuotaProduct as computeQuotaProductOrder,
  quotaProductStatus,
  quotaTabOrder,
} from "./lib/quotaProducts";
import { buildDiagnosticsText, copyText, type SyncRecord } from "./lib/diagnostics";
import { CURSOR_LOGOUT_CONFIRM, CURSOR_LOGOUT_TITLE } from "./lib/cursorLogin";
import { egressModeOption } from "./lib/egressMode";
import { followUpdateOffer } from "./lib/updateOffer";
import { type UpdatePhase, createUpdateRunner, phaseForNewOffer } from "./lib/updateButton";
import {
  type SettingsPage,
  cursorRowConfigurable,
  cursorSignInNeeded,
  opensConfiguration,
  rememberPage,
  restoredPage,
  settingsDestination,
} from "./settings/model";
import { SettingsContext, type ExtraRootSource, type SettingsModel } from "./settings/context";
import { SettingsWindow } from "./settings/SettingsWindow";
import { ConfirmDialog } from "./settings/controls";

function pageStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function SettingsApp() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [appName, setAppName] = useState("Vibe Usage");
  const [settings, setSettingsState] = useState<AppSettings | null>(null);
  // The latest settings this window knows of, including what the panel wrote
  // since (`settings-updated`). A change here patches this copy, never the one
  // the window loaded when it opened: the panel persists the quota tab order
  // and last tab on its own, and writing back a stale whole object erased a
  // freshly dragged order (#53).
  const settingsRef = useRef<AppSettings | null>(null);
  const setSettings = useCallback((next: AppSettings) => {
    settingsRef.current = next;
    setSettingsState(next);
  }, []);
  // Counts `settings-updated` events. A reload waits for slower reads (the
  // CLI's extra roots) before it applies the settings it fetched; if an event
  // arrived meanwhile, that copy is older than the one already shown.
  const settingsEvents = useRef(0);
  const applyPushedSettings = useCallback(
    (next: AppSettings) => {
      settingsEvents.current += 1;
      setSettings(next);
    },
    [setSettings],
  );

  const [page, setPageState] = useState<SettingsPage>(() => restoredPage(pageStorage()));
  const setPage = useCallback((next: SettingsPage) => {
    setPageState(next);
    rememberPage(pageStorage(), next);
  }, []);

  const [syncState, setSyncState] = useState<SyncState>({ status: "idle" });
  const [syncHistory, setSyncHistory] = useState<SyncRecord[]>([]);
  const [autoStart, setAutoStartState] = useState(false);

  const [relinking, setRelinking] = useState(false);
  const [relinkUserCode, setRelinkUserCode] = useState<string | null>(null);
  const [relinkError, setRelinkError] = useState<string | null>(null);
  const [confirmingReset, setConfirmingReset] = useState(false);

  const [quotaError, setQuotaError] = useState<string | null>(null);
  const [quotaProducts, setQuotaProducts] = useState<QuotaProduct[]>([]);
  const [quotaSnapshots, setQuotaSnapshotsState] = useState<ProviderRateLimit[] | null>([]);
  const quotaSnapshotsRef = useRef<ProviderRateLimit[] | null>([]);
  const setQuotaSnapshots = useCallback((next: ProviderRateLimit[] | null) => {
    quotaSnapshotsRef.current = next;
    setQuotaSnapshotsState(next);
  }, []);
  const [quotaBusy, setQuotaBusy] = useState(false);
  const [expanded, setExpanded] = useState<ReadonlySet<RateLimitProvider>>(new Set());
  const [flash, setFlash] = useState<SettingsModel["flash"]>(null);

  const [zCodeStatus, setZCodeStatus] = useState<ZCodeCredentialStatus>({
    bigModelConfigured: false,
    zAiConfigured: false,
  });
  const [zCodeDraft, setZCodeDraft] = useState("");
  const [zCodeReplacing, setZCodeReplacing] = useState(false);
  const [zCodeError, setZCodeError] = useState<string | null>(null);

  const [cursorLogin, setCursorLogin] = useState<CursorLoginStatus | null>(null);
  const [cursorLoginError, setCursorLoginError] = useState<string | null>(null);
  const [confirmingCursorLogout, setConfirmingCursorLogout] = useState(false);

  const [extraRoots, setExtraRoots] = useState<ExtraRoots>({});
  const [extraRootsBusy, setExtraRootsBusy] = useState(false);
  const [extraRootsError, setExtraRootsError] = useState<string | null>(null);

  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [updateOffer, setUpdateOffer] = useState<UpdateInfo | null>(null);
  const [updatePhase, setUpdatePhase] = useState<UpdatePhase>({ kind: "idle" });
  const [startUpdate] = useState(() => createUpdateRunner(api.installUpdate, setUpdatePhase));
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const [diagnosticsText, setDiagnosticsText] = useState<string | null>(null);
  const [diagnosticsCopied, setDiagnosticsCopied] = useState(false);
  const [diagnosticsMessage, setDiagnosticsMessage] = useState<string | null>(null);
  const [testExportMessage, setTestExportMessage] = useState<string | null>(null);

  const flashRequests = useRef(0);

  const loadSyncHistory = useCallback(() => {
    void api.getSyncHistory().then(setSyncHistory).catch(() => {});
  }, []);

  const reload = useCallback(async () => {
    const eventsBefore = settingsEvents.current;
    // Taken first: the entry point that opened (or re-showed) the window.
    // Where it wants the window (macOS `SettingsNavigation.open`): no focus
    // keeps the last page; the panel's gear, a grey tab and the Cursor card
    // land on 订阅配额 at once, the latter two flashing their row below.
    const destination = settingsDestination(await api.takeSettingsFocus().catch(() => null));
    if (destination.kind === "quotas") setPage("quotas");
    // Defaults are initialized by the backend before any window starts.
    const discovered = await api.getQuotaProducts().catch(() => null);
    const [
      nextStatus,
      nextSettings,
      nextSyncState,
      nextAutoStart,
      nextExtraRoots,
      nextZCodeStatus,
      nextCursorLogin,
    ] = await Promise.allSettled([
      api.getAppStatus(),
      api.getSettings(),
      api.getSyncState(),
      api.getLaunchAtLogin(),
      api.getExtraRoots(),
      api.getZCodeCredentialStatus(),
      api.getCursorLogin(),
    ]);

    if (discovered) setQuotaProducts(discovered);
    if (nextStatus.status === "fulfilled") setStatus(nextStatus.value);
    if (nextSettings.status === "fulfilled" && settingsEvents.current === eventsBefore) {
      setSettings(nextSettings.value);
    }
    if (nextSyncState.status === "fulfilled") setSyncState(nextSyncState.value);
    if (nextAutoStart.status === "fulfilled") setAutoStartState(nextAutoStart.value);
    if (nextZCodeStatus.status === "fulfilled") setZCodeStatus(nextZCodeStatus.value);
    if (nextCursorLogin.status === "fulfilled") setCursorLogin(nextCursorLogin.value);
    const extraPatch = extraRootsLoadPatch(nextExtraRoots);
    if (extraPatch.extraRoots !== undefined) setExtraRoots(extraPatch.extraRoots);
    setExtraRootsError(extraPatch.extraRootsError);
    loadSyncHistory();

    if (destination.kind !== "quotas" || !destination.provider) return;
    const provider = destination.provider;
    const region = settingsRef.current?.zCodeQuotaRegion ?? "bigModel";
    const zConfigured =
      nextZCodeStatus.status === "fulfilled" && isZCodeConfigured(nextZCodeStatus.value, region);
    const cursor = nextCursorLogin.status === "fulfilled" ? nextCursorLogin.value : null;
    const cursorSnapshot = quotaSnapshotsRef.current?.find((item) => item.provider === "cursor");
    const cursorNeeds =
      cursorRowConfigurable(cursor) && (destination.signIn || cursorSignInNeeded(cursor, cursorSnapshot));
    if (opensConfiguration(provider, zConfigured, cursorNeeds)) {
      setExpanded((current) => new Set(current).add(provider));
    }
    flashRequests.current += 1;
    setFlash({ provider, request: flashRequests.current });
  }, [setSettings, setPage, loadSyncHistory]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;

    void getName().then(setAppName).catch(() => {});
    void reload();
    const stopQuotaUpdates = observeCachedRateLimits(setQuotaSnapshots);
    const subs = [
      onSyncState((next) => {
        setSyncState(next);
        // The run is recorded before its result is announced.
        if (next.status !== "syncing") loadSyncHistory();
      }),
      onSettingsUpdated(applyPushedSettings),
      onCursorLogin(setCursorLogin),
      // The same offer the panel header shows: found at startup, by the daily
      // check or by 「检查更新」 here; withdrawn by a later check (#36).
      followUpdateOffer(
        { onAvailable: onUpdateAvailable, onCleared: onUpdateCleared, readCurrent: api.getUpdateInfo },
        setUpdateOffer,
      ),
      onSettingsShown(() => {
        void reload();
      }),
      onDeviceLink(async (e) => {
        setRelinking(false);
        setRelinkUserCode(null);
        switch (e.status) {
          case "success":
            setRelinkError(null);
            await reload();
            break;
          case "denied":
            setRelinkError("你拒绝了链接请求。");
            break;
          case "expired":
            setRelinkError("验证码已过期，请重新登录。");
            break;
          case "error":
            setRelinkError(`服务端返回未知错误：${e.message}`);
            break;
        }
      }),
    ];
    return () => {
      stopQuotaUpdates();
      for (const p of subs) void p.then((un) => un());
    };
  }, [reload, applyPushedSettings, setQuotaSnapshots, loadSyncHistory]);

  // Closing hides the window instead of destroying it. Like a new macOS
  // settings window, the next opening starts with every row collapsed and no
  // half-finished key, copy result or open confirmation.
  useEffect(() => {
    const onVisibility = () => {
      if (document.visibilityState !== "hidden") return;
      setExpanded(new Set());
      setFlash(null);
      setZCodeDraft("");
      setZCodeReplacing(false);
      setZCodeError(null);
      setDiagnosticsText(null);
      setDiagnosticsCopied(false);
      setDiagnosticsMessage(null);
      setTestExportMessage(null);
      setConfirmingReset(false);
      setConfirmingCursorLogout(false);
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, []);

  // A failure belonged to the offer it tried to install (same rule as the
  // header badge); a newer offer replaces the last check's line.
  const offeredVersion = updateOffer?.version ?? null;
  useEffect(() => {
    setUpdatePhase(phaseForNewOffer);
    if (offeredVersion) setUpdateMessage(null);
  }, [offeredVersion]);

  // MARK: Actions

  const patchSettings = useCallback(
    (patch: Partial<AppSettings>) => {
      const current = settingsRef.current;
      if (!current) return;
      const next = { ...current, ...patch };
      setSettings(next);
      void api.setSettings(next);
    },
    [setSettings],
  );

  const startRelink = async () => {
    setRelinkError(null);
    setRelinkUserCode(null);
    setRelinking(true);
    try {
      const { userCode } = await api.startDeviceLink();
      setRelinkUserCode(userCode);
    } catch (err) {
      setRelinkError(`无法连接服务端：${formatInvokeError(err)}`);
      setRelinking(false);
    }
  };

  const cancelRelink = () => {
    void api.cancelDeviceLink();
    setRelinking(false);
    setRelinkUserCode(null);
    setRelinkError(null);
  };

  const resetConfig = async () => {
    setConfirmingReset(false);
    await api.resetConfig();
    await reload();
  };

  const toggleQuotaProduct = async (provider: RateLimitProvider, selected: boolean) => {
    setQuotaError(null);
    setQuotaBusy(true);
    try {
      await api.setQuotaProductSelected(provider, selected);
      setSettings(await api.getSettings());
    } catch (err) {
      setQuotaError(formatInvokeError(err));
    } finally {
      setQuotaBusy(false);
    }
  };

  const rediscoverQuotaProducts = async () => {
    setQuotaError(null);
    setQuotaBusy(true);
    try {
      setQuotaProducts(await api.getQuotaProducts());
      setSettings(await api.getSettings());
    } catch (err) {
      setQuotaError(formatInvokeError(err));
    } finally {
      setQuotaBusy(false);
    }
  };

  const catalog = useMemo(() => quotaProducts.map((product) => product.provider), [quotaProducts]);
  const selectedIds = settings?.selectedQuotaProductIds;
  const storedOrder = settings?.quotaProductOrder;
  const tabOrder = useMemo(
    () => quotaTabOrder(catalog, selectedIds ?? [], storedOrder ?? []),
    [catalog, selectedIds, storedOrder],
  );
  const enabledOrder = useMemo(
    () => tabOrder.filter((provider) => selectedIds?.includes(provider)),
    [tabOrder, selectedIds],
  );
  const disabledOrder = useMemo(
    () => tabOrder.filter((provider) => !selectedIds?.includes(provider)),
    [tabOrder, selectedIds],
  );

  // The panel's own order (`quotaProductOrder`): a drop here reorders the
  // panel's tabs too. Computed from this window's latest copy, which already
  // carries anything the panel dragged since (#53).
  const moveQuotaProduct = (provider: RateLimitProvider, target: RateLimitProvider | null) => {
    const current = settingsRef.current;
    if (!current) return;
    const order = quotaTabOrder(catalog, current.selectedQuotaProductIds, current.quotaProductOrder ?? []);
    const next = computeQuotaProductOrder(order, current.selectedQuotaProductIds, provider, target);
    const same = next.length === order.length && next.every((item, index) => item === order[index]);
    if (!same) patchSettings({ quotaProductOrder: next });
  };

  const toggleExpanded = (provider: RateLimitProvider) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(provider)) next.delete(provider);
      else next.add(provider);
      return next;
    });
  };

  const region = settings?.zCodeQuotaRegion ?? "bigModel";
  const zCodeConfigured = isZCodeConfigured(zCodeStatus, region);

  const setZCodeRegion = async (next: ZCodeQuotaRegion) => {
    if (next === region) return;
    setZCodeDraft("");
    setZCodeError(null);
    setZCodeReplacing(false);
    setQuotaBusy(true);
    try {
      await api.setZCodeQuotaRegion(next);
      const [nextSettings, nextStatus] = await Promise.all([
        api.getSettings(),
        api.getZCodeCredentialStatus(),
      ]);
      setSettings(nextSettings);
      setZCodeStatus(nextStatus);
    } catch (err) {
      setZCodeError(formatInvokeError(err));
    } finally {
      setQuotaBusy(false);
    }
  };

  /** Saving never turns the ZCode switch on (the backend's rule, same as macOS). */
  const saveZCodeKey = async () => {
    const key = zCodeDraft.trim();
    if (!settingsRef.current || !key || quotaBusy) return;
    setQuotaBusy(true);
    setZCodeError(null);
    try {
      await api.setZCodeApiKey(region, key);
      setZCodeDraft("");
      setZCodeReplacing(false);
      setZCodeStatus(await api.getZCodeCredentialStatus());
      setSettings(await api.getSettings());
    } catch (err) {
      setZCodeError(formatInvokeError(err));
    } finally {
      setQuotaBusy(false);
    }
  };

  /** Removing the key also turns the ZCode switch off (backend). */
  const removeZCodeKey = async () => {
    if (!settingsRef.current) return;
    setQuotaBusy(true);
    setZCodeError(null);
    try {
      await api.setZCodeApiKey(region, null);
      setZCodeDraft("");
      setZCodeReplacing(false);
      const [nextSettings, nextStatus] = await Promise.all([
        api.getSettings(),
        api.getZCodeCredentialStatus(),
      ]);
      setSettings(nextSettings);
      setZCodeStatus(nextStatus);
    } catch (err) {
      setZCodeError(formatInvokeError(err));
    } finally {
      setQuotaBusy(false);
    }
  };

  const logoutCursor = async () => {
    setConfirmingCursorLogout(false);
    try {
      setCursorLogin(await api.logoutCursor());
    } catch (err) {
      setCursorLoginError(formatInvokeError(err));
    }
  };

  const cursorLoginAction = async (kind: "start" | "cancel" | "logout") => {
    setCursorLoginError(null);
    if (kind === "logout") {
      // An ended sign-in has nothing left on cursor.com to revoke: remove it.
      if (cursorLogin?.state === "expired") void logoutCursor();
      else setConfirmingCursorLogout(true);
      return;
    }
    try {
      setCursorLogin(await (kind === "start" ? api.startCursorLogin() : api.cancelCursorLogin()));
    } catch (err) {
      setCursorLoginError(formatInvokeError(err));
    }
  };

  const setAutoStart = (enabled: boolean) => {
    setAutoStartState(enabled);
    void api.setLaunchAtLogin(enabled);
  };

  const checkUpdate = async () => {
    // A recheck may withdraw the offer (a pulled release, docs/WINDOWS_UPDATE.md),
    // so it stays available next to 「安装更新」.
    setUpdateMessage("检查中…");
    try {
      const info = await api.checkForUpdate();
      setUpdateOffer(info);
      setUpdateMessage(info ? null : "已是最新版本");
    } catch (err) {
      setUpdateMessage(`检查失败: ${formatInvokeError(err)}`);
    }
  };

  const exportTestDiagnostics = async () => {
    setTestExportMessage(null);
    const destination = await save({
      title: "导出测试诊断日志",
      defaultPath: `vibe-usage-diagnostics-${Math.floor(Date.now() / 1000)}.jsonl`,
      filters: [{ name: "JSON Lines", extensions: ["jsonl"] }],
    });
    if (!destination) return;
    try {
      await api.exportTestDiagnostics(destination);
      setTestExportMessage("诊断日志已导出");
    } catch (err) {
      setTestExportMessage(`导出失败：${formatInvokeError(err)}`);
    }
  };

  /** Copies a whitelisted summary; what was copied stays readable underneath. */
  const copyDiagnostics = async () => {
    setDiagnosticsBusy(true);
    setDiagnosticsMessage(null);
    setDiagnosticsCopied(false);
    try {
      const report = await api.collectDiagnostics();
      const text = buildDiagnosticsText(report, quotaProducts);
      setDiagnosticsText(text);
      try {
        await copyText(text);
        setDiagnosticsCopied(true);
      } catch {
        setDiagnosticsMessage("复制失败，请手动复制下面的内容");
      }
    } catch (err) {
      setDiagnosticsMessage(`收集失败：${formatInvokeError(err)}`);
    } finally {
      setDiagnosticsBusy(false);
    }
  };

  const addExtraRoot = async (source: ExtraRootSource) => {
    const path = await open({ directory: true, multiple: false, title: "选择隔离运行时目录" });
    if (!path) return;
    setExtraRootsBusy(true);
    setExtraRootsError(null);
    try {
      await api.addExtraRoot(source, path);
      setExtraRoots(await api.getExtraRoots());
      await api.triggerSync();
    } catch (err) {
      setExtraRootsError(formatInvokeError(err));
    } finally {
      setExtraRootsBusy(false);
    }
  };

  const removeExtraRoot = async (source: ExtraRootSource, path: string) => {
    setExtraRootsBusy(true);
    setExtraRootsError(null);
    try {
      await api.removeExtraRoot(source, path);
      setExtraRoots(await api.getExtraRoots());
      await api.triggerSync();
    } catch (err) {
      setExtraRootsError(formatInvokeError(err));
    } finally {
      setExtraRootsBusy(false);
    }
  };

  // Cursor ended the sign-in Vibe Usage keeps: the row says so even while
  // its card is off (macOS `cursorSignInEnded`).
  const cursorSignInEnded = cursorLogin?.state === "expired";
  const quotaStatus = (product: QuotaProduct) =>
    quotaProductStatus(
      product,
      selectedIds ?? [],
      quotaSnapshots?.find((item) => item.provider === product.provider),
      quotaSnapshots === null,
      zCodeConfigured,
      cursorSignInEnded,
    );

  const model: SettingsModel = {
    status,
    appName,
    settings,
    page,
    setPage,
    autoStart,
    setAutoStart,
    patchSettings,
    syncState,
    syncHistory,
    relinking,
    relinkUserCode,
    relinkError,
    startRelink: () => void startRelink(),
    cancelRelink,
    askReset: () => setConfirmingReset(true),
    quotaProducts,
    quotaSnapshots,
    quotaBusy,
    quotaError,
    enabledOrder,
    disabledOrder,
    quotaStatus,
    toggleQuotaProduct: (provider, selected) => void toggleQuotaProduct(provider, selected),
    rediscoverQuotaProducts: () => void rediscoverQuotaProducts(),
    moveQuotaProduct,
    expanded,
    toggleExpanded,
    flash,
    egressMode: egressModeOption(settings?.egressMode).value,
    zCodeRegion: region,
    zCodeConfigured,
    zCodeDraft,
    setZCodeDraft: (value) => {
      setZCodeDraft(value);
      setZCodeError(null);
    },
    zCodeReplacing,
    beginReplacingZCodeKey: () => {
      setZCodeDraft("");
      setZCodeError(null);
      setZCodeReplacing(true);
    },
    cancelReplacingZCodeKey: () => {
      setZCodeDraft("");
      setZCodeError(null);
      setZCodeReplacing(false);
    },
    zCodeError,
    setZCodeRegion: (next) => void setZCodeRegion(next),
    saveZCodeKey: () => void saveZCodeKey(),
    removeZCodeKey: () => void removeZCodeKey(),
    cursorLogin,
    cursorLoginError,
    cursorLoginAction: (kind) => void cursorLoginAction(kind),
    extraRoots,
    extraRootsBusy,
    extraRootsError,
    addExtraRoot: (source) => void addExtraRoot(source),
    removeExtraRoot: (source, path) => void removeExtraRoot(source, path),
    updateOffer,
    updatePhase,
    updateMessage,
    checkUpdate: () => void checkUpdate(),
    installUpdate: () => void startUpdate(),
    diagnosticsBusy,
    diagnosticsText,
    diagnosticsCopied,
    diagnosticsMessage,
    copyDiagnostics: () => void copyDiagnostics(),
    testExportMessage,
    exportTestDiagnostics: () => void exportTestDiagnostics(),
    openExternal: (url) => void api.openExternal(url),
  };

  return (
    <SettingsContext.Provider value={model}>
      <SettingsWindow />
      {confirmingReset && (
        <ConfirmDialog
          title="确定要重置配置吗？"
          message="这将清除 API Key 并停止自动同步。"
          confirmLabel="重置"
          onConfirm={() => void resetConfig()}
          onCancel={() => setConfirmingReset(false)}
        />
      )}
      {confirmingCursorLogout && (
        <ConfirmDialog
          title={CURSOR_LOGOUT_TITLE}
          message={CURSOR_LOGOUT_CONFIRM}
          confirmLabel="退出"
          onConfirm={() => void logoutCursor()}
          onCancel={() => setConfirmingCursorLogout(false)}
        />
      )}
    </SettingsContext.Provider>
  );
}
