import type { AckVerifier, TickConfig } from "./config";

export const TICK_DEADLINE_MS = 60_000;

export interface TickEnvelope {
  event_id: string; producer_seq: number; payload_digest: string;
  source: string; service: string; application: string; key_id: string;
  credential_epoch: string; monitor_rearm_tuple_digest: string; occurred_at: number;
  signature: string;
}
export interface AckToken {
  ack_version: string; event_id: string; producer_seq: number; payload_digest: string;
  source: string; service: string; application: string; key_id: string;
  credential_epoch: string; monitor_rearm_tuple_digest: string; ingest_commit_id: string;
  committed_at: number; signer_key_id: string; signer_epoch: string; signature: string;
}
export interface AckRecovery {
  recovery_version: string; event_id: string; producer_seq: number; payload_digest: string; source: string; service: string; application: string; key_id: string; credential_epoch: string;
  original_monitor_rearm_tuple_digest: string; ingest_commit_id: string; original_ack_digest: string; revocation_record_digest: string; signer_rotation_manifest_digest: string; signer_manifest_generation: number; signer_manifest_witness_root_digest: string; current_monitor_rearm_tuple_digest: string; recovery_signer_key_id: string; recovery_signer_epoch: string; issued_at: number; signature: string;
}
type Head = { envelope: TickEnvelope; enqueuedAt: number };
type State = { seq: number; head?: Head; terminal?: "ACKED" | "TIMED_OUT" | "CONFIG_UNAVAILABLE" };

const stateKey = "state";
const encoder = new TextEncoder();
const fields = (v: object, keys: readonly string[]) => keys.map((key) => String((v as Record<string, unknown>)[key])).join("\n");
const envelopeFields = ["event_id", "producer_seq", "payload_digest", "source", "service", "application", "key_id", "credential_epoch", "monitor_rearm_tuple_digest", "occurred_at"] as const;
const ackFields = ["ack_version", "event_id", "producer_seq", "payload_digest", "source", "service", "application", "key_id", "credential_epoch", "monitor_rearm_tuple_digest", "ingest_commit_id", "committed_at", "signer_key_id", "signer_epoch"] as const;
const recoveryFields = ["recovery_version", "event_id", "producer_seq", "payload_digest", "source", "service", "application", "key_id", "credential_epoch", "original_monitor_rearm_tuple_digest", "ingest_commit_id", "original_ack_digest", "revocation_record_digest", "signer_rotation_manifest_digest", "signer_manifest_generation", "signer_manifest_witness_root_digest", "current_monitor_rearm_tuple_digest", "recovery_signer_key_id", "recovery_signer_epoch", "issued_at"] as const;

async function hmac(value: string, key: string): Promise<string> {
  const cryptoKey = await crypto.subtle.importKey("raw", encoder.encode(key), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const bytes = await crypto.subtle.sign("HMAC", cryptoKey, encoder.encode(value));
  return [...new Uint8Array(bytes)].map((n) => n.toString(16).padStart(2, "0")).join("");
}
async function digest(value: string): Promise<string> {
  const bytes = await crypto.subtle.digest("SHA-256", encoder.encode(value));
  return [...new Uint8Array(bytes)].map((n) => n.toString(16).padStart(2, "0")).join("");
}
function isAck(value: unknown): value is AckToken {
  return Boolean(value && typeof value === "object" && ackFields.every((key) => key in value) && typeof (value as AckToken).signature === "string");
}
export async function validAck(ack: AckToken, envelope: TickEnvelope, verifier: AckVerifier | undefined): Promise<"valid" | "revoked" | "invalid"> {
  if (!verifier || ack.ack_version !== "1" || ack.event_id !== envelope.event_id || ack.producer_seq !== envelope.producer_seq || ack.payload_digest !== envelope.payload_digest || ack.source !== envelope.source || ack.service !== envelope.service || ack.application !== envelope.application || ack.key_id !== envelope.key_id || ack.credential_epoch !== envelope.credential_epoch || ack.monitor_rearm_tuple_digest !== envelope.monitor_rearm_tuple_digest || !ack.ingest_commit_id || !Number.isFinite(ack.committed_at)) return "invalid";
  return verifier.verify(fields(ack, ackFields), ack.signer_key_id, ack.signer_epoch);
}
export async function validRecovery(recovery: AckRecovery, envelope: TickEnvelope, original: AckToken, verifier: AckVerifier | undefined, now: number): Promise<boolean> {
  if (!verifier || recovery.recovery_version !== "1" || recovery.event_id !== envelope.event_id || recovery.producer_seq !== envelope.producer_seq || recovery.payload_digest !== envelope.payload_digest || recovery.source !== envelope.source || recovery.service !== envelope.service || recovery.application !== envelope.application || recovery.key_id !== envelope.key_id || recovery.credential_epoch !== envelope.credential_epoch || recovery.original_monitor_rearm_tuple_digest !== envelope.monitor_rearm_tuple_digest || recovery.current_monitor_rearm_tuple_digest !== envelope.monitor_rearm_tuple_digest || recovery.ingest_commit_id !== original.ingest_commit_id || !recovery.original_ack_digest || now - envelope.occurred_at > TICK_DEADLINE_MS) return false;
  return (await verifier.verify(fields(recovery, recoveryFields), recovery.recovery_signer_key_id, recovery.recovery_signer_epoch)) === "valid";
}

/** One Durable Object is one ordered lane.  It commits its head before any
 * network I/O and never creates a successor until that head is terminal. */
export class CanaryTickOutbox {
  constructor(private readonly state: DurableObjectState) {}

  async enqueueAndDrain(config: TickConfig | null, now: number, configInvalid = false): Promise<string> {
    let value = (await this.state.storage.get<State>(stateKey)) ?? { seq: 0 };
    if (!value.head) {
      if (!config) {
        value.terminal = "CONFIG_UNAVAILABLE";
        await this.state.storage.put(stateKey, value);
        return "tick config unavailable";
      }
      const producer_seq = value.seq + 1;
      const unsigned = {
        // Event identity is the wire-visible type until T6-W15 supplies the
        // frozen envelope payload schema. This keeps invalid configuration
        // fail-visible without pretending an unbound monitor accepted it.
        event_id: `${configInvalid ? "CANARY_CONFIG_INVALID" : "canary-tick"}-${producer_seq}`, producer_seq,
        payload_digest: await digest(`${configInvalid ? "CANARY_CONFIG_INVALID" : "canary-tick"}:${producer_seq}:${now}`),
        source: config.source, service: config.service, application: config.application,
        key_id: config.keyId, credential_epoch: config.credentialEpoch,
        monitor_rearm_tuple_digest: config.monitorRearmTupleDigest, occurred_at: now,
      };
      const envelope: TickEnvelope = { ...unsigned, signature: await hmac(fields(unsigned, envelopeFields), config.envelopeHmacKey) };
      value = { seq: producer_seq, head: { envelope, enqueuedAt: now } };
      await this.state.storage.put(stateKey, value); // write-ahead before transmit
      await this.state.storage.setAlarm(now + TICK_DEADLINE_MS);
    }
    if (!config) return "tick head held: config unavailable";
    const head = value.head!;
    if (now - head.enqueuedAt > TICK_DEADLINE_MS) {
      value.head = undefined; value.terminal = "TIMED_OUT";
      await this.state.storage.put(stateKey, value);
      await this.state.storage.deleteAlarm();
      return "tick terminal: deadline exceeded";
    }
    try {
      const response = await fetch(config.ingestUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(head.envelope) });
      const candidate: unknown = response.ok ? await response.json().catch(() => null) : null;
      if (!isAck(candidate) || await validAck(candidate, head.envelope, config.ackVerifier) !== "valid") return "tick head pending: invalid ACK";
      value.head = undefined; value.terminal = "ACKED";
      await this.state.storage.put(stateKey, value);
      await this.state.storage.deleteAlarm();
      return "tick terminal: ACKED";
    } catch { return "tick head pending: transmit failed"; }
  }

  /** Alarm preserves the original enqueue clock even if cron delivery pauses.
   * A retry can never use a later tick to restart the 60-second residence cap. */
  async alarm(): Promise<void> {
    const value = await this.state.storage.get<State>(stateKey);
    if (!value?.head || Date.now() - value.head.enqueuedAt < TICK_DEADLINE_MS) return;
    value.head = undefined;
    value.terminal = "TIMED_OUT";
    await this.state.storage.put(stateKey, value);
  }
}
