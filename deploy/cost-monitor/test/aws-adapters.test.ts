import { createHash, generateKeyPairSync, sign as signDigest } from "node:crypto";
import { describe, expect, it } from "vitest";
import { AwsAdapterError, AwsSourceSecrets, LambdaWitnessClient } from "../src/aws_adapters.js";
import { canonicalCheckpointBytes, canonicalWitnessBytes, checkpointRootFor, type SignedCheckpoint, type WitnessReceipt } from "../src/evidence_log.js";
import type { PublicSigningIdentity } from "../src/acks.js";

const journalKeys = generateKeyPairSync("rsa", { modulusLength: 3072 });
const witnessKeys = generateKeyPairSync("rsa", { modulusLength: 3072 });
const identity = (role: "journal" | "witness"): PublicSigningIdentity => ({ keyId: `${role}-key`, epoch: "1", keyArn: `arn:aws:kms:us-east-1:${role === "journal" ? "111111111111" : "222222222222"}:key/${role}`, publicKeySpkiPem: (role === "journal" ? journalKeys.publicKey : witnessKeys.publicKey).export({ type: "spki", format: "pem" }).toString(), role });
const journalIdentity = identity("journal"); const witnessIdentity = identity("witness");
function sign(role: "journal" | "witness", bytes: Uint8Array): string { return signDigest(null, createHash("sha256").update(bytes).digest(), { key: role === "journal" ? journalKeys.privateKey : witnessKeys.privateKey, padding: 6, saltLength: 32 }).toString("base64url"); }
const checkpoint: SignedCheckpoint = { version: "1", logId: "log", sequence: 1, previousRoot: "0".repeat(64), recordDigest: "a".repeat(64), operationId: "op", trustedAtMs: 1_700_000_000_000, signerKeyId: journalIdentity.keyId, signerEpoch: journalIdentity.epoch, signature: "pending" };
checkpoint.signature = sign("journal", canonicalCheckpointBytes(checkpoint));
const checkpointRoot = checkpointRootFor(checkpoint);
function receipt(): WitnessReceipt { const value: WitnessReceipt = { version: "1", logId: "log", sequence: 1, checkpointRoot, previousWitnessRoot: "0".repeat(64), checkpointSignerKeyId: journalIdentity.keyId, checkpointSignerEpoch: journalIdentity.epoch, witnessKeyId: witnessIdentity.keyId, witnessEpoch: witnessIdentity.epoch, trustedAtMs: checkpoint.trustedAtMs, signature: "pending" }; value.signature = sign("witness", canonicalWitnessBytes(value)); return value; }
const registration = { source: "src", service: "svc", application: "app", keyId: "kid", credentialEpoch: "epoch", secretArn: "arn:aws:secretsmanager:us-east-1:111111111111:secret:src", secretVersionId: "version-1", allowedKinds: ["canary-tick"], intervalMs: 300000, sourceVersion: "1", authoritySourceId: null, enabled: true };

function secretClient(response: unknown) { const calls: any[] = []; return { calls, client: { send: async (command: any) => { calls.push(command); return response; } } }; }
function lambdaClient(response: unknown) { const calls: any[] = []; return { calls, client: { config: { region: async () => "us-east-1" }, send: async (command: any) => { calls.push(command); return response; } } }; }
function lambda(response: unknown) { return new LambdaWitnessClient({ client: response as any, functionArn: "arn:aws:lambda:us-east-1:222222222222:function:witness:7", logId: "log", journalIdentity, witnessIdentity, timeoutMs: 1000, maxResponseBytes: 262144 }); }

const encodedKey = Buffer.from("x".repeat(32)).toString("base64url");

describe("AWS source and witness adapters", () => {
  it("loads an exact versioned, scoped SecretString without SecretBinary", async () => {
    const x = secretClient({ ARN: registration.secretArn, VersionId: registration.secretVersionId, SecretString: JSON.stringify({ version: "1", source: "src", service: "svc", application: "app", key_id: "kid", credential_epoch: "epoch", hmac_key_base64url: encodedKey }) });
    await expect(new AwsSourceSecrets({ client: x.client as any, timeoutMs: 500 }).load(registration as any)).resolves.toEqual(new Uint8Array(Buffer.from("x".repeat(32))));
    expect(x.calls[0].input).toEqual({ SecretId: registration.secretArn, VersionId: registration.secretVersionId });
  });
  it("rejects mismatched metadata and malformed scope without leaking secret text", async () => {
    const x = secretClient({ ARN: "wrong", VersionId: registration.secretVersionId, SecretString: JSON.stringify({ version: "1", source: "src", service: "svc", application: "app", key_id: "kid", credential_epoch: "epoch", hmac_key_base64url: encodedKey }) });
    await expect(new AwsSourceSecrets({ client: x.client as any, timeoutMs: 500 }).load(registration as any)).rejects.toBeInstanceOf(AwsAdapterError);
    const y = secretClient({ ARN: registration.secretArn, VersionId: registration.secretVersionId, SecretString: JSON.stringify({ version: "1", source: "src", service: "svc", application: "app", key_id: "kid", credential_epoch: "epoch", hmac_key_base64url: "short" }) });
    await expect(new AwsSourceSecrets({ client: y.client as any, timeoutMs: 500 }).load(registration as any)).rejects.toBeInstanceOf(AwsAdapterError);
    const z = secretClient({ ARN: registration.secretArn, VersionId: registration.secretVersionId, SecretBinary: encodedKey });
    await expect(new AwsSourceSecrets({ client: z.client as any, timeoutMs: 500 }).load(registration as any)).rejects.toBeInstanceOf(AwsAdapterError);
  });
  it("invokes a qualified version and verifies a real RSA witness receipt", async () => {
    const expected = receipt(); const response = { StatusCode: 200, ExecutedVersion: "7", Payload: new TextEncoder().encode(JSON.stringify(expected)) };
    const x = lambdaClient(response); const client = lambda(x.client); const result = await client.accept(checkpoint);
    expect(result).toEqual(expected); expect(x.calls).toHaveLength(1); expect(x.calls[0].input.FunctionName).toContain(":7"); expect(JSON.parse(new TextDecoder().decode(x.calls[0].input.Payload))).toEqual({ action: "accept", checkpoint });
  });
  it("sends the explicit nonce head request and verifies signed genesis", async () => {
    const nonce = "b".repeat(64); const unsigned = { version: "1" as const, logId: "log", nonce, sequence: 0, checkpointRoot: "0".repeat(64), witnessRoot: "0".repeat(64), trustedAtMs: 1_700_000_000_000, signerKeyId: witnessIdentity.keyId, signerEpoch: witnessIdentity.epoch }; const head = { ...unsigned, signature: sign("witness", new TextEncoder().encode(JSON.stringify(Object.values(unsigned)))) };
    const x = lambdaClient({ StatusCode: 200, ExecutedVersion: "7", Payload: new TextEncoder().encode(JSON.stringify(head)) }); const result = await lambda(x.client).readHead(nonce); expect(result).toEqual(head); expect(JSON.parse(new TextDecoder().decode(x.calls[0].input.Payload))).toEqual({ action: "head", nonce, logId: "log" });
  });
  it("refuses invalid checkpoint locally and rejects bad invocation replies", async () => {
    const x = lambdaClient({ StatusCode: 200, ExecutedVersion: "7", Payload: new TextEncoder().encode(JSON.stringify(receipt())) }); const client = lambda(x.client);
    await expect(client.accept({ ...checkpoint, signature: "bad" })).rejects.toBeInstanceOf(AwsAdapterError); expect(x.calls).toHaveLength(0);
    const y = lambdaClient({ StatusCode: 200, ExecutedVersion: "8", Payload: new TextEncoder().encode(JSON.stringify(receipt())) });
    await expect(lambda(y.client).accept(checkpoint)).rejects.toBeInstanceOf(AwsAdapterError);
    expect(() => new LambdaWitnessClient({ client: y.client as any, functionArn: "arn:aws:lambda:us-east-1:333333333333:function:witness:7", logId: "log", journalIdentity, witnessIdentity, timeoutMs: 1000, maxResponseBytes: 100 })).toThrow(AwsAdapterError);
    const nonceClient = lambdaClient({ StatusCode: 200, ExecutedVersion: "7", Payload: new Uint8Array([123]) });
    await expect(lambda(nonceClient.client).readHead("bad")).rejects.toBeInstanceOf(AwsAdapterError); expect(nonceClient.calls).toHaveLength(0);
    const large = lambdaClient({ StatusCode: 200, ExecutedVersion: "7", Payload: new Uint8Array(10) });
    await expect(new LambdaWitnessClient({ client: large.client as any, functionArn: "arn:aws:lambda:us-east-1:222222222222:function:witness:7", logId: "log", journalIdentity, witnessIdentity, timeoutMs: 1000, maxResponseBytes: 4 }).readHead("b".repeat(64))).rejects.toBeInstanceOf(AwsAdapterError);
    const malformed = lambdaClient({ StatusCode: 200, ExecutedVersion: "7", Payload: new TextEncoder().encode("not-json") });
    await expect(lambda(malformed.client).readHead("b".repeat(64))).rejects.toBeInstanceOf(AwsAdapterError);
    const slow = { config: { region: async () => "us-east-1" }, send: () => new Promise(() => {}) };
    await expect(new LambdaWitnessClient({ client: slow as any, functionArn: "arn:aws:lambda:us-east-1:222222222222:function:witness:7", logId: "log", journalIdentity, witnessIdentity, timeoutMs: 10, maxResponseBytes: 100 }).readHead("b".repeat(64))).rejects.toMatchObject({ code: "TIMEOUT" });
    const delayed = { config: { region: async () => "us-east-1" }, send: () => new Promise((resolve) => setTimeout(() => resolve({ StatusCode: 200, ExecutedVersion: "7", Payload: new TextEncoder().encode(JSON.stringify(receipt())) }), 5)) };
    await expect(new LambdaWitnessClient({ client: delayed as any, functionArn: "arn:aws:lambda:us-east-1:222222222222:function:witness:7", logId: "log", journalIdentity, witnessIdentity, timeoutMs: 1, maxResponseBytes: 262144 }).accept(checkpoint)).rejects.toMatchObject({ code: "TIMEOUT" });
  });
});
