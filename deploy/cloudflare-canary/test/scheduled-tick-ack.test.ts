import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { validAck } from "../src/tick_outbox";
import { ackUnsigned, head, signedAck } from "./scheduled-tick-fixtures";

describe("scheduled tick ACK gate", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
  });
  afterEach(() => {
    vi.useRealTimers();
  });
  it("accepts a real Ed25519 signature over the frozen canonical ACK array", async () => {
    const { token, verifier } = await signedAck();
    expect(await validAck(token, head, verifier, 1_002)).toBe("valid");
  });
  it.each([
    ["payload_digest", { payload_digest: "b".repeat(64) }],
    ["commit", { ingest_commit_id: "other-commit" }],
    ["signer identity", { signer_key_id: "other-signer" }],
    ["signature", { signature: "AA" }],
  ])("rejects a mutated %s", async (_label, mutation) => {
    const { token, verifier } = await signedAck();
    expect(
      await validAck({ ...token, ...mutation }, head, verifier, 1_002),
    ).toBe("invalid");
  });
  it("refuses absent trust and a revoked signer", async () => {
    const { token, verifier } = await signedAck();
    expect(await validAck(token, head, undefined, 1_002)).toBe("invalid");
    expect(
      await validAck(
        token,
        head,
        { ...verifier, verify: async () => "revoked" },
        1_002,
      ),
    ).toBe("revoked");
  });
  it("rejects ACKs committed before the head and at/after its deadline", async () => {
    const { token, verifier } = await signedAck();
    const early = await signedAck({ committed_at: 999 });
    expect(await validAck(early.token, head, verifier, 1_002)).toBe("invalid");
    expect(await validAck(token, head, verifier, 61_000)).toBe("invalid");
    expect(ackUnsigned.committed_at).toBeGreaterThan(head.enqueuedAt);
  });
});
