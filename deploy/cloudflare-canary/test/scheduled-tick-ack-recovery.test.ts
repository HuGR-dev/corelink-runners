import { describe, expect, it } from "vitest";
import { validRecovery, type AckRecovery, type AckToken, type TickEnvelope } from "../src/tick_outbox";

const hex = "a".repeat(64);
const envelope: TickEnvelope = { event_id: "event-1", producer_seq: 7, payload_digest: hex, source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", monitor_rearm_tuple_digest: hex, occurred_at: 1_000, signature: "fixture" };
const head = { envelope, enqueuedAt: 1_000 };
const ack: AckToken = { ack_version: "1", event_id: "event-1", producer_seq: 7, payload_digest: hex, source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", monitor_rearm_tuple_digest: hex, ingest_commit_id: "commit", committed_at: 2, signer_key_id: "revoked", signer_epoch: "4", signature: "fixture" };
const recovery: AckRecovery = { recovery_version: "1", event_id: "event-1", producer_seq: 7, payload_digest: hex, source: "canary", service: "canary", application: "corelink", key_id: "lane", credential_epoch: "3", original_monitor_rearm_tuple_digest: hex, ingest_commit_id: "commit", original_ack_digest: hex, revocation_record_digest: hex, signer_rotation_manifest_digest: hex, signer_manifest_generation: 2, signer_manifest_witness_root_digest: hex, current_monitor_rearm_tuple_digest: hex, recovery_signer_key_id: "recovery", recovery_signer_epoch: "5", issued_at: 2, signature: "fixture" };

describe("scheduled tick ACK recovery gate", () => {
  it("accepts only a trusted recovery token for the original head before its deadline", async () => {
    const verifier = { verify: async () => "valid" as const };
    expect(await validRecovery(recovery, head, ack, verifier, 60_999)).toBe(false);
    expect(await validRecovery({ ...recovery, ingest_commit_id: "second" }, head, ack, verifier, 2_000)).toBe(false);
    expect(await validRecovery(recovery, head, ack, verifier, 61_001)).toBe(false);
  });
});
