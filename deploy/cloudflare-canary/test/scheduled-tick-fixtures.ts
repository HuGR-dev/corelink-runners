import type { AckVerifier } from "../src/config";
import type { AckRecovery, AckToken, TickEnvelope } from "../src/tick_outbox";

export const HEX = "a".repeat(64);
export const ACK_FIELDS = ["ack_version", "event_id", "producer_seq", "payload_digest", "source", "service", "application", "key_id", "credential_epoch", "monitor_rearm_tuple_digest", "ingest_commit_id", "committed_at", "signer_key_id", "signer_epoch"] as const;
export const RECOVERY_FIELDS = ["recovery_version", "event_id", "producer_seq", "payload_digest", "source", "service", "application", "key_id", "credential_epoch", "original_monitor_rearm_tuple_digest", "ingest_commit_id", "original_ack_digest", "revocation_record_digest", "signer_rotation_manifest_digest", "signer_manifest_generation", "signer_manifest_witness_root_digest", "current_monitor_rearm_tuple_digest", "recovery_signer_key_id", "recovery_signer_epoch", "issued_at"] as const;

export const envelope: TickEnvelope = {
  event_id: "event-1", producer_seq: 7, payload_digest: HEX, source: "canary",
  service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3",
  monitor_rearm_tuple_digest: HEX, occurred_at: 1_000, signature: "fixture",
};
export const head = { envelope, enqueuedAt: 1_000 };
export const ackUnsigned: Omit<AckToken, "signature"> = {
  ack_version: "1", event_id: envelope.event_id, producer_seq: envelope.producer_seq,
  payload_digest: envelope.payload_digest, source: envelope.source, service: envelope.service,
  application: envelope.application, key_id: envelope.key_id, credential_epoch: envelope.credential_epoch,
  monitor_rearm_tuple_digest: envelope.monitor_rearm_tuple_digest, ingest_commit_id: "commit",
  committed_at: 1_001, signer_key_id: "ack-signer", signer_epoch: "4",
};

export function canonical(value: object, fields: readonly string[]): string {
  return JSON.stringify(fields.map((field) => (value as Record<string, unknown>)[field]));
}
function b64(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes)).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}
function bytes(value: string): Uint8Array {
  return Uint8Array.from(atob(value.replaceAll("-", "+").replaceAll("_", "/") + "=".repeat((4 - value.length % 4) % 4)), (c) => c.charCodeAt(0));
}
export async function sha256(value: string): Promise<string> {
  return [...new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)))].map((n) => n.toString(16).padStart(2, "0")).join("");
}
export async function signedToken<T extends object>(unsigned: T, fields: readonly string[], signerKeyId: string, signerEpoch: string): Promise<{ token: T & { signature: string }; verifier: AckVerifier }> {
  const pair = await crypto.subtle.generateKey("Ed25519", true, ["sign", "verify"]) as CryptoKeyPair;
  const signature = b64(new Uint8Array(await crypto.subtle.sign("Ed25519", pair.privateKey, new TextEncoder().encode(canonical(unsigned, fields)))));
  const publicKey = b64(new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey) as ArrayBuffer));
  return {
    token: { ...unsigned, signature },
    verifier: {
      verify: async (payload, actual, keyId, epoch) => {
        if (keyId !== signerKeyId || epoch !== signerEpoch) return "invalid";
        return await crypto.subtle.verify("Ed25519", await crypto.subtle.importKey("raw", bytes(publicKey), "Ed25519", false, ["verify"]), bytes(actual), new TextEncoder().encode(payload)) ? "valid" : "invalid";
      },
    },
  };
}
export async function signedAck(overrides: Partial<typeof ackUnsigned> = {}) {
  return signedToken({ ...ackUnsigned, ...overrides }, ACK_FIELDS, "ack-signer", "4");
}
export async function signedRecovery(ack: AckToken, overrides: Partial<Omit<AckRecovery, "signature">> = {}) {
  const originalAckDigest = await sha256(JSON.stringify([...JSON.parse(canonical(ack, ACK_FIELDS)), ack.signature]));
  const unsigned: Omit<AckRecovery, "signature"> = {
    recovery_version: "1", event_id: envelope.event_id, producer_seq: envelope.producer_seq,
    payload_digest: envelope.payload_digest, source: envelope.source, service: envelope.service,
    application: envelope.application, key_id: envelope.key_id, credential_epoch: envelope.credential_epoch,
    original_monitor_rearm_tuple_digest: envelope.monitor_rearm_tuple_digest, ingest_commit_id: ack.ingest_commit_id,
    original_ack_digest: originalAckDigest, revocation_record_digest: HEX, signer_rotation_manifest_digest: HEX,
    signer_manifest_generation: 2, signer_manifest_witness_root_digest: HEX,
    current_monitor_rearm_tuple_digest: envelope.monitor_rearm_tuple_digest,
    recovery_signer_key_id: "recovery", recovery_signer_epoch: "5", issued_at: 1_010, ...overrides,
  };
  return signedToken(unsigned, RECOVERY_FIELDS, "recovery", "5");
}
