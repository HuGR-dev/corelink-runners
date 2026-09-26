import { normalizeRedriveIdentity } from "../containment_authority_helpers";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

export type NormalIntakeState = "pending" | "uncertain" | "complete";
export interface NormalIntakeRecord {
  schema_version: 1; event_id: string; body_sha256: string; job_id: string; repo: string;
  installation_id: string; labels: string[]; received_at_ms: number; state: NormalIntakeState; next_attempt_ms: number;
}
export interface NormalIntakeInput {
  schema_version: 1;
  event_id: string; body_sha256: string; job_id: string; repo: string; installation_id: string;
  labels: string[]; received_at_ms: number;
}
export type NormalIntakeOutcome = "complete" | "uncertain" | "retry";

export class NormalIntakeConflictError extends Error {
  readonly status = 409 as const; readonly code = "normal_intake_conflict" as const;
  constructor(message = "normal intake event conflicts with durable evidence") { super(message); this.name = "NormalIntakeConflictError"; }
}
const EVENT = "normal-inbox:v1:event:";
const PENDING = "normal-inbox:v1:pending:";
const UNCERTAIN = "normal-inbox:v1:uncertain:";
const COUNT = "normal-inbox:v1:count";
const MAX = 500;
const LEGACY_SCAN_PAGE_SIZE = 100;
const MAX_TEXT = 256;
const SHA = /^[0-9a-f]{64}$/;
const text = (value: unknown, max = MAX_TEXT, empty = false): value is string =>
  typeof value === "string" && value.length <= max && (empty || value.length > 0) && !/[\u0000-\u001f\u007f]/.test(value);
export const isNormalIntakeEventId = (value: unknown): value is string => text(value);
const safeTime = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
const eventKey = (id: string) => `${EVENT}${encodeURIComponent(id)}`;
const pendingKey = (record: NormalIntakeRecord) => `${PENDING}${String(record.received_at_ms).padStart(16, "0")}:${encodeURIComponent(record.event_id)}`;
const uncertainKey = (record: NormalIntakeRecord) => `${UNCERTAIN}${String(record.received_at_ms).padStart(16, "0")}:${encodeURIComponent(record.event_id)}`;
const validCount = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0 && (value as number) <= MAX;

function fail(message: string): never { throw new Error(`normal intake corruption: ${message}`); }
function validateInput(input: NormalIntakeInput): NormalIntakeInput {
  if (!input || input.schema_version !== 1 || !isNormalIntakeEventId(input.event_id) || !SHA.test(input.body_sha256) || !text(input.installation_id, MAX_TEXT, true)
    || !safeTime(input.received_at_ms) || !Array.isArray(input.labels) || input.labels.length > 32
    || input.labels.some((label) => !text(label, 128))) throw new Error("invalid normal intake record");
  const identity = normalizeRedriveIdentity(input.repo, input.job_id);
  if (!identity) throw new Error("invalid normal intake identity");
  return { schema_version: 1, event_id: input.event_id, body_sha256: input.body_sha256, job_id: identity.job_id,
    repo: identity.repo, installation_id: input.installation_id, labels: [...input.labels], received_at_ms: input.received_at_ms };
}
function validRecord(value: unknown, expectedEventId?: string): value is NormalIntakeRecord {
  if (!value || typeof value !== "object") return false;
  const r = value as Partial<NormalIntakeRecord>;
  const fields = ["body_sha256", "event_id", "installation_id", "job_id", "labels", "next_attempt_ms", "received_at_ms", "repo", "schema_version", "state"];
  if (Object.keys(value).sort().join(",") !== fields.join(",")) return false;
  const identity = normalizeRedriveIdentity(r.repo, r.job_id);
  return r.schema_version === 1 && text(r.event_id) && (!expectedEventId || r.event_id === expectedEventId) && typeof r.body_sha256 === "string" && SHA.test(r.body_sha256)
    && !!identity && identity.repo === r.repo && identity.job_id === r.job_id && text(r.installation_id, MAX_TEXT, true) && Array.isArray(r.labels)
    && r.labels.length <= 32 && r.labels.every((x) => text(x, 128)) && safeTime(r.received_at_ms)
    && (r.state === "pending" || r.state === "uncertain" || r.state === "complete") && safeTime(r.next_attempt_ms);
}
function validateNow(now: number): void { if (!safeTime(now)) throw new Error("invalid normal intake time"); }
function validateBody(body: string): void { if (!SHA.test(body)) throw new Error("invalid normal intake body hash"); }

export class NormalIntakeInbox {
  constructor(private readonly storage: AuthorityStorage) {}

  async uncertainSummary(now = Date.now()): Promise<{ count: number; oldest_age_ms: number | null }> {
    validateNow(now);
    return this.storage.transaction(async tx => {
      const countValue = await tx.get<unknown>(COUNT);
      const pending = await tx.list<string>({ prefix: PENDING, limit: MAX + 1 });
      const uncertain = await tx.list<string>({ prefix: UNCERTAIN, limit: MAX + 1 });
      if (pending.size > MAX || uncertain.size > MAX) fail("active index exceeds capacity");
      const count = countValue === undefined && pending.size === 0 && uncertain.size === 0 ? 0 : countValue;
      if (!validCount(count)) fail("malformed active count");
      if (count !== pending.size + uncertain.size) {
        const indexedCount = pending.size + uncertain.size;
        if (count < indexedCount) fail("active indexes exceed count");
        for (const [key, eventId] of pending) {
          if (typeof eventId !== "string") fail("malformed pending index");
          const record = await tx.get<unknown>(eventKey(eventId));
          if (!validRecord(record, eventId) || record.state !== "pending" || key !== pendingKey(record)) fail("pending index/state mismatch");
        }
        let oldestReceivedAt: number | null = null;
        for (const [key, eventId] of uncertain) {
          if (typeof eventId !== "string") fail("malformed uncertain index");
          const record = await tx.get<unknown>(eventKey(eventId));
          if (!validRecord(record, eventId) || record.state !== "uncertain" || key !== uncertainKey(record)) fail("uncertain index/state mismatch");
          if (oldestReceivedAt === null || record.received_at_ms < oldestReceivedAt) oldestReceivedAt = record.received_at_ms;
        }

        // Completed event rows are retained for delivery dedupe and can greatly
        // outnumber active capacity. Scan them in bounded pages, retaining only
        // the active census. The durable active count tells us how many legacy
        // uncertain rows must be found; stop as soon as that bounded set is
        // complete rather than requiring the retained history to fit one page.
        const expectedLegacy = count - indexedCount;
        let legacyFound = 0;
        let cursor: string | undefined;
        while (legacyFound < expectedLegacy) {
          const page = await tx.list<unknown>({ prefix: EVENT, ...(cursor ? { startAfter: cursor } : {}), limit: LEGACY_SCAN_PAGE_SIZE });
          if (page.size === 0) fail("active count/event mismatch");
          for (const [key, value] of page) {
            cursor = key;
            if (!validRecord(value) || key !== eventKey(value.event_id)) fail("malformed event record during legacy scan");
            if (value.state === "pending") {
              const indexKey = pendingKey(value);
              if (!pending.has(indexKey) || pending.get(indexKey) !== value.event_id) fail("pending index/state mismatch");
            } else if (value.state === "uncertain") {
              const indexKey = uncertainKey(value);
              if (uncertain.has(indexKey)) {
                if (uncertain.get(indexKey) !== value.event_id) fail("uncertain index/state mismatch");
              } else {
                legacyFound++;
                if (legacyFound > expectedLegacy) fail("active count/event mismatch");
                if (oldestReceivedAt === null || value.received_at_ms < oldestReceivedAt) oldestReceivedAt = value.received_at_ms;
                if (legacyFound === expectedLegacy) break;
              }
            }
          }
        }
        return { count: uncertain.size + legacyFound, oldest_age_ms: oldestReceivedAt === null ? null : Math.max(0, now - oldestReceivedAt) };
      }
      let oldestReceivedAt: number | null = null;
      for (const [key, eventId] of uncertain) {
        if (typeof eventId !== "string" || !key.startsWith(UNCERTAIN)) fail("malformed uncertain index");
        const record = await tx.get<unknown>(eventKey(eventId));
        if (!validRecord(record, eventId) || record.state !== "uncertain" || key !== uncertainKey(record)) fail("uncertain index/state mismatch");
        if (oldestReceivedAt === null || record.received_at_ms < oldestReceivedAt) oldestReceivedAt = record.received_at_ms;
      }
      return { count: uncertain.size, oldest_age_ms: oldestReceivedAt === null ? null : Math.max(0, now - oldestReceivedAt) };
    });
  }

  async inspect(eventId: string): Promise<NormalIntakeRecord | null> {
    if (!isNormalIntakeEventId(eventId)) throw new Error("invalid normal intake event id");
    const value = await this.storage.get<unknown>(eventKey(eventId));
    if (value === undefined) return null;
    if (!validRecord(value, eventId)) fail("malformed event record");
    return value;
  }

  async enqueue(input: NormalIntakeInput, now = Date.now(), delayMs = 0): Promise<{ status: "accepted" | "duplicate" | "conflict" | "full"; record?: NormalIntakeRecord }> {
    const normalized = validateInput(input);
    validateNow(now);
    if (!Number.isSafeInteger(delayMs) || delayMs < 0 || now > Number.MAX_SAFE_INTEGER - delayMs) throw new Error("invalid normal intake delay");
    return this.storage.transaction(async (tx: AuthorityTransaction) => {
      const key = eventKey(normalized.event_id);
      const old = await tx.get<unknown>(key);
      if (old !== undefined) {
        if (!validRecord(old, normalized.event_id)) fail("malformed event record");
        if (old.body_sha256 !== normalized.body_sha256 || old.job_id !== normalized.job_id || old.repo !== normalized.repo
          || old.installation_id !== normalized.installation_id || JSON.stringify(old.labels) !== JSON.stringify(normalized.labels)) return { status: "conflict" };
        return { status: "duplicate", record: old };
      }
      const countValue = await tx.get<unknown>(COUNT);
      if (countValue === undefined) {
        const existingEvents = await tx.list({ prefix: EVENT, limit: 1 });
        if (existingEvents.size > 0) fail("missing active count");
      }
      const count = countValue === undefined ? 0 : countValue;
      if (!validCount(count)) fail("malformed active count");
      if (count >= MAX) return { status: "full" };
      const record: NormalIntakeRecord = { schema_version: 1, event_id: normalized.event_id, body_sha256: normalized.body_sha256,
        job_id: normalized.job_id, repo: normalized.repo, installation_id: normalized.installation_id, labels: [...normalized.labels],
        received_at_ms: normalized.received_at_ms, state: "pending", next_attempt_ms: now + delayMs };
      await tx.put(key, record);
      await tx.put(pendingKey(record), record.event_id);
      await tx.put(COUNT, count + 1);
      return { status: "accepted", record };
    });
  }

  async pending(now: number, limit = 25): Promise<NormalIntakeRecord[]> {
    validateNow(now);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 25) throw new Error("invalid normal intake limit");
    return this.storage.transaction(async tx => {
      // The active set has a hard bound. Inspect it completely so delayed heads
      // cannot permanently hide ready work beyond a short page.
      const page = await tx.list<string>({ prefix: PENDING, limit: MAX + 1 });
      if (page.size > MAX) fail("pending index exceeds capacity");
      const result: NormalIntakeRecord[] = [];
      for (const [key, value] of page) {
        if (typeof value !== "string" || !key.startsWith(PENDING)) fail("malformed pending index");
        const record = await tx.get<unknown>(eventKey(value));
        if (!validRecord(record, value)) fail("malformed event record");
        if (record.state !== "pending" || key !== pendingKey(record)) fail("pending index/state mismatch");
        if (record.next_attempt_ms <= now) result.push(record);
      }
      return result.sort((a, b) => a.received_at_ms - b.received_at_ms || a.event_id.localeCompare(b.event_id)).slice(0, limit);
    });
  }

  async settle(eventId: string, expectedBodySha: string, outcome: NormalIntakeOutcome, now: number): Promise<void> {
    if (!isNormalIntakeEventId(eventId) || !["complete", "uncertain", "retry"].includes(outcome)) throw new Error("invalid normal intake settlement");
    validateBody(expectedBodySha); validateNow(now);
    return this.storage.transaction(async (tx: AuthorityTransaction) => {
      const key = eventKey(eventId);
      const value = await tx.get<unknown>(key);
      if (!validRecord(value, eventId)) fail("missing or malformed event record");
      if (value.body_sha256 !== expectedBodySha) throw new NormalIntakeConflictError();
      if (value.state === "complete" || (value.state === "uncertain" && outcome !== "complete")) return;
      const nextState: NormalIntakeState = outcome === "complete" ? "complete" : outcome === "uncertain" ? "uncertain" : "pending";
      if (outcome === "retry" && now > Number.MAX_SAFE_INTEGER - 60_000) throw new Error("invalid normal intake retry time");
      const next: NormalIntakeRecord = { schema_version: 1, event_id: value.event_id, body_sha256: value.body_sha256,
        job_id: value.job_id, repo: value.repo, installation_id: value.installation_id, labels: [...value.labels], received_at_ms: value.received_at_ms,
        state: nextState, next_attempt_ms: outcome === "retry" ? now + 60_000 : value.next_attempt_ms };
      const countValue = await tx.get<unknown>(COUNT);
      if (!validCount(countValue)) fail("malformed active count");
      if (outcome === "complete") { if (countValue < 1) fail("active count underflow"); await tx.delete(pendingKey(value)); await tx.put(COUNT, countValue - 1); }
      else if (outcome === "uncertain") {
        await tx.delete(pendingKey(value));
        await tx.put(uncertainKey(next), next.event_id);
      }
      else await tx.put(pendingKey(next), next.event_id);
      if (outcome === "complete" && value.state === "uncertain") await tx.delete(uncertainKey(value));
      await tx.put(key, next);
      return;
    });
  }
}
