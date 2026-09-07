import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { describe, expect, it } from "vitest";
import { canonicalPageAckPayload, createPageAckToken, verifyPageAckToken, type PageAckFields } from "../src/page_ack_token.js";
import type { AsyncSigner, PublicSigningIdentity } from "../src/acks.js";

const key = generateKeyPairSync("rsa", { modulusLength: 3072 });
const identity: PublicSigningIdentity = { keyId: "page-key", epoch: "7", keyArn: "arn:aws:kms:us-east-1:111111111111:key/page", publicKeySpkiPem: key.publicKey.export({ type: "spki", format: "pem" }).toString(), role: "page-ack" };
const digest = "a".repeat(64);
const fields: PageAckFields = { page_ack_version: "1", incident_id: "incident", page_id: "page", delivery_id: "delivery", destination: "ops", on_call_identity: "human@example", on_call_schedule_digest: digest, action: "ACK", payload_digest: "b".repeat(64), monitor_rearm_tuple_digest: "c".repeat(64), signer_rotation_manifest_digest: "d".repeat(64), acknowledged_at: 1_700_000_000_000, expires_at: 1_700_000_060_000, signer_key_id: identity.keyId, signer_epoch: identity.epoch };
const signer: AsyncSigner = { identity, async sign(bytes) { return sign(null, createHash("sha256").update(bytes).digest(), { key: key.privateKey, padding: 6, saltLength: 32 }).toString("base64url"); } };

describe("human page ACK token codec", () => {
  it("signs and verifies the exact fifteen-field ordered payload", async () => {
    const token = await createPageAckToken(fields, signer);
    expect(verifyPageAckToken(token, identity)).toBe(true);
    expect(canonicalPageAckPayload(fields)).toEqual(new TextEncoder().encode(JSON.stringify(Object.values(fields))));
  });
  it("rejects mutation of every signed field and extra fields", async () => {
    const token = await createPageAckToken(fields, signer);
    for (const field of Object.keys(fields) as Array<keyof PageAckFields>) {
      const mutated = { ...token, [field]: field.endsWith("_at") ? (token[field] as number) + 1 : `${token[field]}-changed` };
      expect(verifyPageAckToken(mutated, identity), field).toBe(false);
    }
    expect(verifyPageAckToken({ ...token, extra: true }, identity)).toBe(false);
  });
  it("enforces role, identity, digest, timestamp, and signature constraints", async () => {
    await expect(createPageAckToken({ ...fields, expires_at: fields.acknowledged_at }, signer)).rejects.toThrow();
    await expect(createPageAckToken({ ...fields, payload_digest: "A".repeat(64) }, signer)).rejects.toThrow();
    expect(verifyPageAckToken(await createPageAckToken(fields, signer), { ...identity, role: "manifest" })).toBe(false);
    expect(verifyPageAckToken(await createPageAckToken(fields, signer), { ...identity, epoch: "8" })).toBe(false);
    expect(verifyPageAckToken({ ...await createPageAckToken(fields, signer), signature: "%%%" }, identity)).toBe(false);
  });
});
