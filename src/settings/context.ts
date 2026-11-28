// Everything the five pages read and do, kept per window so switching pages
// never drops an in-flight re-link, a half-typed key or a save result (macOS
// `SettingsModel` + `SettingsNavigation`). `SettingsApp` owns the state and
// talks to the backend; the pages only read this.

import { createContext, useContext } from "react";
import type { SyncRecord } from "../lib/diagnostics";
import type { EgressMode } from "../lib/egressMode";
import type { QuotaProductStatus } from "../lib/quotaProducts";
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
  ZCodeQuotaRegion,
} from "../lib/types";
import type { UpdatePhase } from "../lib/updateButton";
import type { SettingsPage } from "./model";

export type ExtraRootSource = "codex" | "grok" | "antigravity";

export interface SettingsModel {
  status: AppStatus | null;
  /** 「Vibe Usage」, or 「Vibe Usage Test」 in external-test builds. */
  appName: string;
  settings: AppSettings | null;

  page: SettingsPage;
  setPage: (page: SettingsPage) => void;

  // 通用
  autoStart: boolean;
  setAutoStart: (enabled: boolean) => void;
  patchSettings: (patch: Partial<AppSettings>) => void;

  // 数据同步
  syncState: SyncState;
  syncHistory: SyncRecord[];
  relinking: boolean;
  relinkUserCode: string | null;
  relinkError: string | null;
  startRelink: () => void;
  cancelRelink: () => void;
  askReset: () => void;

  // 订阅配额
  quotaProducts: QuotaProduct[];
  /** null while the panel refreshes (hide the previous warning). */
  quotaSnapshots: ProviderRateLimit[] | null;
  quotaBusy: boolean;
  quotaError: string | null;
  enabledOrder: RateLimitProvider[];
  disabledOrder: RateLimitProvider[];
  quotaStatus: (product: QuotaProduct) => QuotaProductStatus;
  toggleQuotaProduct: (provider: RateLimitProvider, selected: boolean) => void;
  rediscoverQuotaProducts: () => void;
  /** Drop `provider` before `target` (`null`: the end of its own group). */
  moveQuotaProduct: (provider: RateLimitProvider, target: RateLimitProvider | null) => void;
  expanded: ReadonlySet<RateLimitProvider>;
  toggleExpanded: (provider: RateLimitProvider) => void;
  /** The row to flash after arriving from the panel; `request` changes on
   *  every arrival, so clicking the same tab twice flashes twice. */
  flash: { provider: RateLimitProvider; request: number } | null;
  egressMode: EgressMode;

  // ZCode
  zCodeRegion: ZCodeQuotaRegion;
  zCodeConfigured: boolean;
  zCodeDraft: string;
  setZCodeDraft: (value: string) => void;
  zCodeReplacing: boolean;
  beginReplacingZCodeKey: () => void;
  cancelReplacingZCodeKey: () => void;
  zCodeError: string | null;
  setZCodeRegion: (region: ZCodeQuotaRegion) => void;
  saveZCodeKey: () => void;
  removeZCodeKey: () => void;

  // Cursor
  cursorLogin: CursorLoginStatus | null;
  cursorLoginError: string | null;
  cursorLoginAction: (kind: "start" | "cancel" | "logout") => void;

  // 数据目录
  extraRoots: ExtraRoots;
  extraRootsBusy: boolean;
  extraRootsError: string | null;
  addExtraRoot: (source: ExtraRootSource) => void;
  removeExtraRoot: (source: ExtraRootSource, path: string) => void;

  // 关于
  updateOffer: UpdateInfo | null;
  updatePhase: UpdatePhase;
  updateMessage: string | null;
  checkUpdate: () => void;
  installUpdate: () => void;
  diagnosticsBusy: boolean;
  diagnosticsText: string | null;
  diagnosticsCopied: boolean;
  diagnosticsMessage: string | null;
  copyDiagnostics: () => void;
  testExportMessage: string | null;
  exportTestDiagnostics: () => void;
  openExternal: (url: string) => void;
}

export const SettingsContext = createContext<SettingsModel | null>(null);

export function useSettings(): SettingsModel {
  const model = useContext(SettingsContext);
  if (!model) throw new Error("useSettings outside SettingsContext");
  return model;
}
