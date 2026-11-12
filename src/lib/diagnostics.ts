// 「复制诊断信息」: lays out the app's typed diagnostics as short fixed lines
// that fit a chat message, then runs a final scrub over the whole text.
//
// Approved by the maintainer on 2026-11-09 under the architecture approval
// gate (一键诊断方案修订二). Whitelist only: the backend sends enums, numbers,
// booleans and fixed templates; every free string it does send (versions,
// variable names, PATH file names, the device tag, the CLI doctor's codes and
// tool names) is re-checked against its expected shape here, and anything
// else is dropped. Reasons reuse the card's fixed wording (quotaFailureText);
// the CLI doctor section reuses the CLI's own `doctor` wording. Nothing is
// uploaded: the user copies it.

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
  /** The launcher ran its newest cached CLI because `latest` was out of reach here. */
  cliFallback?: LauncherStage | null;
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

// --- the CLI's `doctor --json`, as the backend passes it on (cli_doctor.rs) ---

export interface DoctorCounts {
  buckets: number;
  sessions: number;
}

export interface DoctorProbe {
  reachable: boolean;
  latencyMs?: number | null;
  error?: string | null;
}

export interface DoctorSync {
  startedAt?: number | null;
  surface?: string | null;
  cliVersion?: string | null;
  /** `success` / `failed` / `partial`; absent = the sync recorded no end. */
  result?: string | null;
  durationMs?: number | null;
  uploaded: DoctorCounts;
  failure?: { code: string; status?: number | null } | null;
}

export interface DoctorToolSync {
  startedAt?: number | null;
  buckets: number;
  sessions: number;
  skipped: boolean;
  incomplete: boolean;
  indexing?: { completed: number; total: number } | null;
  warnings: Array<{ code: string; count: number }>;
  error?: string | null;
  serverDropped: number;
}

export interface DoctorTool {
  id: string;
  name: string;
  detected: boolean;
  extraRoots: number;
  mergedInto: string[];
  uploaded: DoctorCounts;
  lastSync?: DoctorToolSync | null;
}

export interface DoctorReport {
  cliVersion?: string | null;
  runtime?: { name: string; version?: string | null } | null;
  sqlite?: string | null;
  account: { configured?: boolean | null; server?: string | null; uploadState?: string | null };
  network: { server?: DoctorProbe | null; npmRegistry?: DoctorProbe | null; registry?: string | null };
  daemon: { platform?: string | null; installed?: boolean | null; mode?: string | null };
  settings: {
    uploadProject?: boolean | null;
    extraRoots?: number | null;
    kikiStartAt?: string | null;
    magpieStartAt?: string | null;
  };
  syncHistory: { available: boolean; entries: DoctorSync[] };
  tools: DoctorTool[];
}

export type CliDoctor =
  | { kind: "ok"; report: DoctorReport }
  | { kind: "startupFailed"; reason: SyncReason }
  | { kind: "timedOut" }
  | { kind: "noDoctorCommand" }
  | { kind: "unrecognizedOutput" }
  | { kind: "unsupportedSchema"; schemaVersion: number }
  | { kind: "failed"; exitCode: number };

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
    lastSyncKey: "noRecord" | "accepted" | "rejected" | "undetermined";
  };
  deviceTag?: string | null;
  network: {
    systemProxy: "none" | "loopback" | "remote" | "unrecognized";
    systemProxyPassedToSync: boolean;
    autoConfigScript: boolean;
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
  /** The CLI's `doctor --json`, or why it could not answer. Absent in reports
   *  from before step 2′. */
  doctor?: CliDoctor | null;
}

// --- shape checks (defence in depth: the backend already sends only these) ---

const VERSION = /^\d{1,6}(\.\d{1,6}){0,3}$/;
const NAME = /^[A-Z0-9_]{1,64}$/;
const PATH_MATCH = /^claude(\.[a-z0-9]{1,8})?$/;
const DEVICE_TAG = /^[0-9a-f]{4}$/;
const TOKEN = /^[A-Za-z0-9]{1,8}$/;
/** A word from the CLI doctor's vocabularies (`read_failed`, `node:sqlite`). */
const CODE = /^[a-z0-9][a-z0-9:_-]{0,39}$/;
const TOOL_ID = /^[a-z0-9][a-z0-9-]{0,39}$/;
const TOOL_NAME = /^[A-Za-z0-9][A-Za-z0-9 .+()-]{0,39}$/;

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

/**
 * How old cached numbers are: minutes below an hour, hours below two days,
 * then days. A stale Grok cache used to read 「数据 149962 分钟前」.
 */
function age(secondsAgo: number): string {
  const minutes = Math.max(0, Math.round(secondsAgo / 60));
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} 小时前`;
  return `${Math.floor(hours / 24)} 天前`;
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
  const fallback = record.cliFallback ? ` · 用了本机缓存的 CLI（${fallbackReason(record.cliFallback)}）` : "";
  return `${when} ${trigger} · ${result}${fallback} · ${seconds(record.durationMs)}`;
}

/** Why the launcher ran a cached CLI; the run's own result is reported as usual. */
function fallbackReason(stage: LauncherStage): string {
  switch (stage) {
    case "network":
      return "连不上 npm 源";
    case "registry":
      return "npm 源返回错误";
    case "download":
      return "新版下载或校验失败";
    case "install":
      return "新版安装失败";
    default:
      return "原因未知";
  }
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

/** One past attempt. No version: it was not recorded at the time, and the
 *  version queried now may belong to a Claude Code that updated since. */
function attemptLine(attempt: ProbeAttempt, index: number): string {
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
  const parts = [originText(attempt.origin, attempt.via), ...steps, result, exit];
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
      ? `    当前候选及版本（复制时查询）：${candidates
          .map(
            (candidate, index) =>
              `${index + 1}）${originText(candidate.origin, candidate.via)} · ${shaped(candidate.version, VERSION) ?? "版本未知"}`,
          )
          .join("；")}`
      : "    当前候选及版本（复制时查询）：没有找到 Claude Code 程序",
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
    lines.push(`    上次读取 ${clock(probe.at)}（不含当时的版本）：${desktop}${tried}`);
    probe.attempts.forEach((attempt, index) => lines.push(attemptLine(attempt, index)));
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

// --- CLI line and the CLI doctor section ---

/**
 * One line on the CLI: the version `doctor` actually ran, or the fixed reason
 * it could not answer (一键诊断方案第七节: 「CLI 启动阶段失败」「CLI 诊断超时」
 * 「CLI 较旧，没有诊断命令」). The rest of the report is there either way.
 */
function cliLine(report: DiagnosticsReport): string {
  const cachedVersion = shaped(report.cli.cachedVersion, VERSION);
  const cached = cachedVersion ? `引导器已缓存 ${cachedVersion}` : "引导器没有缓存";
  const doctor = report.doctor;
  if (!doctor) {
    return `CLI：${report.cli.launcherPresent ? "启动脚本正常" : "启动脚本缺失"} · ${cached} · 本报告不含 CLI 自己的诊断`;
  }
  if (doctor.kind === "ok") {
    return `CLI：启动正常 · 本次运行 ${shaped(doctor.report.cliVersion, VERSION) ?? "版本未知"} · ${cached}`;
  }
  const script = report.cli.launcherPresent ? "" : " · 启动脚本缺失";
  return `CLI：${doctorFailureText(doctor)}${script} · ${cached} · 本报告不含 CLI 诊断和工具明细`;
}

function doctorFailureText(doctor: Exclude<CliDoctor, { kind: "ok" }>): string {
  switch (doctor.kind) {
    case "startupFailed":
      return `启动阶段失败（${startupReason(doctor.reason)}）`;
    case "timedOut":
      return "CLI 诊断超时（30 秒内没有结束）";
    case "noDoctorCommand":
      return "CLI 较旧，没有诊断命令";
    case "unsupportedSchema": {
      const version = number(doctor.schemaVersion);
      const side = version != null && version > 1 ? "较新" : "较旧";
      return `CLI ${side}，诊断格式不认识（schemaVersion ${version ?? "?"}）`;
    }
    case "unrecognizedOutput":
      return "CLI 诊断输出无法识别";
    case "failed":
      return `CLI 诊断命令失败（退出码 ${number(doctor.exitCode) ?? "?"}）`;
    default:
      return "CLI 诊断结果未知";
  }
}

// Vocabularies of the CLI's doctor contract (vibe-usage docs/doctor-json.md),
// worded as the CLI's own `doctor` text. A code not listed here is shown as
// the code itself, as the contract asks.
const SURFACE_LABELS: Record<string, string> = {
  cli: "终端",
  daemon: "后台服务",
  "mac-app": "Mac App",
  "windows-app": "Windows App",
};
const UPLOAD_STATE_LABELS: Record<string, string> = {
  match: "上传记录与当前 Key 一致",
  mismatch: "上传记录属于另一个 Key（下次同步会整段重传）",
  unrecorded: "上传记录没有绑定 Key（旧版本写入，下次同步自动补上）",
  none: "还没有上传记录",
};
const SQLITE_LABELS: Record<string, string> = {
  "node:sqlite": "内置 node:sqlite",
  "sqlite3-cli": "系统 sqlite3 命令",
  none: "都没有（Cursor、OpenCode 等工具读不了）",
};
const PROBE_ERROR_LABELS: Record<string, string> = {
  timeout: "超时",
  dns_failed: "域名解析失败",
  connection_refused: "连接被拒绝",
  connection_reset: "连接被重置",
  tls_failed: "TLS 证书校验失败",
  network_failed: "网络失败",
};
const SYNC_FAILURE_LABELS: Record<string, string> = {
  unauthorized: "API Key 被服务器拒绝",
  settings_unavailable: "读取上传设置失败（网络或服务器）",
  network_failed: "网络失败",
  server_error: "服务器错误",
  invalid_response: "服务器返回无法解析",
  unexpected_error: "意外错误",
};
const WARNING_LABELS: Record<string, string> = {
  read_failed: "读文件失败",
  dir_unreadable: "读目录失败",
  extra_root_unavailable: "额外目录不存在或结构不对",
  extra_root_read_failed: "额外目录读取失败",
  unsupported_format: "格式不认识（工具可能改了格式，或文件损坏）",
  usage_missing: "有会话但没读到用量（工具可能改了格式）",
  legacy_sessions_pending: "旧格式会话待读取（需打开对应 App）",
  merge_failed: "同一会话的分段无法合并",
  store_unavailable: "数据库无法从本进程读取",
  sqlite_unavailable: "没有可用的 SQLite",
  decoder_unavailable: "缺少解压组件（zstd）",
  cursor_token_encrypted: "本地登录令牌已加密（Cursor 3.27+）",
  auth_rejected: "登录凭证被拒绝，需要在该工具里重新登录",
  network_timeout: "网络请求超时",
  network_failed: "网络请求失败",
  remote_http_error: "远端返回错误状态",
  export_unusable: "远端导出不可用，备用接口也没有读到用量",
  fallback_used: "已改用备用接口补回（模型名可能不完整）",
  backfill_paused: "历史补齐本次暂停，下次继续",
  account_changed: "登录账号已更换",
  events_regressed: "最新事件时间回退（可能换成了另一个账号）",
  upload_withheld: "部分窗口暂停上传（Magpie / Kiki 切点保护）",
  parser_error: "解析出错",
  invalid_result: "解析结果无效",
  other: "其他（新的或改过措辞的警告）",
};

/** A contract word in its own wording, the raw code when it is new, else 「未知」. */
function vocabulary(labels: Record<string, string>, value: string | null | undefined): string {
  const code = shaped(value, CODE);
  if (code == null) return "未知";
  return Object.prototype.hasOwnProperty.call(labels, code) ? labels[code] : code;
}

const count = (value: unknown) => (number(value) ?? 0).toLocaleString("en-US");

function countsText(counts: { buckets: number; sessions: number } | null | undefined): string {
  const sessions = number(counts?.sessions) ?? 0;
  return sessions
    ? `${count(counts?.buckets)} buckets / ${count(sessions)} sessions`
    : `${count(counts?.buckets)} buckets`;
}

const hasCounts = (counts: DoctorCounts | null | undefined) =>
  (number(counts?.buckets) ?? 0) > 0 || (number(counts?.sessions) ?? 0) > 0;

function latency(ms: number | null | undefined): string {
  const value = number(ms);
  if (value == null) return "";
  return value < 1000 ? ` ${Math.round(value)}ms` : ` ${(value / 1000).toFixed(1)}s`;
}

function probeText(label: string, probe: DoctorProbe | null | undefined): string {
  if (!probe) return `${label}：未知`;
  return probe.reachable
    ? `${label}可达${latency(probe.latencyMs)}`
    : `${label}连不上（${vocabulary(PROBE_ERROR_LABELS, probe.error)}）`;
}

function syncFailureText(failure: DoctorSync["failure"]): string {
  if (!failure) return "原因未知";
  const text = vocabulary(SYNC_FAILURE_LABELS, failure.code);
  const status = number(failure.status);
  return status != null ? `${text} ${status}` : text;
}

/**
 * The app's timeout that explains a CLI sync with no end: the app started it
 * (`windows-app`) and the CLI's start lies within that run, which the app
 * ended at 120 s. Without one the CLI record stays 「未记录结束，原因未知」 —
 * the app may have quit, the machine slept, or the sync is still running
 * (一键诊断方案第七节「两份怎么对上」).
 */
function appTimeoutFor(entry: DoctorSync, appSyncs: SyncRecord[]): SyncRecord | undefined {
  const started = number(entry.startedAt);
  if (started == null || entry.surface !== "windows-app") return undefined;
  return appSyncs.find((record) => {
    const appStart = number(record.startedAt);
    const duration = number(record.durationMs);
    return (
      record.outcome === "timedOut" &&
      appStart != null &&
      duration != null &&
      appStart <= started &&
      started <= appStart + duration + 1000
    );
  });
}

function doctorSyncLine(entry: DoctorSync, appSyncs: SyncRecord[]): string {
  const when = number(entry.startedAt) != null ? clock(entry.startedAt as number) : "?";
  const surface = shaped(entry.surface, CODE);
  const who = surface ? vocabulary(SURFACE_LABELS, surface) : "未知发起方";
  const uploaded = countsText(entry.uploaded);
  let result: string;
  switch (entry.result) {
    case null:
    case undefined: {
      const timeout = appTimeoutFor(entry, appSyncs);
      result = timeout ? `超时，${seconds(timeout.durationMs)} 时被 App 结束` : "未记录结束，原因未知";
      if (hasCounts(entry.uploaded)) result += ` · 已上传 ${uploaded}`;
      break;
    }
    case "success":
      result = `成功 ${seconds(entry.durationMs)} · ${hasCounts(entry.uploaded) ? `上传 ${uploaded}` : "无新数据"}`;
      break;
    case "partial":
      result = `部分完成 ${seconds(entry.durationMs)} · 已上传 ${uploaded} · ${syncFailureText(entry.failure)}`;
      break;
    case "failed":
      result = `失败 ${seconds(entry.durationMs)} · ${syncFailureText(entry.failure)}`;
      break;
    default:
      result = `结果 ${vocabulary({}, entry.result)}`;
  }
  const version = shaped(entry.cliVersion, VERSION);
  return `    ${when} ${who} · ${result}${version ? ` · CLI ${version}` : ""}`;
}

function toolHasProblem(tool: DoctorTool): boolean {
  const last = tool.lastSync;
  return Boolean(
    last &&
      (last.error || last.indexing || last.incomplete || number(last.serverDropped) || last.warnings?.length),
  );
}

function toolLine(tool: DoctorTool, name: string, width: number, names: Map<string, string>): string {
  const parts = [tool.detected ? "已检测" : "未检测到数据目录"];
  const extraRoots = number(tool.extraRoots) ?? 0;
  if (extraRoots > 0) parts.push(`额外目录 ${extraRoots} 个`);
  const last = tool.lastSync;
  const merged = (tool.mergedInto ?? []).map((id) => shaped(id, TOOL_ID)).filter((id): id is string => !!id);
  if (merged.length) {
    parts.push(`数据并入 ${merged.map((id) => names.get(id) ?? id).join(" / ")}`);
  } else if (!last) {
    parts.push("暂无同步记录");
  } else if (last.error) {
    parts.push(`出错：${vocabulary(WARNING_LABELS, last.error)}（${shaped(last.error, CODE) ?? "?"}）`);
  } else {
    const warnings = (last.warnings ?? []).filter((warning) => shaped(warning.code, CODE));
    const indexing = last.indexing;
    const healthy = !indexing && !last.incomplete && warnings.length === 0;
    parts.push(
      healthy && !number(last.buckets) && !number(last.sessions) ? "上次无新数据" : `上次 ${countsText(last)}`,
    );
    if (indexing) parts.push(`正在建立本地索引 ${count(indexing.completed)}/${count(indexing.total)}`);
    if (last.incomplete) parts.push("已跳过，结果不完整");
    for (const warning of warnings) {
      parts.push(`${vocabulary(WARNING_LABELS, warning.code)} ×${count(warning.count)}（${warning.code}）`);
    }
    if (healthy) parts.push("正常");
  }
  if (number(last?.serverDropped)) {
    parts.push(`服务端丢弃 ${count(last?.serverDropped)}（服务端还没收录这个来源）`);
  }
  if (hasCounts(tool.uploaded)) parts.push(`累计已上传 ${countsText(tool.uploaded)}`);
  return `    ${name.padEnd(width)}  ${parts.join(" · ")}`;
}

/** The CLI's own view, laid out like its `doctor` text. */
function doctorLines(doctor: DoctorReport, appSyncs: SyncRecord[]): string[] {
  const version = shaped(doctor.cliVersion, VERSION);
  const lines = [`CLI 诊断（doctor${version ? `，CLI ${version}` : ""}）：`];

  const runtimeName =
    doctor.runtime?.name === "bun" ? "Bun" : doctor.runtime?.name === "node" ? "Node" : "运行环境未知";
  const runtimeVersion = doctor.runtime ? ` ${shaped(doctor.runtime.version, VERSION) ?? "版本未知"}` : "";
  lines.push(`  运行环境（本次调用）：${runtimeName}${runtimeVersion} · SQLite：${vocabulary(SQLITE_LABELS, doctor.sqlite)}`);

  const account = doctor.account ?? {};
  const server = account.server === "default" ? "默认" : account.server === "custom" ? "自定义" : "未知";
  const accountParts = [`服务器：${server}`];
  if (account.configured) accountParts.push(vocabulary(UPLOAD_STATE_LABELS, account.uploadState));
  lines.push(`  账号：${accountParts.join(" · ")}`);

  const network = doctor.network ?? {};
  const registry =
    network.registry === "official" ? "npm 源（官方）" : network.registry === "custom" ? "npm 源（自定义）" : "npm 源";
  lines.push(`  网络：${probeText("服务器", network.server)} · ${probeText(registry, network.npmRegistry)}`);

  const daemon = doctor.daemon ?? {};
  const platform = shaped(daemon.platform, CODE);
  let daemonText: string;
  if (!platform) daemonText = "CLI 后台服务：本系统不支持";
  else if (!daemon.installed) daemonText = "CLI 后台服务：未安装";
  else {
    const mode = daemon.mode === "npx" ? "npx 每次取最新版" : daemon.mode === "pinned" ? "固定路径" : "运行方式未知";
    daemonText = `CLI 后台服务：已安装（${platform}，${mode}）`;
  }
  const entries = doctor.syncHistory?.entries ?? [];
  const lastDaemon = entries.find((entry) => entry.surface === "daemon");
  if (lastDaemon && number(lastDaemon.startedAt) != null) {
    const result =
      { success: "成功", failed: "失败", partial: "部分完成" }[lastDaemon.result ?? ""] ?? "未记录结束";
    daemonText += ` · 最近一次后台同步 ${clock(lastDaemon.startedAt as number)} ${result}`;
  }
  lines.push(`  后台：${daemonText}`);

  const settings = doctor.settings ?? {};
  const project =
    settings.uploadProject === true ? "上传" : settings.uploadProject === false ? "已隐藏" : "未知（还没成功读过服务端设置）";
  const kiki = settings.kikiStartAt === "set" ? "已设" : "未设";
  const magpie = { set: "已设", none: "全部历史", unset: "未设" }[settings.magpieStartAt ?? ""] ?? "未设";
  lines.push(
    `  设置：项目名：${project} · 额外数据目录 ${number(settings.extraRoots) ?? 0} 个 · Kiki 切点：${kiki} · Magpie 切点：${magpie}`,
  );

  lines.push("  CLI 同步记录（新的在前）：");
  if (entries.length === 0) lines.push("    暂无同步记录");
  for (const entry of entries.slice(0, 10)) lines.push(doctorSyncLine(entry, appSyncs));

  const tools = (doctor.tools ?? []).filter((tool) => shaped(tool.id, TOOL_ID));
  const names = new Map(tools.map((tool) => [tool.id, shaped(tool.name, TOOL_NAME) ?? tool.id]));
  const shown = tools.filter((tool) => tool.detected || toolHasProblem(tool));
  lines.push("  工具（只列检测到的和有问题的）：");
  if (shown.length === 0) lines.push("    未检测到任何工具");
  const width = Math.max(0, ...shown.map((tool) => (names.get(tool.id) ?? tool.id).length));
  for (const tool of shown) lines.push(toolLine(tool, names.get(tool.id) ?? tool.id, width, names));
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

  lines.push(cliLine(report));

  const account = report.account;
  // Only the newest sync counts, and only success or a rejected key is a
  // verdict (same wording on macOS, Windows and the CLI).
  const verdict =
    {
      noRecord: "暂无同步记录",
      accepted: "最近一次同步未被服务器拒绝",
      rejected: "最近一次同步被服务器拒绝（Key 无效）",
      undetermined: "最近一次同步无法判断 Key 是否被拒",
    }[account.lastSyncKey] ?? "最近一次同步无法判断 Key 是否被拒";
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
  const pac = network.autoConfigScript ? " · 自动代理脚本：有" : "";
  lines.push(`网络：${proxy}${handedOn}${pac}`);
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
      parts.push(`数据 ${age(report.generatedAt / 1000 - asOf)}`);
    }
    lines.push(`  ${name}  ${parts.join(" · ")}`);
  }

  if (report.claude.selected || report.claude.lastProbe || report.claude.candidates.length) {
    lines.push(...claudeLines(report));
  }
  if (report.doctor?.kind === "ok") lines.push(...doctorLines(report.doctor.report, report.syncs));
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
    // Long opaque strings, except plain variable names such as
    // CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC (exactly 40 characters).
    .replace(/[A-Za-z0-9_\-.=+/]{40,}/g, (match) => (/^[A-Z0-9_]+$/.test(match) ? match : "***"));
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
