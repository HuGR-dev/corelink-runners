// deploy/cloudflare/src/lib/clw.ts
//
// WP-04: clw Invocation Library (NO LIFECYCLE HOOKS — those are WP-06's).
//
// This file is the SINGLE implementation of `clw` invocation for the DevEnv.
// WP-06 §3.1 imports `hydrateViaClw`, `snapshotViaClw`, `acquireSnapshotLock`,
// `releaseSnapshotLock`, and `clwRefExists` from here.

import type {
  HydrateMetadata,
  SnapshotMetadata,
} from "../types/devenv";

// ────────────────────────────────────────────────────────────────────────
// CONSTANTS — single source of truth for clw invocation
// ────────────────────────────────────────────────────────────────────────

/** Path to the clw binary inside the container (entrypoint.sh installs to /usr/local/bin/clw). */
export const CLW_BIN = "/usr/local/bin/clw";

/** Upper bound on a single snapshot/hydrate. Matches entrypoint.sh's 10-min budget. */
export const CLW_DEFAULT_TIMEOUT_MS = 600_000;

/** Default chunk-upload concurrency. Matches entrypoint.sh and clw's own default. */
export const CLW_CONCURRENCY = "8";

/** Runner ref domain — frozen, matches WP-01 §3.3 STATIC_ENV_VARS.CLW_REF_DOMAIN. */
export const CLW_REF_DOMAIN = "runner";

/** Exec-server port (alongside code-server, per WP-02/03/06). */
export const EXEC_SERVER_PORT = 9090;

/**
 * Provider ingress delivers EXEC_SERVER_AUTH_TOKEN only to the short-lived
 * entrypoint. That bridge writes this regular mode-0400 file, unsets the raw
 * token, and exports the path to the durable exec-server process.
 */
export const EXEC_SERVER_AUTH_TOKEN_FILE = "/run/corelink/exec-server-auth-token";

/** Side-table keys for snapshot metadata + lock + tenant (per WP-06 §3.3 convention). */
export const TENANT_KEY = "clwTenant";
export const SNAPSHOT_LOCK_KEY = "snapshotInProgress";
export const PROFILE_SNAPSHOT_KEY = "lastProfileSnapshot";
export const WORKSPACE_SNAPSHOT_KEY = "lastWorkspaceSnapshot";

export const DEFAULT_CLW_IGNORE_PATTERNS = [
  "target/**",
  "node_modules/.cache/**",
  ".git/objects/pack/**",
  "__pycache__/**",
  ".turbo/**",
  ".next/cache/**",
  "/tmp/**",
] as const;

export const CLW_AUTH_TMPFS_PATH = "/dev/shm/.clw-auth";

/** The exec envelope and the snapshot CLI each have a small, explicit JSON contract. */
export const CLW_EXEC_ENVELOPE_MAX_BYTES = 256 * 1024;
export const CLW_SNAPSHOT_REPORT_MAX_BYTES = 64 * 1024;
const CLW_EXEC_STDERR_MAX_BYTES = 16 * 1024;
const SNAPSHOT_REPORT_FIELDS = [
  "bytes_total",
  "chunks_total",
  "chunks_uploaded",
  "files",
  "name",
  "root",
  "skipped_external_symlinks",
  "unchanged",
] as const;

export type ClwExecResult = { exitCode: number | null; stdout: string; stderr: string };

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value: Record<string, unknown>, keys: readonly string[]): boolean {
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return actual.length === expected.length && actual.every((key, index) => key === expected[index]);
}

function isSafeCounter(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

/**
 * Read and validate the exec-server envelope without buffering an unbounded body.
 * stdout and stderr remain distinct, and malformed RPC data never becomes defaults.
 */
export async function parseClwExecResponse(resp: Response): Promise<ClwExecResult> {
  const contentLength = resp.headers.get("content-length");
  if (contentLength !== null && (!/^\d+$/.test(contentLength) || Number(contentLength) > CLW_EXEC_ENVELOPE_MAX_BYTES)) {
    throw new Error("CLW_EXEC_ENVELOPE_INVALID_SIZE");
  }
  if (!resp.body) throw new Error("CLW_EXEC_ENVELOPE_MISSING_BODY");

  const reader = resp.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > CLW_EXEC_ENVELOPE_MAX_BYTES) {
        await reader.cancel();
        throw new Error("CLW_EXEC_ENVELOPE_INVALID_SIZE");
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }

  let raw: string;
  try {
    const joined = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { joined.set(chunk, offset); offset += chunk.byteLength; }
    raw = new TextDecoder("utf-8", { fatal: true }).decode(joined);
  } catch {
    throw new Error("CLW_EXEC_ENVELOPE_INVALID_UTF8");
  }

  let body: unknown;
  try { body = JSON.parse(raw); } catch { throw new Error("CLW_EXEC_ENVELOPE_INVALID_JSON"); }
  if (!isRecord(body) || !exactKeys(body, ["exit_code", "stdout", "stderr"])) {
    throw new Error("CLW_EXEC_ENVELOPE_INVALID_SHAPE");
  }
  if (!(body.exit_code === null || (typeof body.exit_code === "number" && Number.isSafeInteger(body.exit_code) && body.exit_code >= 0 && body.exit_code <= 255)) ||
      typeof body.stdout !== "string" || typeof body.stderr !== "string" ||
      new TextEncoder().encode(body.stdout).byteLength > CLW_SNAPSHOT_REPORT_MAX_BYTES ||
      new TextEncoder().encode(body.stderr).byteLength > CLW_EXEC_STDERR_MAX_BYTES) {
    throw new Error("CLW_EXEC_ENVELOPE_INVALID_FIELDS");
  }
  return { exitCode: body.exit_code as number | null, stdout: body.stdout, stderr: body.stderr };
}

/**
 * Validate the exact field set emitted by the current `clw snapshot --json`
 * producer. That producer has no schema-version field yet, so treat any shape
 * drift as incompatible until the producer and this consumer change together.
 * The echoed `name` binds this result to the requested snapshot ref.
 */
export function parseClwSnapshotReport(stdout: string, expectedName: string): {
  root: string;
  bytesTotal: number;
  files: number;
  chunksTotal: number;
  chunksUploaded: number;
  unchanged: boolean;
} {
  if (new TextEncoder().encode(stdout).byteLength > CLW_SNAPSHOT_REPORT_MAX_BYTES) {
    throw new Error("CLW_SNAPSHOT_REPORT_INVALID_SIZE");
  }
  let report: unknown;
  try { report = JSON.parse(stdout); } catch { throw new Error("CLW_SNAPSHOT_REPORT_INVALID_JSON"); }
  if (!isRecord(report) || !exactKeys(report, SNAPSHOT_REPORT_FIELDS)) {
    throw new Error("CLW_SNAPSHOT_REPORT_INVALID_SHAPE");
  }
  if (typeof expectedName !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(expectedName) ||
      report.name !== expectedName || typeof report.root !== "string" || !/^[0-9a-f]{64}$/.test(report.root) ||
      !isSafeCounter(report.bytes_total) || !isSafeCounter(report.files) ||
      !isSafeCounter(report.chunks_total) || !isSafeCounter(report.chunks_uploaded) ||
      report.chunks_uploaded > report.chunks_total || typeof report.unchanged !== "boolean" ||
      (report.files === 0 && (report.bytes_total !== 0 || report.chunks_total !== 0)) ||
      (report.bytes_total === 0 && report.chunks_total !== 0) ||
      report.chunks_total > report.bytes_total ||
      (report.unchanged && report.chunks_uploaded !== 0) ||
      !Array.isArray(report.skipped_external_symlinks) ||
      report.skipped_external_symlinks.length > 4096 ||
      report.skipped_external_symlinks.some((path) => typeof path !== "string" || path.length === 0 || path.length > 4096)) {
    throw new Error("CLW_SNAPSHOT_REPORT_INVALID_FIELDS");
  }
  return {
    root: report.root,
    bytesTotal: report.bytes_total,
    files: report.files,
    chunksTotal: report.chunks_total,
    chunksUploaded: report.chunks_uploaded,
    unchanged: report.unchanged,
  };
}

// ────────────────────────────────────────────────────────────────────────
// EXEC HELPER — in-container exec via port 9090
// ────────────────────────────────────────────────────────────────────────

/**
 * Call the in-container exec-server.
 */
export async function containerExec(
  doFetch: (url: string, init?: RequestInit, port?: number) => Promise<Response>,
  argv: readonly string[],
  options: { timeoutMs?: number; execToken?: string } = {},
): Promise<{ exitCode: number; stdout: string; stderr: string }> {
  const timeoutMs = options.timeoutMs ?? CLW_DEFAULT_TIMEOUT_MS;
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (options.execToken) {
    headers["X-Exec-Token"] = options.execToken;
  }
  const resp = await doFetch(
    `http://localhost:${EXEC_SERVER_PORT}/clw`,
    {
      method: "POST",
      headers,
      body: JSON.stringify({ argv }),
      signal: AbortSignal.timeout(timeoutMs),
    },
    EXEC_SERVER_PORT,
  );
  if (!resp.ok) {
    throw new Error(`EXEC_RPC_FAILED: ${resp.status} ${await resp.text()}`);
  }
  return parseClwExecResponse(resp);
}

// ────────────────────────────────────────────────────────────────────────
// CLW INVOCATION HELPERS
// ────────────────────────────────────────────────────────────────────────

/**
 * `clw ls --name <name> --ref-domain runner` — first-run pre-check.
 */
export async function clwRefExists(
  doFetch: (url: string, init?: RequestInit, port?: number) => Promise<Response>,
  name: string,
): Promise<boolean> {
  try {
    const r = await containerExec(
      doFetch,
      ["ls", "--name", name],
      { timeoutMs: 30_000 },
    );
    return r.exitCode === 0;
  } catch {
    return false;
  }
}

/**
 * `clw hydrate <dir> --name <name> --concurrency 8 --json`.
 */
export async function hydrateViaClw(
  doFetch: (url: string, init?: RequestInit, port?: number) => Promise<Response>,
  log: (event: string, fields?: Record<string, unknown>) => void,
  dir: string,
  name: string,
  traceId: string,
): Promise<HydrateMetadata | null> {
  if (!(await clwRefExists(doFetch, name))) {
    log("hydrate_first_run_skip", { traceId, name });
    return null;
  }
  const result = await containerExec(
    doFetch,
    [
      "hydrate", dir,
      "--name", name,
      "--concurrency", CLW_CONCURRENCY,
      "--json",
    ],
    { timeoutMs: CLW_DEFAULT_TIMEOUT_MS },
  );
  if (result.exitCode !== 0) {
    throw new Error(`clw hydrate failed (exit ${result.exitCode}): ${result.stderr}`);
  }
  try {
    const report = JSON.parse(result.stdout.trim()) as {
      root?: unknown;
      files?: unknown;
      bytes_total?: unknown;
      bytes_from_cache?: unknown;
      bytes_downloaded?: unknown;
    };
    return {
      root: typeof report.root === "string" ? report.root : null,
      bytesTotal: Number(report.bytes_total) || 0,
      bytesFromCache: Number(report.bytes_from_cache) || 0,
      bytesDownloaded: Number(report.bytes_downloaded) || 0,
      timestamp: Date.now(),
    };
  } catch (e) {
    log("clw_json_parse_failed", { subcmd: "hydrate", traceId, err: String(e) });
    return {
      root: null,
      bytesTotal: 0,
      bytesFromCache: 0,
      bytesDownloaded: 0,
      timestamp: Date.now(),
    };
  }
}

/**
 * `clw snapshot <dir> --name <name> --concurrency 8 --json [--force]`.
 */
export async function snapshotViaClw(
  doFetch: (url: string, init?: RequestInit, port?: number) => Promise<Response>,
  log: (event: string, fields?: Record<string, unknown>) => void,
  dir: string,
  name: string,
  options: { force: boolean; execToken?: string },
  traceId: string,
): Promise<SnapshotMetadata> {
  const args: string[] = [
    "snapshot", dir,
    "--name", name,
    "--concurrency", CLW_CONCURRENCY,
    "--json",
  ];
  if (options.force) args.push("--force");

  const result = await containerExec(doFetch, args, {
    timeoutMs: CLW_DEFAULT_TIMEOUT_MS,
    execToken: options.execToken,
  });
  if (result.exitCode !== 0) {
    throw new Error(`clw snapshot failed (exit ${result.exitCode}): ${result.stderr}`);
  }
  try {
    const report = parseClwSnapshotReport(result.stdout, name);
    return {
      name,
      root: report.root,
      bytesTotal: report.bytesTotal,
      files: report.files,
      chunksTotal: report.chunksTotal,
      chunksUploaded: report.chunksUploaded,
      unchanged: report.unchanged,
      timestamp: Date.now(),
    };
  } catch (e) {
    log("clw_json_parse_failed", { subcmd: "snapshot", traceId, err: String(e) });
    throw new Error("clw snapshot returned an invalid report");
  }
}

// ────────────────────────────────────────────────────────────────────────
// SNAPSHOT LOCK (side-table, NOT in DevenvState)
// ────────────────────────────────────────────────────────────────────────

export async function acquireSnapshotLock(
  storageGet: <T>(key: string) => T | null | Promise<T | null>,
  storagePut: (key: string, value: unknown) => Promise<void>,
  traceId: string,
): Promise<void> {
  const current = await storageGet<boolean>(SNAPSHOT_LOCK_KEY);
  if (current === true) {
    throw new Error("SNAPSHOT_IN_PROGRESS: another snapshot is running");
  }
  await storagePut(SNAPSHOT_LOCK_KEY, true);
  await storagePut(`${SNAPSHOT_LOCK_KEY}:traceId`, traceId);
}

export async function releaseSnapshotLock(
  storagePut: (key: string, value: unknown) => Promise<void>,
): Promise<void> {
  await storagePut(SNAPSHOT_LOCK_KEY, false);
  await storagePut(`${SNAPSHOT_LOCK_KEY}:traceId`, null);
}

export async function recoverSnapshotLockIfStale(
  storagePut: (key: string, value: unknown) => Promise<void>,
): Promise<void> {
  await storagePut(SNAPSHOT_LOCK_KEY, false);
}
