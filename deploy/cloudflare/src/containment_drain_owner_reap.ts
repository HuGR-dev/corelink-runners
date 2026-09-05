import type { EffectReapProofV1, OwnerPointerV1, OwnerRecordV1, OwnerTuple } from "./containment_effect_ledger";
import {
  activeKey,
  activePointerProjection,
  attemptKey,
  normalizeTuple,
  pointerMatchesAttempt,
  recordValid,
} from "./containment_effect_ledger_records";

interface Storage {
  get<T>(key: string): Promise<T | undefined>;
  put(key: string, value: unknown): Promise<void>;
}

interface DrainMeta {
  schema_version: 1;
  next_pause_seq: number;
  drain_cursor: number;
  backlog_count: number;
  lease_epoch: number;
  lease: { owner: string; epoch: number; expires_ms: number } | null;
  drain_requested: boolean;
}

interface DrainEvent {
  schema_version: 1;
  event_id: string;
  pause_seq: number;
  repo: string;
  job_id: string;
  effect_id: string;
  state: "QUEUED" | "CLAIMED" | "EFFECT_COMMITTED";
  claim: { owner: string; lease_epoch: number } | null;
  effect_permit: { permit_id: string; issued_to_owner: string; issued_to_epoch: number } | null;
}

const metaKey = "containment:v1:meta";
const eventKey = (eventId: string) => `containment:v1:event:${eventId}`;
const pauseKey = (sequence: number) => `containment:v1:pause:${String(sequence).padStart(20, "0")}`;

function exactDrainTuple(value: unknown): OwnerTuple | null {
  const tuple = normalizeTuple(value);
  return tuple && JSON.stringify(tuple) === JSON.stringify(value)
    && tuple.path === "drain" && tuple.reservation_epoch === null
    && tuple.owner === tuple.drain_owner && tuple.lease_epoch === tuple.drain_lease_epoch
    ? tuple : null;
}

function cleanPreEffect(record: OwnerRecordV1): boolean {
  return (record.state === "PREPARED" || record.state === "CLAIM_ACQUIRED")
    && record.permit_id === null && record.binding_id === null
    && record.effect_start_proof_id === null && record.effect_started === false
    && record.tombstone === false;
}

/**
 * Fence an abandoned pre-permit drain owner. The caller supplies a storage
 * transaction, so lease/head authorization and the tombstone commit are one
 * atomic decision. Post-permit or divergent state is never changed.
 */
export async function admitDrainOwnerInTransaction(
  storage: Storage,
  eventId: string,
  suppliedTuple: OwnerTuple,
  now: number,
): Promise<boolean> {
  const tuple = exactDrainTuple(suppliedTuple);
  if (!tuple || tuple.event_id !== eventId || !Number.isSafeInteger(now) || now < 0) return false;
  const meta = await storage.get<DrainMeta>(metaKey);
  if (!meta || meta.schema_version !== 1 || !Number.isSafeInteger(meta.drain_cursor) || meta.drain_cursor < 0
    || !Number.isSafeInteger(meta.backlog_count) || meta.backlog_count < 1
    || !Number.isSafeInteger(meta.next_pause_seq) || meta.next_pause_seq <= meta.drain_cursor
    || typeof meta.drain_requested !== "boolean" || !Number.isSafeInteger(meta.lease_epoch)
    || meta.lease_epoch !== tuple.lease_epoch || !meta.lease || meta.lease.owner !== tuple.owner
    || !Number.isSafeInteger(meta.lease.epoch) || meta.lease.epoch !== tuple.lease_epoch
    || !Number.isSafeInteger(meta.lease.expires_ms) || meta.lease.expires_ms <= now) return false;
  const sequence = meta.drain_cursor + 1;
  if (!Number.isSafeInteger(sequence)) return false;
  const pause = await storage.get<{ schema_version: 1; event_id: string; pause_seq: number }>(pauseKey(sequence));
  const event = await storage.get<DrainEvent>(eventKey(eventId));
  if (!pause || pause.schema_version !== 1 || pause.event_id !== eventId || pause.pause_seq !== sequence || !event
    || !Number.isSafeInteger(pause.pause_seq) || event.schema_version !== 1 || event.event_id !== eventId
    || !Number.isSafeInteger(event.pause_seq) || event.pause_seq !== sequence || event.state !== "CLAIMED"
    || event.claim?.owner !== tuple.owner || !Number.isSafeInteger(event.claim.lease_epoch)
    || event.claim.lease_epoch !== tuple.lease_epoch
    || event.effect_permit !== null || event.repo !== tuple.repo || event.job_id !== tuple.job_id
    || event.effect_id !== tuple.effect_id) return false;

  const pointerKey = activeKey(tuple);
  const pointer = await storage.get<OwnerPointerV1>(pointerKey);
  if (pointer === undefined) return true;
  const oldTuple = exactDrainTuple(pointer.tuple);
  const oldAttempt = oldTuple ? await storage.get<OwnerRecordV1>(attemptKey(oldTuple)) : undefined;
  if (!oldTuple || !oldAttempt || activeKey(oldTuple) !== pointerKey
    || oldTuple.event_id !== eventId || !pointerMatchesAttempt(pointer, oldAttempt, oldTuple)
    || !recordValid(oldAttempt, oldTuple)) return false;
  if (JSON.stringify(oldTuple) === JSON.stringify(tuple)) return cleanPreEffect(oldAttempt);
  if (oldTuple.lease_epoch >= tuple.lease_epoch) return false;
  // A previous authoritative reclaim may have fenced this predecessor and then
  // lost the external claim. Its validated tombstone remains safe for every
  // later lease; generic prepare already knows how to replace it.
  if (oldAttempt.state === "ABORTED_PRE_EFFECT" && oldAttempt.tombstone) return true;
  if (!cleanPreEffect(oldAttempt)) return false;

  const proof: EffectReapProofV1 = {
    schema_version: 1,
    tuple: oldTuple,
    nonce: oldTuple.caller_nonce,
    attempt_key: attemptKey(oldTuple),
    active_pointer_key: pointerKey,
    authority: "containment-reaper-v1",
    checked_at_ms: now,
    no_permit: true,
    no_binding: true,
    no_effect: true,
  };
  const tombstone: OwnerRecordV1 = {
    ...oldAttempt,
    state: "ABORTED_PRE_EFFECT",
    tombstone: true,
    reap_proof: proof,
  };
  await storage.put(attemptKey(oldTuple), tombstone);
  await storage.put(pointerKey, activePointerProjection(tombstone as unknown as Record<string, unknown>) as unknown as OwnerPointerV1);
  return true;
}
