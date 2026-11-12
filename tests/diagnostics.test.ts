import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  buildDiagnosticsText,
  CliDoctor,
  DiagnosticsReport,
  DoctorReport,
  formatDiagnosticsReport,
  scrubDiagnosticsText,
} from "../src/lib/diagnostics";
import { QuotaProduct } from "../src/lib/types";

const products: QuotaProduct[] = [
  { provider: "claudeCode", displayName: "Claude", availability: "ready", isDetected: true },
  { provider: "codex", displayName: "Codex", availability: "ready", isDetected: true },
  { provider: "kimi-code", displayName: "Kimi Code", availability: "ready", isDetected: true },
  { provider: "opencode-go", displayName: "OpenCode Go", availability: "ready", isDetected: true },
];

/** Synthetic values that must never appear in a copied report. */
const SECRETS = [
  "张三",
  "张 三",
  "My Projects",
  "-Users-zhangsan-Coding-secret-project",
  "secret-project",
  "vbu_test_0123456789abcdef",
  "sk-ant-api03-FAKEFAKEFAKE",
  "someone@example.com",
  "eyJhbGciOiJIUzI1NiJ9",
  "user:p%40ss",
  "10.0.0.5",
  "ZhangSan-PC",
];

const NOW = Date.UTC(2026, 10, 9, 6, 32);

function report(overrides: Partial<DiagnosticsReport> = {}): DiagnosticsReport {
  return {
    schemaVersion: 1,
    generatedAt: NOW,
    app: { version: "0.7.5", build: "release" },
    system: { windowsRelease: 11, displayVersion: "23H2", build: 22631, architecture: "x86_64" },
    runtime: { kind: "bundledNode", version: "22.23.1" },
    cli: { launcherPresent: true, cachedVersion: "0.14.9" },
    account: { keyConfigured: true, lastSyncKey: "accepted" },
    deviceTag: "7c2e",
    network: {
      systemProxy: "loopback",
      systemProxyPassedToSync: true,
      autoConfigScript: false,
      variables: [
        { name: "HTTPS_PROXY", user: true, system: false, app: false },
        { name: "HTTP_PROXY", user: false, system: false, app: false },
        { name: "ALL_PROXY", user: false, system: true, app: false },
      ],
    },
    scheduler: { running: true, syncing: false, lastSuccessAt: NOW - 30 * 60_000 },
    syncs: [
      { startedAt: NOW - 10 * 60_000, trigger: "scheduled", outcome: "success", durationMs: 18_000 },
      { startedAt: NOW - 40 * 60_000, trigger: "manual", outcome: "timedOut", durationMs: 120_000 },
      {
        startedAt: NOW - 70 * 60_000,
        trigger: "scheduled",
        outcome: "cliStartupFailed",
        durationMs: 21_000,
        reason: { code: "launcher", stage: "network" },
      },
      {
        startedAt: NOW - 100 * 60_000,
        trigger: "scheduled",
        outcome: "failed",
        durationMs: 3_000,
        reason: { code: "serverStatus", status: 502 },
      },
    ],
    quotas: [
      {
        provider: "claudeCode",
        status: "ok",
        source: "cache",
        liveFailure: { code: "claudeTimedOut" },
        meterCount: 2,
        dataAsOf: NOW / 1000 - 30 * 3600,
      },
      { provider: "codex", status: "ok", source: "live", meterCount: 2 },
      { provider: "kimi-code", status: "unauthorized", meterCount: 0 },
    ],
    claude: {
      selected: true,
      lastProbe: {
        at: NOW - 60_000,
        desktop: { kind: "noRecord" },
        candidatesFound: 2,
        attempts: [
          {
            origin: { kind: "localBin" },
            via: "direct",
            initializeMs: 900,
            failure: { code: "claudeTimedOut" },
            exit: { kind: "endedByApp" },
          },
          {
            origin: { kind: "path", position: 3, extension: "cmd" },
            via: "shimBinary",
            initializeMs: 1200,
            getUsageMs: 2300,
            failure: { code: "claudeNoWindows" },
            exit: { kind: "exited", code: 0 },
            answer: { rateLimitsAvailable: true, fiveHour: false, sevenDay: false, plan: true, error: true },
          },
        ],
      },
      candidates: [
        { origin: { kind: "localBin" }, via: "direct", version: "2.1.282" },
        { origin: { kind: "path", position: 3, extension: "cmd" }, via: "shimBinary", version: "2.1.319" },
      ],
      pathMatches: ["claude.exe", "claude", "claude.cmd"],
      userVariables: { names: ["ANTHROPIC_BASE_URL"], other: 0 },
      settings: { kind: "read", env: { names: [], other: 0 } },
      configDirSet: false,
    },
    ...overrides,
  };
}

describe("copyable diagnostics report", () => {
  it("lays out every section as short fixed lines", () => {
    const text = buildDiagnosticsText(report(), products);
    expect(text).toContain("环境：Windows 11 23H2（build 22631） · x86_64 · App 0.7.5 正式版 · 运行环境：App 内置 Node 22.23.1");
    expect(text).toContain("CLI：启动脚本正常 · 引导器已缓存 0.14.9");
    expect(text).toContain("账号：已配置 Key · 最近一次同步未被服务器拒绝 · 设备标签 7c2e");
    expect(text).toContain("网络：系统代理：有（本机地址） · 已传给同步 · 未传给 Claude 配额读取");
    expect(text).toContain("代理变量：HTTPS_PROXY 用户变量有 · HTTP_PROXY 无 · ALL_PROXY 系统变量有");
    expect(text).toContain("定时 · 成功 · 18s");
    expect(text).toContain("手动 · 超时，2m00s 时被 App 结束");
    expect(text).toContain("CLI 启动阶段失败（引导器连不上 npm 源）");
    expect(text).toContain("失败（服务器错误 502）");
    expect(text).toContain("Claude  正常 · 2 个窗口 · Claude 配额读取超时 · 来源：缓存 · 数据 30 小时前");
    expect(text).toContain("Kimi Code  需重新登录");
    expect(text).toContain(
      "1）~\\.local\\bin\\claude.exe · initialize 0.9s · get_usage 超时 · Claude 配额读取超时 · 被 App 结束",
    );
    expect(text).toContain(
      "2）PATH 上第 3 个（claude.cmd） → 包内 claude.exe · initialize 1.2s · get_usage 2.3s · Claude 没有返回额度窗口 · 退出码 0",
    );
    expect(text).toContain("回答：rate_limits_available true · 5h 无 · 7d 无 · 套餐 有 · error 有");
    expect(text).toContain("PATH 上的 claude.*：claude.exe ×1、claude ×1、claude.cmd ×1");
    expect(text).toContain(
      "相关设置（只列名字）：ANTHROPIC/CLAUDE 开头的用户变量：ANTHROPIC_BASE_URL · settings.json 的 env：无 · CLAUDE_CONFIG_DIR 未设",
    );
    // The fallback scrub leaves a clean report untouched.
    expect(text).toBe(formatDiagnosticsReport(report(), products));
  });

  it("still reports the app's half when the CLI never started", () => {
    const text = buildDiagnosticsText(
      report({
        runtime: null,
        cli: { launcherPresent: true, cachedVersion: null },
        account: { keyConfigured: true, lastSyncKey: "undetermined" },
        syncs: [
          {
            startedAt: NOW,
            trigger: "scheduled",
            outcome: "cliStartupFailed",
            durationMs: 20_500,
            reason: { code: "runtimeUnavailable" },
          },
        ],
      }),
      products,
    );
    expect(text).toContain("运行环境：未找到 Node.js 或 Bun");
    expect(text).toContain("引导器没有缓存");
    expect(text).toContain("最近一次同步无法判断 Key 是否被拒");
    expect(text).toContain("CLI 启动阶段失败（运行环境不可用）");
  });

  it("reports a run on the cached CLI by its own result plus the reason", () => {
    const at = (minutes: number) => NOW - minutes * 60_000;
    const text = buildDiagnosticsText(
      report({
        syncs: [
          { startedAt: at(1), trigger: "scheduled", outcome: "success", durationMs: 9_000, cliFallback: "network" },
          {
            startedAt: at(2),
            trigger: "manual",
            outcome: "failed",
            durationMs: 2_000,
            reason: { code: "serverStatus", status: 502 },
            cliFallback: "download",
          },
          { startedAt: at(3), trigger: "scheduled", outcome: "success", durationMs: 9_000, cliFallback: "registry" },
          { startedAt: at(4), trigger: "scheduled", outcome: "success", durationMs: 9_000, cliFallback: "install" },
          { startedAt: at(5), trigger: "scheduled", outcome: "success", durationMs: 9_000, cliFallback: "other" },
          { startedAt: at(6), trigger: "scheduled", outcome: "success", durationMs: 9_000, cliFallback: null },
        ],
      }),
      products,
    );
    const lines = text.split("\n").filter((line) => line.includes(" · ") && /^\s+\d\d-\d\d/.test(line));
    expect(lines.map((line) => line.replace(/^\s+\d\d-\d\d \d\d:\d\d /, ""))).toEqual([
      "定时 · 成功 · 用了本机缓存的 CLI（连不上 npm 源） · 9.0s",
      "手动 · 失败（服务器错误 502） · 用了本机缓存的 CLI（新版下载或校验失败） · 2.0s",
      "定时 · 成功 · 用了本机缓存的 CLI（npm 源返回错误） · 9.0s",
      "定时 · 成功 · 用了本机缓存的 CLI（新版安装失败） · 9.0s",
      "定时 · 成功 · 用了本机缓存的 CLI（原因未知） · 9.0s",
      "定时 · 成功 · 9.0s",
    ]);
    expect(text).not.toContain("CLI 启动阶段失败");
  });

  it("drops every free string that does not have its expected shape", () => {
    const hostile = report({
      app: { version: "0.7.5 C:\\Users\\张 三\\My Projects", build: "release" },
      system: {
        windowsRelease: 11,
        displayVersion: "张三的电脑",
        build: 22631,
        architecture: "x86_64 ZhangSan-PC",
      },
      runtime: { kind: "bundledNode", version: "-Users-zhangsan-Coding-secret-project" },
      cli: { launcherPresent: true, cachedVersion: "sk-ant-api03-FAKEFAKEFAKE" },
      deviceTag: "ZhangSan-PC",
      network: {
        systemProxy: "remote",
        systemProxyPassedToSync: false,
        autoConfigScript: false,
        variables: [{ name: "http://user:p%40ss@10.0.0.5:8080", user: true, system: false, app: false }],
      },
    });
    hostile.claude.pathMatches = ["claude.cmd", "C:\\Users\\张 三\\My Projects\\claude.cmd"];
    hostile.claude.userVariables = {
      names: ["ANTHROPIC_BASE_URL", "someone@example.com", "vbu_test_0123456789abcdef"],
      other: 1,
    };
    hostile.claude.settings = { kind: "read", env: { names: ["eyJhbGciOiJIUzI1NiJ9.e30.sig"], other: 0 } };
    hostile.claude.candidates[0].version = "2.1.282 (C:\\Users\\张 三)";

    const text = buildDiagnosticsText(hostile, products);
    for (const secret of SECRETS) expect(text).not.toContain(secret);
    expect(text).toContain("ANTHROPIC_BASE_URL、另有 3 个名字格式特殊，未列出");
    expect(text).toContain("未传给同步（App 环境里已有 HTTPS_PROXY）");
  });

  it("the final scrub removes each synthetic secret from raw text", () => {
    const raw = [
      "home C:\\Users\\张 三\\My Projects\\-Users-zhangsan-Coding-secret-project\\a.jsonl",
      "unc \\\\server\\share\\张三",
      "mac /Users/zhangsan/Coding/secret-project",
      "dir -Users-zhangsan-Coding-secret-project",
      "key vbu_test_0123456789abcdef",
      "anthropic sk-ant-api03-FAKEFAKEFAKE",
      "auth Bearer abc.def",
      "mail someone@example.com",
      "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln",
      "proxy http://user:p%40ss@10.0.0.5:8080",
      "blob QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVphYmNkZWZnaGlqa2xtbm9wcXJzdHV2d3h5eg==",
    ].join("\n");
    const scrubbed = scrubDiagnosticsText(raw);
    for (const secret of [...SECRETS, "abc.def", "QUJDREVG", "server"]) {
      expect(scrubbed).not.toContain(secret);
    }
    expect(scrubbed).toContain("Bearer ***");
    expect(scrubbed).toContain("proxy ***");
  });

  it("never pins today's version on a past read", () => {
    const text = buildDiagnosticsText(report(), products);
    const history = text.split("\n").filter((line) => /^\s+\d）/.test(line));
    expect(history.length).toBe(2);
    for (const line of history) expect(line).not.toMatch(/2\.1\.\d+/);
    expect(text).toContain("当前候选及版本（复制时查询）：1）~\\.local\\bin\\claude.exe · 2.1.282");
  });

  it("states the verdict only from the newest sync", () => {
    const verdict = (lastSyncKey: DiagnosticsReport["account"]["lastSyncKey"]) =>
      buildDiagnosticsText(report({ account: { keyConfigured: true, lastSyncKey } }), products);
    expect(verdict("accepted")).toContain("最近一次同步未被服务器拒绝");
    expect(verdict("rejected")).toContain("最近一次同步被服务器拒绝（Key 无效）");
    expect(verdict("undetermined")).toContain("最近一次同步无法判断 Key 是否被拒");
    expect(verdict("noRecord")).toContain("暂无同步记录");
  });

  it("reports an automatic proxy script without its address", () => {
    const text = buildDiagnosticsText(
      report({
        network: {
          systemProxy: "none",
          systemProxyPassedToSync: false,
          autoConfigScript: true,
          variables: [],
        },
      }),
      products,
    );
    expect(text).toContain("网络：系统代理：无 · 自动代理脚本：有");
  });

  it("the scrub keeps plain variable names but masks other long strings", () => {
    const scrubbed = scrubDiagnosticsText(
      "env CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC · token AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcdEF",
    );
    expect(scrubbed).toContain("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC");
    expect(scrubbed).not.toContain("AbCdEfGh");
    expect(scrubbed).toContain("token ***");
  });

  it("explains a Desktop-record read without inventing a probe", () => {
    const text = buildDiagnosticsText(
      report({
        claude: {
          ...report().claude,
          lastProbe: {
            at: NOW,
            desktop: { kind: "used", failure: { code: "desktopRecordStale" } },
            candidatesFound: 0,
            attempts: [],
          },
          candidates: [],
        },
      }),
      products,
    );
    expect(text).toContain("用了桌面端记录（Claude 桌面端记录已过期），没有启动 Claude Code");
    expect(text).toContain("当前候选及版本（复制时查询）：没有找到 Claude Code 程序");
  });
});

describe("data age in the quota section", () => {
  const age = (secondsAgo: number) =>
    buildDiagnosticsText(
      report({
        quotas: [
          { provider: "claudeCode", status: "ok", source: "cache", meterCount: 1, dataAsOf: NOW / 1000 - secondsAgo },
        ],
      }),
      products,
    );

  it("scales from minutes to hours to days", () => {
    expect(age(45 * 60)).toContain("来源：缓存 · 数据 45 分钟前");
    expect(age(30 * 3600)).toContain("来源：缓存 · 数据 30 小时前");
    // A stale Grok cache was shown as 「数据 149962 分钟前」.
    expect(age(47 * 3600 + 59 * 60)).toContain("来源：缓存 · 数据 47 小时前");
    expect(age(48 * 3600)).toContain("来源：缓存 · 数据 2 天前");
    expect(age(149_962 * 60)).toContain("来源：缓存 · 数据 104 天前");
    expect(age(149_962 * 60)).not.toContain("分钟前");
  });
});

describe("CLI doctor (plan step 2′)", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/diagnostics-report.json", import.meta.url), "utf8"),
  ) as DiagnosticsReport;
  const doctorReport = (fixture.doctor as { kind: "ok"; report: DoctorReport }).report;
  const withDoctor = (doctor: CliDoctor, overrides: Partial<DiagnosticsReport> = {}) =>
    buildDiagnosticsText(report({ doctor, ...overrides }), products);
  const cliLine = (text: string) => text.split("\n").find((line) => line.startsWith("CLI："));

  it("names why the CLI could not answer in one fixed phrase, and keeps the app's half", () => {
    const cases: Array<[CliDoctor, string]> = [
      [
        { kind: "startupFailed", reason: { code: "launcher", stage: "network" } },
        "CLI：启动阶段失败（引导器连不上 npm 源） · 引导器已缓存 0.14.9 · 本报告不含 CLI 诊断和工具明细",
      ],
      [
        { kind: "startupFailed", reason: { code: "runtimeUnavailable" } },
        "CLI：启动阶段失败（运行环境不可用） · 引导器已缓存 0.14.9 · 本报告不含 CLI 诊断和工具明细",
      ],
      [
        { kind: "timedOut" },
        "CLI：CLI 诊断超时（30 秒内没有结束） · 引导器已缓存 0.14.9 · 本报告不含 CLI 诊断和工具明细",
      ],
      [
        { kind: "noDoctorCommand" },
        "CLI：CLI 较旧，没有诊断命令 · 引导器已缓存 0.14.9 · 本报告不含 CLI 诊断和工具明细",
      ],
      [
        { kind: "unsupportedSchema", schemaVersion: 2 },
        "CLI：CLI 较新，诊断格式不认识（schemaVersion 2） · 引导器已缓存 0.14.9 · 本报告不含 CLI 诊断和工具明细",
      ],
      [
        { kind: "unrecognizedOutput" },
        "CLI：CLI 诊断输出无法识别 · 引导器已缓存 0.14.9 · 本报告不含 CLI 诊断和工具明细",
      ],
      [
        { kind: "failed", exitCode: 1 },
        "CLI：CLI 诊断命令失败（退出码 1） · 引导器已缓存 0.14.9 · 本报告不含 CLI 诊断和工具明细",
      ],
    ];
    for (const [doctor, line] of cases) {
      const text = withDoctor(doctor);
      expect(cliLine(text)).toBe(line);
      expect(text).not.toContain("CLI 诊断（doctor");
      expect(text).toContain("最近同步（App 记录，新的在前）：");
      expect(text).toContain("Claude 读取明细：");
    }
    expect(
      cliLine(withDoctor({ kind: "timedOut" }, { cli: { launcherPresent: false, cachedVersion: null } })),
    ).toBe("CLI：CLI 诊断超时（30 秒内没有结束） · 启动脚本缺失 · 引导器没有缓存 · 本报告不含 CLI 诊断和工具明细");
  });

  it("explains a CLI sync without an end only by an app timeout that covers it", () => {
    const appTimeout = { startedAt: NOW - 40 * 60_000, trigger: "manual" as const, outcome: "timedOut" as const, durationMs: 120_000 };
    const entry = (startedAt: number, surface: string) => ({
      startedAt,
      surface,
      cliVersion: "0.14.11",
      uploaded: { buckets: 0, sessions: 0 },
    });
    const text = withDoctor(
      {
        kind: "ok",
        report: {
          ...doctorReport,
          syncHistory: {
            available: true,
            entries: [
              entry(appTimeout.startedAt + 15_000, "windows-app"),
              entry(appTimeout.startedAt + 15_000, "cli"),
              entry(appTimeout.startedAt + 200_000, "windows-app"),
            ],
          },
        },
      },
      { syncs: [appTimeout] },
    );
    const lines = text.split("\n").filter((line) => line.includes("CLI 0.14.11") && /^ {4}\d/.test(line));
    expect(lines.map((line) => line.replace(/^ +\d\d-\d\d \d\d:\d\d /, ""))).toEqual([
      "Windows App · 超时，2m00s 时被 App 结束 · CLI 0.14.11",
      "终端 · 未记录结束，原因未知 · CLI 0.14.11",
      "Windows App · 未记录结束，原因未知 · CLI 0.14.11",
    ]);
  });

  it("shows a new code as the code itself and drops strings of the wrong shape", () => {
    const hostile: DoctorReport = {
      ...doctorReport,
      cliVersion: "C:\\Users\\张 三\\My Projects",
      sqlite: "brand:new",
      account: { configured: true, server: "default", uploadState: "someone@example.com" },
      tools: [
        {
          ...doctorReport.tools[0],
          name: "-Users-zhangsan-Coding-secret-project",
          lastSync: {
            ...doctorReport.tools[0].lastSync!,
            warnings: [
              { code: "brand_new_warning", count: 2 },
              { code: "vbu_test_0123456789abcdef Bearer eyJhbGciOiJIUzI1NiJ9", count: 1 },
            ],
          },
        },
      ],
    };
    const text = withDoctor({ kind: "ok", report: hostile });
    for (const secret of SECRETS) expect(text).not.toContain(secret);
    expect(text).toContain("CLI 诊断（doctor）：");
    expect(text).toContain("SQLite：brand:new");
    expect(text).toContain("账号：服务器：默认 · 未知");
    expect(text).toContain("cursor  已检测 · 上次 0 buckets · brand_new_warning ×2（brand_new_warning）");
  });
});

describe("Rust ↔ TypeScript diagnostics contract", () => {
  // Written by the Rust test `contract_fixture_matches_what_rust_serializes`
  // from what serde actually emits, so a field renamed on one side only shows
  // up here as a placeholder instead of silently reaching users.
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/diagnostics-report.json", import.meta.url), "utf8"),
  ) as DiagnosticsReport;
  const text = buildDiagnosticsText(fixture, products);

  it("reads every field Rust serializes", () => {
    for (const line of [
      "环境：Windows 11 23H2（build 22631） · x86_64 · App 0.7.5 正式版 · 运行环境：App 内置 Node 22.23.1",
      "CLI：启动正常 · 本次运行 0.14.11 · 引导器已缓存 0.14.9",
      "账号：已配置 Key · 最近一次同步无法判断 Key 是否被拒 · 设备标签 7c2e",
      "网络：系统代理：有（本机地址） · 已传给同步 · 未传给 Claude 配额读取 · 自动代理脚本：有",
      "代理变量：HTTPS_PROXY 用户变量有 · HTTP_PROXY 无 · ALL_PROXY 系统变量有",
      "定时 · 成功 · 用了本机缓存的 CLI（连不上 npm 源） · 14s",
      "定时 · CLI 启动阶段失败（引导器连不上 npm 源） · 21s",
      "手动 · 超时，2m00s 时被 App 结束",
      "定时 · 失败（CLI 退出码 3） · 3.0s",
      "定时 · 失败（服务器错误 502） · 2.0s",
      "手动 · 失败（Key 被服务器拒绝） · 1.5s",
      "定时 · 成功 · 18s",
      "Claude  正常 · 1 个窗口 · Claude 配额读取超时 · 来源：缓存 · 数据 30 分钟前",
      "Codex  无数据 · Codex 配额接口返回错误 502 · 来源：实时读取",
      "OpenCode Go  无数据 · 未订阅 OpenCode Go",
      "当前候选及版本（复制时查询）：1）~\\.local\\bin\\claude.exe · 2.1.282；2）PATH 上第 3 个（claude.cmd） → 包内 claude.exe · 2.1.319",
      "（不含当时的版本）：桌面端记录：没有 · 找到候选 2 个，试了 2 个",
      "1）~\\.local\\bin\\claude.exe · initialize 0.9s · get_usage 超时 · Claude 配额读取超时 · 被 App 结束",
      "2）PATH 上第 3 个（claude.cmd） → 包内 claude.exe · initialize 1.2s · get_usage 2.3s · Claude 没有返回额度窗口 · 退出码 1 · 回答：rate_limits_available true · 5h 无 · 7d 无 · 套餐 有 · error 有",
      "PATH 上的 claude.*：claude.exe ×1、claude ×1、claude.cmd ×1",
      "ANTHROPIC/CLAUDE 开头的用户变量：ANTHROPIC_BASE_URL · settings.json 的 env：CLAUDE_CODE_USE_BEDROCK · CLAUDE_CONFIG_DIR 已设",
      "CLI 诊断（doctor，CLI 0.14.11）：",
      "  运行环境（本次调用）：Node 22.23.1 · SQLite：内置 node:sqlite",
      "  账号：服务器：默认 · 上传记录与当前 Key 一致",
      "  网络：服务器可达 320ms · npm 源（官方）连不上（超时）",
      "  后台：CLI 后台服务：未安装 · 最近一次后台同步",
      "  设置：项目名：上传 · 额外数据目录 1 个 · Kiki 切点：未设 · Magpie 切点：全部历史",
      "Windows App · 成功 14s · 上传 12 buckets / 3 sessions · CLI 0.14.9",
      // The CLI record without an end lies inside the app's 120 s timeout.
      "Windows App · 超时，2m00s 时被 App 结束 · 已上传 5 buckets · CLI 0.14.11",
      "终端 · 未记录结束，原因未知 · CLI 0.14.10",
      "后台服务 · 失败 2.0s · 服务器错误 502 · CLI 0.14.10",
      "Cursor     已检测 · 上次 0 buckets · 本地登录令牌已加密（Cursor 3.27+） ×1（cursor_token_encrypted） · 累计已上传 840 buckets",
      "Codex CLI  已检测 · 额外目录 1 个 · 上次 30 buckets / 2 sessions · 正在建立本地索引 120/400 · 服务端丢弃 30（服务端还没收录这个来源） · 累计已上传 1,204 buckets / 88 sessions",
      "Cindy      已检测 · 数据并入 Codex CLI / pi-coding-agent",
    ]) {
      expect(text).toContain(line);
    }
  });

  it("shows no placeholder that would hide a field-name mismatch", () => {
    // 「未记录结束，原因未知」 is the contract's own wording for a CLI sync
    // with no end and no app timeout to explain it.
    expect(text.replaceAll("未记录结束，原因未知", "")).not.toMatch(/\?|未知|版本未知/);
  });
});

