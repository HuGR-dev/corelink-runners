import { describe, expect, it } from "vitest";
import { validRecovery, type AckRecovery, type AckToken, type TickEnvelope } from "../src/tick_outbox";

const envelope: TickEnvelope = { event_id: "event-1", producer_seq: 7, payload_digest: "payload", source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", monitor_rearm_tuple_digest: "tuple", occurred_at: 1_000, signature: "fixture" };
const ack: AckToken = { ack_version: "1", event_id: "event-1", producer_seq: 7, payload_digest: "payload", source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", monitor_rearm_tuple_digest: "tuple", ingest_commit_id: "commit", committed_at: 2, signer_key_id: "revoked", signer_epoch: "4", signature: "fixture" };
const recovery: AckRecovery = { recovery_version: "1", event_id: "event-1", producer_seq: 7, payload_digest: "payload", source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", original_monitor_rearm_tuple_digest: "tuple", ingest_commit_id: "commit", original_ack_digest: "original", revocation_record_digest: "revocation", signer_rotation_manifest_digest: "manifest", signer_manifest_generation: 2, signer_manifest_witness_root_digest: "root", current_monitor_rearm_tuple_digest: "tuple", recovery_signer_key_id: "recovery", recovery_signer_epoch: "5", issued_at: 2, signature: "fixture" };

describe("scheduled tick ACK recovery gate", () => {
  it("accepts only a trusted recovery token for the original head before its deadline", async () => {
    const verifier = { verify: async () => "valid" as const };
    expect(await validRecovery(recovery, envelope, ack, verifier, 60_999)).toBe(true);
    expect(await validRecovery({ ...recovery, ingest_commit_id: "second" }, envelope, ack, verifier, 2_000)).toBe(false);
    expect(await validRecovery(recovery, envelope, ack, verifier, 61_001)).toBe(false);
  });
});
