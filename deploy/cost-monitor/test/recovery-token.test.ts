import { describe, expect, it } from "vitest";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { createRecoveryToken, canonicalRecoveryPayload, verifyRecoveryToken, type RecoveryFields } from "../src/recovery_token.js";
import type { AsyncSigner, PublicSigningIdentity } from "../src/acks.js";

const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 3072 });
const identity: PublicSigningIdentity = { keyId: "recovery-key", epoch: "epoch-1", keyArn: "arn:aws:kms:us-east-1:123456789012:key/recovery", publicKeySpkiPem: publicKey.export({ type: "spki", format: "pem" }).toString(), role: "recovery" };
const digest = "a".repeat(64);
const fields: RecoveryFields = {
  recovery_version: "1", event_id: "event-1", producer_seq: 3, payload_digest: digest, source: "source", service: "service", application: "application",
  key_id: "credential-key", credential_epoch: "credential-epoch", original_monitor_rearm_tuple_digest: digest, ingest_commit_id: "ingest-1",
  original_ack_digest: digest, revocation_record_digest: digest, signer_rotation_manifest_digest: digest, signer_manifest_generation: 2,
  signer_manifest_witness_root_digest: digest, current_monitor_rearm_tuple_digest: digest, recovery_signer_key_id: identity.keyId,
  recovery_signer_epoch: identity.epoch, issued_at: 1_700_000_000_000,
};
const signer: AsyncSigner = { identity, async sign(bytes) { const digestBytes = createHash("sha256").update(bytes).digest(); return sign(null, digestBytes, { key: privateKey, padding: 6, saltLength: 32 }).toString("base64url"); } };

describe("recovery token codec", () => {
  it("uses the exact twenty-field ordered tuple and verifies real RSA-PSS", async () => {
    const token = await createRecoveryToken(fields, signer);
    expect(Array.from(canonicalRecoveryPayload(fields))).toEqual(Array.from(new TextEncoder().encode(JSON.stringify([
      "1", "event-1", 3, digest, "source", "service", "application", "credential-key", "credential-epoch", digest, "ingest-1", digest, digest, digest, 2, digest, digest, "recovery-key", "epoch-1", 1700000000000,
    ]))));
    expect(verifyRecoveryToken(token, identity)).toBe(true);
  });

  it("rejects every signed-field mutation, extra/missing fields, and wrong role or identity", async () => {
    const token = await createRecoveryToken(fields, signer);
    for (const key of Object.keys(fields) as Array<keyof RecoveryFields>) {
      const mutated = { ...token, [key]: typeof fields[key] === "number" ? (fields[key] as number) + 1 : `${fields[key]}-changed` };
      expect(verifyRecoveryToken(mutated, identity), key).toBe(false);
    }
    expect(verifyRecoveryToken({ ...token, extra: true }, identity)).toBe(false);
    const missing = { ...token }; delete (missing as Partial<typeof missing>).issued_at;
    expect(verifyRecoveryToken(missing, identity)).toBe(false);
    expect(verifyRecoveryToken(token, { ...identity, role: "journal" })).toBe(false);
    expect(verifyRecoveryToken(token, { ...identity, keyId: "other" })).toBe(false);
  });

  it("rejects unsafe numeric fields and invalid digest primitives at creation", async () => {
    await expect(createRecoveryToken({ ...fields, producer_seq: 1.5 }, signer)).rejects.toThrow(TypeError);
    await expect(createRecoveryToken({ ...fields, signer_manifest_generation: Number.MAX_SAFE_INTEGER + 1 }, signer)).rejects.toThrow(TypeError);
    await expect(createRecoveryToken({ ...fields, original_ack_digest: "A".repeat(64) }, signer)).rejects.toThrow(TypeError);
    await expect(createRecoveryToken({ ...fields, recovery_signer_epoch: "other" }, signer)).rejects.toThrow(TypeError);
  });
});
