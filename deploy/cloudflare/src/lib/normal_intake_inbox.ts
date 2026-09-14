import { normalizeRedriveIdentity } from "../containment_authority_helpers";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";
import { canonicalInstallationId } from "../repo_config_lookup";

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
export type A317ProofPhase = "missing_key" | "wrong_key" | "store_unavailable";
export interface A317ProofRecord {
  schema_version: 1; run_id: string; phase: A317ProofPhase; index: number; nonce: string; expires_at_ms: number; build_sha: string; event_id: string; body_sha256: string;
  authorization_attempts: number; authorization_refusals: number; authorization_state: "pending" | "in_flight" | "refused" | "accepted" | "unknown";
}

export class NormalIntakeConflictError extends Error {
  readonly status = 409 as const; readonly code = "normal_intake_conflict" as const;
  constructor(message = "normal intake event conflicts with durable evidence") { super(message); this.name = "NormalIntakeConflictError"; }
}
const EVENT = "normal-inbox:v1:event:";
const PENDING = "normal-inbox:v1:pending:";
const COUNT = "normal-inbox:v1:count";
const INSTALLATION_TOMBSTONE = "normal-inbox:v1:installation-tombstone:";
const INSTALLATION_TOMBSTONE_DELIVERY = "normal-inbox:v1:installation-tombstone-delivery:";
const A317_PROOF = "normal-inbox:v1:a317-proof:";
const A317_NONCE = "normal-inbox:v1:a317-nonce:";
const A317_SLOT = "normal-inbox:v1:a317-slot:";
const A317_RUN = "normal-inbox:v1:a317-run:";
const MAX = 500;
const MAX_TEXT = 256;
const SHA = /^[0-9a-f]{64}$/;
const text = (value: unknown, max = MAX_TEXT, empty = false): value is string =>
  typeof value === "string" && value.length <= max && (empty || value.length > 0) && !/[\u0000-\u001f\u007f]/.test(value);
const safeTime = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
const eventKey = (id: string) => `${EVENT}${encodeURIComponent(id)}`;
const proofKey = (id: string) => `${A317_PROOF}${encodeURIComponent(id)}`;
const proofNonceKey = (runId: string, nonce: string) => `${A317_NONCE}${encodeURIComponent(runId)}:${encodeURIComponent(nonce)}`;
const proofSlotKey = (runId: string, phase: A317ProofPhase, index: number) => `${A317_SLOT}${encodeURIComponent(runId)}:${phase}:${index}`;
const proofRunKey = (runId: string) => `${A317_RUN}${encodeURIComponent(runId)}`;
export const installationTombstoneKey = (installationId: string) => `${INSTALLATION_TOMBSTONE}${encodeURIComponent(installationId)}`;
const pendingKey = (record: NormalIntakeRecord) => `${PENDING}${String(record.received_at_ms).padStart(16, "0")}:${encodeURIComponent(record.event_id)}`;
const validCount = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0 && (value as number) <= MAX;

function fail(message: string): never { throw new Error(`normal intake corruption: ${message}`); }
function validateInput(input: NormalIntakeInput): NormalIntakeInput {
  if (!input || input.schema_version !== 1 || !text(input.event_id) || !SHA.test(input.body_sha256) || !text(input.installation_id, MAX_TEXT, true)
    || !safeTime(input.received_at_ms) || !Array.isArray(input.labels) || input.labels.length > 32
    || input.labels.some((label) => !text(label, 128))) throw new Error("invalid normal intake record");
  const identity = normalizeRedriveIdentity(input.repo, input.job_id);
  if (!identity) throw new Error("invalid normal intake identity");
  if (input.installation_id !== "" && canonicalInstallationId(input.installation_id) !== input.installation_id) {
    throw new Error("invalid normal intake installation");
  }
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
    && !!identity && identity.repo === r.repo && identity.job_id === r.job_id && text(r.installation_id, MAX_TEXT, true)
    && (r.installation_id === "" || canonicalInstallationId(r.installation_id) === r.installation_id) && Array.isArray(r.labels)
    && r.labels.length <= 32 && r.labels.every((x) => text(x, 128)) && safeTime(r.received_at_ms)
    && (r.state === "pending" || r.state === "uncertain" || r.state === "complete") && safeTime(r.next_attempt_ms);
}
function validateNow(now: number): void { if (!safeTime(now)) throw new Error("invalid normal intake time"); }
function validateBody(body: string): void { if (!SHA.test(body)) throw new Error("invalid normal intake body hash"); }

export class NormalIntakeInbox {
  constructor(private readonly storage: AuthorityStorage) {}

  async enqueue(input: NormalIntakeInput, now = Date.now(), delayMs = 0): Promise<{ status: "accepted" | "duplicate" | "conflict" | "full" | "tombstoned"; record?: NormalIntakeRecord }> {
    const normalized = validateInput(input);
    validateNow(now);
    if (!Number.isSafeInteger(delayMs) || delayMs < 0 || now > Number.MAX_SAFE_INTEGER - delayMs) throw new Error("invalid normal intake delay");
    return this.storage.transaction(async (tx: AuthorityTransaction) => {
      if (await tx.get(installationTombstoneKey(normalized.installation_id)) !== undefined) return { status: "tombstoned" };
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

  /**
   * The A3.17 proof marker is deliberately co-committed with the normal inbox
   * record. It has no effect on ordinary intake and is consumed by its event id.
   */
  async enqueueA317Proof(input: NormalIntakeInput, proof: A317ProofRecord, now = Date.now(), unavailable = false): Promise<{ status: "accepted" | "duplicate" | "conflict" | "full" | "tombstoned"; record?: NormalIntakeRecord }> {
    if (!proof || proof.schema_version !== 1 || !text(proof.run_id, 96) || !text(proof.nonce, 192)
      || !["missing_key", "wrong_key", "store_unavailable"].includes(proof.phase)
      || !Number.isSafeInteger(proof.index) || proof.index < 0 || proof.index > 99 || !safeTime(proof.expires_at_ms) || proof.expires_at_ms <= now) throw new Error("invalid A3.17 proof");
    // This is intentionally before the inbox transaction: the unavailable-store
    // phase must leave no durable marker, nonce consumption, or inbox record.
    if (unavailable) throw new Error("A3.17 injected inbox store unavailable");
    const normalized = validateInput(input);
    return this.storage.transaction(async tx => {
      const marker: A317ProofRecord = { ...proof, event_id: normalized.event_id, body_sha256: normalized.body_sha256, authorization_attempts: 0, authorization_refusals: 0, authorization_state: "pending" };
      const key = eventKey(normalized.event_id), markerKey = proofKey(normalized.event_id), nonceKey = proofNonceKey(proof.run_id, proof.nonce), slotKey = proofSlotKey(proof.run_id, proof.phase, proof.index);
      const runKey = proofRunKey(proof.run_id); const run = await tx.get<{ phase?: unknown; build_sha?: unknown; repo?: unknown; installation_id?: unknown; labels?: unknown; expires_at_ms?: unknown; count?: unknown }>(runKey);
      const runValue = { phase: proof.phase, build_sha: proof.build_sha, repo: normalized.repo, installation_id: normalized.installation_id, labels: normalized.labels, expires_at_ms: proof.expires_at_ms, count: 1 };
      if (run !== undefined && (run.phase !== proof.phase || run.build_sha !== proof.build_sha || run.repo !== normalized.repo || run.installation_id !== normalized.installation_id || JSON.stringify(run.labels) !== JSON.stringify(normalized.labels) || run.expires_at_ms !== proof.expires_at_ms || !Number.isSafeInteger(run.count))) return { status: "conflict" as const };
      const prior = await tx.get<unknown>(markerKey); const slot = await tx.get<unknown>(slotKey); const nonce = await tx.get<unknown>(nonceKey);
      if (prior !== undefined || slot !== undefined || nonce !== undefined) {
        const immutable = prior && typeof prior === "object" ? { ...(prior as A317ProofRecord), authorization_attempts: 0, authorization_refusals: 0, authorization_state: "pending" as const } : prior;
        if (JSON.stringify(immutable) === JSON.stringify(marker) && slot === normalized.event_id && nonce === normalized.event_id) return { status: "duplicate" as const, record: await tx.get<NormalIntakeRecord>(key) };
        return { status: "conflict" as const };
      }
      if (run !== undefined && (run.count as number) >= 100) return { status: "conflict" as const };
      if (await tx.get(installationTombstoneKey(normalized.installation_id)) !== undefined) return { status: "tombstoned" as const };
      if (await tx.get(key) !== undefined) return { status: "conflict" as const };
      const countValue = await tx.get<unknown>(COUNT); if (countValue === undefined && (await tx.list({ prefix: EVENT, limit: 1 })).size > 0) fail("missing active count");
      const count = countValue === undefined ? 0 : countValue; if (!validCount(count)) fail("malformed active count"); if (count >= MAX) return { status: "full" as const };
      const record: NormalIntakeRecord = { schema_version: 1, event_id: normalized.event_id, body_sha256: normalized.body_sha256, job_id: normalized.job_id, repo: normalized.repo, installation_id: normalized.installation_id, labels: [...normalized.labels], received_at_ms: normalized.received_at_ms, state: "pending", next_attempt_ms: now };
      await tx.put(key, record); await tx.put(pendingKey(record), record.event_id); await tx.put(COUNT, count + 1); await tx.put(markerKey, marker); await tx.put(nonceKey, normalized.event_id); await tx.put(slotKey, normalized.event_id); await tx.put(runKey, run === undefined ? runValue : { ...run, count: (run.count as number) + 1 });
      return { status: "accepted" as const, record };
    });
  }

  async a317Proof(eventId: string, now = Date.now()): Promise<A317ProofRecord | null> {
    if (!text(eventId)) throw new Error("invalid A3.17 proof event"); validateNow(now);
    return this.storage.transaction(async tx => {
      const value = await tx.get<unknown>(proofKey(eventId));
      if (value === undefined) return null;
      const p = value as Partial<A317ProofRecord>;
      if (p.schema_version !== 1 || !text(p.run_id, 96) || !text(p.nonce, 192) || !["missing_key", "wrong_key", "store_unavailable"].includes(p.phase as string)
        || !Number.isSafeInteger(p.index) || p.index! < 0 || p.index! > 99 || !safeTime(p.expires_at_ms)) fail("malformed A3.17 proof marker");
      if (p.expires_at_ms! <= now) return null;
      return p as A317ProofRecord;
    });
  }
  async a317Pending(runId: string | undefined, phase: A317ProofPhase | undefined, now: number, limit = 25): Promise<NormalIntakeRecord[]> {
    validateNow(now); if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX) throw new Error("invalid A3.17 proof limit");
    return this.storage.transaction(async tx => {
      const markers = await tx.list<A317ProofRecord>({ prefix: A317_PROOF, limit: MAX + 1 }); if (markers.size > MAX) fail("A3.17 proof index exceeds capacity");
      const result: NormalIntakeRecord[] = [];
      for (const [key, proof] of markers) {
        if (!proof || proof.expires_at_ms <= now || (runId !== undefined && proof.run_id !== runId) || (phase !== undefined && proof.phase !== phase)) continue;
        const id = decodeURIComponent(key.slice(A317_PROOF.length)); const record = await tx.get<unknown>(eventKey(id));
        if (!validRecord(record, id)) fail("missing A3.17 inbox record");
        if (record.state === "pending" && record.next_attempt_ms <= now) result.push(record);
      }
      return result.sort((a, b) => a.received_at_ms - b.received_at_ms || a.event_id.localeCompare(b.event_id)).slice(0, limit);
    });
  }

  async beginA317Authorization(eventId: string): Promise<boolean> {
    return this.storage.transaction(async tx => {
      const proof = await tx.get<A317ProofRecord>(proofKey(eventId));
      if (!proof) fail("missing A3.17 proof marker");
      if (proof.authorization_state !== "pending") return false;
      await tx.put(proofKey(eventId), { ...proof, authorization_state: "in_flight" as const, authorization_attempts: proof.authorization_attempts + 1 }); return true;
    });
  }
  async finishA317Authorization(eventId: string, status: "refused401" | "refused403" | "accepted2xx" | "unknown"): Promise<void> {
    await this.storage.transaction(async tx => {
      const proof = await tx.get<A317ProofRecord>(proofKey(eventId)); if (!proof) fail("invalid A3.17 authorization transition");
      const refused = status === "refused401" || status === "refused403";
      const terminal = status === "accepted2xx" ? "accepted" as const : refused ? "refused" as const : "unknown" as const;
      if (proof.authorization_state === terminal) return;
      if (proof.authorization_state !== "in_flight") fail("invalid A3.17 authorization transition");
      await tx.put(proofKey(eventId), { ...proof, authorization_state: terminal, authorization_refusals: proof.authorization_refusals + (refused ? 1 : 0) });
    });
  }

  async a317Snapshot(runId: string, now = Date.now()): Promise<{ schema_version: 1; run_id: string; accepted: number; pending: number; complete: number; uncertain: number; authorization_attempts: number; authorization_refusals: number; authorization_accepted: number; authorization_unknown: number }> {
    if (!text(runId, 96)) throw new Error("invalid A3.17 run"); validateNow(now);
    return this.storage.transaction(async tx => {
      const markers = await tx.list<A317ProofRecord>({ prefix: A317_PROOF, limit: MAX + 1 });
      if (markers.size > MAX) fail("A3.17 proof index exceeds capacity");
      let accepted = 0, pending = 0, complete = 0, uncertain = 0, authorization_attempts = 0, authorization_refusals = 0, authorization_accepted = 0, authorization_unknown = 0;
      for (const [key, proof] of markers) {
        if (proof?.run_id !== runId || proof.expires_at_ms <= now) continue;
        const id = decodeURIComponent(key.slice(A317_PROOF.length));
        const record = await tx.get<unknown>(eventKey(id));
        if (!validRecord(record, id)) fail("missing A3.17 inbox record");
        accepted++; authorization_attempts += proof.authorization_attempts ?? 0; authorization_refusals += proof.authorization_refusals ?? 0; if (proof.authorization_state === "accepted") authorization_accepted++; if (proof.authorization_state === "unknown") authorization_unknown++; if (record.state === "pending") pending++; else if (record.state === "complete") complete++; else uncertain++;
      }
      return { schema_version: 1, run_id: runId, accepted, pending, complete, uncertain, authorization_attempts, authorization_refusals, authorization_accepted, authorization_unknown };
    });
  }

  async cleanupExpiredA317Proofs(now = Date.now()): Promise<void> {
    validateNow(now);
    await this.storage.transaction(async tx => {
      const markers = await tx.list<A317ProofRecord>({ prefix: A317_PROOF, limit: MAX + 1 });
      if (markers.size > MAX) fail("A3.17 proof index exceeds capacity");
      for (const [key, proof] of markers) {
        if (!proof || proof.expires_at_ms > now) continue;
        const id = decodeURIComponent(key.slice(A317_PROOF.length)); const record = await tx.get<unknown>(eventKey(id));
        if (validRecord(record, id)) {
          if (record.state !== "complete") { const count = await tx.get<unknown>(COUNT); if (!validCount(count) || count < 1) fail("malformed active count"); await tx.put(COUNT, count - 1); }
          if (record.state === "pending") await tx.delete(pendingKey(record));
          await tx.delete(eventKey(id));
        }
        await tx.delete(key); await tx.delete(proofNonceKey(proof.run_id, proof.nonce)); await tx.delete(proofSlotKey(proof.run_id, proof.phase, proof.index));
        const runKey = proofRunKey(proof.run_id); const run = await tx.get<{ count?: unknown }>(runKey);
        if (run && Number.isSafeInteger(run.count)) { if ((run.count as number) <= 1) await tx.delete(runKey); else await tx.put(runKey, { ...run, count: (run.count as number) - 1 }); }
      }
    });
  }

  /** Installation deletion is durable authority, not a KV TTL hint. */
  async tombstoneInstallation(installationId: string, eventId: string, bodySha: string, now = Date.now()): Promise<"accepted" | "duplicate" | "conflict"> {
    if (canonicalInstallationId(installationId) !== installationId || !text(eventId) || !SHA.test(bodySha)) throw new Error("invalid installation tombstone");
    validateNow(now);
    return this.storage.transaction(async tx => {
      const key = installationTombstoneKey(installationId);
      const deliveryKey = `${INSTALLATION_TOMBSTONE_DELIVERY}${encodeURIComponent(eventId)}`;
      const priorDelivery = await tx.get<{ installation_id?: unknown; body_sha256?: unknown }>(deliveryKey);
      if (priorDelivery !== undefined) {
        if (priorDelivery?.installation_id !== installationId || priorDelivery?.body_sha256 !== bodySha) return "conflict";
        return "duplicate";
      }
      const prior = await tx.get<{ event_id?: unknown; body_sha256?: unknown }>(key);
      if (prior !== undefined) {
        if (prior?.event_id !== eventId || prior?.body_sha256 !== bodySha) return "conflict";
        return "duplicate";
      }
      const page = await tx.list<unknown>({ prefix: EVENT, limit: MAX + 1 });
      if (page.size > MAX) fail("event index exceeds capacity");
      let count = await tx.get<unknown>(COUNT);
      if (count === undefined) count = 0;
      if (!validCount(count)) fail("malformed active count");
      for (const [eventKeyName, raw] of page) {
        const record = raw as NormalIntakeRecord;
        if (!validRecord(record) || eventKeyName !== eventKey(record.event_id)) fail("malformed event record");
        if (record.installation_id !== installationId || record.state !== "pending") continue;
        await tx.delete(pendingKey(record));
        await tx.put(eventKeyName, { ...record, state: "complete" as const });
        if (count < 1) fail("active count underflow");
        count--;
      }
      await tx.put(COUNT, count);
      const record = { schema_version: 1, installation_id: installationId, event_id: eventId, body_sha256: bodySha, deleted_at_ms: now };
      await tx.put(key, record);
      await tx.put(deliveryKey, record);
      return "accepted";
    });
  }

  async installationTombstoned(installationId: string): Promise<boolean> {
    // Historical repository-hook mappings may use a non-numeric synthetic
    // installation id.  They can never equal an App deletion tombstone, so they
    // remain compatible while the deletion ingress itself validates strictly.
    if (canonicalInstallationId(installationId) !== installationId) return false;
    return (await this.storage.get(installationTombstoneKey(installationId))) !== undefined;
  }

  /**
   * Admit a selected normal-intake record in the same authority transaction as
   * the installation deletion fence.  Selection is only advisory: a deletion
   * committed before this transaction linearizes must refuse every later
   * authorization, claim, mint, and provider effect.
   */
  async admit(eventId: string): Promise<boolean> {
    if (!text(eventId)) return false;
    return this.storage.transaction(async tx => {
      const record = await tx.get<unknown>(eventKey(eventId));
      if (!validRecord(record, eventId) || record.state !== "pending") return false;
      return (await tx.get(installationTombstoneKey(record.installation_id))) === undefined;
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
    if (!text(eventId) || !["complete", "uncertain", "retry"].includes(outcome)) throw new Error("invalid normal intake settlement");
    validateBody(expectedBodySha); validateNow(now);
    return this.storage.transaction(async (tx: AuthorityTransaction) => {
      const key = eventKey(eventId);
      const value = await tx.get<unknown>(key);
      if (!validRecord(value, eventId)) fail("missing or malformed event record");
      if (value.body_sha256 !== expectedBodySha) throw new NormalIntakeConflictError();
      if (value.state === "complete" || value.state === "uncertain") return;
      const nextState: NormalIntakeState = outcome === "complete" ? "complete" : outcome === "uncertain" ? "uncertain" : "pending";
      if (outcome === "retry" && now > Number.MAX_SAFE_INTEGER - 60_000) throw new Error("invalid normal intake retry time");
      const next: NormalIntakeRecord = { schema_version: 1, event_id: value.event_id, body_sha256: value.body_sha256,
        job_id: value.job_id, repo: value.repo, installation_id: value.installation_id, labels: [...value.labels], received_at_ms: value.received_at_ms,
        state: nextState, next_attempt_ms: outcome === "retry" ? now + 60_000 : value.next_attempt_ms };
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
