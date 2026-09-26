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
import { bumpMetrics } from "./metrics.js";

/** The ingest cap is deliberately below the server's 1024-event limit. */
export const BILLING_FLUSH_CHUNK_SIZE = 100;
export const BILLING_USAGE_PREFIX = "usage:";
export const BILLING_QUARANTINE_PREFIX = "usage:quarantine:";
export const BILLING_FLUSH_CURSOR_KEY = "usage:flush:cursor";
const BILLING_SETTLED_PREFIX = "usage:settled:";
export const BILLING_SETTLEMENT_TTL_S = USAGE_LEDGER_TTL_S;
/** Keep an ingest acknowledgement bounded before parsing it. */
export const BILLING_ACK_MAX_BYTES = 1_048_576;

export interface BillingFlushResult {
  scanned: number;
  pushed: number;
  quarantined: number;
  failed: number;
  pages: number;
  accepted: number;
  deduped: number;
  rejected: number;
  conflicts: number;
  ambiguous: number;
  transportFailed: number;
  settlementWriteFailed: number;
  quarantineWriteFailed: number;
}

class BillingFlushError extends Error {
  constructor(readonly kind: "ambiguous" | "transport", message: string) {
    super(message);
  }
}

function invalidAcknowledgement(message: string): never {
  throw new BillingFlushError("ambiguous", message);
}

interface UsageListPage {
  keys: { name: string }[];
  cursor?: string;
  list_complete?: boolean;
}

type BillingRecordOutcomeKind = "accepted" | "deduped" | "rejected" | "conflict";

interface BillingRecordOutcome {
  index: number;
  idem_key: string | null;
  outcome: BillingRecordOutcomeKind;
  reason?: string;
}

interface BillingBatchAcknowledgement {
  outcomes: BillingRecordOutcome[];
  accepted: number;
  deduped: number;
  rejected: number;
  total: number;
}

interface PendingBillingEvent {
  key: string;
  raw: string;
  event: UsageEvent;
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
  if (typeof value.startedMs !== "number" || !Number.isFinite(value.startedMs)) throw new Error("startedMs");
  if (typeof value.completedMs !== "number" || !Number.isFinite(value.completedMs)) throw new Error("completedMs");
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, required: string[], optional: string[] = []): boolean {
  const keys = Object.keys(value);
  return required.every((key) => Object.hasOwn(value, key))
    && keys.every((key) => required.includes(key) || optional.includes(key));
}

function nonNegativeCount(value: unknown, max: number): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= max ? value : null;
}

async function readBoundedResponseBody(response: Response): Promise<string> {
  const reader = response.body?.getReader();
  if (!reader) return "";
  const decoder = new TextDecoder();
  let body = "";
  let bytes = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return body + decoder.decode();
    bytes += value.byteLength;
    if (bytes > BILLING_ACK_MAX_BYTES) {
      await reader.cancel();
      throw new Error("billing usage acknowledgement too large");
    }
    body += decoder.decode(value, { stream: true });
  }
}

async function postBatch(env: BillingEnv, events: UsageEvent[], signal?: AbortSignal): Promise<BillingBatchAcknowledgement> {
  let response: Response;
  try {
    response = await fetch(env.BILLING_INGEST_URL ?? "", {
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
  } catch (error) {
    throw new BillingFlushError("transport", (error as Error).message);
  }
  // The server uses 409 for a batch containing a durable conflict and 422 for
  // an entirely rejected batch. Both responses carry useful per-record
  // outcomes and must be parsed; every other non-202 is a transport/batch
  // failure whose records remain retryable.
  if (response.status !== 202 && response.status !== 409 && response.status !== 422) {
    throw new BillingFlushError("transport", `billing usage-push ${response.status}`);
  }
  const contentLength = response.headers.get("content-length");
  if (contentLength !== null && (!/^\d+$/.test(contentLength) || Number(contentLength) > BILLING_ACK_MAX_BYTES)) {
    return invalidAcknowledgement("billing usage acknowledgement too large");
  }
  const body = await readBoundedResponseBody(response).catch((error: unknown) =>
    invalidAcknowledgement((error as Error).message));
  let value: unknown = null;
  try {
    value = JSON.parse(body);
  } catch {
    return invalidAcknowledgement("billing usage acknowledgement invalid JSON");
  }
  if (!isRecord(value) || !hasExactKeys(value, ["outcomes", "accepted", "deduped", "rejected", "total"])
    || !Array.isArray(value.outcomes)) {
    return invalidAcknowledgement("billing usage acknowledgement missing outcomes");
  }
  if (value.outcomes.length !== events.length) {
    return invalidAcknowledgement("billing usage acknowledgement outcome length mismatch");
  }
  const accepted = nonNegativeCount(value.accepted, events.length);
  const deduped = nonNegativeCount(value.deduped, events.length);
  const rejected = nonNegativeCount(value.rejected, events.length);
  const total = nonNegativeCount(value.total, events.length);
  if (accepted === null || deduped === null || rejected === null || total === null) {
    return invalidAcknowledgement("billing usage acknowledgement counts invalid");
  }
  const outcomes: BillingRecordOutcome[] = [];
  let actualAccepted = 0;
  let actualDeduped = 0;
  let actualRejected = 0;
  let actualConflicts = 0;
  for (let index = 0; index < events.length; index += 1) {
    const item = value.outcomes[index];
    if (!isRecord(item) || !hasExactKeys(item, ["index", "idem_key", "outcome"], ["reason"])
      || item.index !== index
      || (item.outcome !== "accepted" && item.outcome !== "deduped"
        && item.outcome !== "rejected" && item.outcome !== "conflict")) {
      return invalidAcknowledgement("billing usage acknowledgement outcome ordering invalid");
    }
    const idemKey = item.idem_key;
    if (idemKey !== null && typeof idemKey !== "string") {
      return invalidAcknowledgement("billing usage acknowledgement idem key invalid");
    }
    if (idemKey !== events[index].idem_key) {
      // This worker only sends valid events. A missing or mismatched key would
      // make settlement attribution ambiguous, so retain the complete chunk.
      return invalidAcknowledgement("billing usage acknowledgement idem key mismatch");
    }
    const rawReason: unknown = item.reason;
    const hasReason = rawReason !== undefined;
    if ((hasReason && (typeof rawReason !== "string" || rawReason.length === 0))
      || ((item.outcome === "rejected" || item.outcome === "conflict") !== hasReason)) {
      return invalidAcknowledgement("billing usage acknowledgement reason invalid");
    }
    const reason = typeof rawReason === "string" ? rawReason : undefined;
    const outcome = item.outcome as BillingRecordOutcomeKind;
    if (outcome === "accepted") actualAccepted += 1;
    else if (outcome === "deduped") actualDeduped += 1;
    else if (outcome === "rejected") actualRejected += 1;
    else actualConflicts += 1;
    outcomes.push({ index, idem_key: idemKey, outcome, ...(reason !== undefined ? { reason } : {}) });
  }
  if (accepted !== actualAccepted || deduped !== actualDeduped || rejected !== actualRejected
    || total !== accepted + deduped) {
    return invalidAcknowledgement("billing usage acknowledgement counts mismatch");
  }
  if (actualConflicts > 0 && response.status !== 409) {
    return invalidAcknowledgement("billing usage acknowledgement conflict status mismatch");
  }
  if (actualConflicts === 0 && actualRejected === events.length && response.status !== 422) {
    return invalidAcknowledgement("billing usage acknowledgement rejection status mismatch");
  }
  if (actualConflicts === 0 && actualRejected !== events.length && response.status !== 202) {
    return invalidAcknowledgement("billing usage acknowledgement success status mismatch");
  }
  return { outcomes, accepted, deduped, rejected, total };
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
  const result: BillingFlushResult = {
    scanned: 0,
    pushed: 0,
    quarantined: 0,
    failed: 0,
    pages: 0,
    accepted: 0,
    deduped: 0,
    rejected: 0,
    conflicts: 0,
    ambiguous: 0,
    transportFailed: 0,
    settlementWriteFailed: 0,
    quarantineWriteFailed: 0,
  };
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
    const events: PendingBillingEvent[] = [];
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
        if (!settled) events.push({ key: item.name, raw, event });
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
      let acknowledgementParsed = false;
      try {
        const acknowledgement = await postBatch(env, chunk.map((item) => item.event), controller.signal);
        acknowledgementParsed = true;
        const settled = acknowledgement.outcomes.filter((item) => item.outcome === "accepted" || item.outcome === "deduped");
        const explicitFailures = acknowledgement.outcomes.filter((item) => item.outcome === "rejected" || item.outcome === "conflict");
        result.accepted += acknowledgement.accepted;
        result.deduped += acknowledgement.deduped;
        result.rejected += acknowledgement.rejected;
        result.conflicts += acknowledgement.outcomes.filter((item) => item.outcome === "conflict").length;
        await Promise.all(settled.map((item) => kv.put(settledKey(chunk[item.index].event.idem_key), String(Date.now()), {
          expirationTtl: BILLING_SETTLEMENT_TTL_S,
        })));
        result.pushed += settled.length;
        for (const item of explicitFailures) {
          quarantines.push({
            key: chunk[item.index].key,
            raw: chunk[item.index].raw,
            reason: `billing_ingest_${item.outcome}${item.reason ? `:${item.reason}` : ""}`,
          });
        }
      } catch (error) {
        // Keep every source record. The same idem keys make the retry safe.
        result.failed += chunk.length;
        if (error instanceof BillingFlushError && error.kind === "ambiguous") {
          result.ambiguous += chunk.length;
        } else if (error instanceof BillingFlushError && error.kind === "transport") {
          result.transportFailed += chunk.length;
        } else if (acknowledgementParsed) {
          // The acknowledgement parsed, so an exception here is a settlement
          // write failure. Keep all sources; already written markers are safe.
          result.settlementWriteFailed += 1;
        }
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
      result.quarantineWriteFailed += 1;
      logEvent("error", "billing_usage_quarantine_failed", { key: item.key, error: (error as Error).message });
    }
  }
  const metricCounts: [string, number][] = [
    ["billing_ingest_accepted", result.accepted],
    ["billing_ingest_deduped", result.deduped],
    ["billing_ingest_rejected", result.rejected],
    ["billing_ingest_conflict", result.conflicts],
    ["billing_ingest_ambiguous", result.ambiguous],
    ["billing_ingest_transport_failed", result.transportFailed],
    ["billing_settlement_write_failed", result.settlementWriteFailed],
    ["billing_quarantine_write_failed", result.quarantineWriteFailed],
  ];
  for (const [name, count] of metricCounts) {
    if (count > 0) await bumpMetrics(env as Parameters<typeof bumpMetrics>[0], ...Array.from({ length: count }, () => name));
  }
  return result;
}
