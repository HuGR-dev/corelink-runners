import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { describe, expect, it } from "vitest";
import { createAck, type AckFields, type AsyncSigner, type PublicSigningIdentity } from "../src/acks.js";
import { canonicalHistoricalTerminalPayload, createHistoricalTerminal, verifyHistoricalTerminal } from "../src/terminal_ack.js";

const pair = generateKeyPairSync("rsa", { modulusLength: 3072 });
const identity: PublicSigningIdentity = {
  keyId: "ingest-key", epoch: "7", keyArn: "arn:aws:kms:region:acct:key/id",
  publicKeySpkiPem: pair.publicKey.export({ type: "spki", format: "pem" }).toString(), role: "ingest-ack",
};
const signer: AsyncSigner = { identity, async sign(bytes) {
  return sign(null, createHash("sha256").update(bytes).digest(), { key: pair.privateKey, padding: 6, saltLength: 32 }).toString("base64url");
} };
const fields: AckFields = {
  ack_version: "1", event_id: "event-1", producer_seq: 1, payload_digest: "a".repeat(64), source: "canary",
  service: "canary", application: "corelink", key_id: "lane", credential_epoch: "1",
  monitor_rearm_tuple_digest: "b".repeat(64), ingest_commit_id: "commit", committed_at: 1001,
  signer_key_id: identity.keyId, signer_epoch: identity.epoch,
};

describe("historical terminal ACK", () => {
  it("signs and verifies the nested ACK and terminal canonical tuple", async () => {
    const ack = await createAck(fields, signer);
    const terminal = await createHistoricalTerminal(ack, signer);
    expect(verifyHistoricalTerminal(terminal, fields, identity)).toBe(true);
    expect(canonicalHistoricalTerminalPayload({ ...terminal, signature: undefined } as never)).toBeInstanceOf(Uint8Array);
  });
  it("rejects mutations and signer identity substitution", async () => {
    const ack = await createAck(fields, signer);
    const terminal = await createHistoricalTerminal(ack, signer);
    expect(verifyHistoricalTerminal({ ...terminal, terminal_at: 1002 }, fields, identity)).toBe(false);
    expect(verifyHistoricalTerminal({ ...terminal, ack: { ...ack, committed_at: 1002 } }, fields, identity)).toBe(false);
    expect(verifyHistoricalTerminal({ ...terminal, extra: true }, fields, identity)).toBe(false);
    expect(verifyHistoricalTerminal({ ...terminal, signer_epoch: "old" }, fields, identity)).toBe(false);
  });
});
