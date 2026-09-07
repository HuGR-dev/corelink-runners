import { describe, expect, it, vi } from "vitest";
import { DynamoSigningCustody, type CustodyRecord } from "../src/signer_registry.js";
import { MemoryStateStore } from "../src/state.js";

const digest = "a".repeat(64);
const receipt = { journalReceipt: { versionId: "receipt" } } as any;
const identity = { keyId: "manifest-key", epoch: "7", keyArn: "arn:aws:kms:us-east-1:123456789012:key/manifest", publicKeySpkiPem: "pem", role: "manifest" as const };

describe("signer custody authority", () => {
  it("reads and resolves the audited Dynamo custody record including its key ARN", async () => {
    const record: CustodyRecord = { version: "1", digest, keyId: identity.keyId, epoch: identity.epoch, role: "manifest", keyArn: identity.keyArn, receipt };
    const payload = { type: "CUSTODY_RECORD", digest, keyId: identity.keyId, epoch: identity.epoch, role: "manifest", keyArn: identity.keyArn, custodyVersion: "1" };
    const audit = { verify: vi.fn(async () => undefined), read: vi.fn(async () => ({ payload })) };
    const store = new MemoryStateStore();
    const custody = new DynamoSigningCustody({ store, audit: audit as never, kms: {} as never, namespace: "ns", identities: { resolve: async () => identity } });
    await custody.install(record);
    await expect(custody.read(digest)).resolves.toEqual(record);
    await expect(custody.resolve(await custody.read(digest))).resolves.toMatchObject({ identity: { keyArn: identity.keyArn, keyId: identity.keyId, epoch: identity.epoch, role: "manifest" } });
    expect(audit.read).toHaveBeenCalled();
  });
});
