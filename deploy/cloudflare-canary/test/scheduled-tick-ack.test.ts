import { describe, expect, it } from "vitest";
import { validAck, type AckToken, type TickEnvelope } from "../src/tick_outbox";

const hex = "a".repeat(64);
const envelope: TickEnvelope = { event_id: "event-1", producer_seq: 7, payload_digest: hex, source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", monitor_rearm_tuple_digest: hex, occurred_at: 1, signature: "fixture" };
const head = { envelope, enqueuedAt: 1 };
const ack: AckToken = { ack_version: "1", event_id: "event-1", producer_seq: 7, payload_digest: hex, source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", monitor_rearm_tuple_digest: hex, ingest_commit_id: "commit", committed_at: 2, signer_key_id: "ack-signer", signer_epoch: "4", signature: "fixture" };
function b64(bytes: Uint8Array): string { return btoa(String.fromCharCode(...bytes)).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, ""); }
async function signed(): Promise<{ token: AckToken; verifier: { verify(payload: string, signature: string): Promise<"valid" | "invalid"> } }> {
  const pair = await crypto.subtle.generateKey("Ed25519", true, ["sign", "verify"]) as CryptoKeyPair;
  const unsigned = [ack.ack_version, ack.event_id, ack.producer_seq, ack.payload_digest, ack.source, ack.service, ack.application, ack.key_id, ack.credential_epoch, ack.monitor_rearm_tuple_digest, ack.ingest_commit_id, ack.committed_at, ack.signer_key_id, ack.signer_epoch];
  const payload = JSON.stringify(unsigned);
  const token = { ...ack, signature: b64(new Uint8Array(await crypto.subtle.sign("Ed25519", pair.privateKey, new TextEncoder().encode(payload)))) };
  const publicKey = b64(new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey) as ArrayBuffer));
  return { token, verifier: { verify: async (candidate, signature) => {
    const raw = Uint8Array.from(atob(signature.replaceAll("-", "+").replaceAll("_", "/") + "=".repeat((4 - signature.length % 4) % 4)), c => c.charCodeAt(0));
    const key = await crypto.subtle.importKey("raw", Uint8Array.from(atob(publicKey.replaceAll("-", "+").replaceAll("_", "/") + "=".repeat((4 - publicKey.length % 4) % 4)), c => c.charCodeAt(0)), "Ed25519", false, ["verify"]);
    return await crypto.subtle.verify("Ed25519", key, raw, new TextEncoder().encode(candidate)) ? "valid" : "invalid";
  } } };
}

describe("scheduled tick ACK gate", () => {
  it("requires every frozen ACK field and a real Ed25519 signature", async () => {
    const { token, verifier } = await signed();
    expect(await validAck(token, head, verifier, 3)).toBe("valid");
    expect(await validAck({ ...token, payload_digest: "b".repeat(64) }, head, verifier, 3)).toBe("invalid");
    expect(await validAck({ ...token, signature: `${token.signature.slice(0, -1)}A` }, head, verifier, 3)).toBe("invalid");
    expect(await validAck(token, head, undefined, 3)).toBe("invalid");
  });
  it("rejects a signer that is cryptographically current but revoked", async () => {
    expect(await validAck(ack, head, { verify: async () => "revoked" }, 3)).toBe("revoked");
  });
});
