import type { UsageEvent } from "../lib.js";

/** One frozen event awaiting an at-least-once HTTP delivery. */
export interface DevenvUsagePending {
  readonly event: UsageEvent;
  readonly sessionUuid: string;
  readonly createdAtMs: number;
  readonly attempts: number;
}

export const DEVENV_USAGE_PENDING_KEY = "devenv:usage:pending";
export const DEVENV_USAGE_SETTLED_KEY = "devenv:usage:settled";

export function freezeDevenvUsage(event: UsageEvent, sessionUuid: string, createdAtMs: number): DevenvUsagePending {
  return { event, sessionUuid, createdAtMs, attempts: 0 };
}

export function nextDevenvUsageAttempt(pending: DevenvUsagePending): DevenvUsagePending {
  return { ...pending, attempts: pending.attempts + 1 };
}
