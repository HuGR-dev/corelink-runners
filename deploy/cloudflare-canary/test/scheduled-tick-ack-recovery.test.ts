import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { validRecovery } from "../src/tick_outbox";
import { head, signedAck, signedRecovery } from "./scheduled-tick-fixtures";

describe("scheduled tick ACK recovery gate", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
  });
  afterEach(() => {
    vi.useRealTimers();
  });
  it("accepts a real recovery signature bound to SHA-256(canonical ACK + signature)", async () => {
    const original = await signedAck({ signer_key_id: "revoked" });
    const recovery = await signedRecovery(original.token);
    expect(
      await validRecovery(
        recovery.token,
        head,
        original.token,
        recovery.verifier,
      ),
    ).toBe(true);
  });
  it.each([
    ["commit", { ingest_commit_id: "second" }],
    ["original ACK digest", { original_ack_digest: "b".repeat(64) }],
    ["revocation record", { revocation_record_digest: "b".repeat(64) }],
    ["manifest generation", { signer_manifest_generation: 3 }],
    ["current tuple", { current_monitor_rearm_tuple_digest: "b".repeat(64) }],
  ])("rejects a recovery mutation: %s", async (_label, mutation) => {
    const original = await signedAck({ signer_key_id: "revoked" });
    const recovery = await signedRecovery(original.token);
    expect(
      await validRecovery(
        { ...recovery.token, ...mutation },
        head,
        original.token,
        recovery.verifier,
      ),
    ).toBe(false);
  });
  it("rejects a late recovery, wrong verifier, and altered original ACK", async () => {
    const original = await signedAck({ signer_key_id: "revoked" });
    const recovery = await signedRecovery(original.token);
    expect(
      await validRecovery(recovery.token, head, original.token, undefined),
    ).toBe(false);
    const altered = { ...original.token, ingest_commit_id: "other" };
    expect(
      await validRecovery(recovery.token, head, altered, recovery.verifier),
    ).toBe(false);
  });
});
