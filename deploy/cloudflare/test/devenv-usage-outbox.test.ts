import { describe, expect, it } from "vitest";
import {
  freezeDevenvUsage,
  nextDevenvUsageAttempt,
  DEVENV_USAGE_PENDING_KEY,
  DEVENV_USAGE_SETTLED_KEY,
} from "../src/lib/devenv_usage_outbox.js";

describe("DevEnv usage outbox", () => {
  it("freezes one event and increments attempts without changing its payload", () => {
    const event = {
      tenant_id: "3560e213-1e23-4fd0-8871-7033c6052ebd",
      event_kind: "runner_vcpu_seconds",
      qty: 8,
      billing_period: "2026-07",
      region: "iad",
      source: "corelink-runners/spawn-worker",
      time_ms: 1000,
      idem_key: "a".repeat(64),
    };
    const pending = freezeDevenvUsage(event, "82597479-9350-4f0d-8871-7033c6052ebd", 1000);
    const retry = nextDevenvUsageAttempt(pending);
    expect(DEVENV_USAGE_PENDING_KEY).toBe("devenv:usage:pending");
    expect(DEVENV_USAGE_SETTLED_KEY).toBe("devenv:usage:settled");
    expect(pending.attempts).toBe(0);
    expect(retry.attempts).toBe(1);
    expect(retry.event).toBe(event);
    expect(retry.sessionUuid).toBe(pending.sessionUuid);
    expect(retry.createdAtMs).toBe(pending.createdAtMs);
  });
});
