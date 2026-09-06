import { describe, expect, it, vi } from "vitest";
import { createWitnessHandler, validateWitnessConfig, type WitnessConfig, type WitnessRuntimeDependencies } from "../src/witness_runtime.js";

const identity = (role: "journal" | "witness", account: string, keyId: string) => ({ keyId, epoch: "1", keyArn: `arn:aws:kms:us-east-1:${account}:key/${keyId}`, publicKeySpkiPem: "-----BEGIN PUBLIC KEY-----\nfixture\n-----END PUBLIC KEY-----", role });
const config = (): WitnessConfig => ({ version: "1", region: "us-east-1", monitorAccountId: "111111111111", verifierAccountId: "222222222222", stateTable: "state", stateNamespace: "monitor", journalBucket: "journal", journalPrefix: "witness", journalRetentionMs: 8 * 24 * 60 * 60 * 1000, allowedLogIds: ["log-a"], journalIdentity: identity("journal", "111111111111", "journal"), witnessIdentity: identity("witness", "222222222222", "witness"), trustedTime: { endpoint: "https://timestamp.digicert.com", rootPem: "root", intermediatePem: "intermediate", crlUrls: ["https://crl.example/a", "https://crl.example/b"], minimumTimeMs: 1, maxAdvanceMs: 60_000, timeoutMs: 5_000, maxResponseBytes: 262144, opensslPath: "/usr/bin/openssl" } });
const context = { invokedFunctionArn: "arn:aws:lambda:us-east-1:111111111111:function:witness:7" };

describe("witness lambda runtime", () => {
  it("validates exact configuration and identity domains", () => { const valid = validateWitnessConfig(config()); expect(valid.allowedLogIds).toEqual(["log-a"]); expect(() => validateWitnessConfig({ ...config(), witnessIdentity: identity("witness", "111111111111", "witness") })).toThrow(); expect(() => validateWitnessConfig({ ...config(), extra: true })).toThrow(); });
  it("routes accepted requests only to an allowlisted witness", async () => {
    const accept = vi.fn(async (checkpoint: unknown) => ({ checkpoint })); const readHead = vi.fn(async (nonce: string) => ({ nonce }));
    const dependencies = { witnesses: new Map([["log-a", { accept, readHead }]]) } as unknown as WitnessRuntimeDependencies;
    const run = createWitnessHandler(config(), dependencies); const checkpoint = { logId: "log-a", sequence: 1 };
    await expect(run({ action: "accept", checkpoint }, context)).resolves.toEqual({ checkpoint }); await expect(run({ action: "head", logId: "log-a", nonce: "n" }, context)).resolves.toEqual({ nonce: "n" }); expect(accept).toHaveBeenCalledTimes(1); expect(readHead).toHaveBeenCalledTimes(1);
  });
  it("rejects malformed, unknown-log, unqualified, and extra-field requests", async () => {
    const accept = vi.fn(); const dependencies = { witnesses: new Map([["log-a", { accept, readHead: vi.fn() }]]) } as unknown as WitnessRuntimeDependencies; const run = createWitnessHandler(config(), dependencies);
    for (const event of [{ action: "accept" }, { action: "head", logId: "other", nonce: "n" }, { action: "accept", checkpoint: { logId: "log-a" }, extra: 1 }]) await expect(run(event, context)).rejects.toThrow("witness request failed");
    await expect(run({ action: "accept", checkpoint: { logId: "log-a" } }, { invokedFunctionArn: "arn:aws:lambda:us-east-1:111111111111:function:witness:$LATEST" })).rejects.toThrow("witness request failed"); expect(accept).not.toHaveBeenCalled();
  });
});
