import {
  containmentInvalidKey,
  containmentOutboxKey,
  isValidInvalidConfigRecord,
  isValidOutboxRecord,
  type ContainmentMetaShape,
  type ContainmentOutboxShape,
  type InvalidConfigShape,
  type RedrivePermitShape,
  type RedriveReservationShape,
} from "./containment_authority_helpers";

export const CONTAINMENT_META_KEY = "containment:v1:meta";
export const CONTAINMENT_INVALID_PREFIX = "containment:v1:invalid:";
export const CONTAINMENT_OUTBOX_PREFIX = "containment:v1:outbox:";
export const CONTAINMENT_INDEX_META_KEY = "containment:v1:job-index-meta";
export const INVALID_CONFIG_SWITCHES = new Set(["AUTOSCALER_INTAKE_PAUSED", "AUTOSCALER_REDRIVE_PAUSED"]);
export const SHA256_HEX = /^[0-9a-f]{64}$/;
export const DRAIN_LEASE_TTL_MS = 120_000;
export const DRAIN_RENEW_THRESHOLD_MS = 30_000;
export const REDRIVE_RESERVATION_TTL_MS = 120_000;

export type ContainmentMeta = ContainmentMetaShape;
export type InvalidConfigRecord = InvalidConfigShape;
export type ContainmentOutboxRecord = ContainmentOutboxShape;
export type ContainmentRedriveReservation = RedriveReservationShape;
export type ContainmentRedrivePermit = RedrivePermitShape;
export type ContainmentState = "QUEUED" | "CLAIMED" | "EFFECT_COMMITTED";
export type ContainmentEffectWitnessKind = "spawn_claim" | "attempt" | "placement" | "lease" | "result";
export const CONTAINMENT_EFFECT_WITNESS_KINDS: readonly ContainmentEffectWitnessKind[] = ["spawn_claim", "attempt", "placement", "lease", "result"];

export interface ContainmentEffectEvidence {
  schema_version: 1;
  kind: ContainmentEffectWitnessKind;
  effect_id: string;
  event_id: string;
  job_id: string;
  permit_id: string;
  source_key: string;
  // Recovery cannot depend on mutable spawn/orphan/handle keys, which may be
  // deleted after completion, so the witness carries the immutable bytes.
  source_value: string;
  source_sha256: string;
  terminal?: "DELIVERED";
  attempt_count?: number;
}

export interface ContainmentEvent {
  schema_version: 1;
  event_id: string;
  pause_seq: number;
  received_at_ms: number;
  body_sha256: string;
  raw_payload: string;
  action: string;
  job_id: string;
  repo: string;
  installation_id: string;
  labels: string[];
  effect_id: string;
  state: ContainmentState;
  claim: { owner: string; lease_epoch: number } | null;
  effect_permit: { permit_id: string; issued_to_owner: string; issued_to_epoch: number } | null;
}

export interface ContainmentPause {
  schema_version: 1;
  event_id: string;
  pause_seq: number;
}

export function containmentEffectEvidenceKey(effectId: string, kind: ContainmentEffectWitnessKind): string {
  return `containment:v1:effect:${encodeURIComponent(effectId)}:${kind}`;
}

export function containmentEffectJobKey(jobId: string): string {
  return `containment:v1:job:${jobId}`;
}

export function leaseMatches(meta: ContainmentMeta, owner: string, epoch: number, now: number): boolean {
  return !!meta.lease && meta.lease.owner === owner && meta.lease.epoch === epoch && meta.lease.expires_ms > now;
}

export function isCurrentHead(meta: ContainmentMeta, event: ContainmentEvent): boolean {
  return event.pause_seq === meta.drain_cursor + 1;
}

export function canonicalContainmentEvidence(witness: ContainmentEffectEvidence): string {
  return JSON.stringify({
    schema_version: witness.schema_version,
    kind: witness.kind,
    effect_id: witness.effect_id,
    event_id: witness.event_id,
    job_id: witness.job_id,
    permit_id: witness.permit_id,
    source_key: witness.source_key,
    source_value: witness.source_value,
    source_sha256: witness.source_sha256,
    ...(witness.terminal ? { terminal: witness.terminal } : {}),
    ...(witness.attempt_count !== undefined ? { attempt_count: witness.attempt_count } : {}),
  });
}

interface ContainmentStorage {
  get<T>(key: string): Promise<T | undefined>;
  put(key: string, value: unknown): Promise<void>;
  list<T>(options: { prefix: string }): Promise<Map<string, T>>;
}

type Sha256Hex = (value: string) => Promise<string>;

export async function recordInvalidConfigInStorage(
  storage: ContainmentStorage,
  switchName: string,
  rawValue: string,
  rawValueSha256: string,
  sha256Hex: Sha256Hex,
): Promise<ContainmentOutboxRecord> {
  if (!INVALID_CONFIG_SWITCHES.has(switchName) || !SHA256_HEX.test(rawValueSha256)) {
    throw new TypeError("unsupported or malformed invalid-config identity");
  }
  if (await sha256Hex(rawValue) !== rawValueSha256) {
    throw new TypeError("invalid-config digest does not match raw value");
  }
  const signalId = await sha256Hex(`containment:v1:config-invalid\n${switchName}\n${rawValueSha256}`);
  const key = containmentInvalidKey(switchName, rawValueSha256);
  const prior = await storage.get<InvalidConfigRecord>(key);
  if (prior && (!isValidInvalidConfigRecord(prior, key) || prior.signal_id !== signalId)) {
    throw new TypeError("malformed invalid-config record");
  }
  if (!prior) {
    await storage.put(key, { schema_version: 1, signal_id: signalId, switch_name: switchName, raw_value_sha256: rawValueSha256 } satisfies InvalidConfigRecord);
  }
  const outboxKey = containmentOutboxKey(signalId);
  const existingOutbox = await storage.get<ContainmentOutboxRecord>(outboxKey);
  if (existingOutbox && (!isValidOutboxRecord(existingOutbox) || existingOutbox.signal_id !== signalId)) {
    throw new TypeError("malformed invalid-config outbox record");
  }
  const outbox = existingOutbox ?? { schema_version: 1 as const, signal_id: signalId, state: "PENDING" as const, attempts: 0 };
  if (!existingOutbox) await storage.put(outboxKey, outbox);
  return outbox;
}

export async function pendingInvalidConfigInStorage(
  storage: ContainmentStorage,
  sha256Hex: Sha256Hex,
): Promise<ContainmentOutboxRecord[]> {
  const records = await storage.list<ContainmentOutboxRecord>({ prefix: CONTAINMENT_OUTBOX_PREFIX });
  const identities = await storage.list<InvalidConfigRecord>({ prefix: CONTAINMENT_INVALID_PREFIX });
  const validSignals = new Set<string>();
  for (const [key, identity] of identities) {
    if (!isValidInvalidConfigRecord(identity, key)) continue;
    const expected = await sha256Hex(`containment:v1:config-invalid\n${identity.switch_name}\n${identity.raw_value_sha256}`);
    if (identity.signal_id === expected) validSignals.add(identity.signal_id);
  }
  return [...records.entries()]
    .filter(([key, record]) => isValidOutboxRecord(record)
      && key === containmentOutboxKey(record.signal_id)
      && record.state === "PENDING"
      && validSignals.has(record.signal_id))
    .map(([, record]) => record);
}

export async function markInvalidConfigAttemptInStorage(storage: ContainmentStorage, signalId: string): Promise<void> {
  const key = containmentOutboxKey(signalId);
  const record = await storage.get<ContainmentOutboxRecord>(key);
  if (record?.signal_id === signalId && record.state === "PENDING") {
    await storage.put(key, { ...record, attempts: record.attempts + 1 });
  }
}

export async function acknowledgeInvalidConfigInStorage(storage: ContainmentStorage, signalId: string): Promise<void> {
  const key = containmentOutboxKey(signalId);
  const record = await storage.get<ContainmentOutboxRecord>(key);
  if (record?.signal_id === signalId && record.state === "PENDING") {
    await storage.put(key, { ...record, state: "DELIVERED" });
  }
}
