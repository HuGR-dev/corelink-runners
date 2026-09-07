// Runtime-agnostic reconciliation discovery and handoff helpers.
//
// The registry is the authority for authorization candidates. This module only
// consumes its bounded cursor stream; membership/eligibility decisions belong to
// the next reconciliation stage.

import type { KvLike } from "./lib";
import { canonicalInstallationId } from "./repo_config_lookup";

export interface ReconcilerRepository {
  repo: string;
  installationId: string;
}

export interface ReconcilerRegistryEnv {
  RECONCILER_REGISTRY_URL?: string;
  RECONCILER_REGISTRY_AUTH_KEY?: string;
}

const MAX_REGISTRY_PAGES = 100;
const REGISTRY_TIMEOUT_MS = 5_000;
const MAX_REGISTRY_BYTES = 256 * 1024;
const HANDOFF_LEASE_MS = 30_000;
const REPO_RE = /^[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?\/[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?$/;

function validRepo(value: unknown): string | null {
  if (typeof value !== "string") return null;
  return REPO_RE.test(value) ? value : null;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

async function readBoundedBody(response: Response): Promise<Uint8Array | null> {
  const reader = response.body?.getReader();
  if (!reader) return null;
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    while (true) {
      const part = await reader.read();
      if (part.done) break;
      total += part.value.byteLength;
      if (total > MAX_REGISTRY_BYTES) {
        await reader.cancel("registry response exceeds byte limit");
        return null;
      }
      chunks.push(part.value);
    }
    const body = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      body.set(chunk, offset);
      offset += chunk.byteLength;
    }
    return body;
  } catch {
    await reader.cancel().catch(() => {});
    return null;
  } finally {
    reader.releaseLock();
  }
}

/**
 * Poll the authoritative authorization-candidate registry.
 *
 * A malformed page, repeated cursor, missing auth header,
 * or an incomplete stream is an unknown answer and returns `null`.  Returning
 * `[]` would incorrectly authorize no repositories as a complete inventory.
 */
export async function discoverAuthorizationCandidates(
  env: ReconcilerRegistryEnv,
  fetcher: typeof fetch = fetch,
): Promise<ReconcilerRepository[] | null> {
  const base = env.RECONCILER_REGISTRY_URL?.trim();
  const auth = env.RECONCILER_REGISTRY_AUTH_KEY?.trim();
  if (!base || !auth) return null;
  let baseUrl: URL;
  try {
    baseUrl = new URL(base);
    if (baseUrl.protocol !== "https:") return null;
  } catch {
    return null;
  }
  let cursor: string | undefined;
  const seenCursors = new Set<string>();
  const found = new Map<string, ReconcilerRepository>();

  for (let pageNumber = 0; pageNumber < MAX_REGISTRY_PAGES; pageNumber++) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), REGISTRY_TIMEOUT_MS);
    let response: Response;
    try {
      const url = new URL(baseUrl.toString());
      if (cursor) url.searchParams.set("cursor", cursor);
      response = await fetcher(url.toString(), {
        method: "GET",
        redirect: "error",
        headers: {
          "x-corelink-internal-auth": auth,
          accept: "application/json",
          "user-agent": "corelink-spawn-worker-reconciler",
        },
        signal: controller.signal,
      });
      if (!response.ok) return null;
      const bytes = await readBoundedBody(response);
      if (!bytes) return null;
      const text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(bytes);
      const body: unknown = JSON.parse(text);
      if (!record(body)
        || body.schema_version !== 1
        || body.source !== "runner_authorization_candidates"
        || !Array.isArray(body.repositories)
        || !Object.prototype.hasOwnProperty.call(body, "next_cursor")
        || (body.next_cursor !== null && typeof body.next_cursor !== "string")) return null;
      for (const entry of body.repositories) {
        if (!record(entry)
          || typeof entry.repo_full_name !== "string"
          || (typeof entry.installation_id !== "string" && typeof entry.installation_id !== "number")) return null;
        const repo = validRepo(entry.repo_full_name);
        const install = canonicalInstallationId(entry.installation_id);
        if (!repo || !install) return null;
        found.set(`${repo}\u0000${install}`, { repo, installationId: install });
      }
      const next = body.next_cursor;
      if (next === null) {
        return [...found.values()].sort((a, b) => a.repo.localeCompare(b.repo) || a.installationId.localeCompare(b.installationId));
      }
      if (next === "" || seenCursors.has(next)) return null;
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
