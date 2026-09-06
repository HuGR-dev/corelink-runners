import { normalizeRedriveIdentity } from "../containment_authority_helpers";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

export type NormalIntakeState = "pending" | "uncertain" | "complete";
export interface NormalIntakeRecord {
  schema_version: 1; event_id: string; body_sha256: string; job_id: string; repo: string;
  installation_id: string; labels: string[]; received_at_ms: number; state: NormalIntakeState; next_attempt_ms: number;
}
export interface NormalIntakeInput {
  event_id: string; body_sha256: string; job_id: string; repo: string; installation_id: string;
  labels: string[]; received_at_ms: number;
}
export type NormalIntakeOutcome = "complete" | "uncertain" | "retry";

export class NormalIntakeConflictError extends Error {
  readonly status = 409 as const; readonly code = "normal_intake_conflict" as const;
  constructor(message = "normal intake event conflicts with durable evidence") { super(message); this.name = "NormalIntakeConflictError"; }
}
export class NormalIntakeCapacityError extends Error {
  readonly status = 503 as const; readonly code = "normal_intake_capacity" as const;
  constructor() { super("normal intake inbox is at capacity"); this.name = "NormalIntakeCapacityError"; }
}

const EVENT = "normal-inbox:v1:event:";
const PENDING = "normal-inbox:v1:pending:";
const COUNT = "normal-inbox:v1:count";
const MAX = 500;
const MAX_TEXT = 256;
const SHA = /^[0-9a-f]{64}$/;
const text = (value: unknown, max = MAX_TEXT, empty = false): value is string =>
  typeof value === "string" && value.length <= max && (empty || value.length > 0) && !/[\u0000-\u001f\u007f]/.test(value);
const safeTime = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
const eventKey = (id: string) => `${EVENT}${encodeURIComponent(id)}`;
const pendingKey = (record: NormalIntakeRecord) => `${PENDING}${String(record.received_at_ms).padStart(16, "0")}:${encodeURIComponent(record.event_id)}`;
const validCount = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0 && (value as number) <= MAX;

function fail(message: string): never { throw new Error(`normal intake corruption: ${message}`); }
function validateInput(input: NormalIntakeInput): NormalIntakeInput {
  if (!input || !text(input.event_id) || !SHA.test(input.body_sha256) || !text(input.installation_id, MAX_TEXT, true)
    || !safeTime(input.received_at_ms) || !Array.isArray(input.labels) || input.labels.length > 32
    || input.labels.some((label) => !text(label, 128))) throw new Error("invalid normal intake record");
  const identity = normalizeRedriveIdentity(input.repo, input.job_id);
  if (!identity) throw new Error("invalid normal intake identity");
  return { ...input, repo: identity.repo, job_id: identity.job_id, labels: [...input.labels] };
}
function validRecord(value: unknown): value is NormalIntakeRecord {
  if (!value || typeof value !== "object") return false;
  const r = value as Partial<NormalIntakeRecord>;
  return r.schema_version === 1 && text(r.event_id) && typeof r.body_sha256 === "string" && SHA.test(r.body_sha256)
    && text(r.job_id) && text(r.repo) && text(r.installation_id, MAX_TEXT, true) && Array.isArray(r.labels)
    && r.labels.length <= 32 && r.labels.every((x) => text(x, 128)) && safeTime(r.received_at_ms)
    && (r.state === "pending" || r.state === "uncertain" || r.state === "complete") && safeTime(r.next_attempt_ms);
}
function validateNow(now: number): void { if (!safeTime(now)) throw new Error("invalid normal intake time"); }
function validateBody(body: string): void { if (!SHA.test(body)) throw new Error("invalid normal intake body hash"); }

export class NormalIntakeInbox {
  constructor(private readonly storage: AuthorityStorage) {}

  async enqueue(input: NormalIntakeInput, now = Date.now(), delayMs = 0): Promise<{ status: "accepted" | "duplicate" | "conflict" | "full"; record?: NormalIntakeRecord }> {
    const normalized = validateInput(input);
    validateNow(now);
    if (!Number.isSafeInteger(delayMs) || delayMs < 0 || now > Number.MAX_SAFE_INTEGER - delayMs) throw new Error("invalid normal intake delay");
    return this.storage.transaction(async (tx: AuthorityTransaction) => {
      const key = eventKey(normalized.event_id);
      const old = await tx.get<unknown>(key);
      if (old !== undefined) {
        if (!validRecord(old)) fail("malformed event record");
        if (old.body_sha256 !== normalized.body_sha256 || old.job_id !== normalized.job_id || old.repo !== normalized.repo
          || old.installation_id !== normalized.installation_id || JSON.stringify(old.labels) !== JSON.stringify(normalized.labels)) return { status: "conflict" };
        return { status: "duplicate", record: old };
      }
      const countValue = await tx.get<unknown>(COUNT);
      const count = countValue === undefined ? 0 : countValue;
      if (!validCount(count)) fail("malformed active count");
      if (count >= MAX) return { status: "full" };
      const record: NormalIntakeRecord = { schema_version: 1, ...normalized, state: "pending", next_attempt_ms: now + delayMs };
      await tx.put(key, record);
      await tx.put(pendingKey(record), record.event_id);
      await tx.put(COUNT, count + 1);
      return { status: "accepted", record };
    });
  }

  async pending(now: number, limit = 25): Promise<NormalIntakeRecord[]> {
    validateNow(now);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 25) throw new Error("invalid normal intake limit");
    const page = await this.storage.list<string>({ prefix: PENDING, limit });
    const result: NormalIntakeRecord[] = [];
    for (const [key, value] of page) {
      if (typeof value !== "string" || !key.startsWith(PENDING)) fail("malformed pending index");
      const record = await this.storage.get<unknown>(eventKey(value));
      if (!validRecord(record)) fail("malformed event record");
      if (record.event_id !== value || record.state !== "pending") fail("pending index/state mismatch");
      if (record.next_attempt_ms <= now) result.push(record);
    }
    return result;
  }

  async settle(eventId: string, expectedBodySha: string, outcome: NormalIntakeOutcome, now: number): Promise<void> {
    if (!text(eventId) || !["complete", "uncertain", "retry"].includes(outcome)) throw new Error("invalid normal intake settlement");
    validateBody(expectedBodySha); validateNow(now);
    return this.storage.transaction(async (tx: AuthorityTransaction) => {
      const key = eventKey(eventId);
      const value = await tx.get<unknown>(key);
      if (!validRecord(value)) fail("missing or malformed event record");
      if (value.body_sha256 !== expectedBodySha) throw new NormalIntakeConflictError();
      if (value.state === "complete" || value.state === "uncertain") return;
      const nextState: NormalIntakeState = outcome === "complete" ? "complete" : outcome === "uncertain" ? "uncertain" : "pending";
      const next: NormalIntakeRecord = { ...value, state: nextState, next_attempt_ms: outcome === "retry" ? now + 60_000 : value.next_attempt_ms };
      const countValue = await tx.get<unknown>(COUNT);
      if (!validCount(countValue)) fail("malformed active count");
      if (outcome === "complete") { if (countValue < 1) fail("active count underflow"); await tx.delete(pendingKey(value)); await tx.put(COUNT, countValue - 1); }
      else if (outcome === "uncertain") await tx.delete(pendingKey(value));
      else await tx.put(pendingKey(next), next.event_id);
      await tx.put(key, next);
      return;
    });
  }
}
