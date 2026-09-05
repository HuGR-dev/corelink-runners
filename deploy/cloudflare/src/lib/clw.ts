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
  const body = (await resp.json()) as { exit_code: number; stdout: string; stderr: string };
  return { exitCode: body.exit_code, stdout: body.stdout, stderr: body.stderr };
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
    const report = JSON.parse(result.stdout.trim()) as {
      root?: unknown;
      files?: unknown;
      bytes_total?: unknown;
      chunks_total?: unknown;
      chunks_uploaded?: unknown;
      unchanged?: unknown;
      skipped_external_symlinks?: unknown;
    };
    return {
      name,
      root: typeof report.root === "string" ? report.root : "",
      bytesTotal: Number(report.bytes_total) || 0,
      files: Number(report.files) || 0,
      chunksTotal: Number(report.chunks_total) || 0,
      chunksUploaded: Number(report.chunks_uploaded) || 0,
      unchanged: Boolean(report.unchanged),
      timestamp: Date.now(),
    };
  } catch (e) {
    log("clw_json_parse_failed", { subcmd: "snapshot", traceId, err: String(e) });
    return {
      name,
      root: "",
      bytesTotal: 0,
      files: 0,
      chunksTotal: 0,
      chunksUploaded: 0,
      unchanged: false,
      timestamp: Date.now(),
    };
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
