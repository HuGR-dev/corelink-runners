import { describe, expect, it } from "vitest";
import { classifyLifecycle, LifecycleRefusalError } from "../src/lifecycle.js";

const registration = {
  source: "canary-lifecycle", service: "svc", application: "app", keyId: "k",
  credentialEpoch: 1, secretArn: "arn", allowedKinds: ["canary-lifecycle"], intervalMs: 60_000,
  sourceVersion: "v1", authoritySourceId: "source-a", enabled: true,
};
const base = {
  source_id: "source-a", monotonic_seq: 1, transition_id: "transition-a", state: "healthy" as const,
  transition_at: 1_000, source_version: "v1", sampled_at: 1_000, nonce: "nonce-a",
};

describe("lifecycle classifier", () => {
  it("accepts a pinned fresh healthy authority", () => {
    expect(classifyLifecycle(base, registration, null, null, 2_000)).toEqual({
      status: "healthy", reason: "healthy", nonce: "nonce-a",
      authority: { source_id: "source-a", monotonic_seq: 1, transition_id: "transition-a", state: "healthy", transition_at: 1_000, source_version: "v1" },
    });
  });

  it.each([
    ["source_id_not_pinned", { source_id: "other" }],
    ["source_version_not_pinned", { source_version: "v2" }],
    ["nonce_replayed", { nonce: "old" }],
    ["transition_in_future", { transition_at: 2_001 }],
    ["sample_in_future", { sampled_at: 2_001 }],
  ] as const)("refuses %s", (reason, change) => {
    const payload = { ...base, ...change };
    const priorNonce = reason === "nonce_replayed" ? "old" : null;
    expect(() => classifyLifecycle(payload, registration, null, priorNonce, 2_000)).toThrow(LifecycleRefusalError);
    expect(() => classifyLifecycle(payload, registration, null, priorNonce, 2_000)).toThrow(reason);
  });

  it("rejects sequence regression and changed authority at an equal sequence", () => {
    const prior = classifyLifecycle(base, registration, null, null, 2_000).authority;
    expect(() => classifyLifecycle({ ...base, monotonic_seq: 0 }, registration, prior, "prior", 2_000)).toThrow("monotonic_seq_invalid");
    expect(() => classifyLifecycle({ ...base, monotonic_seq: 1 }, registration, { ...prior, monotonic_seq: 2 }, "prior", 2_000)).toThrow("sequence_regressed");
    expect(() => classifyLifecycle({ ...base, transition_id: "other" }, registration, prior, "prior", 2_000)).toThrow("same_sequence_changed");
  });

  it("accepts an equal byte-identical authority only with a new nonce", () => {
    const first = classifyLifecycle(base, registration, null, null, 2_000);
    expect(classifyLifecycle({ ...base, nonce: "nonce-b" }, registration, first.authority, first.nonce, 2_000)).toEqual({ ...first, nonce: "nonce-b" });
  });

  it("returns unknown for stale samples and never reuses healthy status", () => {
    const result = classifyLifecycle({ ...base, nonce: "nonce-b" }, registration, null, null, 122_001);
    expect(result.status).toBe("unknown");
    expect(result.reason).toBe("sample_stale");
  });

  it.each([
    ["monotonic_seq", { monotonic_seq: Number.MAX_SAFE_INTEGER + 1 }],
    ["sampled_at", { sampled_at: -1 }],
    ["nonce", { nonce: "" }],
  ] as const)("refuses invalid %s", (_label, change) => {
    expect(() => classifyLifecycle({ ...base, ...change }, registration, null, null, 2_000)).toThrow(LifecycleRefusalError);
  });
});
