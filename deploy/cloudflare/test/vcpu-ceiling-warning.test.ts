/**
 * Near-ceiling warning — the customer must learn they are approaching the
 * included `max_vcpu_h` allowance BEFORE the invoice tells them.
 *
 * Overage is priced at 3× COGS, so crossing the line is expensive. For an SMB
 * self-serve buyer, a surprise invoice is a churn event rather than an upgrade
 * conversation — which is the entire reason this exists.
 *
 * These tests cover the PURE decision (`vcpuWarningStep`). The KV plumbing in
 * index.ts is a thin wrapper over it; the arithmetic and the once-per-threshold
 * discipline are what can silently go wrong.
 */
import { describe, it, expect } from "vitest";
import {
  vcpuWarningStep,
  vcpuCeilingKey,
  vcpuUsageKey,
  vcpuWarnedKey,
  VCPU_WARN_THRESHOLDS,
} from "../src/lib";

const H = 3600; // seconds per vCPU-hour

describe("vcpuWarningStep — the near-ceiling decision", () => {
  it("says nothing below the first threshold", () => {
    // 79 of 100 vCPU-h. Warning here would train the customer to ignore it.
    const r = vcpuWarningStep(79 * H, 100, new Set());
    expect(r.crossed).toBeNull();
    expect(r.consumedVcpuH).toBeCloseTo(79);
    expect(r.fraction).toBeCloseTo(0.79);
  });

  it("warns at exactly 80%, the boundary itself", () => {
    // `>=`, not `>`: landing exactly on the threshold must warn, or a tenant
    // whose usage lands cleanly on the line is never told.
    const r = vcpuWarningStep(80 * H, 100, new Set());
    expect(r.crossed).toBe(0.8);
  });

  it("warns at exactly 100% — being AT the allowance is already spending it", () => {
    const r = vcpuWarningStep(100 * H, 100, new Set([0.8]));
    expect(r.crossed).toBe(1.0);
    expect(r.fraction).toBeCloseTo(1.0);
  });

  it("reports only the HIGHEST threshold when one job jumps past both", () => {
    // A single long job can take a tenant from 0% to 150%. They need to hear
    // "you are over", not "you are at 80%" followed by "you are over" — the
    // actionable state, not the history.
    const r = vcpuWarningStep(150 * H, 100, new Set());
    expect(r.crossed).toBe(1.0);
  });

  it("stays silent once a threshold has already been announced", () => {
    // A tenant parked at 85% runs a thousand jobs. An alert that repeats on
    // every one is an alert that gets filtered — the same as no alert at all.
    const r = vcpuWarningStep(85 * H, 100, new Set([0.8]));
    expect(r.crossed).toBeNull();
  });

  it("still announces 100% to a tenant already warned at 80%", () => {
    // The escalation must survive the dedup: silence here is the exact case
    // this feature exists to prevent — quietly crossing into paid overage.
    const r = vcpuWarningStep(101 * H, 100, new Set([0.8]));
    expect(r.crossed).toBe(1.0);
  });

  it("goes silent for good once BOTH thresholds are announced", () => {
    const r = vcpuWarningStep(400 * H, 100, new Set([0.8, 1.0]));
    expect(r.crossed).toBeNull();
  });

  it("NO ceiling on file ⇒ never warns (absent is not zero)", () => {
    // A tenant with no metered allowance has nothing to be near. Treating
    // absent as 0 would divide by zero and warn EVERY such tenant on their
    // first job — the loudest possible way to be wrong.
    for (const ceiling of [null, undefined, 0, -100]) {
      const r = vcpuWarningStep(999 * H, ceiling as number | null | undefined, new Set());
      expect(r.crossed).toBeNull();
      expect(r.fraction).toBe(0);
      expect(Number.isFinite(r.fraction)).toBe(true); // never Infinity/NaN
    }
  });

  it("clamps negative consumption instead of reporting a negative fraction", () => {
    // Clock skew already clamps the billable qty; the warning must agree, or the
    // two disagree about the same job.
    const r = vcpuWarningStep(-5000, 100, new Set());
    expect(r.consumedVcpuH).toBe(0);
    expect(r.crossed).toBeNull();
  });

  it("scales with the tier — a big allowance is not warned at a small one's usage", () => {
    // 100 vCPU-h consumed: 100% of starter, but only ~42% of team's 240.
    expect(vcpuWarningStep(100 * H, 100, new Set()).crossed).toBe(1.0);
    expect(vcpuWarningStep(100 * H, 240, new Set()).crossed).toBeNull();
  });

  it("thresholds are ordered ascending — the highest-wins scan depends on it", () => {
    const t = [...VCPU_WARN_THRESHOLDS];
    expect(t).toEqual([...t].sort((a, b) => a - b));
  });
});

describe("vcpu KV keys — namespaces that cannot collide", () => {
  it("separates ceiling / usage / warned, and scopes usage per period", () => {
    const T = "acme";
    expect(vcpuCeilingKey(T)).toBe("vceil:acme");
    expect(vcpuUsageKey(T, "2026-08")).toBe("vused:acme:2026-08");
    expect(vcpuWarnedKey(T, "2026-08", 0.8)).toBe("vwarn:acme:2026-08:0.8");
    // A new month starts a fresh counter AND fresh warnings — an allowance is
    // monthly, so last month's "already warned" must not silence this month.
    expect(vcpuUsageKey(T, "2026-09")).not.toBe(vcpuUsageKey(T, "2026-08"));
    expect(vcpuWarnedKey(T, "2026-09", 0.8)).not.toBe(vcpuWarnedKey(T, "2026-08", 0.8));
    // And the two thresholds are tracked independently.
    expect(vcpuWarnedKey(T, "2026-08", 1)).not.toBe(vcpuWarnedKey(T, "2026-08", 0.8));
  });

  it("does not collide with the other prefixes sharing this KV namespace", () => {
    const keys = [vcpuCeilingKey("t"), vcpuUsageKey("t", "2026-08"), vcpuWarnedKey("t", "2026-08", 1)];
    for (const k of keys) {
      for (const other of ["spawn:", "done:", "conc:", "jtenant:", "jhandle:", "rhandle:", "orphan:", "usage:", "rldl:"]) {
        expect(k.startsWith(other)).toBe(false);
      }
    }
    expect(new Set(keys).size).toBe(keys.length);
  });
});
