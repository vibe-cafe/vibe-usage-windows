// Settings window — port of Views/SettingsView.swift (grouped Form, 420px
// content in a 460×620 window). macOS's "在 Dock 中显示" has no Windows
// equivalent (no Dock) and is intentionally omitted.

import { useCallback, useEffect, useState } from "react";
import { AlertCircle, CheckCircle2, ChevronRight, RefreshCw } from "lucide-react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, onDeviceLink, onSettingsShown, onSyncState } from "./lib/api";
import { formatInvokeError } from "./lib/errors";
import { extraRootsLoadPatch } from "./lib/extraRoots";
import {
  AppSettings,
  AppStatus,
  ExtraRoots,
  QuotaProduct,
  RateLimitProvider,
  SyncState,
  ZCodeCredentialStatus,
  ZCodeQuotaRegion,
} from "./lib/types";
import { formatRelativeTime } from "./lib/formatters";
import { compactQuotaStatus, isZCodeConfigured, quotaProductStatusText } from "./lib/quotaProducts";
import { ProviderIcon } from "./components/ProviderIcon";

export function SettingsApp() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [syncState, setSyncState] = useState<SyncState>({ status: "idle" });
  const [autoStart, setAutoStart] = useState(false);

  const [isRelinking, setIsRelinking] = useState(false);
  const [relinkUserCode, setRelinkUserCode] = useState<string | null>(null);
  const [relinkError, setRelinkError] = useState<string | null>(null);
  const [quotaError, setQuotaError] = useState<string | null>(null);
  const [quotaProducts, setQuotaProducts] = useState<QuotaProduct[]>([]);
  const [quotaBusy, setQuotaBusy] = useState(false);
  const [zCodeStatus, setZCodeStatus] = useState<ZCodeCredentialStatus>({
    bigModelConfigured: false,
    zAiConfigured: false,
  });
  const [zCodeApiKey, setZCodeApiKey] = useState("");
  const [zCodeMessage, setZCodeMessage] = useState<string | null>(null);
  const [showResetConfirm, setShowResetConfirm] = useState(false);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [diagnosticMessage, setDiagnosticMessage] = useState<string | null>(null);
  const [extraRoots, setExtraRoots] = useState<ExtraRoots>({});
  const [extraRootsBusy, setExtraRootsBusy] = useState(false);
  const [extraRootsError, setExtraRootsError] = useState<string | null>(null);
  const [isZCodeExpanded, setZCodeExpanded] = useState(false);
  const [isQuotaSourcesExpanded, setQuotaSourcesExpanded] = useState(false);
  const [isIsolatedRootsExpanded, setIsIsolatedRootsExpanded] = useState(false);

  const reload = useCallback(async () => {
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

    if (discovered) setQuotaProducts(discovered);
    if (nextStatus.status === "fulfilled") setStatus(nextStatus.value);
    if (nextSettings.status === "fulfilled") setSettings(nextSettings.value);
    if (nextSyncState.status === "fulfilled") setSyncState(nextSyncState.value);
    if (nextAutoStart.status === "fulfilled") setAutoStart(nextAutoStart.value);
    if (nextZCodeStatus.status === "fulfilled") setZCodeStatus(nextZCodeStatus.value);
    const extraPatch = extraRootsLoadPatch(nextExtraRoots);
    if (extraPatch.extraRoots !== undefined) {
      setExtraRoots(extraPatch.extraRoots);
    }
    setExtraRootsError(extraPatch.extraRootsError);
  }, []);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;

    void reload();
    const subs = [
      onSyncState(setSyncState),
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
      for (const p of subs) void p.then((un) => un());
    };
  }, [reload]);

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
    if (!settings) return;
    const next = { ...settings, ...patch };
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
    setUpdateMessage("检查中…");
    try {
      const info = await api.checkForUpdate();
      setUpdateMessage(info ? `发现新版本 ${info.version}` : "已是最新版本");
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
  const isolatedRootCount = Object.values(extraRoots).reduce(
    (total, paths) => total + (paths?.length ?? 0),
    0,
  );
  const isolatedRootsSummary = isolatedRootCount === 0 ? "未添加" : `${isolatedRootCount} 个目录`;

  return (
    <div
      className="h-screen overflow-hidden font-sans text-[13px]"
      style={{ background: "#1C1C1E", color: "#E8E8E8" }}
    >
      <div className="no-scrollbar mx-auto flex h-full max-w-[430px] flex-col gap-4 overflow-y-auto px-4 py-4">
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
        <Section
          title={`订阅配额（${settings?.selectedQuotaProductIds.length ?? 0}）`}
          footer="选中的产品会在面板中各显示一张卡片。"
        >
          {quotaProducts
            .filter((product) => product.provider !== "zcode")
            .map((product) => (
              <ProductRow
                key={product.provider}
                product={product}
                statusText={quotaProductStatusText(product)}
                selected={settings?.selectedQuotaProductIds.includes(product.provider) ?? false}
                disabled={quotaBusy}
                onToggle={(value) => void toggleQuotaProduct(product.provider, value)}
              />
            ))}

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
                  <ProviderIcon provider="zcode" />
                  <span className="flex min-w-0 flex-col">
                    <span className="text-[13px]">{zCodeProduct.displayName}</span>
                    <span className="truncate text-[10.5px]" style={{ color: "#8C8C8C" }}>
                      {compactQuotaStatus(
                        quotaProductStatusText(
                          zCodeProduct,
                          zCodeStatus,
                          settings?.zCodeQuotaRegion ?? "bigModel",
                        ),
                      )}
                    </span>
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
                  <span className="text-[10.5px]" style={{ color: "#8C8C8C" }}>
                    只使用这里填写的区域 Key，不读取 ZCode 登录凭据。
                  </span>
                  {zCodeMessage && <span className="text-[11px] text-emerald-400">{zCodeMessage}</span>}
                </div>
              )}
            </div>
          )}

          <Disclosure
            label="数据来源与检测"
            expanded={isQuotaSourcesExpanded}
            onToggle={() => setQuotaSourcesExpanded((value) => !value)}
          >
            <p className="text-[10.5px] leading-relaxed" style={{ color: "#8C8C8C" }}>
              检测状态仅用于推荐，所有产品都可手动选择；未选择的产品不会联网读取配额。
            </p>
            <p className="text-[10.5px] leading-relaxed" style={{ color: "#8C8C8C" }}>
              Grok 读取本机订阅记录；Cursor 复用客户端登录；Antigravity / AGY 使用官方 agy 登录。配额仅在本机展示。
            </p>
            <p className="text-[10.5px] leading-relaxed" style={{ color: "#8C8C8C" }}>
              Kimi Code 使用其官方 CLI 登录。
            </p>
            <div className="flex justify-end">
              <SmallButton disabled={quotaBusy} onClick={() => void rediscoverQuotaProducts()}>
                <span className="flex items-center gap-1.5">
                  <RefreshCw size={11} /> 重新检测
                </span>
              </SmallButton>
            </div>
          </Disclosure>

          {quotaError && (
            <div
              title={quotaError}
              className="line-clamp-2 break-words px-3 py-2 text-xs text-red-400"
            >
              {quotaError}
            </div>
          )}
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
            <p className="text-[10.5px] leading-relaxed" style={{ color: "#8C8C8C" }}>
              可为每种工具添加多个 Multica 或其他隔离目录；默认目录仍会照常统计。
            </p>
            {extraRootsError && <span className="text-xs text-red-400">{extraRootsError}</span>}
          </Disclosure>
        </Section>

        {status?.testDiagnosticsAvailable && (
          <Section title="测试诊断" footer="仅测试构建可用；只含脱敏错误码、Provider、版本与系统信息，不含路径、账号、Key。">
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
            <div className="flex items-center gap-2">
              {updateMessage && (
                <span className="text-xs" style={{ color: "#9E9E9E" }}>
                  {updateMessage}
                </span>
              )}
              {status?.updatesAvailable ? (
                <SmallButton onClick={() => void checkUpdate()}>检查更新</SmallButton>
              ) : status ? (
                <span className="text-xs text-neutral-400">外测版不检查更新</span>
              ) : null}
            </div>
          </Row>
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

/** One product switch: official icon, display name, and the discovery/read
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
        <ProviderIcon provider={product.provider} />
        <span className="flex min-w-0 flex-col">
          <span className="text-[13px]">{product.displayName}</span>
          <span className="truncate text-[10.5px]" style={{ color: "#8C8C8C" }}>
            {statusText}
          </span>
        </span>
      </div>
      <Toggle checked={selected} disabled={disabled} onChange={onToggle} />
    </div>
  );
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
      className="rounded-md px-2.5 py-1 text-xs disabled:opacity-50"
      style={{ background: "#48484A", color: "#E8E8E8" }}
    >
      {children}
    </button>
  );
}
