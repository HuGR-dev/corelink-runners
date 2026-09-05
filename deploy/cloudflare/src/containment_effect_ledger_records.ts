import type {
  ContainmentEffectBinding, ContainmentEffectPermit, ContainmentEffectReceipt, EffectStartProofV1,
  EffectReapProofV1, OwnerPath, OwnerPointerV1, OwnerRecordV1, OwnerTuple, SpawnOwnerRequest,
} from "./containment_effect_ledger";

export const HEX = /^[0-9a-f]{64}$/;
export const NONCE = /^[0-9a-f]{32}$/;
const REPO = /^[a-z0-9](?:[a-z0-9_.-]*[a-z0-9])?\/[a-z0-9](?:[a-z0-9_.-]*[a-z0-9])?$/;
export const PREFIX = "containment:v1:";
const enc = (v: string) => encodeURIComponent(v);
export const trim = (v: string) => v.replace(/^[\u0009-\u000d\u0020]+|[\u0009-\u000d\u0020]+$/g, "");
export const validEpoch = (n: unknown): n is number => Number.isSafeInteger(n) && (n as number) >= 1;
export const validTime = (n: unknown): n is number => Number.isSafeInteger(n) && (n as number) >= 0;
export const validText = (v: unknown): v is string => typeof v === "string" && v.length > 0 && v.length <= 512 && !/[\u0000\n\r]/.test(v);

function normalizeRepoJob(repo: unknown, job: unknown): { repo: string; job_id: string } | null {
  if (typeof repo !== "string" || typeof job !== "string") return null;
  const r = trim(repo).toLowerCase(); if (!REPO.test(r) || !/^[1-9][0-9]*$/.test(job)) return null;
  try { if (BigInt(job) > BigInt(Number.MAX_SAFE_INTEGER)) return null; } catch { return null; }
  return { repo: r, job_id: String(BigInt(job)) };
}
export function normalizeTuple(input: unknown, nonce?: unknown): OwnerTuple | null {
  if (!input || typeof input !== "object") return null;
  const t = input as Partial<OwnerTuple>; const id = normalizeRepoJob(t.repo, t.job_id); const n = nonce ?? t.caller_nonce;
  if (!id || (nonce !== undefined && t.caller_nonce !== nonce) || !validText(t.event_id) || !validText(t.effect_id)
    || !validText(t.owner) || !validText(t.token) || !NONCE.test(String(n ?? ""))) return null;
  if (t.path !== "intake" && t.path !== "drain" && t.path !== "redrive") return null;
  if (!validEpoch(t.lease_epoch)) return null;
  if (t.path === "redrive" ? !validEpoch(t.reservation_epoch) : t.reservation_epoch !== null) return null;
  if (t.path === "redrive" && (!validText(t.drain_owner) || !validEpoch(t.drain_lease_epoch))) return null;
  if (t.path !== "redrive" && (t.drain_owner !== null || t.drain_lease_epoch !== null)) return null;
  const reservation_epoch = t.reservation_epoch === undefined ? null : t.reservation_epoch;
  return { ...id, path: t.path as OwnerPath, event_id: t.event_id, reservation_epoch,
    effect_id: t.effect_id, owner: t.owner, token: t.token, lease_epoch: t.lease_epoch,
    drain_owner: t.path === "redrive" ? t.drain_owner! : null,
    drain_lease_epoch: t.path === "redrive" ? t.drain_lease_epoch! : null, caller_nonce: String(n) };
}
export function requestTuple(request: SpawnOwnerRequest): OwnerTuple | null {
  if (!request || request.schema_version !== 1 || typeof request.caller_nonce !== "string" || !NONCE.test(request.caller_nonce)
    || (request.now !== undefined && !validTime(request.now))) return null;
  return normalizeTuple(request.tuple, request.caller_nonce);
}
export const activeKey = (t: OwnerTuple) => `${PREFIX}spawn-active:${t.repo}/${t.job_id}/${t.path}/${enc(t.effect_id)}`;
export const attemptKey = (t: OwnerTuple) => `${PREFIX}spawn-attempt:${t.repo}/${t.job_id}/${t.path}/${enc(t.effect_id)}/${enc(t.caller_nonce)}`;
export const mirrorKey = (t: OwnerTuple) => `${PREFIX}spawn-mirror:${t.repo}/${t.job_id}/${t.path}/${enc(t.effect_id)}`;
export const startKey = (t: OwnerTuple) => `${PREFIX}effect-start:${t.repo}/${t.job_id}/${t.path}/${enc(t.effect_id)}`;
export const bindingKey = (t: OwnerTuple) => `${PREFIX}effect-binding:${t.repo}/${t.job_id}/${t.path}/${enc(t.effect_id)}`;

export function activePointerProjection(value: Record<string, unknown>): Record<string, unknown> {
  const pointer = { ...value };
  delete pointer.created_ms; delete pointer.expires_ms; delete pointer.effect_started;
  delete pointer.permit; delete pointer.binding; delete pointer.effect_observation; delete pointer.provider_receipt;
  delete pointer.reap_proof;
  return pointer;
}
export function permitValid(v: unknown, t: OwnerTuple): v is ContainmentEffectPermit {
  if (!v || typeof v !== "object") return false; const p = v as Partial<ContainmentEffectPermit>;
  return p.schema_version === 1 && p.repo === t.repo && p.job_id === t.job_id && p.path === t.path
    && p.event_id === t.event_id && p.reservation_epoch === t.reservation_epoch && p.effect_id === t.effect_id
    && p.issued_to_owner === t.owner && p.issued_to_epoch === t.lease_epoch && validText(p.permit_id)
    && HEX.test(p.owner_token_digest ?? "") && validTime(p.issued_at_ms) && validTime(p.expires_ms);
}
export function proofValid(v: unknown, t: OwnerTuple, permit: string): v is EffectStartProofV1 {
  if (!v || typeof v !== "object") return false; const p = v as Partial<EffectStartProofV1>;
  return p.schema_version === 1 && p.repo === t.repo && p.job_id === t.job_id && p.path === t.path
    && p.event_id === t.event_id && p.reservation_epoch === t.reservation_epoch && p.effect_id === t.effect_id
    && p.permit_id === permit && p.owner === t.owner && p.token === t.token && p.lease_epoch === t.lease_epoch
    && p.caller_nonce === t.caller_nonce && p.writer === "ContainmentDO" && validText(p.proof_id)
    && HEX.test(p.effect_request_digest ?? "") && validTime(p.started_at_ms);
}
export function bindingValid(v: unknown, id: string | null): v is ContainmentEffectBinding {
  if (!v || typeof v !== "object") return false; const b = v as Partial<ContainmentEffectBinding>;
  return b.schema_version === 1 && validText(b.provider) && validText(b.resource_id)
    && validText(b.idempotency_key) && HEX.test(b.binding_sha256 ?? "") && b.binding_sha256 === id;
}
function reapProofValid(v: unknown, t: OwnerTuple): v is EffectReapProofV1 {
  if (!v || typeof v !== "object") return false; const p = v as Partial<EffectReapProofV1>;
  return p.schema_version === 1 && JSON.stringify(p.tuple) === JSON.stringify(t) && p.nonce === t.caller_nonce
    && p.attempt_key === attemptKey(t) && p.active_pointer_key === activeKey(t)
    && (p.authority === "containment-reaper-v1" || p.authority === "owner-abort-v1")
    && validTime(p.checked_at_ms) && p.no_permit === true && p.no_binding === true && p.no_effect === true;
}
export function pointerValid(v: unknown, t: OwnerTuple, attempt?: unknown): v is OwnerPointerV1 {
  if (!v || typeof v !== "object") return false; const p = v as Partial<OwnerPointerV1>;
  const states = ["PREPARED", "CLAIM_ACQUIRED", "PERMIT_ISSUED", "BOUND", "DRIVING", "COMMITTED", "ABORTED_PRE_EFFECT", "UNKNOWN"];
  return p.schema_version === 1 && p.nonce === t.caller_nonce && JSON.stringify(p.tuple) === JSON.stringify(t)
    && p.repo === t.repo && p.job_id === t.job_id && p.path === t.path && p.event_id === t.event_id
    && p.reservation_epoch === t.reservation_epoch && p.effect_id === t.effect_id && p.owner === t.owner
    && p.token === t.token && p.lease_epoch === t.lease_epoch && p.drain_owner === t.drain_owner
    && p.drain_lease_epoch === t.drain_lease_epoch && p.caller_nonce === t.caller_nonce
    && p.attempt_key === attemptKey(t) && states.includes(p.state as string)
    && (p.permit_id === null || validText(p.permit_id)) && (p.binding_id === null || validText(p.binding_id))
    && (p.effect_start_proof_id === null || validText(p.effect_start_proof_id)) && typeof p.tombstone === "boolean"
    && (!attempt || (recordValid(attempt, t) && JSON.stringify(p) === JSON.stringify(activePointerProjection(attempt as unknown as Record<string, unknown>))))
    && ((p.state === "PREPARED" || p.state === "CLAIM_ACQUIRED")
      ? p.permit_id === null && p.binding_id === null && p.effect_start_proof_id === null && p.tombstone === false
      : p.state === "PERMIT_ISSUED" ? p.permit_id !== null && p.binding_id === null && p.tombstone === false
      : p.state === "ABORTED_PRE_EFFECT" ? p.permit_id === null && p.binding_id === null && p.effect_start_proof_id === null && p.tombstone === true
      : p.state === "UNKNOWN" ? p.tombstone === false
      : p.permit_id !== null && p.binding_id !== null && p.effect_start_proof_id !== null && p.tombstone === false);
}
export function pointerMatchesAttempt(pointer: unknown, attempt: unknown, t: OwnerTuple): boolean {
  return pointerValid(pointer, t, attempt);
}
export function recordValid(v: unknown, t: OwnerTuple): v is OwnerRecordV1 {
  if (!v || typeof v !== "object") return false; const r = v as Partial<OwnerRecordV1> & {
    permit?: ContainmentEffectPermit; binding?: ContainmentEffectBinding; effect_observation?: ContainmentEffectReceipt; reap_proof?: EffectReapProofV1;
  };
  const states = ["PREPARED", "CLAIM_ACQUIRED", "PERMIT_ISSUED", "BOUND", "DRIVING", "COMMITTED", "ABORTED_PRE_EFFECT", "UNKNOWN"];
  if (r.schema_version !== 1 || r.nonce !== t.caller_nonce || r.repo !== t.repo || r.job_id !== t.job_id
    || r.path !== t.path || r.event_id !== t.event_id || r.reservation_epoch !== t.reservation_epoch
    || r.effect_id !== t.effect_id || r.owner !== t.owner || r.token !== t.token || r.lease_epoch !== t.lease_epoch
    || r.drain_owner !== t.drain_owner || r.drain_lease_epoch !== t.drain_lease_epoch || r.caller_nonce !== t.caller_nonce
    || JSON.stringify(r.tuple) !== JSON.stringify(t) || !states.includes(r.state as string)
    || (r.permit_id !== null && !permitValid(r.permit, t))
    || (r.permit_id === null && r.permit !== undefined)
    || (r.binding_id !== null && !bindingValid(r.binding, r.binding_id ?? null))
    || (r.binding_id === null && r.binding !== undefined)
    || (r.effect_start_proof_id === null) !== (r.effect_started === false)
    || !validTime(r.created_ms) || !validTime(r.expires_ms) || typeof r.tombstone !== "boolean") return false;
  if (r.state === "PREPARED" || r.state === "CLAIM_ACQUIRED") return r.permit_id === null && r.binding_id === null && r.effect_start_proof_id === null && r.tombstone === false;
  if (r.state === "PERMIT_ISSUED") return r.permit_id !== null && r.binding_id === null && r.tombstone === false;
  if (r.state === "BOUND" || r.state === "DRIVING") return r.permit_id !== null && r.binding_id !== null && r.effect_start_proof_id !== null && r.tombstone === false;
  if (r.state === "COMMITTED") {
    const o = r.effect_observation;
    return r.permit_id !== null && r.binding_id !== null && r.effect_start_proof_id !== null && r.tombstone === false
      && !!o && o.schema_version === 1 && o.trusted === true && o.repo === t.repo && o.job_id === t.job_id
      && o.path === t.path && o.event_id === t.event_id && o.reservation_epoch === t.reservation_epoch
      && o.effect_id === t.effect_id && o.nonce === t.caller_nonce && o.permit_id === r.permit_id
      && !!r.binding && o.binding_sha256 === r.binding_id && o.binding_sha256 === r.binding.binding_sha256
      && o.provider === r.binding.provider && o.resource_id === r.binding.resource_id
      && o.idempotency_key === r.binding.idempotency_key && validText(o.provider) && validText(o.resource_id)
      && validText(o.idempotency_key) && validText(o.receipt_id) && HEX.test(o.receipt_sha256)
      && validText(o.provider_signature);
  }
  if (r.state === "ABORTED_PRE_EFFECT") return r.tombstone === true && r.permit_id === null && r.binding_id === null && r.effect_start_proof_id === null && r.effect_started === false && reapProofValid(r.reap_proof, t);
  return r.state === "UNKNOWN" && r.tombstone === false;
}
