import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { describe, expect, it } from "vitest";
import { ACK_VERSION, AwsKmsSigner, canonicalAckPayload, createAck, verifyAck, verifyOrderedFields, type AckFields, type AsyncSigner, type PublicSigningIdentity } from "../src/acks.js";

const key = generateKeyPairSync("rsa", { modulusLength: 3072 });
const pem = key.publicKey.export({ type: "spki", format: "pem" }).toString();
const identity: PublicSigningIdentity = { keyId: "key-1", epoch: "7", keyArn: "arn:aws:kms:us-east-1:1:key/key-1", publicKeySpkiPem: pem, role: "ingest-ack" };
const fields: AckFields = { ack_version: ACK_VERSION, event_id: "event-1", producer_seq: 1, payload_digest: "a".repeat(64), source: "source", service: "service", application: "app", key_id: "credential-key", credential_epoch: "3", monitor_rearm_tuple_digest: "b".repeat(64), ingest_commit_id: "commit", committed_at: 1, signer_key_id: identity.keyId, signer_epoch: identity.epoch };
const signer: AsyncSigner = { identity, async sign(bytes) { return sign(null, createHash("sha256").update(bytes).digest(), { key: key.privateKey, padding: 6, saltLength: 32 }).toString("base64url"); } };

describe("ordered ACK token", () => {
  it("creates and verifies RSA-PSS ACKs", async () => { const token = await createAck(fields, signer); expect(verifyAck(token, fields, identity)).toBe(true); expect(canonicalAckPayload(fields)).toEqual(new TextEncoder().encode(JSON.stringify(Object.values(fields)))); });
  it("rejects field mutation, extras, wrong identity, and unsafe values", async () => {
    const token = await createAck(fields, signer);
    expect(verifyAck({ ...token, service: "other" }, fields, identity)).toBe(false);
    expect(verifyAck({ ...token, extra: 1 }, fields, identity)).toBe(false);
    expect(verifyAck(token, fields, { ...identity, epoch: "8" })).toBe(false);
    await expect(createAck({ ...fields, producer_seq: Number.MAX_SAFE_INTEGER + 1 }, signer)).rejects.toThrow();
  });
  it("uses KMS digest signing with the configured RSA key", async () => {
    const calls: unknown[] = [];
    const der = key.publicKey.export({ type: "spki", format: "der" });
    const client = { send: async (command: { input: Record<string, unknown> }) => {
      calls.push(command.input);
      if (calls.length === 1) return { KeySpec: "RSA_3072", KeyUsage: "SIGN_VERIFY", SigningAlgorithms: ["RSASSA_PSS_SHA_256"], PublicKey: der };
      return { KeyId: identity.keyArn, SigningAlgorithm: "RSASSA_PSS_SHA_256", Signature: sign(null, command.input.Message as Uint8Array, { key: key.privateKey, padding: 6, saltLength: 32 }) };
    } };
    const kms = new AwsKmsSigner({ client: client as never, identity });
    const signature = await kms.sign(canonicalAckPayload(fields));
    expect(signature).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(calls[0]).toEqual({ KeyId: identity.keyArn });
    expect(calls[1]).toMatchObject({ KeyId: identity.keyArn, MessageType: "DIGEST", SigningAlgorithm: "RSASSA_PSS_SHA_256" });
    expect((calls[1] as { Message: Uint8Array }).Message).toHaveLength(32);
  });
  it("verifies the ordered payload with a valid test fixture signature", () => {
    const bytes = canonicalAckPayload(fields);
    const signature = sign(null, createHash("sha256").update(bytes).digest(), { key: key.privateKey, padding: 6, saltLength: 32 }).toString("base64url");
    expect(verifyOrderedFields(bytes, signature, identity)).toBe(true);
    expect(verifyOrderedFields(bytes, `${signature.slice(0, -1)}x`, identity)).toBe(false);
  });
});
