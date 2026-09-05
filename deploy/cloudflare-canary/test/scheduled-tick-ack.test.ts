import { describe, expect, it } from "vitest";
import { validAck, type AckToken, type TickEnvelope } from "../src/tick_outbox";

const envelope: TickEnvelope = { event_id: "event-1", producer_seq: 7, payload_digest: "payload", source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", monitor_rearm_tuple_digest: "tuple", occurred_at: 1, signature: "fixture" };
const ack: AckToken = { ack_version: "1", event_id: "event-1", producer_seq: 7, payload_digest: "payload", source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", monitor_rearm_tuple_digest: "tuple", ingest_commit_id: "commit", committed_at: 2, signer_key_id: "ack-signer", signer_epoch: "4", signature: "fixture" };
const trusted = { verify: async () => "valid" as const };

describe("scheduled tick ACK gate", () => {
  it("requires every frozen ACK field and an independently injected trusted verifier", async () => {
    expect(await validAck(ack, envelope, trusted)).toBe("valid");
    expect(await validAck({ ...ack, payload_digest: "other" }, envelope, trusted)).toBe("invalid");
    expect(await validAck(ack, envelope, undefined)).toBe("invalid");
  });
  it("rejects a signer that is cryptographically current but revoked", async () => {
    expect(await validAck(ack, envelope, { verify: async () => "revoked" })).toBe("revoked");
  });
});
