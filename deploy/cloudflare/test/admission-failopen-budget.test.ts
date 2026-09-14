// ─────────────────────────────────────────────────────────────────────────────
// Legacy pure fail-open decision helpers. Production A3.16 coverage is in
// admission-budget-authority.test.ts and exercises the ContainmentDO authority.
// ─────────────────────────────────────────────────────────────────────────────
//
// `acquireConcurrencySlot` admits when the slot Durable Object THROWS, so an infra
// hiccup never blocks a legitimate job. Before this budget existed, that yes was
// unconditional: for as long as the DO kept throwing, NOTHING enforced the tenant's
// entitlement or FLEET_MAX_CONCURRENCY, and every arrival was admitted. That is the
// one shape that turns "flat concurrency, unlimited minutes" into unbounded spend —
// the audit's RH3, confirmed at HEAD.
//
// ⚠️ THE LOAD-BEARING CONTRACT HERE is that the budget separates a HICCUP from an
// OUTAGE, in both directions. A handful of errors must still admit — a budget that
// refuses the first error would block legitimate CI on noise, which is the failure
// the fail-open was written to prevent. A sustained stream must stop admitting —
// during an outage no ceiling is being enforced by anyone.
//
// If a future change makes the budget unbounded again, cell 3 goes red. If it makes
// the first error refuse, cell 1 goes red.

import { describe, it, expect } from "vitest";
import {
  decideFailOpenAdmission,
  failOpenWindowKey,
  FAILOPEN_MAX_PER_WINDOW,
  FAILOPEN_WINDOW_S,
} from "../src/lib";

describe("decideFailOpenAdmission", () => {
  it("cell 1 — a hiccup is absorbed: the first error still admits", () => {
    expect(decideFailOpenAdmission(0)).toEqual({ admitted: true });
  });

  it("cell 2 — admits every count strictly under the budget", () => {
    for (let n = 0; n < FAILOPEN_MAX_PER_WINDOW; n++) {
      expect(decideFailOpenAdmission(n).admitted).toBe(true);
    }
  });

  it("cell 3 — an OUTAGE stops admitting once the budget is spent", () => {
    const v = decideFailOpenAdmission(FAILOPEN_MAX_PER_WINDOW);
    expect(v.admitted).toBe(false);
    expect(v.reason).toBe("slot_failopen_budget_exhausted");
  });

  it("cell 4 — stays refused well past the budget (no wrap, no reset)", () => {
    expect(decideFailOpenAdmission(FAILOPEN_MAX_PER_WINDOW * 100).admitted).toBe(false);
  });

  // An unreadable counter is NOT zero. If the slot DO and the counter store are BOTH
  // unavailable, nothing anywhere is bounding the fleet, and admitting into that is
  // precisely the unbounded case this exists to close. Two independent stores failing
  // at once is an outage, not a hiccup.
  it("cell 5 — an UNREADABLE counter refuses; unknown is never treated as zero", () => {
    const v = decideFailOpenAdmission(null);
    expect(v.admitted).toBe(false);
    expect(v.reason).toBe("slot_failopen_budget_unreadable");
  });

  // A zero count is only the pure helper's fresh-window input. Missing authority is
  // covered by the production caller tests and refuses closed.
  it("cell 5b — a zero count is a fresh window, not an unknown: it admits", () => {
    expect(decideFailOpenAdmission(0)).toEqual({ admitted: true });
    expect(decideFailOpenAdmission(null).admitted).toBe(false);
  });

  it("cell 6 — the budget is explicit and small, so an outage cannot run a fleet", () => {
    expect(FAILOPEN_MAX_PER_WINDOW).toBeGreaterThan(0);
    expect(FAILOPEN_MAX_PER_WINDOW).toBeLessThanOrEqual(10);
  });
});

describe("failOpenWindowKey", () => {
  it("cell 7 — one bucket per window, and the bucket advances with time", () => {
    const t = 1_800_000_000_000;
    expect(failOpenWindowKey(t)).toBe(failOpenWindowKey(t + FAILOPEN_WINDOW_S * 1000 - 1));
    expect(failOpenWindowKey(t)).not.toBe(failOpenWindowKey(t + FAILOPEN_WINDOW_S * 1000));
  });

  it("cell 8 — the key is namespaced so it cannot collide with sbox:/spawn:/usage:", () => {
    expect(failOpenWindowKey(1_800_000_000_000)).toMatch(/^failopen:\d+$/);
  });
});
