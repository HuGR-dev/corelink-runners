/**
 * T6-W9's frozen C1–C5 rule/channel contract, consumed by T6-W12.
 * This module routes already classified conditions. It performs no detection,
 * scheduling or delivery, and cannot certify an external monitor capability.
 * Authority: 2026-09-01-round3-remediation-delta.md, A6.18.
 */
export const CRITICAL_RULES = [
  { pillar: "C1", ruleId: "control-plane-unavailable", conditions: ["lifecycle_failed", "readiness_failed", "breaker_open", "source_missing"] },
  { pillar: "C2", ruleId: "deployment-verification-failed", conditions: ["deploy_failed", "version_unverified", "rollback_failed", "expected_result_missing"] },
  { pillar: "C3", ruleId: "job-lifecycle-stuck-or-leaked", conditions: ["queued_stuck", "spawn_stuck", "active_stuck", "release_stuck", "resource_leaked"] },
  { pillar: "C4", ruleId: "usage-accounting-divergent", conditions: ["ledger_usage_missing", "provider_usage_missing", "usage_divergent"] },
  { pillar: "C5", ruleId: "customer-journey-failed", conditions: ["signup_failed", "payment_failed", "installation_failed", "green_job_failed", "journey_missing"] },
] as const;

/** Logical channel; the actual external transport is bound by O-MONITORHOST. */
export const CRITICAL_CHANNEL_ID = "external-primary-page";
export type CriticalPillar = typeof CRITICAL_RULES[number]["pillar"];
export interface CriticalRoute {
  pillar: CriticalPillar;
  ruleId: string;
  condition: string;
  channelId: typeof CRITICAL_CHANNEL_ID;
  /** A configured route reference, not a secret and not proof of delivery. */
  routeRef: string;
}

/** Reject unknown conditions and an unbound channel; never silently drop them. */
export function routeCriticalCondition(
  pillar: string,
  condition: string,
  bindings: Readonly<Record<string, string | undefined>>,
): CriticalRoute {
  const rule = CRITICAL_RULES.find(candidate =>
    candidate.pillar === pillar &&
    (candidate.conditions as readonly string[]).includes(condition));
  if (!rule) throw new Error("unknown critical condition");
  const routeRef = bindings[CRITICAL_CHANNEL_ID]?.trim();
  if (!routeRef) throw new Error("critical channel unbound");
  return { pillar: rule.pillar, ruleId: rule.ruleId, condition,
    channelId: CRITICAL_CHANNEL_ID, routeRef };
}
