import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { describe, expect, it } from "vitest";
import { createSignerManifest, signerManifestDigest, verifySignerManifest, type SignerManifestFields } from "../src/signer_manifest.js";

const digest = (value: string) => createHash("sha256").update(value).digest("hex");
const keys = generateKeyPairSync("rsa", { modulusLength: 3072 });
const identity = { keyId: "manifest-key", epoch: "7", keyArn: "arn:manifest", publicKeySpkiPem: keys.publicKey.export({ type: "spki", format: "pem" }).toString(), role: "manifest" as const };
const signer = { identity, sign: async (bytes: Uint8Array) => sign(null, createHash("sha256").update(bytes).digest(), { key: keys.privateKey, padding: 6, saltLength: 32 }).toString("base64url") };
const fields: SignerManifestFields = {
  manifest_version: "1", manifest_generation: 3, active_signer_key_id: "active", active_signer_epoch: "4", next_signer_key_id: "next", next_signer_epoch: "5",
  revoked_signer_set_digest: digest("revoked"), overlap_started_at: 1_700_000_000_000, overlap_expires_at: 1_700_000_100_000,
  recovery_custody_digest: digest("custody"), monitor_rearm_tuple_digest: digest("rearm"), previous_manifest_digest: "0".repeat(64),
  manifest_issuer_key_id: identity.keyId, manifest_issuer_epoch: identity.epoch, worm_log_id: "manifest-log", witness_checkpoint_sequence: 9,
  witness_previous_root_digest: "0".repeat(64), witness_root_digest: digest("witness"), issued_at: 1_700_000_000_001,
};

describe("signer manifest codec", () => {
  it("creates the exact ordered signed tuple and stable digest", async () => {
    const token = await createSignerManifest(fields, signer);
    expect(verifySignerManifest(token, identity)).toBe(true);
    expect(signerManifestDigest(token)).toBe(signerManifestDigest({ signature: token.signature, ...Object.fromEntries(Object.entries(fields).reverse()) } as never));
    expect(token.manifest_version).toBe("1");
  });
  it("rejects mutation of every signed field and the signature", async () => {
    const token = await createSignerManifest(fields, signer);
    const mutations: Record<keyof SignerManifestFields, unknown> = {
      manifest_version: "2", manifest_generation: 4, active_signer_key_id: "other-active", active_signer_epoch: "8", next_signer_key_id: "other-next", next_signer_epoch: "9",
      revoked_signer_set_digest: digest("r2"), overlap_started_at: fields.overlap_started_at + 1, overlap_expires_at: fields.overlap_expires_at + 1,
      recovery_custody_digest: digest("c2"), monitor_rearm_tuple_digest: digest("m2"), previous_manifest_digest: digest("p2"), manifest_issuer_key_id: "other-issuer", manifest_issuer_epoch: "8", worm_log_id: "other-log", witness_checkpoint_sequence: 10, witness_previous_root_digest: digest("wp2"), witness_root_digest: digest("w2"), issued_at: fields.issued_at + 1,
    };
    for (const [field, value] of Object.entries(mutations)) expect(verifySignerManifest({ ...token, [field]: value }, identity)).toBe(false);
    expect(signerManifestDigest({ ...token, signature: `${token.signature}x` })).not.toBe(signerManifestDigest(token));
  });
  it("rejects malformed fields, interval/genesis violations, and issuer mismatch", async () => {
    const token = await createSignerManifest(fields, signer);
    expect(verifySignerManifest({ ...token, extra: true }, identity)).toBe(false);
    expect(verifySignerManifest(Object.fromEntries(Object.entries(token).filter(([key]) => key !== "worm_log_id")), identity)).toBe(false);
    expect(verifySignerManifest({ ...token, manifest_generation: "3" }, identity)).toBe(false);
    expect(verifySignerManifest({ ...token, overlap_expires_at: fields.overlap_started_at }, identity)).toBe(false);
    expect(verifySignerManifest({ ...token, active_signer_key_id: fields.next_signer_key_id, active_signer_epoch: fields.next_signer_epoch }, identity)).toBe(false);
    expect(verifySignerManifest({ ...token, witness_root_digest: "0".repeat(64) }, identity)).toBe(false);
    expect(verifySignerManifest(token, { ...identity, role: "witness" })).toBe(false);
    expect(verifySignerManifest(token, { ...identity, keyId: "wrong" })).toBe(false);
    expect(verifySignerManifest(token, { ...identity, epoch: "wrong" })).toBe(false);
    await expect(createSignerManifest({ ...fields, manifest_issuer_epoch: "wrong" }, signer)).rejects.toThrow();
  });
});
