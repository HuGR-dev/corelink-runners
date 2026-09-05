import { describe, expect, it } from "vitest";
import { buildDevenvUsageEvent, type DevenvUsageInput } from "../src/lib/devenv_usage.js";

const tenantId = "3560e213-1e23-4fd0-8871-7033c6052ebd";
const sessionId = "82597479-9350-4f0d-8871-7033c6052ebd";
const base: DevenvUsageInput = {
  tenantId,
  sessionId,
  tier: "standard-4",
  startedAtMs: Date.parse("2026-06-30T23:59:59.900Z"),
  completedAtMs: Date.parse("2026-07-01T00:00:02.100Z"),
  region: "iad",
};

describe("buildDevenvUsageEvent", () => {
  it("uses measured floored wall seconds, tier vCPUs, completion month, and namespaced key", async () => {
    const result = await buildDevenvUsageEvent(base);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.event).toMatchObject({
      tenant_id: tenantId,
      event_kind: "runner_vcpu_seconds",
      qty: 8,
      billing_period: "2026-07",
      region: "iad",
      time_ms: base.completedAtMs,
    });
    expect(result.event.idem_key).toMatch(/^[0-9a-f]{64}$/);
    expect(result.event.source).toBe("corelink-runners/spawn-worker");
  });

  it("does not apply a fabricated 30-second minimum", async () => {
    const result = await buildDevenvUsageEvent({
      ...base,
      startedAtMs: 10_000,
      completedAtMs: 10_001,
      tier: "power-8",
    });
    expect(result).toMatchObject({ ok: true, event: { qty: 0 } });
  });

  it("returns typed failures for malformed identity, tier, clocks, and region", async () => {
    const cases: Array<[keyof DevenvUsageInput, unknown, string]> = [
      ["tenantId", "acme", "invalid_tenant_uuid"],
      ["sessionId", "00000000-0000-0000-0000-000000000000", "invalid_session_uuid"],
      ["tier", "standard-99", "invalid_tier"],
      ["tier", "__proto__", "invalid_tier"],
      ["startedAtMs", Number.NaN, "invalid_started_at"],
      ["completedAtMs", Number.MAX_SAFE_INTEGER + 1, "invalid_completed_at"],
      ["region", "IAD", "invalid_region"],
    ];
    for (const [field, value, code] of cases) {
      const result = await buildDevenvUsageEvent({ ...base, [field]: value } as DevenvUsageInput);
      expect(result).toMatchObject({ ok: false, error: { code, field } });
    }
  });

  it("rejects reversed timestamps and accepts an exact boundary", async () => {
    const reversed = await buildDevenvUsageEvent({ ...base, startedAtMs: 2, completedAtMs: 1 });
    expect(reversed).toMatchObject({ ok: false, error: { code: "completed_before_started" } });
    const boundary = await buildDevenvUsageEvent({ ...base, startedAtMs: 7, completedAtMs: 7 });
    expect(boundary).toMatchObject({ ok: true, event: { qty: 0 } });
  });

  it("keeps DevEnv keys disjoint from the decimal GitHub job namespace", async () => {
    const result = await buildDevenvUsageEvent(base);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.event.idem_key).not.toBe("82597479935");
  });
});
