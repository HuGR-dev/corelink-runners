import type { TickConfig } from "./config";

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
type Head = { envelope: TickEnvelope; enqueuedAt: number };
type State = { seq: number; head?: Head; terminal?: "ACKED" | "TIMED_OUT" | "CONFIG_UNAVAILABLE" };

const stateKey = "state";
const encoder = new TextEncoder();
const fields = (v: Record<string, unknown>, keys: readonly string[]) => keys.map((key) => String(v[key])).join("\n");
const envelopeFields = ["event_id", "producer_seq", "payload_digest", "source", "service", "application", "key_id", "credential_epoch", "monitor_rearm_tuple_digest", "occurred_at"] as const;
const ackFields = ["ack_version", "event_id", "producer_seq", "payload_digest", "source", "service", "application", "key_id", "credential_epoch", "monitor_rearm_tuple_digest", "ingest_commit_id", "committed_at", "signer_key_id", "signer_epoch"] as const;

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

/** One Durable Object is one ordered lane.  It commits its head before any
 * network I/O and never creates a successor until that head is terminal. */
export class CanaryTickOutbox {
  constructor(private readonly state: DurableObjectState) {}

  async enqueueAndDrain(config: TickConfig | null, now: number): Promise<string> {
    let value = (await this.state.storage.get<State>(stateKey)) ?? { seq: 0 };
    if (!value.head) {
      if (!config) {
        value.terminal = "CONFIG_UNAVAILABLE";
        await this.state.storage.put(stateKey, value);
        return "tick config unavailable";
      }
      const producer_seq = value.seq + 1;
      const unsigned = {
        event_id: `canary-tick-${producer_seq}`, producer_seq,
        payload_digest: await digest(`canary-tick:${producer_seq}:${now}`),
        source: config.source, service: config.service, application: config.application,
        key_id: config.keyId, credential_epoch: config.credentialEpoch,
        monitor_rearm_tuple_digest: config.monitorRearmTupleDigest, occurred_at: now,
      };
      const envelope: TickEnvelope = { ...unsigned, signature: await hmac(fields(unsigned, envelopeFields), config.envelopeHmacKey) };
      value = { seq: producer_seq, head: { envelope, enqueuedAt: now } };
      await this.state.storage.put(stateKey, value); // write-ahead before transmit
    }
    if (!config) return "tick head held: config unavailable";
    const head = value.head!;
    if (now - head.enqueuedAt > TICK_DEADLINE_MS) {
      value.head = undefined; value.terminal = "TIMED_OUT";
      await this.state.storage.put(stateKey, value);
      return "tick terminal: deadline exceeded";
    }
    try {
      const response = await fetch(config.ingestUrl, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(head.envelope) });
      const candidate: unknown = response.ok ? await response.json().catch(() => null) : null;
      if (!isAck(candidate) || !await this.validAck(candidate, head.envelope, config)) return "tick head pending: invalid ACK";
      value.head = undefined; value.terminal = "ACKED";
      await this.state.storage.put(stateKey, value);
      return "tick terminal: ACKED";
    } catch { return "tick head pending: transmit failed"; }
  }

  private async validAck(ack: AckToken, envelope: TickEnvelope, config: TickConfig): Promise<boolean> {
    if (ack.ack_version !== "1" || ack.event_id !== envelope.event_id || ack.producer_seq !== envelope.producer_seq ||
      ack.payload_digest !== envelope.payload_digest || ack.source !== envelope.source || ack.service !== envelope.service ||
      ack.application !== envelope.application || ack.key_id !== envelope.key_id || ack.credential_epoch !== envelope.credential_epoch ||
      ack.monitor_rearm_tuple_digest !== envelope.monitor_rearm_tuple_digest || !ack.ingest_commit_id || !Number.isFinite(ack.committed_at)) return false;
    return ack.signature === await hmac(fields(ack, ackFields), config.ackHmacKey);
  }
}
