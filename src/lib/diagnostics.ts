// 「复制诊断信息」: lays out the app's typed diagnostics as short fixed lines
// that fit a chat message, then runs a final scrub over the whole text.
//
// Approved by the maintainer on 2026-11-09 under the architecture approval
// gate (一键诊断方案修订二). Whitelist only: the backend sends enums, numbers,
// booleans and fixed templates; every free string it does send (versions,
// variable names, PATH file names, the device tag) is re-checked against its
// expected shape here, and anything else is dropped. Reasons reuse the card's
// fixed wording (quotaFailureText). Nothing is uploaded: the user copies it.

import { providerLabel, quotaEmptyStateText, quotaFailureText, quotaSourceText } from "./quotaProducts";
import {
  QuotaProduct,
  RateLimitEmptyReason,
  RateLimitLiveFailure,
  RateLimitProvider,
  RateLimitSource,
} from "./types";

export type SyncTrigger = "scheduled" | "manual";
export type SyncOutcome = "success" | "failed" | "timedOut" | "cliStartupFailed";
export type LauncherStage = "network" | "registry" | "download" | "install" | "other";
export type SyncReason =
  | { code: "runtimeUnavailable" }
  | { code: "spawnFailed" }
  | { code: "launcher"; stage: LauncherStage }
  | { code: "keyRejected" }
  | { code: "serverStatus"; status: number }
  | { code: "cliExit"; exitCode: number }
  | { code: "waitFailed" };

export interface SyncRecord {
  startedAt: number;
  trigger: SyncTrigger;
  outcome: SyncOutcome;
  durationMs: number;
  reason?: SyncReason | null;
}

export type LaunchOrigin =
  | { kind: "override" }
  | { kind: "localBin" }
  | { kind: "claudeLocal" }
  | { kind: "localPrograms" }
  | { kind: "path"; position: number; extension: "exe" | "cmd" | "bat" };
export type LaunchVia = "direct" | "shimBinary" | "shimScript";

export type ProcessExit =
  | { kind: "notStarted" }
  | { kind: "exited"; code?: number | null }
  | { kind: "endedByApp" };

export interface AnswerFields {
  rateLimitsAvailable?: boolean | null;
  fiveHour: boolean;
  sevenDay: boolean;
  plan: boolean;
  error: boolean;
}

export interface ProbeAttempt {
  origin: LaunchOrigin;
  via: LaunchVia;
  initializeMs?: number | null;
  getUsageMs?: number | null;
  failure?: RateLimitLiveFailure | null;
  exit: ProcessExit;
  answer?: AnswerFields | null;
}

export interface ProbeRecord {
  at: number;
  desktop: { kind: "noRecord" } | { kind: "used"; failure?: RateLimitLiveFailure | null };
  candidatesFound: number;
  attempts: ProbeAttempt[];
}

export interface CandidateNow {
  origin: LaunchOrigin;
  via: LaunchVia;
  version?: string | null;
}

export interface VariableNames {
  names: string[];
  other: number;
}

export interface DiagnosticsReport {
  schemaVersion: number;
  generatedAt: number;
  app: { version: string; build: "release" | "externalTest" | "development" };
  system: {
    windowsRelease?: number | null;
    displayVersion?: string | null;
    build?: number | null;
    architecture: string;
  };
  runtime?: { kind: "bundledNode" | "systemNode" | "bun"; version?: string | null } | null;
  cli: { launcherPresent: boolean; cachedVersion?: string | null };
  account: {
    keyConfigured: boolean;
    lastSyncKey: "noRecord" | "accepted" | "rejected" | "unfinished" | "undetermined";
  };
  deviceTag?: string | null;
  network: {
    systemProxy: "none" | "loopback" | "remote" | "unrecognized";
    systemProxyPassedToSync: boolean;
    variables: Array<{ name: string; user: boolean; system: boolean; app: boolean }>;
  };
  scheduler: { running: boolean; syncing: boolean; lastSuccessAt?: number | null };
  syncs: SyncRecord[];
  quotas: Array<{
    provider: RateLimitProvider;
    status: string;
    source?: RateLimitSource | null;
    liveFailure?: RateLimitLiveFailure | null;
    emptyReason?: RateLimitEmptyReason | null;
    meterCount: number;
    dataAsOf?: number | null;
  }>;
  claude: {
    selected: boolean;
    lastProbe?: ProbeRecord | null;
    candidates: CandidateNow[];
    pathMatches: string[];
    userVariables: VariableNames;
    settings:
      | { kind: "missing" }
      | { kind: "unreadable" }
      | { kind: "read"; env: VariableNames };
    configDirSet: boolean;
  };
}

// --- shape checks (defence in depth: the backend already sends only these) ---

const VERSION = /^\d{1,6}(\.\d{1,6}){0,3}$/;
const NAME = /^[A-Z0-9_]{1,64}$/;
const PATH_MATCH = /^claude(\.[a-z0-9]{1,8})?$/;
const DEVICE_TAG = /^[0-9a-f]{4}$/;
const TOKEN = /^[A-Za-z0-9]{1,8}$/;

const shaped = (value: string | null | undefined, pattern: RegExp): string | null =>
  typeof value === "string" && pattern.test(value) ? value : null;

const number = (value: unknown): number | null =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

// --- formatting helpers ---

const pad = (value: number) => String(value).padStart(2, "0");

function clock(epochMs: number, withDate = true): string {
  const date = new Date(epochMs);
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  return withDate ? `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${time}` : time;
}

function header(epochMs: number): string {
  const date = new Date(epochMs);
  const offset = -date.getTimezoneOffset();
  const sign = offset >= 0 ? "+" : "-";
  const hours = Math.floor(Math.abs(offset) / 60);
  const minutes = Math.abs(offset) % 60;
  const zone = `UTC${sign}${hours}${minutes ? `:${pad(minutes)}` : ""}`;
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${clock(epochMs, false)}（${zone}）`;
}

function seconds(ms: number | null | undefined): string {
  const value = number(ms);
  if (value == null) return "?";
  if (value < 10_000) return `${(value / 1000).toFixed(1)}s`;
  if (value < 60_000) return `${Math.round(value / 1000)}s`;
  const total = Math.round(value / 1000);
  return `${Math.floor(total / 60)}m${pad(total % 60)}s`;
}

function originText(origin: LaunchOrigin, via: LaunchVia): string {
  let base: string;
  switch (origin.kind) {
    case "override":
      base = "VIBE_USAGE_CLAUDE_BIN 指定的程序";
      break;
    case "localBin":
      base = "~\\.local\\bin\\claude.exe";
      break;
    case "claudeLocal":
      base = "~\\.claude\\local\\claude.exe";
      break;
    case "localPrograms":
      base = "%LOCALAPPDATA%\\Programs\\Claude Code\\claude.exe";
      break;
    case "path": {
      const position = number(origin.position) ?? "?";
      const extension = ["exe", "cmd", "bat"].includes(origin.extension) ? origin.extension : "?";
      base = `PATH 上第 ${position} 个（claude.${extension}）`;
      break;
    }
    default:
      base = "未知位置";
  }
  if (via === "shimBinary") return `${base} → 包内 claude.exe`;
  if (via === "shimScript") return `${base} → Node 运行 cli.js`;
  return base;
}

const sameOrigin = (left: LaunchOrigin, right: LaunchOrigin) =>
  JSON.stringify(left) === JSON.stringify(right);

function names(list: VariableNames | null | undefined): string {
  const shown = (list?.names ?? []).filter((name) => NAME.test(name));
  const hidden = (number(list?.other) ?? 0) + (list?.names ?? []).length - shown.length;
  const parts = [...shown];
  if (hidden > 0) parts.push(`另有 ${hidden} 个名字格式特殊，未列出`);
  return parts.length ? parts.join("、") : "无";
}

function syncLine(record: SyncRecord): string {
  const when = number(record.startedAt) != null ? clock(record.startedAt) : "?";
  const trigger = record.trigger === "manual" ? "手动" : "定时";
  const reason = record.reason;
  let result: string;
  switch (record.outcome) {
    case "success":
      result = "成功";
      break;
    case "timedOut":
      return `${when} ${trigger} · 超时，${seconds(record.durationMs)} 时被 App 结束`;
    case "cliStartupFailed":
      result = `CLI 启动阶段失败（${startupReason(reason)}）`;
      break;
    default:
      result = `失败（${failureReason(reason)}）`;
  }
  return `${when} ${trigger} · ${result} · ${seconds(record.durationMs)}`;
}

function startupReason(reason: SyncReason | null | undefined): string {
  if (reason?.code === "runtimeUnavailable") return "运行环境不可用";
  if (reason?.code === "spawnFailed") return "进程启动失败";
  if (reason?.code === "launcher") {
    switch (reason.stage) {
      case "network":
        return "引导器连不上 npm 源";
      case "registry":
        return "npm 源返回错误";
      case "download":
        return "下载或校验 CLI 失败";
      case "install":
        return "安装 CLI 失败";
      default:
        return "引导器其他错误";
    }
  }
  return "原因未知";
}

function failureReason(reason: SyncReason | null | undefined): string {
  switch (reason?.code) {
    case "keyRejected":
      return "Key 被服务器拒绝";
    case "serverStatus":
      return `服务器错误 ${number(reason.status) ?? "?"}`;
    case "cliExit":
      return `CLI 退出码 ${number(reason.exitCode) ?? "?"}`;
    case "waitFailed":
      return "等待进程失败";
    default:
      return "原因未知";
  }
}

function quotaStatusText(status: string, meterCount: number, provider: RateLimitProvider): string {
  switch (status) {
    case "ok":
      return `正常 · ${meterCount} 个窗口`;
    case "noData":
      return "无数据";
    case "unauthorized":
      return "需重新登录";
    case "retryableError":
      return "可重试";
    case "error":
      return "读取失败";
    case "disabled":
      return provider === "cursor" ? "待接入" : "未启用";
    case "notRead":
      return "本次启动以来未读取";
    default:
      return "状态未知";
  }
}

function attemptLine(attempt: ProbeAttempt, index: number, candidates: CandidateNow[]): string {
  const version = shaped(
    candidates.find((candidate) => sameOrigin(candidate.origin, attempt.origin))?.version,
    VERSION,
  );
  const timedOut = attempt.failure?.code === "claudeTimedOut";
  const steps: string[] = [];
  if (number(attempt.initializeMs) != null) steps.push(`initialize ${seconds(attempt.initializeMs)}`);
  else if (timedOut) steps.push("initialize 超时");
  if (number(attempt.getUsageMs) != null) steps.push(`get_usage ${seconds(attempt.getUsageMs)}`);
  else if (timedOut && number(attempt.initializeMs) != null) steps.push("get_usage 超时");
  const result = attempt.failure ? quotaFailureText(attempt.failure, "Claude") : "读取成功";
  const exit =
    attempt.exit.kind === "exited"
      ? number(attempt.exit.code) != null
        ? `退出码 ${attempt.exit.code}`
        : "已退出"
      : attempt.exit.kind === "endedByApp"
        ? "被 App 结束"
        : "未启动";
  const parts = [originText(attempt.origin, attempt.via), version ?? "版本未知", ...steps, result, exit];
  const answer = attempt.answer;
  if (answer) {
    const available =
      answer.rateLimitsAvailable == null ? "缺失" : answer.rateLimitsAvailable ? "true" : "false";
    const has = (value: boolean) => (value ? "有" : "无");
    parts.push(
      `回答：rate_limits_available ${available} · 5h ${has(answer.fiveHour)} · 7d ${has(answer.sevenDay)} · 套餐 ${has(answer.plan)} · error ${has(answer.error)}`,
    );
  }
  return `      ${index + 1}）${parts.join(" · ")}`;
}

function claudeLines(report: DiagnosticsReport): string[] {
  const claude = report.claude;
  const lines = ["  Claude 读取明细："];
  const candidates = claude.candidates ?? [];
  lines.push(
    candidates.length
      ? `    当前候选：${candidates
          .map(
            (candidate, index) =>
              `${index + 1}）${originText(candidate.origin, candidate.via)} · ${shaped(candidate.version, VERSION) ?? "版本未知"}`,
          )
          .join("；")}`
      : "    当前候选：没有找到 Claude Code 程序",
  );
  const probe = claude.lastProbe;
  if (!probe) {
    lines.push("    上次读取：本次启动以来没有读取过");
  } else {
    const desktop =
      probe.desktop.kind === "used"
        ? `用了桌面端记录${probe.desktop.failure ? `（${quotaFailureText(probe.desktop.failure, "Claude")}）` : ""}，没有启动 Claude Code`
        : "桌面端记录：没有";
    const tried =
      probe.desktop.kind === "used"
        ? ""
        : ` · 找到候选 ${number(probe.candidatesFound) ?? 0} 个，试了 ${probe.attempts.length} 个`;
    lines.push(`    上次读取 ${clock(probe.at)}：${desktop}${tried}`);
    probe.attempts.forEach((attempt, index) => lines.push(attemptLine(attempt, index, candidates)));
  }
  const counts = new Map<string, number>();
  for (const match of claude.pathMatches ?? []) {
    const name = shaped(match, PATH_MATCH) ?? "其他";
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  lines.push(
    `    PATH 上的 claude.*：${
      counts.size ? [...counts].map(([name, count]) => `${name} ×${count}`).join("、") : "无"
    }`,
  );
  const settings =
    claude.settings.kind === "read"
      ? names(claude.settings.env)
      : claude.settings.kind === "unreadable"
        ? "文件无法解析"
        : "文件不存在";
  lines.push(
    `    相关设置（只列名字）：ANTHROPIC/CLAUDE 开头的用户变量：${names(claude.userVariables)} · settings.json 的 env：${settings} · CLAUDE_CONFIG_DIR ${claude.configDirSet ? "已设" : "未设"}`,
  );
  return lines;
}

/** The report text before the final scrub. Exported for tests. */
export function formatDiagnosticsReport(
  report: DiagnosticsReport,
  products: QuotaProduct[],
): string {
  const lines: string[] = [];
  lines.push(`Vibe Usage 诊断报告 v${number(report.schemaVersion) ?? "?"} · ${header(report.generatedAt)}`);

  const system = report.system;
  const release = number(system.windowsRelease);
  const display = shaped(system.displayVersion, TOKEN);
  const build = number(system.build);
  const windows = release
    ? `Windows ${release}${display ? ` ${display}` : ""}${build ? `（build ${build}）` : ""}`
    : "Windows（版本未知）";
  const arch = shaped(system.architecture, /^[a-z0-9_]{1,16}$/) ?? "?";
  const buildKind = { release: "正式版", externalTest: "外测版", development: "开发版" }[report.app.build] ?? "";
  const runtime = report.runtime
    ? `${{ bundledNode: "App 内置 Node", systemNode: "系统 Node", bun: "Bun" }[report.runtime.kind] ?? "未知"} ${shaped(report.runtime.version, VERSION) ?? "版本未知"}`
    : "未找到 Node.js 或 Bun";
  lines.push(
    `环境：${windows} · ${arch} · App ${shaped(report.app.version, VERSION) ?? "?"} ${buildKind} · 运行环境：${runtime}`,
  );

  const cached = shaped(report.cli.cachedVersion, VERSION);
  lines.push(
    `CLI：${report.cli.launcherPresent ? "启动脚本正常" : "启动脚本缺失"} · ${cached ? `引导器已缓存 ${cached}` : "引导器没有缓存"} · 本报告不含 CLI 自己的诊断`,
  );

  const account = report.account;
  // Only the newest sync counts (same rule as macOS): an older verdict behind
  // a timeout would point a network problem at the key.
  const verdict =
    {
      noRecord: "暂无同步记录",
      accepted: "最近一次同步未被服务器拒绝",
      rejected: "最近一次同步被服务器拒绝",
      unfinished: "最近一次同步没有完成，无法判断 Key 是否被拒",
      undetermined: "最近一次同步失败，无法判断 Key 是否被拒",
    }[account.lastSyncKey] ?? "无法判断 Key 是否被拒";
  const tag = shaped(report.deviceTag, DEVICE_TAG);
  lines.push(
    `账号：${account.keyConfigured ? "已配置 Key" : "未配置 Key"} · ${verdict}${tag ? ` · 设备标签 ${tag}` : ""}`,
  );

  const network = report.network;
  const proxy = {
    none: "系统代理：无",
    loopback: "系统代理：有（本机地址）",
    remote: "系统代理：有（外部地址）",
    unrecognized: "系统代理：已开启，但格式无法识别",
  }[network.systemProxy] ?? "系统代理：未知";
  const handedOn =
    network.systemProxy === "loopback" || network.systemProxy === "remote"
      ? ` · ${network.systemProxyPassedToSync ? "已传给同步" : "未传给同步（App 环境里已有 HTTPS_PROXY）"} · 未传给 Claude 配额读取`
      : "";
  lines.push(`网络：${proxy}${handedOn}`);
  const variables = (network.variables ?? []).map((variable) => {
    const name = shaped(variable.name, NAME) ?? "?";
    const where = [
      variable.user ? "用户变量" : null,
      variable.system ? "系统变量" : null,
      variable.app ? "App 环境" : null,
    ].filter(Boolean);
    return `${name} ${where.length ? `${where.join("、")}有` : "无"}`;
  });
  lines.push(`      代理变量：${variables.join(" · ") || "未知"}`);

  const scheduler = report.scheduler;
  lines.push(
    `后台：${
      scheduler.running
        ? "App 定时同步运行中（面板打开时每分钟，托盘时每 30 分钟）"
        : "App 定时同步未运行"
    }${scheduler.syncing ? " · 正在同步" : ""}${
      number(scheduler.lastSuccessAt) != null ? ` · 上次成功 ${clock(scheduler.lastSuccessAt as number)}` : ""
    }`,
  );

  lines.push("最近同步（App 记录，新的在前）：");
  if (report.syncs.length === 0) lines.push("  暂无记录");
  for (const record of report.syncs.slice(0, 10)) lines.push(`  ${syncLine(record)}`);

  lines.push("订阅配额：");
  if (report.quotas.length === 0) lines.push("  没有开启任何产品");
  for (const card of report.quotas) {
    const name = providerLabel(card.provider, products);
    const parts = [quotaStatusText(card.status, number(card.meterCount) ?? 0, card.provider)];
    if (card.emptyReason) {
      parts.push(
        quotaEmptyStateText(
          { provider: card.provider, status: { kind: "noData" }, emptyReason: card.emptyReason },
          true,
          false,
          name,
        ),
      );
    }
    if (card.liveFailure) parts.push(quotaFailureText(card.liveFailure, name));
    if (card.source) parts.push(`来源：${quotaSourceText(card.source)}`);
    const asOf = number(card.dataAsOf);
    if (asOf != null && card.source && card.source !== "live") {
      const minutes = Math.max(0, Math.round((report.generatedAt / 1000 - asOf) / 60));
      parts.push(`数据 ${minutes} 分钟前`);
    }
    lines.push(`  ${name}  ${parts.join(" · ")}`);
  }

  if (report.claude.selected || report.claude.lastProbe || report.claude.candidates.length) {
    lines.push(...claudeLines(report));
  }
  lines.push("—— 只含固定字段和错误码：不含 API Key、令牌、对话内容、项目名和任何目录名 ——");
  return lines.join("\n");
}

/**
 * Final safety net over the whole text (一键诊断方案第六节第 9 条). The layout
 * above emits none of these, so in practice this changes nothing; it exists
 * so that a future field cannot leak a secret on its own.
 */
export function scrubDiagnosticsText(text: string): string {
  return text
    // Any URL (a proxy address, with or without credentials) goes entirely.
    .replace(/[A-Za-z][A-Za-z0-9+.-]*:\/\/[^\s·]*/g, "***")
    .replace(/(?<![A-Za-z])[A-Za-z]:[\\/][^\n·]*/g, "~")
    .replace(/\\\\[^\\\s][^\n·]*/g, "~")
    .replace(/\/(?:Users|home)\/[^\n·]*/g, "~")
    .replace(/-(?:Users|home)-[^\s·]*/g, "***")
    .replace(/vbu_[A-Za-z0-9_-]+/g, "***")
    .replace(/sk-[A-Za-z0-9_-]+/g, "***")
    .replace(/Bearer\s+\S+/g, "Bearer ***")
    .replace(/eyJ[A-Za-z0-9_-]*(?:\.[A-Za-z0-9_-]*)*/g, "***")
    .replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, "***")
    .replace(/[A-Za-z0-9_\-.=+/]{40,}/g, "***");
}

export function buildDiagnosticsText(report: DiagnosticsReport, products: QuotaProduct[]): string {
  return scrubDiagnosticsText(formatDiagnosticsReport(report, products));
}

/** Clipboard write from a click; falls back to a hidden textarea. */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // WebView without clipboard permission: use the selection fallback.
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  const copied = document.execCommand("copy");
  area.remove();
  if (!copied) throw new Error("剪贴板不可用");
}
