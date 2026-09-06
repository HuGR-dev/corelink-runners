import {
  buildUsageEvent,
  cfAccessHeaders,
  logEvent,
  type BillingEnv,
  type KvLike,
  type UsageEvent,
  type UsageLedgerRecord,
  USAGE_LEDGER_TTL_S,
} from "./lib.js";

/** The ingest cap is deliberately below the server's 1024-event limit. */
export const BILLING_FLUSH_CHUNK_SIZE = 100;
export const BILLING_USAGE_PREFIX = "usage:";
export const BILLING_QUARANTINE_PREFIX = "usage:quarantine:";
export const BILLING_FLUSH_CURSOR_KEY = "usage:flush:cursor";
const BILLING_SETTLED_PREFIX = "usage:settled:";
export const BILLING_SETTLEMENT_TTL_S = USAGE_LEDGER_TTL_S;

export interface BillingFlushResult {
  scanned: number;
  pushed: number;
  quarantined: number;
  failed: number;
  pages: number;
}

interface UsageListPage {
  keys: { name: string }[];
  cursor?: string;
  list_complete?: boolean;
}

function quarantineKey(sourceKey: string): string {
  return `${BILLING_QUARANTINE_PREFIX}${encodeURIComponent(sourceKey)}`;
}

function settledKey(jobId: string): string {
  return `${BILLING_SETTLED_PREFIX}${encodeURIComponent(jobId)}`;
}

function parseRecord(raw: string, key: string): UsageLedgerRecord {
  const value = JSON.parse(raw) as Partial<UsageLedgerRecord>;
  if (!value || typeof value.jobId !== "string" || value.jobId.length === 0) throw new Error("jobId");
  if (typeof value.tenant !== "string" || value.tenant.length === 0) throw new Error("tenant");
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value.tenant)) throw new Error("tenant_uuid");
  if (!Number.isFinite(value.startedMs)) throw new Error("startedMs");
  if (!Number.isFinite(value.completedMs)) throw new Error("completedMs");
  if (!Number.isSafeInteger(value.startedMs) || value.startedMs < 0 || !Number.isSafeInteger(value.completedMs) || value.completedMs < value.startedMs) throw new Error("timestamp_range");
  if (typeof value.region !== "string" || !/^[a-z]{3}$/.test(value.region)) throw new Error("region");
  if (key !== `${BILLING_USAGE_PREFIX}${value.jobId}`) throw new Error("key/job mismatch");
  return value as UsageLedgerRecord;
}

async function quarantine(kv: KvLike, key: string, raw: string, reason: string): Promise<void> {
  // The source is removed only after the quarantine copy is durable. A failed
  // quarantine leaves the source available for the next cursor pass.
  await kv.put(quarantineKey(key), JSON.stringify({ source_key: key, raw, reason, quarantined_at_ms: Date.now() }));
  await kv.delete(key);
}

async function postBatch(env: BillingEnv, events: UsageEvent[], signal?: AbortSignal): Promise<void> {
  const response = await fetch(env.BILLING_INGEST_URL ?? "", {
    method: "POST",
    headers: {
      ...cfAccessHeaders(env),
      "x-corelink-internal-auth": env.BILLING_INGEST_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    body: JSON.stringify(events),
    ...(signal ? { signal } : {}),
  });
  if (!response.ok) throw new Error(`billing usage-push ${response.status}`);
}

/**
 * Flush every durable completed-job record reachable from the KV cursor.
 * Records stay durable after a successful push because ingest is at-least-once
 * and `idem_key` is the exactly-once boundary. A failed chunk therefore retries
 * on the next cron tick without losing pending usage.
 */
export async function flushBillingUsageBacklog(
  env: BillingEnv & { RUNNER_JOB_PATS?: KvLike },
  options: { maxPages?: number; chunkSize?: number } = {},
): Promise<BillingFlushResult> {
  const result: BillingFlushResult = { scanned: 0, pushed: 0, quarantined: 0, failed: 0, pages: 0 };
  const kv = env.RUNNER_JOB_PATS;
  if (!kv || !env.BILLING_INGEST_URL || !env.BILLING_INGEST_AUTH_KEY || !kv.list) return result;
  const maxPages = Math.max(1, options.maxPages ?? 100);
  const chunkSize = Math.max(1, Math.min(1024, options.chunkSize ?? BILLING_FLUSH_CHUNK_SIZE));
  let cursor = await kv.get(BILLING_FLUSH_CURSOR_KEY) ?? undefined;
  let exhausted = false;
  const seenCursors = new Set<string>();
  const quarantines: { key: string; raw: string; reason: string }[] = [];
  for (let pageNo = 0; pageNo < maxPages; pageNo += 1) {
    if (cursor && seenCursors.has(cursor)) {
      result.failed += 1;
      logEvent("error", "billing_flush_cursor_repeated", { cursor });
      break;
    }
    if (cursor) seenCursors.add(cursor);
    let page: UsageListPage;
    try {
      page = await kv.list({ prefix: BILLING_USAGE_PREFIX, ...(cursor ? { cursor } : {}) }) as UsageListPage;
    } catch (error) {
      logEvent("error", "billing_flush_list_failed", { page: pageNo, error: (error as Error).message });
      result.failed += 1;
      break;
    }
    result.pages += 1;
    const events: UsageEvent[] = [];
    for (const item of page.keys ?? []) {
      if (!item.name.startsWith(BILLING_USAGE_PREFIX)
        || item.name.startsWith(BILLING_QUARANTINE_PREFIX)
        || item.name.startsWith(BILLING_SETTLED_PREFIX)
        || item.name === BILLING_FLUSH_CURSOR_KEY) continue;
      result.scanned += 1;
      let raw: string | null;
      try {
        raw = await kv.get(item.name);
      } catch (error) {
        result.failed += 1;
        logEvent("error", "billing_usage_read_failed", { key: item.name, error: (error as Error).message });
        continue;
      }
      if (raw === null) continue;
      try {
        const rec = parseRecord(raw, item.name);
        const event = await buildUsageEvent({
          tenantId: rec.tenant,
          jobId: rec.jobId,
          startedMs: rec.startedMs,
          completedMs: rec.completedMs,
          region: rec.region,
        });
        let settled: string | null;
        try {
          settled = await kv.get(settledKey(event.idem_key));
        } catch (error) {
          result.failed += 1;
          logEvent("error", "billing_settlement_read_failed", { key: item.name, error: (error as Error).message });
          continue;
        }
        if (!settled) events.push(event);
      } catch (error) {
        // Defer deletion until the cursor has been exhausted. KV cursors are
        // opaque snapshots; mutating the namespace while walking them can make
        // a later page skip the key immediately after the poison record.
        quarantines.push({ key: item.name, raw, reason: (error as Error).message });
      }
    }
    let pageFailed = false;
    for (let i = 0; i < events.length; i += chunkSize) {
      const chunk = events.slice(i, i + chunkSize);
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 5_000);
      try {
        await postBatch(env, chunk, controller.signal);
        result.pushed += chunk.length;
        await Promise.all(chunk.map((event) => kv.put(settledKey(event.idem_key), String(Date.now()), {
          expirationTtl: BILLING_SETTLEMENT_TTL_S,
        })));
      } catch (error) {
        // Keep every source record. The same idem keys make the retry safe.
        result.failed += chunk.length;
        pageFailed = true;
        logEvent("error", "billing_flush_push_failed", { count: chunk.length, error: (error as Error).message });
      } finally {
        clearTimeout(timer);
      }
    }
    if (pageFailed) break;
    if (page.list_complete !== false) { exhausted = true; break; }
    if (!page.cursor) {
      result.failed += 1;
      logEvent("error", "billing_flush_cursor_missing", { page: pageNo });
      break;
    }
    cursor = page.cursor;
    await kv.put(BILLING_FLUSH_CURSOR_KEY, cursor);
  }
  if (exhausted) await kv.delete(BILLING_FLUSH_CURSOR_KEY).catch(() => {});
  for (const item of quarantines) {
    try {
      await quarantine(kv, item.key, item.raw, item.reason);
      result.quarantined += 1;
    } catch (error) {
      result.failed += 1;
      logEvent("error", "billing_usage_quarantine_failed", { key: item.key, error: (error as Error).message });
    }
  }
  return result;
}
