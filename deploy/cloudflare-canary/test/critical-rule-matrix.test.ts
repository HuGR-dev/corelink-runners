import { describe, expect, it } from "vitest";
import { CRITICAL_RULES, routeCriticalCondition } from "../src/critical_rule_matrix";

const bindings = { "external-primary-page": "fixture-independent-monitor-primary" };
const cases = [
  ["C1", "control-plane-unavailable", ["lifecycle_failed", "readiness_failed", "breaker_open", "source_missing"]],
  ["C2", "deployment-verification-failed", ["deploy_failed", "version_unverified", "rollback_failed", "expected_result_missing"]],
  ["C3", "job-lifecycle-stuck-or-leaked", ["queued_stuck", "spawn_stuck", "active_stuck", "release_stuck", "resource_leaked"]],
  ["C4", "usage-accounting-divergent", ["ledger_usage_missing", "provider_usage_missing", "usage_divergent"]],
  ["C5", "customer-journey-failed", ["signup_failed", "payment_failed", "installation_failed", "green_job_failed", "journey_missing"]],
] as const;

describe("T6-W9 named C1–C5 rule/channel contract", () => {
  it.each(cases)("routes every classified condition for %s", (pillar, ruleId, conditions) => {
    for (const condition of conditions) {
      expect(routeCriticalCondition(pillar, condition, bindings)).toEqual({
        pillar, ruleId, condition, channelId: "external-primary-page",
        routeRef: "fixture-independent-monitor-primary",
      });
    }
  });
  it("owns exactly the five canonical pillars", () => {
    expect(CRITICAL_RULES.map(rule => rule.pillar)).toEqual(["C1", "C2", "C3", "C4", "C5"]);
  });
  it("refuses unbound or blank delivery routes", () => {
    for (const config of [{}, { "external-primary-page": "   " }]) {
      expect(() => routeCriticalCondition("C1", "breaker_open", config)).toThrow("critical channel unbound");
    }
  });
  it("refuses unknown pillars and cross-pillar conditions", () => {
    expect(() => routeCriticalCondition("C6", "breaker_open", bindings)).toThrow("unknown critical condition");
    expect(() => routeCriticalCondition("C4", "breaker_open", bindings)).toThrow("unknown critical condition");
  });
});
