// Runtime-agnostic reconciliation discovery and handoff helpers.
//
// The registry is the authority for customer recovery.  A caller must never
// turn "all repositories visible to a GitHub token" into a recovery set: that
// would both widen the credential scope and make a transient registry failure
// look like an empty (therefore safe) answer.  The registry response is a
// bounded, snapshot-pinned cursor stream of repositories that are both
// eligible and verified, together with the installation that authorizes the
// GitHub read and subsequent spawn.

import type { KvLike } from "./lib";

export interface ReconcilerRepository {
  repo: string;
  installationId: string;
}

export interface ReconcilerRegistryEnv {
  RECONCILER_REGISTRY_URL?: string;
  RECONCILER_REGISTRY_AUTH_KEY?: string;
}

export interface RegistryPage {
  schema_version: 1;
  source: "runner_repo_allowlist";
  snapshot_id: string;
  repositories: Array<{
    repo_full_name?: string;
    installation_id?: string | number;
  }>;
  next_cursor?: string | null;
}

const MAX_REGISTRY_PAGES = 100;
const REGISTRY_TIMEOUT_MS = 5_000;
const MAX_REGISTRY_BYTES = 256 * 1024;
const HANDOFF_LEASE_MS = 30_000;
const REPO_RE = /^[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?\/[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?$/;

function canonicalRepo(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const parts = value.trim().split("/");
  if (parts.length !== 2) return null;
  const repo = `${parts[0].trim().toLowerCase()}/${parts[1].trim().toLowerCase()}`;
  return REPO_RE.test(repo) ? repo : null;
}

function installationId(value: unknown): string | null {
  const id = typeof value === "number" && Number.isSafeInteger(value)
    ? String(value)
    : typeof value === "string" ? value.trim() : "";
  return /^[1-9][0-9]*$/.test(id) ? id : null;
}

/**
 * Poll the authoritative eligible-repository registry.
 *
 * A malformed page, changed snapshot, repeated cursor, missing auth header,
 * or an incomplete stream is an unknown answer and returns `null`.  Returning
 * `[]` would incorrectly authorize no repositories as a complete inventory.
 */
export async function discoverEligibleRepositories(
  env: ReconcilerRegistryEnv,
  fetcher: typeof fetch = fetch,
): Promise<ReconcilerRepository[] | null> {
  const base = env.RECONCILER_REGISTRY_URL?.trim();
  const auth = env.RECONCILER_REGISTRY_AUTH_KEY?.trim();
  if (!base || !auth) return null;
  let cursor: string | undefined;
  let snapshot: string | undefined;
  const seenCursors = new Set<string>();
  const found = new Map<string, ReconcilerRepository>();

  for (let pageNumber = 0; pageNumber < MAX_REGISTRY_PAGES; pageNumber++) {
    const url = new URL(base);
    if (cursor) url.searchParams.set("cursor", cursor);
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), REGISTRY_TIMEOUT_MS);
    let response: Response;
    try {
      response = await fetcher(url.toString(), {
        headers: {
          "x-corelink-internal-auth": auth,
          accept: "application/json",
          "user-agent": "corelink-spawn-worker-reconciler",
        },
        signal: controller.signal,
      });
      if (!response.ok) return null;
      const bytes = await response.arrayBuffer();
      if (bytes.byteLength > MAX_REGISTRY_BYTES) return null;
      const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
      const body = JSON.parse(text) as Partial<RegistryPage>;
      if (body.schema_version !== 1 || body.source !== "runner_repo_allowlist" || typeof body.snapshot_id !== "string" || body.snapshot_id.length === 0 || !Array.isArray(body.repositories)) return null;
      if (snapshot === undefined) snapshot = body.snapshot_id;
      if (snapshot !== body.snapshot_id) return null;
      for (const entry of body.repositories) {
        const repo = canonicalRepo(entry.repo_full_name);
        const install = installationId(entry.installation_id);
        if (!repo || !install) return null;
        const prior = found.get(repo);
        if (prior && prior.installationId !== install) return null;
        found.set(repo, { repo, installationId: install });
      }
      const next = body.next_cursor;
      if (next == null || next === "") {
        return [...found.values()].sort((a, b) => a.repo.localeCompare(b.repo));
      }
      if (typeof next !== "string" || seenCursors.has(next)) return null;
      seenCursors.add(next);
      cursor = next;
    } catch {
      return null;
    } finally {
      clearTimeout(timeout);
    }
  }
  return null;
}

export interface ReconcileHandoff {
  schema_version: 1;
  repo: string;
  job_id: string;
  installation_id: string;
  labels: string[];
  snapshot_id?: string;
  enqueued_at_ms: number;
}

export function reconcileHandoffKey(repo: string, jobId: string): string {
  return `reconcile:handoff:${repo}:${jobId}`;
}

/** Durable idempotency marker for work handed to waitUntil. */
export async function claimReconcileHandoff(
  kv: KvLike | undefined,
  handoff: ReconcileHandoff,
  nowMs = Date.now(),
): Promise<boolean> {
  if (!kv) return true;
  const key = reconcileHandoffKey(handoff.repo, handoff.job_id);
  const existing = await kv.get(key);
  if (existing) {
    try {
      const prior = JSON.parse(existing) as Partial<ReconcileHandoff>;
      if (typeof prior.enqueued_at_ms === "number" && nowMs - prior.enqueued_at_ms > HANDOFF_LEASE_MS) {
        await kv.delete(key);
      } else {
        return false;
      }
    } catch {
      // A corrupt marker is not evidence of a live handoff. Replace it with a
      // canonical record so recovery is durable and inspectable.
      await kv.delete(key);
    }
  }
  await kv.put(key, JSON.stringify(handoff), { expirationTtl: 1800 });
  return true;
}

export async function releaseReconcileHandoff(kv: KvLike | undefined, repo: string, jobId: string): Promise<void> {
  if (!kv) return;
  await kv.delete(reconcileHandoffKey(repo, jobId)).catch(() => {});
}
