import { describe, it, expect } from "vitest";
import {
  evaluate,
  applyCooldown,
  positiveDelta,
  counterReset,
  inBusinessWindow,
  type RulesConfig,
  type Alert,
} from "../src/rules";
import { formatAlertEmail } from "../src/notify";
import type { Snapshot } from "../src/types";

// ── snapshot builders ──────────────────────────────────────────────────────────

function surface(counters: Record<string, number> = {}, status = 200, reachable = true) {
  return { reachable, status, counters };
}

function snap(over: Partial<Snapshot> = {}): Snapshot {
  return {
    at: 1_000_000,
    fabric: surface(),
    fabricHealth: { reachable: true, status: 200 },
    spawn: surface(),
    ...over,
  };
}

const cfg: RulesConfig = { now: 1_000_000, stalenessMs: 0 };

function keys(alerts: Alert[]): string[] {
  return alerts.map((a) => a.key).sort();
}

// ── counter helpers ─────────────────────────────────────────────────────────────

describe("positiveDelta", () => {
  it("returns the positive rise", () => {
    expect(positiveDelta({ a: 3 }, { a: 5 }, "a")).toBe(2);
  });
  it("clamps a decrease (reset) to 0", () => {
    expect(positiveDelta({ a: 5 }, { a: 1 }, "a")).toBe(0);
  });
  it("returns 0 with no baseline (first run / added counter)", () => {
    expect(positiveDelta(undefined, { a: 5 }, "a")).toBe(0);
    expect(positiveDelta({}, { a: 5 }, "a")).toBe(0);
  });
  it("returns 0 for an absent current counter", () => {
    expect(positiveDelta({ a: 5 }, {}, "a")).toBe(0);
  });
});

describe("counterReset", () => {
  it("detects a backwards counter", () => {
    expect(counterReset({ a: 5, b: 2 }, { a: 0, b: 3 })).toBe(true);
  });
  it("is false when all counters hold or rise", () => {
    expect(counterReset({ a: 5 }, { a: 5, b: 1 })).toBe(false);
  });
  it("is false without both snapshots", () => {
    expect(counterReset(undefined, { a: 1 })).toBe(false);
  });
});

describe("inBusinessWindow", () => {
  const at13 = Date.UTC(2026, 0, 1, 13, 0, 0);
  const at2 = Date.UTC(2026, 0, 1, 2, 0, 0);
  it("is always true with no window", () => {
    expect(inBusinessWindow(at2, { now: at2, stalenessMs: 0 })).toBe(true);
  });
  it("respects a normal window", () => {
    const c = { now: at13, stalenessMs: 1, businessHoursUtc: { start: 13, end: 23 } };
    expect(inBusinessWindow(at13, c)).toBe(true);
    expect(inBusinessWindow(at2, c)).toBe(false);
  });
  it("supports a wrap-around window (22..6)", () => {
    const c = { now: at2, stalenessMs: 1, businessHoursUtc: { start: 22, end: 6 } };
    expect(inBusinessWindow(at2, c)).toBe(true);
    expect(inBusinessWindow(at13, c)).toBe(false);
  });
});

// ── health rule ─────────────────────────────────────────────────────────────────

describe("health rule", () => {
  it("fires CRITICAL when health is unreachable", () => {
    const cur = snap({ fabricHealth: { reachable: false, status: 0 } });
    const a = evaluate(null, cur, cfg).alerts.find((x) => x.key === "health:fabric");
    expect(a?.severity).toBe("critical");
  });
  it("fires CRITICAL when health is non-200", () => {
    const cur = snap({ fabricHealth: { reachable: true, status: 503 } });
    const a = evaluate(null, cur, cfg).alerts.find((x) => x.key === "health:fabric");
    expect(a?.severity).toBe("critical");
    expect(a?.detail).toContain("503");
  });
  it("does NOT fire when health is 200", () => {
    const a = evaluate(null, snap(), cfg).alerts.find((x) => x.key === "health:fabric");
    expect(a).toBeUndefined();
  });
});

// ── surface reachability / auth posture ─────────────────────────────────────────

describe("surface posture rule", () => {
  it("fires CRITICAL when a counter surface is unreachable", () => {
    const cur = snap({ spawn: surface({}, 0, false) });
    const a = evaluate(null, cur, cfg).alerts.find((x) => x.key === "unreachable:spawn");
    expect(a?.severity).toBe("critical");
  });
  it("stays SILENT on 404 (not armed yet)", () => {
    const cur = snap({ fabric: surface({}, 404), spawn: surface({}, 404) });
    const posture = evaluate(null, cur, cfg).alerts.filter((x) => x.key.startsWith("auth:") || x.key.startsWith("status:") || x.key.startsWith("unreachable:"));
    expect(posture).toHaveLength(0);
  });
  it("fires WARN on 401 (key mismatch)", () => {
    const cur = snap({ fabric: surface({}, 401) });
    const a = evaluate(null, cur, cfg).alerts.find((x) => x.key === "auth:fabric");
    expect(a?.severity).toBe("warn");
  });
  it("fires WARN on an unexpected status", () => {
    const cur = snap({ spawn: surface({}, 500) });
    const a = evaluate(null, cur, cfg).alerts.find((x) => x.key === "status:spawn");
    expect(a?.severity).toBe("warn");
    expect(a?.title).toContain("500");
  });
});

// ── delta rules ─────────────────────────────────────────────────────────────────

describe("delta rules", () => {
  it("mint_failures rise ⇒ CRITICAL", () => {
    const prev = snap({ fabric: surface({ mint_failures: 0 }) });
    const cur = snap({ fabric: surface({ mint_failures: 2 }) });
    const a = evaluate(prev, cur, cfg).alerts.find((x) => x.key === "delta:fabric:mint_failures");
    expect(a?.severity).toBe("critical");
    expect(a?.title).toContain("+2");
  });
  it("spawn_failed rise ⇒ CRITICAL", () => {
    const prev = snap({ spawn: surface({ spawn_failed: 1 }) });
    const cur = snap({ spawn: surface({ spawn_failed: 4 }) });
    const a = evaluate(prev, cur, cfg).alerts.find((x) => x.key === "delta:spawn:spawn_failed");
    expect(a?.severity).toBe("critical");
    expect(a?.title).toContain("+3");
  });
  it("provision_capacity_503 rise ⇒ WARN", () => {
    const prev = snap({ fabric: surface({ provision_capacity_503: 5 }) });
    const cur = snap({ fabric: surface({ provision_capacity_503: 6 }) });
    const a = evaluate(prev, cur, cfg).alerts.find((x) => x.key === "delta:fabric:provision_capacity_503");
    expect(a?.severity).toBe("warn");
  });
  it("revoke_failures rise ⇒ WARN", () => {
    const prev = snap({ fabric: surface({ revoke_failures: 0 }) });
    const cur = snap({ fabric: surface({ revoke_failures: 1 }) });
    const a = evaluate(prev, cur, cfg).alerts.find((x) => x.key === "delta:fabric:revoke_failures");
    expect(a?.severity).toBe("warn");
  });
  it("does NOT fire when a counter holds steady", () => {
    const prev = snap({ fabric: surface({ mint_failures: 3 }) });
    const cur = snap({ fabric: surface({ mint_failures: 3 }) });
    const a = evaluate(prev, cur, cfg).alerts.find((x) => x.key === "delta:fabric:mint_failures");
    expect(a).toBeUndefined();
  });
  it("does NOT fire on the first run (no baseline)", () => {
    const cur = snap({ fabric: surface({ mint_failures: 9 }) });
    const a = evaluate(null, cur, cfg).alerts.find((x) => x.key === "delta:fabric:mint_failures");
    expect(a).toBeUndefined();
  });
});

// ── reset detection ─────────────────────────────────────────────────────────────

describe("reset rule", () => {
  it("fires INFO when fabricd counters went backwards", () => {
    const prev = snap({ fabric: surface({ leases_acquired: 10, mint_failures: 2 }) });
    const cur = snap({ fabric: surface({ leases_acquired: 0, mint_failures: 0 }) });
    const alerts = evaluate(prev, cur, cfg).alerts;
    const reset = alerts.find((x) => x.key === "reset:fabric");
    expect(reset?.severity).toBe("info");
    // And crucially, the backwards mint_failures does NOT masquerade as a delta alert.
    expect(alerts.find((x) => x.key === "delta:fabric:mint_failures")).toBeUndefined();
  });
  it("does not fire the reset when a surface was not 200 both sides", () => {
    const prev = snap({ fabric: surface({ leases_acquired: 10 }, 404) });
    const cur = snap({ fabric: surface({ leases_acquired: 0 }) });
    expect(evaluate(prev, cur, cfg).alerts.find((x) => x.key === "reset:fabric")).toBeUndefined();
  });
});

// ── staleness ───────────────────────────────────────────────────────────────────

describe("staleness rule", () => {
  const HOUR = 3_600_000;
  it("is OFF when stalenessMs=0", () => {
    const prev = snap({ lastCompletionAt: 0 });
    const cur = snap({ at: 100 * HOUR });
    const a = evaluate(prev, cur, { now: 100 * HOUR, stalenessMs: 0 }).alerts;
    expect(a.find((x) => x.key === "staleness:no-completions")).toBeUndefined();
  });
  it("fires WARN when no completions past the window", () => {
    const prev = snap({ at: 0, lastCompletionAt: 0 });
    const cur = snap({ at: 5 * HOUR, fabric: surface({ leases_closed: 7 }), spawn: surface({ webhook_job_completed: 3 }) });
    // prev had no completion counters, cur has them but == baseline (no prev value ⇒ no increase)
    const res = evaluate(prev, cur, { now: 5 * HOUR, stalenessMs: 4 * HOUR });
    expect(res.alerts.find((x) => x.key === "staleness:no-completions")?.severity).toBe("warn");
  });
  it("does NOT fire when a completion happened this cycle", () => {
    const prev = snap({ at: 0, lastCompletionAt: 0, fabric: surface({ leases_closed: 2 }) });
    const cur = snap({ at: 5 * HOUR, fabric: surface({ leases_closed: 5 }) });
    const res = evaluate(prev, cur, { now: 5 * HOUR, stalenessMs: 4 * HOUR });
    expect(res.alerts.find((x) => x.key === "staleness:no-completions")).toBeUndefined();
    expect(res.lastCompletionAt).toBe(5 * HOUR); // advanced to now
  });
  it("does NOT fire without a completion baseline (fresh canary)", () => {
    const cur = snap({ at: 5 * HOUR });
    const res = evaluate(null, cur, { now: 5 * HOUR, stalenessMs: 1 });
    expect(res.alerts.find((x) => x.key === "staleness:no-completions")).toBeUndefined();
    expect(res.lastCompletionAt).toBe(5 * HOUR); // seeded to now
  });
  it("is gated by the business window", () => {
    const at2utc = Date.UTC(2026, 0, 1, 2, 0, 0);
    const prev = snap({ at: 0, lastCompletionAt: 0 });
    const cur = snap({ at: at2utc });
    const res = evaluate(prev, cur, {
      now: at2utc,
      stalenessMs: 1,
      businessHoursUtc: { start: 13, end: 23 },
    });
    expect(res.alerts.find((x) => x.key === "staleness:no-completions")).toBeUndefined();
  });
});

// ── a clean run produces nothing ─────────────────────────────────────────────────

describe("healthy steady-state", () => {
  it("produces no alerts", () => {
    const prev = snap({ fabric: surface({ mint_failures: 1, leases_acquired: 4 }), spawn: surface({ spawn_failed: 0 }) });
    const cur = snap({ fabric: surface({ mint_failures: 1, leases_acquired: 6 }), spawn: surface({ spawn_failed: 0 }) });
    expect(evaluate(prev, cur, cfg).alerts).toHaveLength(0);
  });
});

// ── cooldown ─────────────────────────────────────────────────────────────────────

describe("applyCooldown", () => {
  const a1: Alert = { key: "health:fabric", severity: "critical", title: "t", detail: "d" };
  const a2: Alert = { key: "delta:fabric:mint_failures", severity: "critical", title: "t", detail: "d" };

  it("sends everything on an empty cooldown map + records timestamps", () => {
    const r = applyCooldown([a1, a2], {}, 1000, 60_000);
    expect(keys(r.toSend)).toEqual(keys([a1, a2]));
    expect(r.cooldowns["health:fabric"]).toBe(1000);
    expect(r.cooldowns["delta:fabric:mint_failures"]).toBe(1000);
  });

  it("suppresses an alert still within cooldown", () => {
    const r = applyCooldown([a1], { "health:fabric": 1000 }, 1000 + 30_000, 60_000);
    expect(r.toSend).toHaveLength(0);
    expect(r.cooldowns["health:fabric"]).toBe(1000); // unchanged
  });

  it("re-sends after the cooldown elapses", () => {
    const r = applyCooldown([a1], { "health:fabric": 1000 }, 1000 + 61_000, 60_000);
    expect(r.toSend).toHaveLength(1);
    expect(r.cooldowns["health:fabric"]).toBe(1000 + 61_000);
  });

  it("sends a not-yet-seen key while another is cooling", () => {
    const r = applyCooldown([a1, a2], { "health:fabric": 1000 }, 1000 + 10_000, 60_000);
    expect(keys(r.toSend)).toEqual(["delta:fabric:mint_failures"]);
  });
});

// ── email formatting ─────────────────────────────────────────────────────────────

describe("formatAlertEmail", () => {
  it("prefixes the subject with the top severity and counts alerts", () => {
    const alerts: Alert[] = [
      { key: "a", severity: "info", title: "i", detail: "d" },
      { key: "b", severity: "critical", title: "c", detail: "d" },
    ];
    const { subject, text } = formatAlertEmail(alerts, 0);
    expect(subject).toContain("CRITICAL");
    expect(subject).toContain("2 alerts");
    expect(text).toContain("[CRITICAL] c");
    expect(text).toContain("[INFO] i");
  });
  it("singularizes one alert", () => {
    const { subject } = formatAlertEmail([{ key: "a", severity: "warn", title: "w", detail: "d" }], 0);
    expect(subject).toContain("WARN");
    expect(subject).toContain("1 alert");
    expect(subject).not.toContain("1 alerts");
  });
});
