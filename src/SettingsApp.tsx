// Settings window — port of Views/SettingsView.swift (grouped Form, 420px
// content in a 460×620 window; on Windows the window is resizable and the
// column widens with it). macOS's "在 Dock 中显示" has no Windows
// equivalent (no Dock) and is intentionally omitted.

import { useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, Check, CheckCircle2, ChevronRight, ChevronsUpDown, RefreshCw } from "lucide-react";
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
import {
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
import { formatRelativeTime } from "./lib/formatters";
import { isZCodeConfigured, quotaProductStatusText } from "./lib/quotaProducts";
import { buildDiagnosticsText, copyText } from "./lib/diagnostics";
import { CURSOR_LOGOUT_CONFIRM, CURSOR_LOGOUT_TITLE, cursorLoginRow } from "./lib/cursorLogin";
import { EGRESS_MODES, egressModeOption } from "./lib/egressMode";
import { followUpdateOffer } from "./lib/updateOffer";
import {
  type UpdatePhase,
  createUpdateRunner,
  phaseForNewOffer,
  updateButtonEnabled,
} from "./lib/updateButton";
import { PROVIDER_ICON_OFF_COLOR, ProviderIcon } from "./components/ProviderIcon";

export function SettingsApp() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [settings, setSettingsState] = useState<AppSettings | null>(null);
  // The latest settings this window knows of, including what the panel wrote
  // since (`settings-updated`). A switch here patches this copy, never the one
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
  const [syncState, setSyncState] = useState<SyncState>({ status: "idle" });
  const [autoStart, setAutoStart] = useState(false);

  const [isRelinking, setIsRelinking] = useState(false);
  const [relinkUserCode, setRelinkUserCode] = useState<string | null>(null);
  const [relinkError, setRelinkError] = useState<string | null>(null);
  const [quotaError, setQuotaError] = useState<string | null>(null);
  const [quotaProducts, setQuotaProducts] = useState<QuotaProduct[]>([]);
  const [quotaSnapshots, setQuotaSnapshots] = useState<ProviderRateLimit[] | null>([]);
  const [quotaBusy, setQuotaBusy] = useState(false);
  const [zCodeStatus, setZCodeStatus] = useState<ZCodeCredentialStatus>({
    bigModelConfigured: false,
    zAiConfigured: false,
  });
  const [zCodeApiKey, setZCodeApiKey] = useState("");
  const [zCodeMessage, setZCodeMessage] = useState<string | null>(null);
  const [showResetConfirm, setShowResetConfirm] = useState(false);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [updateOffer, setUpdateOffer] = useState<UpdateInfo | null>(null);
  const [updatePhase, setUpdatePhase] = useState<UpdatePhase>({ kind: "idle" });
  const [startUpdate] = useState(() => createUpdateRunner(api.installUpdate, setUpdatePhase));
  const [diagnosticMessage, setDiagnosticMessage] = useState<string | null>(null);
  const [reportBusy, setReportBusy] = useState(false);
  const [reportMessage, setReportMessage] = useState<string | null>(null);
  const [reportText, setReportText] = useState<string | null>(null);
  const [reportExpanded, setReportExpanded] = useState(false);
  const [extraRoots, setExtraRoots] = useState<ExtraRoots>({});
  const [extraRootsBusy, setExtraRootsBusy] = useState(false);
  const [extraRootsError, setExtraRootsError] = useState<string | null>(null);
  const [isZCodeExpanded, setZCodeExpanded] = useState(false);
  const [isIsolatedRootsExpanded, setIsIsolatedRootsExpanded] = useState(false);
  const [cursorLogin, setCursorLogin] = useState<CursorLoginStatus | null>(null);
  const [cursorLogoutConfirm, setCursorLogoutConfirm] = useState(false);
  const [cursorLogoutError, setCursorLogoutError] = useState<string | null>(null);
  // The Cursor card's sign-in prompt opened this window: scroll to the row.
  const [focusCursorLogin, setFocusCursorLogin] = useState(false);
  const [highlightCursorLogin, setHighlightCursorLogin] = useState(false);
  const cursorLoginRef = useRef<HTMLDivElement>(null);

  const reload = useCallback(async () => {
    const eventsBefore = settingsEvents.current;
    // Defaults are initialized by the backend before any window starts.
    const discovered = await api.getQuotaProducts().catch(() => null);
    const [
      nextStatus,
      nextSettings,
      nextSyncState,
      nextAutoStart,
      nextExtraRoots,
      nextZCodeStatus,
    ] = await Promise.allSettled([
      api.getAppStatus(),
      api.getSettings(),
      api.getSyncState(),
      api.getLaunchAtLogin(),
      api.getExtraRoots(),
      api.getZCodeCredentialStatus(),
    ]);

    void api.getCursorLogin().then(setCursorLogin).catch(() => {});
    void api
      .takeSettingsFocus()
      .then((focus) => {
        if (focus === "cursorLogin") setFocusCursorLogin(true);
      })
      .catch(() => {});
    if (discovered) setQuotaProducts(discovered);
    if (nextStatus.status === "fulfilled") setStatus(nextStatus.value);
    if (nextSettings.status === "fulfilled" && settingsEvents.current === eventsBefore) {
      setSettings(nextSettings.value);
    }
    if (nextSyncState.status === "fulfilled") setSyncState(nextSyncState.value);
    if (nextAutoStart.status === "fulfilled") setAutoStart(nextAutoStart.value);
    if (nextZCodeStatus.status === "fulfilled") setZCodeStatus(nextZCodeStatus.value);
    const extraPatch = extraRootsLoadPatch(nextExtraRoots);
    if (extraPatch.extraRoots !== undefined) {
      setExtraRoots(extraPatch.extraRoots);
    }
    setExtraRootsError(extraPatch.extraRootsError);
  }, [setSettings]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;

    void reload();
    const stopQuotaUpdates = observeCachedRateLimits(setQuotaSnapshots);
    const subs = [
      onSyncState(setSyncState),
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
        setIsRelinking(false);
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
  }, [reload, applyPushedSettings]);

  useEffect(() => {
    if (!focusCursorLogin) return;
    const row = cursorLoginRef.current;
    if (!row) return;
    setFocusCursorLogin(false);
    row.scrollIntoView({ block: "center" });
    setHighlightCursorLogin(true);
  }, [focusCursorLogin, quotaProducts]);

  useEffect(() => {
    if (!highlightCursorLogin) return;
    const timer = window.setTimeout(() => setHighlightCursorLogin(false), 1600);
    return () => window.clearTimeout(timer);
  }, [highlightCursorLogin]);

  // A failure belonged to the offer it tried to install (same rule as the
  // header badge); a newer offer (e.g. found by the daily check after an
  // earlier 「已是最新版本」) replaces the last check's line.
  const offeredVersion = updateOffer?.version ?? null;
  useEffect(() => {
    setUpdatePhase(phaseForNewOffer);
    if (offeredVersion) setUpdateMessage(null);
  }, [offeredVersion]);

  const relink = async () => {
    setRelinkError(null);
    setRelinkUserCode(null);
    setIsRelinking(true);
    try {
      const { userCode } = await api.startDeviceLink();
      setRelinkUserCode(userCode);
    } catch (err) {
      setRelinkError(`无法连接服务端：${formatInvokeError(err)}`);
      setIsRelinking(false);
    }
  };

  const cancelRelink = () => {
    void api.cancelDeviceLink();
    setIsRelinking(false);
    setRelinkUserCode(null);
    setRelinkError(null);
  };

  const patchSettings = (patch: Partial<AppSettings>) => {
    const current = settingsRef.current;
    if (!current) return;
    const next = { ...current, ...patch };
    setSettings(next);
    void api.setSettings(next);
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

  const setZCodeRegion = async (region: ZCodeQuotaRegion) => {
    setZCodeApiKey("");
    setZCodeMessage(null);
    setQuotaError(null);
    setQuotaBusy(true);
    try {
      await api.setZCodeQuotaRegion(region);
      const [nextSettings, nextStatus] = await Promise.all([
        api.getSettings(),
        api.getZCodeCredentialStatus(),
      ]);
      setSettings(nextSettings);
      setZCodeStatus(nextStatus);
    } catch (err) {
      setQuotaError(formatInvokeError(err));
    } finally {
      setQuotaBusy(false);
    }
  };

  const saveZCodeApiKey = async () => {
    if (!settings || !zCodeApiKey.trim()) return;
    setQuotaBusy(true);
    setQuotaError(null);
    setZCodeMessage(null);
    try {
      await api.setZCodeApiKey(settings.zCodeQuotaRegion, zCodeApiKey.trim());
      setZCodeApiKey("");
      setZCodeStatus(await api.getZCodeCredentialStatus());
      setSettings(await api.getSettings());
      setZCodeMessage("已安全保存到 Windows 凭据管理器");
    } catch (err) {
      setQuotaError(formatInvokeError(err));
    } finally {
      setQuotaBusy(false);
    }
  };

  const removeZCodeApiKey = async () => {
    if (!settings) return;
    setQuotaBusy(true);
    setQuotaError(null);
    setZCodeMessage(null);
    try {
      await api.setZCodeApiKey(settings.zCodeQuotaRegion, null);
      setZCodeApiKey("");
      const [nextSettings, nextStatus] = await Promise.all([
        api.getSettings(),
        api.getZCodeCredentialStatus(),
      ]);
      setSettings(nextSettings);
      setZCodeStatus(nextStatus);
      setZCodeMessage("已移除当前区域的 API Key");
    } catch (err) {
      setQuotaError(formatInvokeError(err));
    } finally {
      setQuotaBusy(false);
    }
  };

  const runCursorLoginAction = async (kind: "start" | "cancel" | "logout") => {
    setCursorLogoutError(null);
    if (kind === "logout") {
      // An ended sign-in has nothing left on cursor.com to revoke: remove it.
      if (cursorLogin?.state === "expired") void logoutCursor();
      else setCursorLogoutConfirm(true);
      return;
    }
    try {
      setCursorLogin(await (kind === "start" ? api.startCursorLogin() : api.cancelCursorLogin()));
    } catch (err) {
      setCursorLogoutError(formatInvokeError(err));
    }
  };

  const logoutCursor = async () => {
    setCursorLogoutConfirm(false);
    try {
      setCursorLogin(await api.logoutCursor());
    } catch (err) {
      setCursorLogoutError(formatInvokeError(err));
    }
  };

  const toggleAutoStart = (enabled: boolean) => {
    setAutoStart(enabled);
    void api.setLaunchAtLogin(enabled);
  };

  const resetConfig = async () => {
    setShowResetConfirm(false);
    await api.resetConfig();
    await reload();
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

  const exportDiagnostics = async () => {
    setDiagnosticMessage(null);
    const destination = await save({
      title: "导出测试诊断日志",
      defaultPath: `vibe-usage-diagnostics-${Math.floor(Date.now() / 1000)}.jsonl`,
      filters: [{ name: "JSON Lines", extensions: ["jsonl"] }],
    });
    if (!destination) return;
    try {
      await api.exportTestDiagnostics(destination);
      setDiagnosticMessage("诊断日志已导出");
    } catch (err) {
      setDiagnosticMessage(`导出失败：${formatInvokeError(err)}`);
    }
  };

  const copyDiagnosticsReport = async () => {
    setReportBusy(true);
    setReportMessage(null);
    try {
      const report = await api.collectDiagnostics();
      const text = buildDiagnosticsText(report, quotaProducts);
      setReportText(text);
      try {
        await copyText(text);
        setReportMessage("已复制，可以直接贴到群里");
      } catch {
        setReportMessage("复制失败，请展开后手动复制");
        setReportExpanded(true);
      }
    } catch (err) {
      setReportMessage(`收集失败：${formatInvokeError(err)}`);
    } finally {
      setReportBusy(false);
    }
  };

  const addExtraRoot = async (source: "codex" | "grok" | "antigravity") => {
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

  const removeExtraRoot = async (
    source: "codex" | "grok" | "antigravity",
    path: string,
  ) => {
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

  const region = settings?.zCodeQuotaRegion ?? "bigModel";
  const zCodeConfigured = isZCodeConfigured(zCodeStatus, region);
  const zCodeProduct = quotaProducts.find((product) => product.provider === "zcode");
  const statusText = (product: QuotaProduct) => quotaProductStatusText(
    product,
    settings?.selectedQuotaProductIds ?? [],
    quotaSnapshots?.find((item) => item.provider === product.provider),
    quotaSnapshots === null,
    zCodeConfigured,
  );
  const isolatedRootCount = Object.values(extraRoots).reduce(
    (total, paths) => total + (paths?.length ?? 0),
    0,
  );
  const isolatedRootsSummary = isolatedRootCount === 0 ? "未添加" : `${isolatedRootCount} 个目录`;

  return (
    // The window is resizable: the whole window scrolls, and the column
    // widens with it up to a comfortable reading width, then stays centered.
    // At the default 460 width the 15px gutter keeps the original 430 column.
    <div
      className="no-scrollbar h-screen overflow-y-auto px-[15px] font-sans text-[13px]"
      style={{ background: "#1C1C1E", color: "#E8E8E8" }}
    >
      <div className="mx-auto flex max-w-[640px] flex-col gap-4 px-4 py-4">
        {/* 数据同步 — account key plus sync health; the former standalone
            「上次同步」 row rides along as a caption instead of a full row. */}
        <Section title="数据同步">
          <Row label="API Key">
            <div className="flex flex-col items-end gap-1.5">
              <div className="flex items-center gap-2">
                <span className="font-mono text-xs" style={{ color: "#808080" }}>
                  {status?.apiKeyDisplay ?? "未配置"}
                </span>
                <SmallButton disabled={isRelinking} onClick={() => void relink()}>
                  {isRelinking ? "等待确认…" : "重新链接"}
                </SmallButton>
                {isRelinking && <SmallButton onClick={cancelRelink}>取消</SmallButton>}
              </div>
              {relinkUserCode && (
                <span className="font-mono text-xs" style={{ color: "#9E9E9E" }}>
                  验证码: {relinkUserCode}
                </span>
              )}
              {relinkError && (
                <span
                  title={relinkError}
                  className="line-clamp-2 max-w-[280px] break-words text-xs text-red-400"
                >
                  {relinkError}
                </span>
              )}
            </div>
          </Row>
          <Row label="同步状态">
            <div className="flex flex-col items-end gap-0.5">
              <span className="flex items-center gap-1 text-xs" style={{ color: "#B0B0B0" }}>
                {syncState.status === "syncing" ? (
                  <>
                    <div className="spinner h-3 w-3" /> 同步中...
                  </>
                ) : syncState.status === "error" ? (
                  <>
                    <AlertCircle size={13} color="#EF4444" />
                    <span className="max-w-[260px] truncate">{syncState.message ?? "错误"}</span>
                  </>
                ) : (
                  <>
                    <CheckCircle2 size={13} color="#34C759" />
                    {syncState.status === "success" ? "同步成功" : "正常"}
                  </>
                )}
              </span>
              {syncState.lastSyncAt && (
                <span className="text-[11px]" style={{ color: "#9E9E9E" }}>
                  上次同步 {formatRelativeTime(new Date(syncState.lastSyncAt))}
                </span>
              )}
            </div>
          </Row>
        </Section>

        {/* 订阅配额 — every product is a plain switch, so the section stays
            flat; only ZCode (the one product that needs a key) owns a row that
            opens, and it sits last. */}
        <Section title={`订阅配额（${settings?.selectedQuotaProductIds.length ?? 0}）`}>
          {quotaProducts
            .filter((product) => product.provider !== "zcode")
            .map((product) => {
              const row = (
                <ProductRow
                  key={product.provider}
                  product={product}
                  statusText={statusText(product)}
                  selected={settings?.selectedQuotaProductIds.includes(product.provider) ?? false}
                  disabled={quotaBusy}
                  onToggle={(value) => void toggleQuotaProduct(product.provider, value)}
                />
              );
              if (product.provider !== "cursor") return row;
              // Cursor's own sign-in for Vibe Usage sits under its switch: it
              // serves the quota card and the usage sync alike.
              return (
                <div
                  key={product.provider}
                  ref={cursorLoginRef}
                  className="flex flex-col transition-colors duration-500"
                  style={{ background: highlightCursorLogin ? "rgba(10,132,255,0.14)" : undefined }}
                >
                  {row}
                  {cursorLogin && (
                    <CursorLoginBlock
                      status={cursorLogin}
                      confirmingLogout={cursorLogoutConfirm}
                      error={cursorLogoutError}
                      onAction={(kind) => void runCursorLoginAction(kind)}
                      onConfirmLogout={() => void logoutCursor()}
                      onCancelLogout={() => setCursorLogoutConfirm(false)}
                    />
                  )}
                </div>
              );
            })}

          {zCodeProduct && (
            <div className="flex flex-col">
              <div className="flex min-h-[38px] items-center gap-3 px-3 py-1.5">
                <button
                  aria-expanded={isZCodeExpanded}
                  className="flex min-w-0 grow items-center gap-2 text-left"
                  onClick={() => setZCodeExpanded((value) => !value)}
                >
                  <ChevronRight
                    size={12}
                    strokeWidth={2.5}
                    className="shrink-0 transition-transform duration-150"
                    style={{ color: "#8C8C8C", transform: isZCodeExpanded ? "rotate(90deg)" : undefined }}
                  />
                  <ProviderIcon
                    provider="zcode"
                    size={22}
                    color={
                      settings?.selectedQuotaProductIds.includes("zcode")
                        ? undefined
                        : PROVIDER_ICON_OFF_COLOR
                    }
                  />
                  <span className="flex min-w-0 flex-col">
                    <span className="text-[13px]">{zCodeProduct.displayName}</span>
                    <QuotaStatusLabel text={statusText(zCodeProduct)} />
                  </span>
                </button>
                <Toggle
                  checked={
                    settings?.selectedQuotaProductIds.includes("zcode") ?? false
                  }
                  disabled={quotaBusy}
                  onChange={(value) => void toggleQuotaProduct("zcode", value)}
                />
              </div>

              {isZCodeExpanded && (
                <div className="flex flex-col gap-2 px-3 pb-3 pl-[38px]">
                  <select
                    aria-label="ZCode 账号区域"
                    value={settings?.zCodeQuotaRegion ?? "bigModel"}
                    disabled={!settings || quotaBusy}
                    onChange={(event) => void setZCodeRegion(event.target.value as ZCodeQuotaRegion)}
                    className="w-full rounded bg-[#48484A] px-2 py-1 text-xs text-white outline-none disabled:opacity-50"
                  >
                    <option value="bigModel">BigModel 国内</option>
                    <option value="zAI">Z.ai 海外</option>
                  </select>
                  <div className="flex items-center gap-2">
                    <input
                      type="password"
                      value={zCodeApiKey}
                      disabled={!settings || quotaBusy}
                      autoComplete="off"
                      placeholder={zCodeConfigured ? "新 Key" : "输入 Coding Plan API Key"}
                      onChange={(event) => setZCodeApiKey(event.target.value)}
                      className="min-w-0 grow rounded-md border border-white/10 bg-[#1C1C1E] px-2.5 py-1.5 font-mono text-xs text-white outline-none focus:border-white/25 disabled:opacity-50"
                    />
                    <SmallButton
                      disabled={!zCodeApiKey.trim() || quotaBusy}
                      onClick={() => void saveZCodeApiKey()}
                    >
                      {zCodeConfigured ? "更新" : "保存"}
                    </SmallButton>
                    {zCodeConfigured && (
                      <button
                        disabled={quotaBusy}
                        className="shrink-0 text-xs text-red-400 disabled:opacity-50"
                        onClick={() => void removeZCodeApiKey()}
                      >
                        移除
                      </button>
                    )}
                  </div>
                  {zCodeMessage && <span className="text-[11px] text-emerald-400">{zCodeMessage}</span>}
                </div>
              )}
            </div>
          )}

          {/* The explanations that used to sit here (how each product is read)
              live in PRIVACY.md; the settings keep only what the user acts on
              (maintainer, 2026-11-26). */}
          <Row label="数据来源与检测">
            <SmallButton disabled={quotaBusy} onClick={() => void rediscoverQuotaProducts()}>
              <span className="flex items-center gap-1.5">
                <RefreshCw size={11} /> 重新检测
              </span>
            </SmallButton>
          </Row>

          {quotaError && (
            <div
              title={quotaError}
              className="line-clamp-2 break-words px-3 py-2 text-xs text-red-400"
            >
              {quotaError}
            </div>
          )}
        </Section>

        {/* 隐私保护 — the egress check (`egress_check.rs`): one three-way
            menu, like the macOS pop-up menu (System Settings' 「显示滚动条」).
            No explanatory line under it (maintainer, 2026-11-26); what each
            option does is in PRIVACY.md. */}
        <Section title="隐私保护">
          <Row label="没检测到代理时">
            <MenuSelect
              label="没检测到代理时"
              value={egressModeOption(settings?.egressMode).value}
              options={EGRESS_MODES}
              disabled={!settings}
              onChange={(value) => patchSettings({ egressMode: value })}
            />
          </Row>
        </Section>

        {/* 常规 — tray and startup preferences cover the same subject: how the
            app behaves outside this window. */}
        <Section title="常规">
          <Row label="托盘显示费用">
            <Toggle
              checked={settings?.showCostInTray ?? true}
              onChange={(v) => patchSettings({ showCostInTray: v })}
            />
          </Row>
          <Row label="托盘显示 Token">
            <Toggle
              checked={settings?.showTokensInTray ?? false}
              onChange={(v) => patchSettings({ showTokensInTray: v })}
            />
          </Row>
          <Row label="开机自启动">
            <Toggle checked={autoStart} onChange={toggleAutoStart} />
          </Row>
        </Section>

        {/* 数据目录（高级）— extra scan directories are a set-once concern, so
            they collapse into a row that still shows its state. */}
        <Section title="数据目录（高级）">
          <Disclosure
            label="隔离运行时目录"
            summary={isolatedRootsSummary}
            expanded={isIsolatedRootsExpanded}
            onToggle={() => setIsIsolatedRootsExpanded((value) => !value)}
          >
            {([
              ["codex", "Codex"],
              ["grok", "Grok"],
              ["antigravity", "Antigravity / AGY"],
            ] as const).map(([source, label]) => (
              <div key={source} className="flex flex-col gap-1.5">
                <div className="flex items-center justify-between gap-2">
                  <span>{label}</span>
                  <SmallButton disabled={extraRootsBusy} onClick={() => void addExtraRoot(source)}>
                    添加目录…
                  </SmallButton>
                </div>
                {(extraRoots[source] ?? []).map((path) => (
                  <div key={path} className="flex min-w-0 items-center gap-2">
                    <span
                      className="min-w-0 flex-1 truncate font-mono text-[11px]"
                      style={{ color: "#8C8C8C" }}
                      title={path}
                    >
                      {path}
                    </span>
                    <button
                      className="shrink-0 text-xs text-red-400 disabled:opacity-50"
                      disabled={extraRootsBusy}
                      onClick={() => void removeExtraRoot(source, path)}
                    >
                      移除
                    </button>
                  </div>
                ))}
              </div>
            ))}
            {extraRootsError && <span className="text-xs text-red-400">{extraRootsError}</span>}
          </Disclosure>
        </Section>

        {status?.testDiagnosticsAvailable && (
          <Section title="测试诊断">
            <Row label="外测日志">
              <div className="flex items-center gap-2">
                {diagnosticMessage && (
                  <span className="max-w-[190px] truncate text-[11px] text-neutral-400">
                    {diagnosticMessage}
                  </span>
                )}
                <SmallButton onClick={() => void exportDiagnostics()}>导出…</SmallButton>
              </div>
            </Row>
          </Section>
        )}

        {/* 关于 */}
        <Section title="关于">
          <Row label="版本">
            <span className="text-xs" style={{ color: "#9E9E9E" }}>
              {status?.version ?? ""}
            </span>
          </Row>
          <Row label="检查更新">
            <div className="flex min-w-0 items-center gap-2">
              {status?.updatesAvailable ? (
                <>
                  {(() => {
                    const failed = updatePhase.kind === "failed" ? updatePhase.reason : null;
                    const text =
                      failed != null
                        ? `更新失败：${failed}`
                        : (updateMessage ?? (updateOffer ? `发现新版本 ${updateOffer.version}` : null));
                    return (
                      text && (
                        <span
                          className="min-w-0 truncate text-xs"
                          style={{ color: failed != null ? "#FF6B6B" : "#9E9E9E" }}
                          title={text}
                        >
                          {text}
                        </span>
                      )
                    );
                  })()}
                  <SmallButton
                    disabled={!updateButtonEnabled(updatePhase)}
                    onClick={() => void checkUpdate()}
                  >
                    检查更新
                  </SmallButton>
                  {updateOffer && (
                    // Install from here: the same download-and-install as the
                    // panel's 「发现更新」 (no second confirmation on Windows,
                    // docs/PARITY.md 平台差异 5).
                    <SmallButton
                      disabled={!updateButtonEnabled(updatePhase)}
                      onClick={() => void startUpdate()}
                    >
                      {settingsUpdateButtonLabel(updatePhase)}
                    </SmallButton>
                  )}
                </>
              ) : status ? (
                <span className="text-xs text-neutral-400">外测版不检查更新</span>
              ) : null}
            </div>
          </Row>
          <Row label="诊断信息">
            <div className="flex min-w-0 items-center gap-2">
              {reportMessage && (
                <span className="min-w-0 truncate text-[11px] text-neutral-400" title={reportMessage}>
                  {reportMessage}
                </span>
              )}
              {reportText && (
                <button
                  className="shrink-0 text-[11px] text-neutral-400 underline-offset-2 hover:underline"
                  aria-expanded={reportExpanded}
                  onClick={() => setReportExpanded((value) => !value)}
                >
                  {reportExpanded ? "收起" : "查看全文"}
                </button>
              )}
              <SmallButton disabled={reportBusy} onClick={() => void copyDiagnosticsReport()}>
                {reportBusy ? "收集中…" : "复制诊断信息"}
              </SmallButton>
            </div>
          </Row>
          {reportText && reportExpanded && (
            <pre
              className="mx-3 mb-2.5 max-h-56 select-text overflow-auto whitespace-pre-wrap break-all rounded-md px-2.5 py-2 font-mono text-[10px] leading-relaxed"
              style={{ background: "#1C1C1E", color: "#B0B0B0" }}
            >
              {reportText}
            </pre>
          )}
        </Section>

        {/* 危险操作 */}
        <Section title="危险操作">
          {!showResetConfirm ? (
            <Row label="">
              <button className="text-[13px] text-red-400" onClick={() => setShowResetConfirm(true)}>
                重置配置
              </button>
            </Row>
          ) : (
            <div className="flex flex-col gap-2 px-3 py-2.5">
              <span className="text-xs" style={{ color: "#B0B0B0" }}>
                确定要重置配置吗？这将清除 API Key 并停止自动同步。
              </span>
              <div className="flex justify-end gap-2">
                <SmallButton onClick={() => setShowResetConfirm(false)}>取消</SmallButton>
                <button
                  className="rounded-md bg-red-500 px-3 py-1 text-xs font-medium text-white"
                  onClick={() => void resetConfig()}
                >
                  重置
                </button>
              </div>
            </div>
          )}
        </Section>
      </div>
    </div>
  );
}

function Section({
  title,
  footer,
  children,
}: {
  title?: string;
  footer?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      {title && (
        <span className="px-2 text-xs font-medium" style={{ color: "#8C8C8C" }}>
          {title}
        </span>
      )}
      <div
        className="flex flex-col divide-y rounded-[10px]"
        style={{ background: "#2A2A2C", borderColor: "#3A3A3C" }}
      >
        {children}
      </div>
      {footer && (
        <span className="px-2 text-[11px]" style={{ color: "#737373" }}>
          {footer}
        </span>
      )}
    </div>
  );
}

/** One product switch: monochrome mark, display name, and the discovery/read
 *  status line the panel's selector prints in full. */
function ProductRow({
  product,
  statusText,
  selected,
  disabled,
  onToggle,
}: {
  product: QuotaProduct;
  statusText: string;
  selected: boolean;
  disabled: boolean;
  onToggle: (selected: boolean) => void;
}) {
  return (
    <div className="flex min-h-[38px] items-center justify-between gap-3 px-3 py-1.5">
      <div className="flex min-w-0 items-center gap-2">
        {/* A 22 px slot, on products in the primary colour, off ones dimmed
            (macOS 订阅配额 table: ProviderGlyph 18 in a 22 frame, 72% when off). */}
        <ProviderIcon
          provider={product.provider}
          size={22}
          color={selected ? undefined : PROVIDER_ICON_OFF_COLOR}
        />
        <span className="flex min-w-0 flex-col">
          <span className="text-[13px]">{product.displayName}</span>
          <QuotaStatusLabel text={statusText} />
        </span>
      </div>
      <Toggle checked={selected} disabled={disabled} onChange={onToggle} />
    </div>
  );
}

/** The Cursor sign-in under its switch: state on the left, the button on the
 *  right (copy in `lib/cursorLogin`, same as macOS). */
function CursorLoginBlock({
  status,
  confirmingLogout,
  error,
  onAction,
  onConfirmLogout,
  onCancelLogout,
}: {
  status: CursorLoginStatus;
  confirmingLogout: boolean;
  error: string | null;
  onAction: (kind: "start" | "cancel" | "logout") => void;
  onConfirmLogout: () => void;
  onCancelLogout: () => void;
}) {
  const row = cursorLoginRow(status);
  if (!row.text && !row.action) return null;
  const errorText = error ?? row.error;
  return (
    <div className="flex flex-col gap-1.5 px-3 pb-2.5 pl-[42px]">
      {confirmingLogout ? (
        <div className="flex flex-col gap-2">
          <span className="flex flex-col gap-0.5">
            <span className="text-xs font-medium" style={{ color: "#E8E8E8" }}>
              {CURSOR_LOGOUT_TITLE}
            </span>
            <span className="text-xs leading-relaxed" style={{ color: "#B0B0B0" }}>
              {CURSOR_LOGOUT_CONFIRM}
            </span>
          </span>
          <div className="flex justify-end gap-2">
            <SmallButton onClick={onCancelLogout}>取消</SmallButton>
            <button
              className="rounded-md bg-red-500 px-3 py-1 text-xs font-medium text-white"
              onClick={onConfirmLogout}
            >
              退出
            </button>
          </div>
        </div>
      ) : (
        <div className="flex items-center gap-2">
          <span
            className={`min-w-0 grow text-xs ${status.state === "waiting" ? "leading-relaxed" : "truncate"}`}
            style={{ color: "#B0B0B0" }}
            title={row.text ?? undefined}
          >
            {row.text}
          </span>
          {row.secondary && (
            <button
              className="shrink-0 text-xs underline-offset-2 hover:underline"
              style={{ color: "#9E9E9E" }}
              onClick={() => onAction(row.secondary!.kind)}
            >
              {row.secondary.label}
            </button>
          )}
          {row.action && (
            <SmallButton onClick={() => onAction(row.action!.kind)}>{row.action.label}</SmallButton>
          )}
        </div>
      )}
      {errorText && <span className="text-[11px] text-red-400">{errorText}</span>}
    </div>
  );
}

function QuotaStatusLabel({ text }: { text: string }) {
  const color = text === "出错" ? "#FF6B6B" : "#8C8C8C";
  return <span className="truncate text-[10.5px]" style={{ color }}>{text}</span>;
}

/** Collapsed row that still shows its state, so a set-once setting costs one
 *  line instead of a screen. */
function Disclosure({
  label,
  summary,
  expanded,
  onToggle,
  children,
}: {
  label: string;
  summary?: string;
  expanded: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col">
      <button
        aria-expanded={expanded}
        className="flex min-h-[38px] w-full items-center gap-2 px-3 py-1.5 text-left"
        onClick={onToggle}
      >
        <ChevronRight
          size={12}
          strokeWidth={2.5}
          className="shrink-0 transition-transform duration-150"
          style={{ color: "#8C8C8C", transform: expanded ? "rotate(90deg)" : undefined }}
        />
        <span className="shrink-0 text-[13px]">{label}</span>
        <span className="grow" />
        {summary && (
          <span className="min-w-0 truncate text-[11px]" style={{ color: "#8C8C8C" }} title={summary}>
            {summary}
          </span>
        )}
      </button>
      {expanded && <div className="flex flex-col gap-2.5 px-3 pb-3 pl-[38px]">{children}</div>}
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div
      className="flex min-h-[38px] items-center justify-between gap-3 px-3 py-1.5"
      style={{ borderColor: "#3A3A3C" }}
    >
      <span className="shrink-0 text-[13px]">{label}</span>
      {children}
    </div>
  );
}

/**
 * A pop-up menu like the macOS one (System Settings' 「显示滚动条」): the
 * button shows the chosen option; the list opens under it with a check on
 * that option. Arrow keys move, Enter or Space picks, Escape or a click
 * outside closes.
 */
function MenuSelect<T extends string>({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  disabled?: boolean;
  onChange: (value: T) => void;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const selected = Math.max(0, options.findIndex((option) => option.value === value));

  useEffect(() => {
    if (!open) return;
    setActive(selected);
    list.current?.focus();
    const close = (event: MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    // The settings window is hidden, not destroyed, when closed: a menu left
    // open must not greet the user next time.
    const dismiss = () => setOpen(false);
    document.addEventListener("mousedown", close);
    window.addEventListener("blur", dismiss);
    document.addEventListener("visibilitychange", dismiss);
    return () => {
      document.removeEventListener("mousedown", close);
      window.removeEventListener("blur", dismiss);
      document.removeEventListener("visibilitychange", dismiss);
    };
  }, [open, selected]);

  // Picking or cancelling hands the keyboard back to the button.
  const closeToButton = () => {
    setOpen(false);
    button.current?.focus();
  };

  const pick = (index: number) => {
    closeToButton();
    if (options[index].value !== value) onChange(options[index].value);
  };

  return (
    <div ref={root} className="relative shrink-0">
      <button
        ref={button}
        type="button"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
          }
        }}
        className="flex items-center gap-1.5 rounded-md bg-[#48484A] py-1 pl-2.5 pr-1.5 text-xs text-white outline-none focus-visible:ring-2 focus-visible:ring-[#0A84FF] disabled:opacity-50"
      >
        <span>{options[selected]?.label}</span>
        <ChevronsUpDown size={12} className="text-white/70" />
      </button>
      {open && (
        <ul
          ref={list}
          role="listbox"
          aria-label={label}
          tabIndex={-1}
          aria-activedescendant={`${label}-${active}`}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setActive((index) => Math.min(options.length - 1, index + 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setActive((index) => Math.max(0, index - 1));
            } else if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              pick(active);
            } else if (event.key === "Escape") {
              event.preventDefault();
              closeToButton();
            } else if (event.key === "Tab") {
              setOpen(false);
            }
          }}
          className="absolute right-0 top-full z-20 mt-1 min-w-full rounded-lg border border-white/10 bg-[#2C2C2E] p-1 shadow-xl outline-none"
        >
          {options.map((option, index) => (
            <li
              key={option.value}
              id={`${label}-${index}`}
              role="option"
              aria-selected={index === selected}
              onMouseEnter={() => setActive(index)}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => pick(index)}
              className="flex cursor-default items-center gap-1.5 whitespace-nowrap rounded-md py-1 pl-1.5 pr-3 text-xs text-white"
              style={{ background: index === active ? "#0A84FF" : "transparent" }}
            >
              <Check size={12} className={index === selected ? "opacity-100" : "opacity-0"} />
              {option.label}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Toggle({
  checked,
  disabled,
  onChange,
}: {
  checked: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className="relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors duration-150 disabled:opacity-50"
      style={{ background: checked ? "#34C759" : "#48484A" }}
    >
      <span
        className="absolute top-[2px] h-[18px] w-[18px] rounded-full bg-white shadow transition-all duration-150"
        style={{ left: checked ? 18 : 2 }}
      />
    </button>
  );
}

function settingsUpdateButtonLabel(phase: UpdatePhase): string {
  switch (phase.kind) {
    case "idle":
      return "安装更新";
    case "downloading":
      return "正在下载…";
    case "launching":
      return "正在启动安装…";
    case "failed":
      return "重试";
  }
}

function SmallButton({
  children,
  disabled,
  onClick,
}: {
  children: React.ReactNode;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      disabled={disabled}
      onClick={onClick}
      className="shrink-0 whitespace-nowrap rounded-md px-2.5 py-1 text-xs disabled:opacity-50"
      style={{ background: "#48484A", color: "#E8E8E8" }}
    >
      {children}
    </button>
  );
}
