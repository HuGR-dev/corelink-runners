import type { KvLike } from "./lib";
import {
  HEX, NONCE, OWNER_RECORD_TTL_MS, PREFIX, activeKey, activePointerProjection, attemptKey, bindingKey,
  bindingValid, mirrorKey, normalizeTuple, permitValid, pointerMatchesAttempt, pointerValid, proofValid, recordValid, requestTuple,
  startKey, validText, validTime,
} from "./containment_effect_ledger_records";

export type OwnerPath = "intake" | "drain" | "redrive";
export type ContainmentEffectState =
  | "PREPARED" | "CLAIM_ACQUIRED" | "PERMIT_ISSUED" | "BOUND"
  | "DRIVING" | "COMMITTED" | "ABORTED_PRE_EFFECT" | "UNKNOWN";
export interface OwnerTuple {
  repo: string;
  job_id: string;
  path: OwnerPath;
  event_id: string;
  reservation_epoch: number | null;
  effect_id: string;
  owner: string;
  token: string;
  lease_epoch: number;
  drain_owner: string | null;
  drain_lease_epoch: number | null;
  caller_nonce: string;
}
export interface SpawnOwnerRequest {
  schema_version: 1;
  tuple: OwnerTuple;
  caller_nonce: string;
  observation_kind?: SpawnMirrorObservation["kind"];
  observation_digest?: string | null;
  now?: number;
}
export interface ContainmentEffectIdentity { repo: string; job_id: string; effect_id: string }
export interface ContainmentEffectBinding {
  schema_version: 1;
  provider: string;
  resource_id: string;
  idempotency_key: string;
  binding_sha256: string;
}
export interface ContainmentEffectPermit {
  schema_version: 1;
  permit_id: string;
  repo: string;
  job_id: string;
  path: OwnerPath;
  event_id: string;
  effect_id: string;
  reservation_epoch: number | null;
  issued_to_owner: string;
  issued_to_epoch: number;
  owner_token_digest: string;
  issued_at_ms: number;
  expires_ms: number;
}
export interface EffectStartProofV1 {
  schema_version: 1;
  proof_id: string;
  repo: string;
  job_id: string;
  path: OwnerPath;
  event_id: string;
  reservation_epoch: number | null;
  effect_id: string;
  permit_id: string;
  owner: string;
  token: string;
  lease_epoch: number;
  caller_nonce: string;
  effect_request_digest: string;
  writer: "ContainmentDO";
  started_at_ms: number;
}
export interface ContainmentEffectReceipt {
  schema_version: 1;
  trusted: true;
  repo: string;
  job_id: string;
  path: OwnerPath;
  event_id: string;
  reservation_epoch: number | null;
  effect_id: string;
  provider: string;
  resource_id: string;
  idempotency_key: string;
  nonce: string;
  permit_id: string;
  binding_sha256: string;
  receipt_id: string;
  receipt_sha256: string;
  provider_signature: string;
}
export interface OwnerRecordV1 {
  schema_version: 1;
  tuple: OwnerTuple;
  repo: string;
  job_id: string;
  path: OwnerPath;
  event_id: string;
  reservation_epoch: number | null;
  effect_id: string;
  owner: string;
  token: string;
  lease_epoch: number;
  drain_owner: string | null;
  drain_lease_epoch: number | null;
  caller_nonce: string;
  nonce: string;
  state: ContainmentEffectState;
  permit_id: string | null;
  binding_id: string | null;
  effect_start_proof_id: string | null;
  effect_started: boolean;
  created_ms: number;
  expires_ms: number;
  tombstone: boolean;
  // Durable records carry these typed payloads alongside their identity
  // projection; keeping them optional preserves the pre-R14 wire shape.
  permit?: ContainmentEffectPermit | null;
  binding?: ContainmentEffectBinding | null;
  effect_observation?: EffectObservationV1 | ContainmentEffectReceipt;
  attempt_key?: string;
  reap_proof?: EffectReapProofV1;
}
export interface EffectReapProofV1 {
  schema_version: 1;
  tuple: OwnerTuple;
  nonce: string;
  attempt_key: string;
  active_pointer_key: string;
  authority: "containment-reaper-v1" | "owner-abort-v1";
  checked_at_ms: number;
  no_permit: true;
  no_binding: true;
  no_effect: true;
}
export interface OwnerPointerV1 {
  schema_version: 1;
  tuple: OwnerTuple;
  repo: string; job_id: string; path: OwnerPath; event_id: string;
  reservation_epoch: number | null; effect_id: string; owner: string; token: string;
  lease_epoch: number; drain_owner: string | null; drain_lease_epoch: number | null;
  caller_nonce: string; nonce: string; attempt_key: string; state: ContainmentEffectState;
  permit_id: string | null; binding_id: string | null; effect_start_proof_id: string | null;
  tombstone: boolean;
}
export interface SpawnMirrorPayloadV1 {
  schema_version: 1;
  tuple: OwnerTuple;
  tuple_digest: string;
  caller_nonce: string;
  result: "acquired" | "owned" | "busy" | "legacy_unknown" | "unavailable";
  owner: string | null;
  token: string | null;
  lease_epoch: number | null;
  permit_id: string | null;
  attempt_key: string;
  active_pointer_key: string;
  written_at_ms: number;
}
export interface SpawnMirrorObservation {
  schema_version: 1;
  kind: "missing" | "exact" | "mismatch" | "legacy" | "unavailable";
  key: string;
  payload: string | null;
  payload_digest: string | null;
  observed_at_ms: number;
}
export interface EffectObservationV1 {
  schema_version: 1;
  observation_id: string;
  repo: string;
  job_id: string;
  path: OwnerPath;
  event_id: string;
  reservation_epoch: number | null;
  effect_id: string;
  permit_id: string;
  proof_id: string;
  status: "started" | "committed" | "absent" | "unknown";
  external_effect_digest: string | null;
  trusted: true;
  observed_at_ms: number;
  observer: string;
}
export interface OwnerResult {
  schema_version: 1;
  kind: "prepared" | "acquired" | "owned" | "busy" | "legacy_unknown"
    | "permit_issued" | "already_started" | "bound" | "driving"
    | "committed" | "aborted" | "rejected" | "unknown" | "missing" | "reaped_predecessor" | "unavailable";
  tuple_digest: string;
  attempt_key: string;
  active_pointer_key: string;
  permit: ContainmentEffectPermit | null;
  proof: EffectStartProofV1 | null;
  state: ContainmentEffectState | null;
  record?: OwnerRecordV1;
}
export type SpawnClaimResult = {
  schema_version: 1;
  kind: "acquired" | "owned" | "busy" | "legacy_unknown" | "unavailable";
  tuple_digest: string;
  caller_nonce: string;
  attempt_key: string;
  active_pointer_key: string;
  permit_id: string | null;
  lease_epoch: number | null;
};
export interface ContainmentEffectPrepareInput { identity: ContainmentEffectIdentity; owner: string; lease_epoch: number; now?: number }
export interface ContainmentEffectTransition { identity: ContainmentEffectIdentity; nonce: string; owner: string; owner_token: string; lease_epoch: number; now?: number }
export interface ContainmentEffectReapInput { identity: ContainmentEffectIdentity; nonce: string; authority: "containment-reaper-v1"; lease_epoch: number; stale_after_ms: number; now?: number }
export type ContainmentEffectResult = { status: "prepared" | "active" | "committed" | "transitioned" | "aborted" | "reaped"; attempt: ContainmentEffectAttempt } | { status: "stale" | "busy" | "invalid" | "unknown_terminal" | "mirror_unavailable" | "mirror_mismatch" };
export type ContainmentEffectReadback =
  | { kind: "missing"; state: null }
  | { kind: "owned"; state: ContainmentEffectState }
  | { kind: "committed"; state: "COMMITTED" };
export interface ContainmentEffectAttempt extends OwnerRecordV1 { repo: string; job_id: string; effect_id: string; nonce: string; owner: string; owner_token: string; lease_epoch: number; created_at_ms: number; updated_at_ms: number; permit: ContainmentEffectPermit | null; binding: ContainmentEffectBinding | null; provider_receipt: ContainmentEffectReceipt | null }
export interface ContainmentEffectMirror { schema_version: 1; repo: string; job_id: string; effect_id: string; nonce: string; owner: string; owner_token: string; lease_epoch: number; state: Exclude<ContainmentEffectState, "ABORTED_PRE_EFFECT">; permit_id: string | null; binding_sha256: string | null }

interface EffectStorage {
  get<T>(key: string): Promise<T | undefined>;
  put(key: string, value: unknown): Promise<void>;
  transaction<T>(fn: (s: EffectStorage) => Promise<T>): Promise<T>;
}
function legacyTuple(i: ContainmentEffectIdentity, nonce: string, owner: string, token: string, epoch: number): OwnerTuple | null { return normalizeTuple({ ...i, path: "redrive", event_id: `${PREFIX}legacy:${i.effect_id}`, reservation_epoch: epoch, effect_id: i.effect_id, owner, token, lease_epoch: epoch, drain_owner: owner, drain_lease_epoch: epoch, caller_nonce: nonce }); }
export function containmentSpawnActiveKey(t: OwnerTuple): string { return activeKey(t); }
export function containmentSpawnAttemptKey(t: OwnerTuple): string { return attemptKey(t); }
export function containmentSpawnMirrorKey(t: OwnerTuple): string { return mirrorKey(t); }
export function containmentEffectPointerKey(i: ContainmentEffectIdentity | OwnerTuple): string {
  if ("path" in i) { const t = normalizeTuple(i); return t ? activeKey(t) : ""; }
  const t = legacyTuple(i, "00000000000000000000000000000001", "legacy-owner", "legacy-token", 1);
  return t ? activeKey(t) : "";
}
export function containmentEffectMirrorKey(i: ContainmentEffectIdentity | OwnerTuple): string {
  if ("path" in i) { const t = normalizeTuple(i); return t ? mirrorKey(t) : ""; }
  const t = legacyTuple(i, "00000000000000000000000000000001", "legacy-owner", "legacy-token", 1);
  return t ? mirrorKey(t) : "";
}
export async function sha256(value: string): Promise<string> { const b = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)); return [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, "0")).join(""); }
export async function ownerTupleDigest(t: OwnerTuple): Promise<string> { return sha256(JSON.stringify(t)); }
function rejected(): OwnerResult { return { schema_version: 1, kind: "rejected", tuple_digest: "", attempt_key: "", active_pointer_key: "", permit: null, proof: null, state: null }; }
function mirrorShapeValid(v: unknown, t: OwnerTuple): v is SpawnMirrorPayloadV1 {
  if (!v || typeof v !== "object") return false;
  const m = v as Partial<SpawnMirrorPayloadV1>;
  return m.schema_version === 1 && m.caller_nonce === t.caller_nonce
    && JSON.stringify(m.tuple) === JSON.stringify(t) && HEX.test(m.tuple_digest ?? "")
    && validText(m.attempt_key) && validText(m.active_pointer_key)
    && (m.result === "acquired" || m.result === "owned")
    && m.owner === t.owner && m.token === t.token
    && m.lease_epoch === t.lease_epoch
    && (m.permit_id === null || validText(m.permit_id))
    && m.attempt_key === attemptKey(t) && m.active_pointer_key === activeKey(t)
    && validTime(m.written_at_ms);
}
async function mirrorValid(v: unknown, t: OwnerTuple): Promise<boolean> {
  if (!mirrorShapeValid(v, t)) return false;
  return (v as SpawnMirrorPayloadV1).tuple_digest === await ownerTupleDigest(t);
}
async function ownerMirrorEvidenceValid(kv: KvLike | undefined, t: OwnerTuple, record: OwnerRecordV1): Promise<boolean> {
  if (!kv) return false;
  let raw: string | null;
  try { raw = await kv.get(mirrorKey(t)); } catch { return false; }
  const missingAllowed = record.state === "PREPARED" || record.state === "CLAIM_ACQUIRED" || record.state === "ABORTED_PRE_EFFECT";
  if (raw === null) return missingAllowed;
  if (record.state === "PREPARED") return false;
  try {
    const parsed = JSON.parse(raw) as SpawnMirrorPayloadV1;
    return parsed.result === "acquired" && parsed.permit_id === null && await mirrorValid(parsed, t);
  } catch { return false; }
}
function trustedReceipt(r: ContainmentEffectReceipt, t: OwnerTuple, permit: string, binding: ContainmentEffectBinding | null): boolean {
  return r.schema_version === 1 && r.trusted === true && !!binding && r.repo === t.repo
    && r.job_id === t.job_id && r.path === t.path
    && r.event_id === t.event_id && r.reservation_epoch === t.reservation_epoch
    && r.effect_id === t.effect_id
    && r.nonce === t.caller_nonce && r.permit_id === permit
    && r.binding_sha256 === binding.binding_sha256 && r.provider === binding.provider
    && r.resource_id === binding.resource_id && r.idempotency_key === binding.idempotency_key
    && validText(r.provider)
    && validText(r.resource_id) && validText(r.idempotency_key)
    && validText(r.receipt_id) && HEX.test(r.receipt_sha256)
    && validText(r.provider_signature);
}
function bindingPayloadValid(raw: string, t: OwnerTuple, permit: string, binding: ContainmentEffectBinding): boolean {
  try {
    const x = JSON.parse(raw);
    return raw === JSON.stringify(x) && x?.schema_version === 1 && x.permit_id === permit
      && JSON.stringify(x.tuple) === JSON.stringify(t) && bindingValid(x.binding, binding.binding_sha256)
      && JSON.stringify(x.binding) === JSON.stringify(binding);
  } catch { return false; }
}
function compat(r: OwnerRecordV1, t: OwnerTuple, receipt: ContainmentEffectReceipt | null = null): ContainmentEffectAttempt {
  const x = r as OwnerRecordV1 & { permit?: ContainmentEffectPermit; binding?: ContainmentEffectBinding };
  return {
    ...r, repo: t.repo, job_id: t.job_id, effect_id: t.effect_id,
    nonce: t.caller_nonce, owner: t.owner, owner_token: t.token,
    lease_epoch: t.lease_epoch,
    created_at_ms: r.created_ms, updated_at_ms: r.created_ms,
    permit: x.permit ?? null, binding: x.binding ?? null,
    provider_receipt: receipt,
  };
}
export class ContainmentEffectLedger {
  private readonly storage: EffectStorage;
  constructor(storage: EffectStorage, private readonly kv?: KvLike) {
    const pointerPut = (s: EffectStorage, key: string, value: unknown) =>
      s.put(key, key.startsWith(`${PREFIX}spawn-active:`)
        ? activePointerProjection(value as Record<string, unknown>) : value);
    this.storage = {
      get: storage.get.bind(storage),
      put: (key, value) => pointerPut(storage, key, value),
      transaction: fn => storage.transaction(s => fn({
        get: s.get.bind(s), put: (key, value) => pointerPut(s, key, value),
        transaction: s.transaction.bind(s),
      })),
    };
  }
  private async out(t: OwnerTuple, kind: OwnerResult["kind"], state: ContainmentEffectState | null, record?: OwnerRecordV1): Promise<OwnerResult> { return { schema_version: 1, kind, tuple_digest: await ownerTupleDigest(t), attempt_key: attemptKey(t), active_pointer_key: activeKey(t), permit: null, proof: null, state, record }; }
  async observe(pointer: string, attempt: string): Promise<OwnerResult> {
    const activePrefix = `${PREFIX}spawn-active:`;
    const suffix = pointer.startsWith(activePrefix) ? pointer.slice(activePrefix.length) : null;
    const attemptPrefix = suffix === null ? "" : `${PREFIX}spawn-attempt:${suffix}/`;
    const nonce = attempt.startsWith(attemptPrefix) ? attempt.slice(attemptPrefix.length) : "";
    const startSidecarKey = suffix === null ? "" : `${PREFIX}effect-start:${suffix}`;
    const bindingSidecarKey = suffix === null ? "" : `${PREFIX}effect-binding:${suffix}`;
    const mirrorSidecarKey = suffix === null ? "" : `${PREFIX}spawn-mirror:${suffix}`;
    const snapshot = await this.storage.transaction(async s => ({
      p: await s.get<OwnerPointerV1>(pointer),
      a: await s.get<OwnerRecordV1>(attempt),
      start: startSidecarKey ? await s.get<unknown>(startSidecarKey) : undefined,
    }));
    const { p, a } = snapshot;
    const unknown = (): OwnerResult => ({ schema_version: 1, kind: "unknown", tuple_digest: "",
      attempt_key: attempt, active_pointer_key: pointer, permit: null, proof: null, state: "UNKNOWN" });
    if (!this.kv) return unknown();
    const mirrorEvidence = async (): Promise<string | null | undefined> => {
      if (!mirrorSidecarKey || !this.kv) return null;
      try { return await this.kv.get(mirrorSidecarKey); } catch { return undefined; }
    };
    const bindingEvidence = async (): Promise<string | null | undefined> => {
      if (!bindingSidecarKey || !this.kv) return null;
      try { return await this.kv.get(bindingSidecarKey); } catch { return undefined; }
    };
    if (p === undefined && a === undefined) {
      if (suffix === null || !/^[0-9a-f]{32}$/.test(nonce) || snapshot.start !== undefined || !this.kv) return unknown();
      const [binding, mirror] = await Promise.all([bindingEvidence(), mirrorEvidence()]);
      if (binding !== null || mirror !== null) return unknown();
      return { schema_version: 1, kind: "missing", tuple_digest: "",
        attempt_key: attempt, active_pointer_key: pointer, permit: null,
        proof: null, state: null };
    }
    // A validated pre-effect tombstone is a fenced predecessor, not an
    // unresolved owner. Treat it as vacant for the next nonce so prepare()
    // can atomically replace it. Every other pointer-without-this-attempt
    // shape remains unknown and must fail closed before claim/preparation.
    if (p !== undefined && a === undefined) {
      const predecessor = p && typeof p === "object" ? normalizeTuple(p.tuple) : null;
      const previousAttemptKey = predecessor && attemptKey(predecessor);
      const noncePrefix = previousAttemptKey ? `${previousAttemptKey.slice(0, previousAttemptKey.lastIndexOf("/") + 1)}` : "";
      const nextNonce = noncePrefix && attempt.startsWith(noncePrefix) ? attempt.slice(noncePrefix.length) : "";
      const previous = predecessor && previousAttemptKey
        ? await this.storage.get<OwnerRecordV1>(previousAttemptKey) : undefined;
      const predecessorMirrorSafe = !!predecessor && !!previous
        && await ownerMirrorEvidenceValid(this.kv, predecessor, previous);
      if (predecessor && activeKey(predecessor) === pointer && nextNonce !== predecessor.caller_nonce
        && /^[0-9a-f]{32}$/.test(nextNonce) && p.state === "ABORTED_PRE_EFFECT" && p.tombstone === true
        && previous && recordValid(previous, predecessor) && previous.state === "ABORTED_PRE_EFFECT"
        && previous.tombstone === true && pointerMatchesAttempt(p, previous, predecessor)
        && !!this.kv && snapshot.start === undefined
        && await bindingEvidence() === null && predecessorMirrorSafe) {
        return { schema_version: 1, kind: "reaped_predecessor", tuple_digest: "",
          attempt_key: attempt, active_pointer_key: pointer, permit: null,
          proof: null, state: "ABORTED_PRE_EFFECT" };
      }
      return unknown();
    }
    const t = a ? normalizeTuple(a.tuple) : null;
    const permitOk = !!a && !!t && (a.permit_id === null
      ? a.permit === undefined
      : permitValid(a.permit, t) && a.permit?.permit_id === a.permit_id
        && await sha256(t.token) === a.permit.owner_token_digest);
    if (!t || pointer !== activeKey(t) || attempt !== attemptKey(t)
      || !p || !a || !permitOk || !recordValid(a, t) || !pointerMatchesAttempt(p, a, t) || p.attempt_key !== attempt || p.nonce !== a.nonce) {
      return unknown();
    }
    if (a.effect_start_proof_id !== null) {
      if (!proofValid(snapshot.start, t, a.permit_id ?? "") || snapshot.start.proof_id !== a.effect_start_proof_id) return unknown();
    } else if (snapshot.start !== undefined) return unknown();
    const bindingSidecar = await bindingEvidence();
    if (a.binding_id !== null) {
      if (!a.permit_id || !a.binding || !this.kv || typeof bindingSidecar !== "string"
        || !bindingPayloadValid(bindingSidecar, t, a.permit_id, a.binding)) return unknown();
    } else if (bindingSidecar !== null) return unknown();
    if (!await ownerMirrorEvidenceValid(this.kv, t, a)) return unknown();
    return this.out(a.tuple, a.state === "COMMITTED" ? "committed" : "owned", a.state, a);
  }
  async inspectIntakeOwner(tuple: OwnerTuple): Promise<ContainmentEffectReadback> {
    const t = normalizeTuple(tuple);
    if (!t || t.path !== "intake") throw new Error("invalid normal intake effect identity");
    if (!this.kv) throw new Error("normal intake readback sidecar evidence unavailable");
    const snapshot = await this.storage.transaction(async s => ({
      pointer: await s.get<unknown>(activeKey(t)),
      attempt: await s.get<unknown>(attemptKey(t)),
      proof: await s.get<unknown>(startKey(t)),
    }));
    const { pointer, attempt, proof } = snapshot;
    if (pointer === undefined && attempt === undefined) {
      if (proof !== undefined) throw new Error("normal intake readback has unresolved sidecar evidence");
      try {
        const [binding, mirror] = await Promise.all([this.kv.get(bindingKey(t)), this.kv.get(mirrorKey(t))]);
        if (binding !== null || mirror !== null) throw new Error("normal intake readback has unresolved sidecar evidence");
      } catch { throw new Error("normal intake readback sidecar evidence unavailable"); }
      return { kind: "missing", state: null };
    }
    if (pointer === undefined || attempt === undefined || !recordValid(attempt, t)
      || !pointerMatchesAttempt(pointer, attempt, t)) throw new Error("normal intake effect corruption: owner evidence mismatch");

    const record = attempt as OwnerRecordV1;
    if (record.attempt_key !== attemptKey(t) || record.nonce !== t.caller_nonce
      || !pointerValid(pointer, t, record)) throw new Error("normal intake effect corruption: invalid owner evidence");

    if ((record.permit_id === null) !== (record.permit === undefined)
      || (record.permit_id !== null && record.permit?.permit_id !== record.permit_id)) {
      throw new Error("normal intake effect corruption: divergent permit evidence");
    }

    if (record.permit_id !== null && (await sha256(t.token)) !== record.permit?.owner_token_digest) {
      throw new Error("normal intake effect corruption: invalid permit evidence");
    }
    if (record.effect_start_proof_id !== null) {
      if (!proofValid(proof, t, record.permit_id ?? "") || proof.proof_id !== record.effect_start_proof_id) {
        throw new Error("normal intake effect corruption: invalid start proof");
      }
    } else if (proof !== undefined) {
      throw new Error("normal intake effect corruption: divergent start proof");
    }

    if (record.binding_id !== null) {
      if (!record.binding || !bindingValid(record.binding, record.binding_id) || !record.permit_id || !this.kv) {
        throw new Error("normal intake effect corruption: invalid binding evidence");
      }
      let raw: string | null;
      try { raw = await this.kv.get(bindingKey(t)); }
      catch { throw new Error("normal intake effect corruption: binding evidence unavailable"); }
      if (!raw || !bindingPayloadValid(raw, t, record.permit_id, record.binding)) {
        throw new Error("normal intake effect corruption: divergent binding evidence");
      }
    } else if (this.kv) {
      let raw: string | null;
      try { raw = await this.kv.get(bindingKey(t)); }
      catch { throw new Error("normal intake effect corruption: binding evidence unavailable"); }
      if (raw !== null) throw new Error("normal intake effect corruption: divergent binding evidence");
    }

    if (!await ownerMirrorEvidenceValid(this.kv, t, record)) {
      throw new Error("normal intake effect corruption: divergent mirror evidence");
    }

    if (record.state === "COMMITTED") {
      if (!record.permit_id || !record.binding || !record.effect_observation
        || !trustedReceipt(record.effect_observation as ContainmentEffectReceipt, t, record.permit_id, record.binding)) {
        throw new Error("normal intake effect corruption: invalid receipt evidence");
      }
      return { kind: "committed", state: "COMMITTED" };
    }
    return { kind: "owned", state: record.state };
  }
  async prepare(request: SpawnOwnerRequest): Promise<OwnerResult> {
    const t = requestTuple(request);
    if (!t) return rejected();
    return this.storage.transaction(async s => {
      const raw = await s.get<OwnerPointerV1>(activeKey(t));
      const orphan = await s.get<OwnerRecordV1>(attemptKey(t));
      if (raw === undefined && orphan !== undefined) return this.out(t, "legacy_unknown", "UNKNOWN");
      if (raw !== undefined) {
        const oldTuple = normalizeTuple(raw.tuple);
        const oldAttempt = oldTuple ? await s.get<OwnerRecordV1>(attemptKey(oldTuple)) : undefined;
        if (!oldTuple || !oldAttempt || !pointerMatchesAttempt(raw, oldAttempt, oldTuple)
          || (!raw.tombstone && !pointerValid(raw, t)) || (raw.tombstone && raw.nonce === t.caller_nonce)) {
          return this.out(t, "legacy_unknown", "UNKNOWN");
        }
      }
      if (raw && !raw.tombstone) {
        const current = await s.get<OwnerRecordV1>(attemptKey(t));
        if (!current || !pointerMatchesAttempt(raw, current, t)) return this.out(t, "legacy_unknown", "UNKNOWN");
        return this.out(t, current.state === "COMMITTED" ? "owned" : "busy", current.state, current);
      }
      const created = request.now ?? Date.now();
      if (!validTime(created) || !validTime(created + OWNER_RECORD_TTL_MS)) return this.out(t, "rejected", null);
      const r: OwnerRecordV1 = {
        schema_version: 1, tuple: t, ...t, nonce: t.caller_nonce, state: "PREPARED",
        permit_id: null, binding_id: null, effect_start_proof_id: null,
        effect_started: false, created_ms: created, expires_ms: created + OWNER_RECORD_TTL_MS,
        tombstone: false, attempt_key: attemptKey(t),
      };
      await s.put(attemptKey(t), r);
      await s.put(activeKey(t), r);
      return this.out(t, "prepared", r.state, r);
    });
  }
  async acquire(request: SpawnOwnerRequest): Promise<OwnerResult> {
    const t = requestTuple(request);
    if (!t) return rejected();
    return this.storage.transaction(async s => {
      const p = await s.get<OwnerPointerV1>(activeKey(t));
      const a = await s.get<OwnerRecordV1>(attemptKey(t));
      if (!p || !a || !pointerMatchesAttempt(p, a, t) || p.nonce !== a.nonce) {
        return this.out(t, "legacy_unknown", "UNKNOWN");
      }
      if (a.state === "ABORTED_PRE_EFFECT" || a.tombstone) {
        return this.out(t, "rejected", a.state, a);
      }
      if (a.state === "PREPARED") {
        const n = { ...a, state: "CLAIM_ACQUIRED" as const };
        await s.put(attemptKey(t), n);
        await s.put(activeKey(t), n);
        return this.out(t, "acquired", n.state, n);
      }
      const owned = ["COMMITTED", "DRIVING", "BOUND", "PERMIT_ISSUED", "CLAIM_ACQUIRED"].includes(a.state);
      return this.out(t, owned ? "owned" : "busy", a.state, a);
    });
  }
  async mirror(request: SpawnOwnerRequest, result: "acquired" | "owned" = "acquired"): Promise<SpawnMirrorObservation> {
    const t = requestTuple(request);
    const unavailable = (key = ""): SpawnMirrorObservation => ({
      schema_version: 1, kind: "unavailable", key, payload: null,
      payload_digest: null, observed_at_ms: Date.now(),
    });
    if (!t || !this.kv) return unavailable();
    const key = mirrorKey(t);
    const authorization = await this.storage.transaction(async s => {
      const p = await s.get<OwnerPointerV1>(activeKey(t)); const a = await s.get<OwnerRecordV1>(attemptKey(t));
      const owned = ["CLAIM_ACQUIRED", "PERMIT_ISSUED", "BOUND", "DRIVING", "COMMITTED"].includes(a?.state ?? "");
      const ok = !!a && !!p && pointerMatchesAttempt(p, a, t) && (result === "acquired" ? a.state === "CLAIM_ACQUIRED" : owned);
      return { ok, permit_id: ok ? a!.permit_id : null };
    });
    if (!authorization.ok) return unavailable(key);
    const payload: SpawnMirrorPayloadV1 = {
      schema_version: 1, tuple: t, tuple_digest: await ownerTupleDigest(t),
      caller_nonce: t.caller_nonce, result, owner: t.owner, token: t.token,
      lease_epoch: t.lease_epoch, permit_id: authorization.permit_id,
      attempt_key: attemptKey(t), active_pointer_key: activeKey(t), written_at_ms: Date.now(),
    };
    const text = JSON.stringify(payload);
    try {
      await this.kv.put(key, text);
      const back = await this.kv.get(key);
      if (back === null) return { schema_version: 1, kind: "missing", key, payload: null, payload_digest: null, observed_at_ms: Date.now() };
      const parsed = JSON.parse(back);
      const digest = await sha256(back);
      if (back !== text || !(await mirrorValid(parsed, t))) return { schema_version: 1, kind: "mismatch", key, payload: back, payload_digest: digest, observed_at_ms: Date.now() };
      return { schema_version: 1, kind: "exact", key, payload: back, payload_digest: digest, observed_at_ms: Date.now() };
    } catch {
      return unavailable(key);
    }
  }
  async confirm(request: SpawnOwnerRequest, mirror_digest: string, readback_digest: string, external_permit_id?: string): Promise<OwnerResult> {
    const t = requestTuple(request);
    if (!t || (external_permit_id !== undefined && !validText(external_permit_id)) || !HEX.test(mirror_digest) || mirror_digest !== readback_digest
      || request.observation_kind !== "exact" || request.observation_digest !== mirror_digest) return rejected();
    if (!this.kv) return rejected();
    let mirrorRaw: string | null;
    try { mirrorRaw = await this.kv.get(mirrorKey(t)); } catch { return rejected(); }
    if (!mirrorRaw) return rejected();
    let mirrorValue: unknown;
    try { mirrorValue = JSON.parse(mirrorRaw); } catch { return rejected(); }
    if (!(await mirrorValid(mirrorValue, t)) || await sha256(mirrorRaw) !== mirror_digest) return rejected();
    return this.storage.transaction(async s => {
      const a: any = await s.get(attemptKey(t));
      const p: any = await s.get(activeKey(t));
      if (!a || !p || !pointerMatchesAttempt(p, a, t)
        || p.attempt_key !== attemptKey(t) || a.nonce !== p.nonce) return this.out(t, "unknown", "UNKNOWN");
      if (a.state === "PERMIT_ISSUED" || a.state === "BOUND" || a.state === "DRIVING") {
        if (external_permit_id !== undefined && a.permit_id !== external_permit_id) return this.out(t, "unknown", "UNKNOWN");
        return this.out(t, "owned", a.state, a);
      }
      if (a.state !== "CLAIM_ACQUIRED") return this.out(t, "rejected", a.state, a);
      const issued = Date.now();
      const permit: ContainmentEffectPermit = {
        schema_version: 1, permit_id: external_permit_id ?? crypto.randomUUID(), repo: t.repo,
        job_id: t.job_id, path: t.path, event_id: t.event_id,
        reservation_epoch: t.reservation_epoch, effect_id: t.effect_id,
        issued_to_owner: t.owner, issued_to_epoch: t.lease_epoch,
        owner_token_digest: await sha256(t.token), issued_at_ms: issued, expires_ms: issued + 120_000,
      };
      const n: any = { ...a, state: "PERMIT_ISSUED", permit_id: permit.permit_id, permit };
      await s.put(attemptKey(t), n); await s.put(activeKey(t), n);
      const out = await this.out(t, "permit_issued", n.state, n); out.permit = permit; return out;
    });
  }
  async beginEffect(request: SpawnOwnerRequest, permit_id: string): Promise<OwnerResult> { return this.begin(request, permit_id); }
  async beginReservedEffect(request: SpawnOwnerRequest, permit_id: string): Promise<OwnerResult> { return this.begin(request, permit_id); }
  private async begin(request: SpawnOwnerRequest, permit_id: string): Promise<OwnerResult> {
    const t = requestTuple(request); if (!t || !validText(permit_id)) return rejected();
    return this.storage.transaction(async s => {
      const a: any = await s.get(attemptKey(t)); const p: any = await s.get(activeKey(t)); const existing = await s.get<EffectStartProofV1>(startKey(t));
      if (!a || !p || !pointerMatchesAttempt(p, a, t) || a.permit_id !== permit_id
        || !permitValid(a.permit, t) || await sha256(t.token) !== a.permit.owner_token_digest) return this.out(t, "unknown", "UNKNOWN");
      // A crash after bind leaves a durable BOUND record and proof. Returning
      // that proof is a read-only idempotent recovery path; it never creates a
      // new provider effect start. DRIVING/COMMITTED are handled by observe.
      if (a.state === "BOUND" && a.effect_start_proof_id) {
        if (!proofValid(existing, t, permit_id) || existing.proof_id !== a.effect_start_proof_id) return this.out(t, "unknown", "UNKNOWN");
        const out = await this.out(t, "already_started", a.state, a); out.permit = a.permit; out.proof = existing; return out;
      }
      if (a.state !== "PERMIT_ISSUED") return this.out(t, "rejected", a.state, a);
      if (a.effect_start_proof_id) {
        if (!proofValid(existing, t, permit_id) || existing.proof_id !== a.effect_start_proof_id) return this.out(t, "unknown", "UNKNOWN");
        const out = await this.out(t, "already_started", a.state, a); out.permit = a.permit; out.proof = existing; return out;
      }
      const proof: EffectStartProofV1 = { schema_version: 1, proof_id: crypto.randomUUID(), repo: t.repo, job_id: t.job_id, path: t.path, event_id: t.event_id, reservation_epoch: t.reservation_epoch, effect_id: t.effect_id, permit_id, owner: t.owner, token: t.token, lease_epoch: t.lease_epoch, caller_nonce: t.caller_nonce, effect_request_digest: await sha256(JSON.stringify(t)), writer: "ContainmentDO", started_at_ms: Date.now() };
      const n = { ...a, effect_start_proof_id: proof.proof_id, effect_started: true };
      await s.put(startKey(t), proof); await s.put(attemptKey(t), n); await s.put(activeKey(t), n);
      const out = await this.out(t, "already_started", n.state, n); out.permit = a.permit; out.proof = proof; return out;
    });
  }
  async bind(request: SpawnOwnerRequest, permit_id: string, proof_id: string, binding: ContainmentEffectBinding): Promise<OwnerResult> {
    const t = requestTuple(request); if (!t || !validText(permit_id) || !validText(proof_id)
      || !bindingValid(binding, binding?.binding_sha256 ?? null) || !this.kv) return rejected();
    const authorized = await this.storage.transaction(async s => {
      const a: any = await s.get(attemptKey(t)); const p: any = await s.get(activeKey(t));
      const proof = await s.get<EffectStartProofV1>(startKey(t));
      if (!a || !p || !pointerMatchesAttempt(p, a, t) || !recordValid(a, t)
        || a.permit_id !== permit_id || a.effect_start_proof_id !== proof_id
        || !proofValid(proof, t, permit_id) || !permitValid(a.permit, t)
        || await sha256(t.token) !== a.permit.owner_token_digest) return this.out(t, "unknown", "UNKNOWN");
      if (a.state === "BOUND") return a.binding_id === binding.binding_sha256 ? this.out(t, "bound", a.state, a) : this.out(t, "rejected", a.state, a);
      return a.state === "PERMIT_ISSUED" ? true : this.out(t, "rejected", a.state, a);
    });
    if (authorized !== true) return authorized;
    const payload = JSON.stringify({ schema_version: 1, tuple: t, permit_id, binding });
    try {
      await this.kv.put(bindingKey(t), payload); const back = await this.kv.get(bindingKey(t));
      if (back !== payload) return rejected();
    } catch { return rejected(); }
    return this.storage.transaction(async s => {
      const a: any = await s.get(attemptKey(t)); const p: any = await s.get(activeKey(t)); const proof = await s.get<EffectStartProofV1>(startKey(t));
      if (!a || !p || !pointerMatchesAttempt(p, a, t) || !recordValid(a, t) || a.permit_id !== permit_id || a.effect_start_proof_id !== proof_id || !proofValid(proof, t, permit_id) || !permitValid(a.permit, t) || await sha256(t.token) !== a.permit.owner_token_digest) return this.out(t, "unknown", "UNKNOWN");
      if (a.state === "BOUND") return a.binding_id === binding.binding_sha256 ? this.out(t, "bound", a.state, a) : this.out(t, "rejected", a.state, a);
      if (a.state !== "PERMIT_ISSUED") return this.out(t, "rejected", a.state, a);
      let current: string | null;
      try { current = await this.kv!.get(bindingKey(t)); } catch { return this.out(t, "unknown", "UNKNOWN", a); }
      if (!current || current !== payload || await sha256(current) !== await sha256(payload)
        || !bindingPayloadValid(current, t, permit_id, binding)) return this.out(t, "unknown", "UNKNOWN", a);
      const n = { ...a, state: "BOUND" as const, binding_id: binding.binding_sha256, binding };
      await s.put(attemptKey(t), n); await s.put(activeKey(t), n);
      const out = await this.out(t, "bound", n.state, n); out.permit = a.permit; out.proof = proof; return out;
    });
  }
  async markDriving(request: SpawnOwnerRequest, permit_id: string, proof_id: string): Promise<OwnerResult> {
    const t = requestTuple(request); if (!t) return rejected();
    return this.storage.transaction(async s => {
      const a: any = await s.get(attemptKey(t)); const p: any = await s.get(activeKey(t)); const proof = await s.get<EffectStartProofV1>(startKey(t));
      if (!a || !p || !pointerMatchesAttempt(p, a, t) || !recordValid(a, t) || a.permit_id !== permit_id || a.effect_start_proof_id !== proof_id || !proofValid(proof, t, permit_id) || !permitValid(a.permit, t) || await sha256(t.token) !== a.permit.owner_token_digest) return this.out(t, "unknown", "UNKNOWN");
      if (a.state === "DRIVING") { const out = await this.out(t, "already_started", a.state, a); out.permit = a.permit; out.proof = proof; return out; }
      if (a.state !== "BOUND") return this.out(t, "rejected", a.state, a);
      const n = { ...a, state: "DRIVING" as const }; await s.put(attemptKey(t), n); await s.put(activeKey(t), n);
      const out = await this.out(t, "driving", n.state, n); out.permit = a.permit; out.proof = proof; return out;
    });
  }
  async commitEffect(legacy: ContainmentEffectTransition & { permit_id: string; receipt: ContainmentEffectReceipt }): Promise<ContainmentEffectResult>;
  async commitEffect(request: SpawnOwnerRequest, permit_id: string, proof_id: string, observation: ContainmentEffectReceipt): Promise<OwnerResult>;
  async commitEffect(request: SpawnOwnerRequest | (ContainmentEffectTransition & { permit_id: string; receipt: ContainmentEffectReceipt }), permit_id?: string, proof_id?: string, observation?: ContainmentEffectReceipt): Promise<OwnerResult | ContainmentEffectResult> {
    if ("identity" in request) return this.commitLegacy(request);
    const t = requestTuple(request); if (!t || !permit_id || !proof_id || !observation) return rejected();
    return this.storage.transaction(async s => {
      const a: any = await s.get(attemptKey(t)); const p: any = await s.get(activeKey(t)); const proof = await s.get<EffectStartProofV1>(startKey(t));
      if (!a || !p || !pointerMatchesAttempt(p, a, t) || !recordValid(a, t) || !proofValid(proof, t, permit_id) || a.permit_id !== permit_id || a.effect_start_proof_id !== proof_id || !permitValid(a.permit, t) || await sha256(t.token) !== a.permit.owner_token_digest) return this.out(t, "unknown", "UNKNOWN");
      if (a.state === "COMMITTED") return this.out(t, "committed", a.state, a);
      if (a.state !== "DRIVING") return this.out(t, "unknown", "UNKNOWN", a);
      if (!trustedReceipt(observation, t, permit_id, a.binding ?? null)) { const unknown = { ...a, state: "UNKNOWN" as const }; await s.put(attemptKey(t), unknown); await s.put(activeKey(t), unknown); return this.out(t, "unknown", unknown.state, unknown); }
      const n = { ...a, state: "COMMITTED" as const, effect_observation: observation }; await s.put(attemptKey(t), n); await s.put(activeKey(t), n); const out = await this.out(t, "committed", n.state, n); out.permit = a.permit; out.proof = proof; return out;
    });
  }
  async abort(request: SpawnOwnerRequest, owner = request.tuple.owner, token = request.tuple.token): Promise<OwnerResult> { return this.abortReap(request, owner, token, false, 0); }
  async freezeUnknown(request: SpawnOwnerRequest): Promise<OwnerResult> {
    const t = requestTuple(request); if (!t) return rejected();
    return this.storage.transaction(async s => {
      const a: any = await s.get(attemptKey(t)); const p: any = await s.get(activeKey(t));
      if (!a || !p || !pointerMatchesAttempt(p, a, t) || !recordValid(a, t)
        || a.permit_id !== null || a.binding_id !== null || a.effect_start_proof_id !== null
        || (a.state !== "PREPARED" && a.state !== "CLAIM_ACQUIRED")) return this.out(t, "unknown", "UNKNOWN", a);
      const frozen = { ...a, state: "UNKNOWN" as const };
      await s.put(attemptKey(t), frozen); await s.put(activeKey(t), frozen);
      return this.out(t, "unknown", frozen.state, frozen);
    });
  }
  async reap(request: SpawnOwnerRequest, stale_after_ms: number, authority = "containment-reaper-v1"): Promise<OwnerResult> { if (authority !== "containment-reaper-v1" || !Number.isSafeInteger(stale_after_ms) || stale_after_ms < 1) return rejected(); return this.abortReap(request, "", "", true, stale_after_ms); }
  private async abortReap(request: SpawnOwnerRequest, owner: string, token: string, reap: boolean, stale: number): Promise<OwnerResult> {
    const t = requestTuple(request); if (!t) return rejected();
    return this.storage.transaction(async s => {
      const p = await s.get<any>(activeKey(t)); const a = await s.get<any>(attemptKey(t));
      if (!p || !a || !pointerMatchesAttempt(p, a, t) || p.attempt_key !== attemptKey(t) || p.nonce !== a.nonce) return this.out(t, "unknown", "UNKNOWN");
      if (a.tuple.owner !== t.owner || a.tuple.token !== t.token || a.tuple.drain_lease_epoch !== t.drain_lease_epoch || (!reap && (a.tuple.owner !== owner || a.tuple.token !== token))) return this.out(t, "rejected", a.state, a);
      if (a.state !== "PREPARED" && a.state !== "CLAIM_ACQUIRED") return this.out(t, a.state === "DRIVING" ? "unknown" : "rejected", a.state, a);
      if (reap && (request.now ?? Date.now()) < a.expires_ms + stale) return this.out(t, "busy", a.state, a);
      if (a.permit_id !== null || a.binding_id !== null || a.effect_start_proof_id !== null || a.effect_started || a.tombstone) return this.out(t, "rejected", a.state, a);
      const checked = request.now ?? Date.now();
      if (!validTime(checked)) return this.out(t, "rejected", a.state, a);
      const reap_proof: EffectReapProofV1 = { schema_version: 1, tuple: t, nonce: t.caller_nonce,
        attempt_key: attemptKey(t), active_pointer_key: activeKey(t),
        authority: reap ? "containment-reaper-v1" : "owner-abort-v1", checked_at_ms: checked,
        no_permit: true, no_binding: true, no_effect: true };
      const tomb = { ...a, state: "ABORTED_PRE_EFFECT" as const, tombstone: true, reap_proof };
      await s.put(attemptKey(t), tomb); await s.put(activeKey(t), tomb); return this.out(t, "aborted", tomb.state, tomb);
    });
  }

  // Compatibility methods preserve the untouched ContainmentDO seam. No compatibility operation
  // can clear a permit, binding, start proof, or DRIVING record.
  async getEffectAttempt(i: ContainmentEffectIdentity, nonce: string): Promise<ContainmentEffectAttempt | null> { if (!NONCE.test(nonce)) return null; const pointer = await this.storage.get<OwnerPointerV1>(containmentEffectPointerKey(i)); if (!pointer || !pointerValid(pointer, pointer.tuple) || pointer.nonce !== nonce || pointer.attempt_key !== attemptKey(pointer.tuple)) return null; const r = await this.storage.get<OwnerRecordV1>(pointer.attempt_key); return r && pointerMatchesAttempt(pointer, r, pointer.tuple) ? compat(r, pointer.tuple) : null; }
  async prepareEffect(i: ContainmentEffectPrepareInput): Promise<ContainmentEffectResult> { const t = legacyTuple(i.identity, "00000000000000000000000000000001", i.owner, crypto.randomUUID(), i.lease_epoch); if (!t) return { status: "invalid" }; const r = await this.prepare({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }); return r.record ? { status: r.kind === "prepared" ? "prepared" : r.kind === "owned" ? "committed" : "active", attempt: compat(r.record, t) } : { status: "busy" }; }
  async acquireEffectClaim(i: ContainmentEffectTransition): Promise<ContainmentEffectResult> { const t = legacyTuple(i.identity, i.nonce, i.owner, i.owner_token, i.lease_epoch); if (!t) return { status: "stale" }; const r = await this.acquire({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }); return r.record ? { status: r.kind === "acquired" ? "transitioned" : "busy", attempt: compat(r.record, t) } : { status: "stale" }; }
  async issueEffectPermit(i: ContainmentEffectTransition): Promise<ContainmentEffectResult> { const t = legacyTuple(i.identity, i.nonce, i.owner, i.owner_token, i.lease_epoch); if (!t) return { status: "stale" }; const m = await this.mirror({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }); const r = await this.confirm({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce, observation_kind: m.kind, observation_digest: m.payload_digest }, m.payload_digest ?? "", m.payload_digest ?? ""); return r.record ? { status: r.kind === "permit_issued" || r.kind === "owned" ? "transitioned" : "busy", attempt: compat(r.record, t) } : { status: "stale" }; }
  async bindEffect(i: ContainmentEffectTransition & { permit_id: string; binding: ContainmentEffectBinding }): Promise<ContainmentEffectResult> { const t = legacyTuple(i.identity, i.nonce, i.owner, i.owner_token, i.lease_epoch); if (!t) return { status: "stale" }; const b = await this.beginEffect({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }, i.permit_id); if (!b.proof) return { status: "busy" }; const r = await this.bind({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }, i.permit_id, b.proof.proof_id, i.binding); return r.record ? { status: r.kind === "bound" ? "transitioned" : "busy", attempt: compat(r.record, t) } : { status: "stale" }; }
  async beginEffectDrive(i: ContainmentEffectTransition & { permit_id: string }): Promise<ContainmentEffectResult> { const t = legacyTuple(i.identity, i.nonce, i.owner, i.owner_token, i.lease_epoch); if (!t || !this.kv) return { status: "mirror_unavailable" }; const m = await readContainmentEffectMirror(this.kv, t); if (!m) return { status: "mirror_mismatch" }; const a = await this.getEffectAttempt(i.identity, i.nonce); if (!a?.effect_start_proof_id) return { status: "stale" }; const r = await this.markDriving({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }, i.permit_id, a.effect_start_proof_id); return r.record ? { status: r.kind === "driving" ? "transitioned" : "unknown_terminal", attempt: compat(r.record, t) } : { status: "stale" }; }
  async commitLegacy(i: ContainmentEffectTransition & { permit_id: string; receipt: ContainmentEffectReceipt }): Promise<ContainmentEffectResult> { const t = legacyTuple(i.identity, i.nonce, i.owner, i.owner_token, i.lease_epoch); if (!t) return { status: "stale" }; const a = await this.getEffectAttempt(i.identity, i.nonce); if (!a?.effect_start_proof_id) return { status: "stale" }; const r = await this.commitEffect({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }, i.permit_id, a.effect_start_proof_id, i.receipt); return r.record ? { status: r.kind === "committed" ? "committed" : "busy", attempt: compat(r.record, t, i.receipt) } : { status: "stale" }; }
  async abortEffect(i: ContainmentEffectTransition): Promise<ContainmentEffectResult> { const t = legacyTuple(i.identity, i.nonce, i.owner, i.owner_token, i.lease_epoch); const r = t ? await this.abort({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }, i.owner, i.owner_token) : null; return r?.record ? { status: "aborted", attempt: compat(r.record, t!) } : { status: "busy" }; }
  async reapEffect(i: ContainmentEffectReapInput): Promise<ContainmentEffectResult> { const t = legacyTuple(i.identity, i.nonce, "legacy-owner", "legacy-token", i.lease_epoch); const r = t ? await this.reap({ schema_version: 1, tuple: t, caller_nonce: t.caller_nonce }, i.stale_after_ms, i.authority) : null; return r?.record ? { status: "reaped", attempt: compat(r.record, t!) } : { status: "busy" }; }
}
export function containmentEffectMirrorFromAttempt(a: ContainmentEffectAttempt): ContainmentEffectMirror | null {
  if (a.state === "ABORTED_PRE_EFFECT" || a.state === "UNKNOWN") return null;
  return {
    schema_version: 1,
    repo: a.repo,
    job_id: a.job_id,
    effect_id: a.effect_id,
    nonce: a.nonce,
    owner: a.owner,
    owner_token: a.owner_token,
    lease_epoch: a.lease_epoch,
    state: a.state,
    permit_id: a.permit?.permit_id ?? null,
    binding_sha256: a.binding?.binding_sha256 ?? null,
  };
}
export async function readContainmentEffectMirror(kv: KvLike | undefined, i: ContainmentEffectIdentity | OwnerTuple): Promise<ContainmentEffectMirror | null> {
  if (!kv || !("path" in i)) return null;
  try {
    const t = normalizeTuple(i); if (!t) return null;
    const raw = await kv.get(mirrorKey(t)); if (!raw) return null;
    const x = JSON.parse(raw); if (raw !== JSON.stringify(x) || !(await mirrorValid(x, t))) return null;
    return { schema_version: 1, repo: t.repo, job_id: t.job_id, effect_id: t.effect_id,
      nonce: t.caller_nonce, owner: t.owner, owner_token: t.token, lease_epoch: t.lease_epoch,
      state: "PERMIT_ISSUED", permit_id: x.permit_id, binding_sha256: null };
  } catch { return null; }
}
