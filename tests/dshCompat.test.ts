import { spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";
import { afterEach, describe, expect, it } from "vitest";
import { cliPath } from "./cliUnderTest.mjs";

/**
 * Regression coverage for the DSH (DeepSeek Harness) parser the Windows app runs.
 *
 * The app does not bundle the CLI: `sync_engine.rs` runs
 * `src-tauri/resources/cli-bootstrap.mjs`, which resolves
 * `@vibe-cafe/vibe-usage@latest` from the registry and executes it with the
 * bundled Node. These tests therefore drive the *published* parser that
 * resolution produced. That is the point of them: 0.5.12 shipped a V0-only DSH
 * parser while DeepSeek Harness had moved to `session.v3.jsonl.zstd`, and a test
 * that reads a checked-in snapshot cannot catch a snapshot that is already stale.
 *
 * The parser runs in a subprocess, so these assert the artifact users execute
 * rather than a copy of its logic. Resolution needs the registry (cached after
 * the first fetch); no real DSH store is mutated.
 */

const HERE = dirname(fileURLToPath(import.meta.url));
const PARSER_PATH = cliPath("src/parsers/dsh.js");
const DRIVER = join(HERE, "run-dsh-parser.mjs");

const hasBuiltinZstd = typeof zlib.zstdCompressSync === "function";

/** Half-hour boundary shared by every fixture record. */
const BUCKET_START = "2026-01-02T03:00:00.000Z";
const T0 = Date.parse("2026-01-02T03:05:00.000Z");
const MINUTE = 60_000;
const CWD = "C:\\fixtures\\dsh-compat-proj";
const PROJECT = "dsh-compat-proj";
const MODEL = "deepseek-v4-test";
const PROJECT_KEY = `--${CWD.replace(/\\/g, "-").replace(/:/g, "")}--`;

/**
 * Expected totals for the current-generation fixture, column by column.
 *
 * DSH usage counters are disjoint: `inputTokens` is uncached input,
 * `cacheWriteTokens` joins it (the shared bucket schema has no cache-write
 * column), `cacheReadTokens` maps to cached input, and `outputTokens` already
 * contains `reasoningTokens`, which is therefore subtracted out.
 */
const EXPECTED_V3 = {
  inputTokens: 820, // (300 + 50) + (400 + 70)
  cachedInputTokens: 200, // 100 + 100
  outputTokens: 90, // (80 - 30) + (60 - 20)
  reasoningOutputTokens: 50, // 30 + 20
};

/** Expected totals for the legacy-generation fixture. */
const EXPECTED_V0 = {
  inputTokens: 7,
  cachedInputTokens: 1,
  outputTokens: 2, // 3 - 1
  reasoningOutputTokens: 1,
};

const tempDirs: string[] = [];

afterEach(() => {
  while (tempDirs.length > 0) {
    rmSync(tempDirs.pop()!, { recursive: true, force: true });
  }
});

/** DSH concatenates one independent zstd frame per durable append batch. */
function zstdFrames(records: unknown[]): Buffer {
  return Buffer.concat(
    records.map((record) => zlib.zstdCompressSync(Buffer.from(JSON.stringify(record) + "\n"))),
  );
}

function header(id: string, version: number, extra: Record<string, unknown> = {}) {
  return {
    type: "session",
    version,
    id,
    createdAt: T0,
    cwd: CWD,
    ...(version >= 2 ? { isSeeded: false } : {}),
    ...extra,
  };
}

function userRecord(seq: number, offsetMinutes: number, id: string) {
  return {
    type: "user/message",
    seq,
    time: T0 + offsetMinutes * MINUTE,
    data: { content: "fixture prompt", source: { kind: "user", rpcId: `rpc-${seq}` }, role: "user", id },
  };
}

function assistantRecord(
  seq: number,
  offsetMinutes: number,
  id: string,
  usage: Record<string, number>,
) {
  return {
    type: "assistant/message",
    seq,
    time: T0 + offsetMinutes * MINUTE,
    data: {
      turn: 1,
      step: seq,
      message: { role: "assistant", id, source: { kind: "model", provider: "deepseek-official", model: MODEL } },
      usage,
      stream: { truncated: false },
    },
  };
}

/** Current generation: `session.v3.jsonl.zstd` with `isSeeded` and per-record seq. */
function v3Records(id: string) {
  return [
    header(id, 3),
    userRecord(1, 0, `${id}-user-1`),
    assistantRecord(2, 1, `${id}-a-1`, {
      inputTokens: 300, cacheWriteTokens: 50, cacheReadTokens: 100,
      outputTokens: 80, reasoningTokens: 30, totalTokens: 530,
    }),
    assistantRecord(3, 2, `${id}-a-2`, {
      inputTokens: 400, cacheWriteTokens: 70, cacheReadTokens: 100,
      outputTokens: 60, reasoningTokens: 20, totalTokens: 630,
    }),
  ];
}

/** Legacy generation: bare `session.jsonl`, no seq, no isSeeded. */
function v0Records(id: string) {
  return [
    header(id, 0),
    { type: "user/message", time: T0, data: { source: { kind: "user" } } },
    {
      type: "assistant/message",
      time: T0 + MINUTE,
      data: {
        message: { role: "assistant", source: { kind: "model", model: MODEL } },
        usage: { inputTokens: 7, cacheReadTokens: 1, outputTokens: 3, reasoningTokens: 1 },
      },
    },
  ];
}

function makeStore() {
  const root = mkdtempSync(join(tmpdir(), "vibe-usage-dsh-"));
  tempDirs.push(root);
  const sessionsDir = join(root, "sessions");
  mkdirSync(sessionsDir, { recursive: true });
  return sessionsDir;
}

function writeSession(
  sessionsDir: string,
  sessionId: string,
  filename: string,
  records: unknown[],
) {
  const dir = join(sessionsDir, PROJECT_KEY, sessionId);
  mkdirSync(dir, { recursive: true });
  const payload = filename.endsWith(".zstd")
    ? zstdFrames(records)
    : Buffer.from(records.map((record) => JSON.stringify(record) + "\n").join(""));
  writeFileSync(join(dir, filename), payload);
  return dir;
}

interface ParserResult {
  ok: boolean;
  buckets?: Array<Record<string, number | string>>;
  sessions?: unknown[];
  skipped?: boolean;
  warnings?: string[];
  error?: { name: string; code: string | null; message: string };
}

/** Run the published DSH parser from the installer resources against a fixture store. */
function runParser(sessionsDir: string, parserPath?: string): ParserResult {
  const env: NodeJS.ProcessEnv = { ...process.env, VIBE_USAGE_DSH_SESSIONS: sessionsDir };
  env.VIBE_USAGE_DSH_PARSER_PATH = parserPath ?? PARSER_PATH;

  const proc = spawnSync(process.execPath, [DRIVER, sessionsDir], { encoding: "utf8", env });
  if (proc.status !== 0) {
    throw new Error(`driver exited ${proc.status}: ${proc.stderr || proc.stdout}`);
  }
  return JSON.parse(proc.stdout) as ParserResult;
}

function totals(result: ParserResult) {
  const sum = { inputTokens: 0, cachedInputTokens: 0, outputTokens: 0, reasoningOutputTokens: 0 };
  for (const bucket of result.buckets ?? []) {
    sum.inputTokens += Number(bucket.inputTokens ?? 0);
    sum.cachedInputTokens += Number(bucket.cachedInputTokens ?? 0);
    sum.outputTokens += Number(bucket.outputTokens ?? 0);
    sum.reasoningOutputTokens += Number(bucket.reasoningOutputTokens ?? 0);
  }
  return sum;
}

describe("published DSH parser — parser generation", () => {
  it("accepts the current DSH session format generation, not just V0", () => {
    const source = readFileSync(PARSER_PATH, "utf8");

    // A V0-only parser pins the constant to 0 and compares for equality, which is
    // what silently dropped every session.v3 file.
    const maxVersion = /MAX_SESSION_FORMAT_VERSION\s*=\s*(\d+)/.exec(source);
    expect(maxVersion, "parser must declare MAX_SESSION_FORMAT_VERSION").not.toBeNull();
    expect(Number(maxVersion![1])).toBeGreaterThanOrEqual(3);

    // Generation addressing must be filename-driven: `session[.vN].jsonl[.zstd]`.
    expect(source).toMatch(/SESSION_FILENAME\s*=\s*\/\^session/);
    expect(source).toMatch(/\\\.v\(\[1-9\]/);
    expect(source).not.toMatch(/SESSION_FORMAT_VERSION\s*=\s*0\s*;/);
  });
});

describe("published DSH parser — current generation", () => {
  it("enumerates and parses session.v3.jsonl.zstd", { skip: !hasBuiltinZstd }, () => {
    const sessionsDir = makeStore();
    writeSession(sessionsDir, "session-current-v3", "session.v3.jsonl.zstd", v3Records("session-current-v3"));

    const result = runParser(sessionsDir);

    expect(result.ok).toBe(true);
    expect(result.buckets).toHaveLength(1);
    expect(result.buckets![0]).toMatchObject({
      source: "dsh",
      model: MODEL,
      project: PROJECT,
      bucketStart: BUCKET_START,
    });
    expect(totals(result)).toEqual(EXPECTED_V3);
    expect(result.sessions).toHaveLength(1);
  });

  it("parses a plaintext session.v3.jsonl without requiring zstd", () => {
    const sessionsDir = makeStore();
    writeSession(sessionsDir, "session-plain-v3", "session.v3.jsonl", v3Records("session-plain-v3"));

    const result = runParser(sessionsDir);

    expect(result.ok).toBe(true);
    expect(totals(result)).toEqual(EXPECTED_V3);
  });

  it("keeps reasoning, cache read, cache write and input disjoint", { skip: !hasBuiltinZstd }, () => {
    const sessionsDir = makeStore();
    writeSession(sessionsDir, "session-buckets", "session.v3.jsonl.zstd", v3Records("session-buckets"));

    const result = runParser(sessionsDir);
    const bucket = result.buckets![0];

    // Cache writes fold into input; they must not also appear anywhere else.
    expect(bucket.inputTokens).toBe(820);
    expect(bucket.cachedInputTokens).toBe(200);
    // Reasoning is carved out of output, never added on top of it.
    expect(bucket.outputTokens).toBe(90);
    expect(bucket.reasoningOutputTokens).toBe(50);
    expect(bucket.totalTokens).toBe(960);
    expect(Number(bucket.inputTokens) + Number(bucket.outputTokens)).toBe(910);
    expect(
      Number(bucket.inputTokens) +
        Number(bucket.cachedInputTokens) +
        Number(bucket.outputTokens) +
        Number(bucket.reasoningOutputTokens),
    ).toBe(1160);
  });

  it("ignores non-canonical session files in a session directory", { skip: !hasBuiltinZstd }, () => {
    const sessionsDir = makeStore();
    const dir = writeSession(
      sessionsDir,
      "session-noise",
      "session.v3.jsonl.zstd",
      v3Records("session-noise"),
    );
    // Temporary and backup artifacts DSH never treats as a generation.
    writeFileSync(join(dir, "session.v3.jsonl.zstd.tmp"), Buffer.from("noise"));
    writeFileSync(join(dir, "session.v3.backup.jsonl"), Buffer.from("{}\n"));

    const result = runParser(sessionsDir);

    expect(result.ok).toBe(true);
    expect(totals(result)).toEqual(EXPECTED_V3);
  });
});

describe("published DSH parser — generation selection", () => {
  it("counts only the newest generation when V0 and V3 coexist", { skip: !hasBuiltinZstd }, () => {
    const sessionsDir = makeStore();
    const dir = writeSession(sessionsDir, "session-migrated", "session.jsonl.zstd", v0Records("session-migrated"));
    writeFileSync(
      join(dir, "session.v3.jsonl.zstd"),
      zstdFrames(v3Records("session-migrated")),
    );

    const result = runParser(sessionsDir);

    expect(result.ok).toBe(true);
    expect(result.buckets).toHaveLength(1);
    // The frozen predecessor must neither replace nor supplement the migration.
    expect(totals(result)).toEqual(EXPECTED_V3);
    expect(totals(result).inputTokens).not.toBe(EXPECTED_V0.inputTokens);
    expect(totals(result).inputTokens).not.toBe(
      EXPECTED_V0.inputTokens + EXPECTED_V3.inputTokens,
    );
  });

  it("counts a legacy-only store at its legacy values", () => {
    const sessionsDir = makeStore();
    writeSession(sessionsDir, "session-legacy", "session.jsonl", v0Records("session-legacy"));

    const result = runParser(sessionsDir);

    expect(result.ok).toBe(true);
    expect(totals(result)).toEqual(EXPECTED_V0);
  });

  it("does not fall back to a frozen generation when the current one is unsupported", () => {
    const sessionsDir = makeStore();
    const dir = writeSession(sessionsDir, "session-newer", "session.jsonl", v0Records("session-newer"));
    // A generation this CLI cannot read. Falling back to session.jsonl would
    // upload stale numbers under the current session's identity.
    writeFileSync(join(dir, "session.v9.jsonl"), Buffer.from("{}\n"));

    const result = runParser(sessionsDir);

    expect(result.ok).toBe(true);
    expect(result.buckets).toHaveLength(0);
    expect(result.skipped).toBe(true);
    expect(result.warnings?.join(" ")).toMatch(/update Vibe Usage/i);
  });
});

describe("published DSH parser — real store smoke test", () => {
  /**
   * Opt-in, never part of an automated run.
   *
   * These assertions describe a log written by a real DSH runtime, which is the
   * only fixture that can prove the parser agrees with the shipped writer. That
   * makes them valuable locally and unacceptable as an automated gate: reading
   * `~/.dsh` would let the developer's own machine decide whether the suite
   * passes, and a clean CI runner has no such store at all. The environment is
   * therefore never consulted unless VIBE_USAGE_LIVE_DSH_SMOKE=1 is set
   * explicitly. The packaged-runtime contract (bundled node + bundled CLI) is
   * covered by a separate installer/runtime smoke test.
   */
  const liveSmoke = process.env.VIBE_USAGE_LIVE_DSH_SMOKE?.trim() === "1";
  const liveProjectOverride = process.env.VIBE_USAGE_LIVE_DSH_PROJECT?.trim() ?? "";
  const liveSessions = join(process.env.DSH_HOME?.trim() || join(homedir(), ".dsh"), "sessions");
  // DSH encodes a cwd as `--` + cwd with ':' dropped and both separators mapped
  // to '-' + `--`. Deriving the key this way keeps the lookup honest: a mismatch
  // is reported as a skip reason instead of silently finding nothing.
  const projectKey = `--${process.cwd().replace(/:/g, "").replace(/[\\/]/g, "-")}--`;
  const liveProjects = liveSmoke && existsSync(liveSessions)
    ? readdirSync(liveSessions, { withFileTypes: true })
      .filter((entry) => entry.isDirectory()
        && (liveProjectOverride ? entry.name === liveProjectOverride : entry.name === projectKey))
      .map((entry) => entry.name)
    : [];

  const liveSkip = !liveSmoke
    ? "opt-in only: set VIBE_USAGE_LIVE_DSH_SMOKE=1 to run against a real DSH store"
    : liveProjects.length === 0
      ? `no live project ${liveProjectOverride || projectKey} under ${liveSessions}`
      : false;

  it("parses a live DSH project without a format warning", { skip: liveSkip }, () => {
    const root = mkdtempSync(join(tmpdir(), "vibe-usage-dsh-live-"));
    tempDirs.push(root);
    // The parser resolves $DSH_HOME/sessions; only the sessions subtree is needed.
    const sessionsDir = join(root, "sessions");
    mkdirSync(sessionsDir, { recursive: true });
    for (const name of liveProjects) {
      cpSync(join(liveSessions, name), join(sessionsDir, name), { recursive: true });
    }

    const result = runParser(sessionsDir);

    expect(result.ok).toBe(true);
    expect(result.buckets?.length).toBeGreaterThan(0);
    // A live store must not produce "skipping ... (session ... uses format
    // version N)": that warning is how the V0-only regression surfaced.
    const warnings = result.warnings ?? [];
    expect(warnings.filter((warning) => /format version|update Vibe Usage/i.test(warning))).toEqual([]);
  });
});
