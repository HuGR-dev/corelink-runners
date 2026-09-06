import { billingPeriod, usageIdemKey, type UsageEvent } from "../lib.js";
import { DEVENV_TIERS, type DevenvTier } from "../types/devenv.js";

/** Inputs captured by one DevEnv session's terminal lifecycle callback. */
export interface DevenvUsageInput {
  readonly tenantId: string;
  readonly sessionId: string;
  readonly tier: DevenvTier;
  readonly startedAtMs: number;
  readonly completedAtMs: number;
  /** Cloudflare colo / billing region, lower-case ISO-like three-letter code. */
  readonly region: string;
}

export type DevenvUsageErrorCode =
  | "invalid_tenant_uuid"
  | "invalid_session_uuid"
  | "invalid_tier"
  | "invalid_started_at"
  | "invalid_completed_at"
  | "completed_before_started"
  | "invalid_region";

export interface DevenvUsageError {
  readonly code: DevenvUsageErrorCode;
  readonly message: string;
  readonly field: keyof DevenvUsageInput;
}

export type DevenvUsageResult =
  | { readonly ok: true; readonly event: UsageEvent }
  | { readonly ok: false; readonly error: DevenvUsageError };

// Canonical UUID syntax, excluding the nil UUID. UUID version/variant are not
// constrained because tenant identifiers may be issued by systems other than
// crypto.randomUUID while still being real UUIDs.
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const NIL_UUID = "00000000-0000-0000-0000-000000000000";

function isRealUuid(value: unknown): value is string {
  return typeof value === "string" && UUID_RE.test(value) && value.toLowerCase() !== NIL_UUID;
}

function failure(
  code: DevenvUsageErrorCode,
  field: keyof DevenvUsageInput,
  message: string,
): DevenvUsageResult {
  return { ok: false, error: { code, field, message } };
}

function validTimestamp(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= 8_640_000_000_000_000;
}

/**
 * Build the canonical billing event for a completed DevEnv session.
 *
 * Validation happens before calling `buildUsageEvent`; malformed lifecycle
 * state therefore returns a typed failure and cannot emit a partial event.
 * Floors elapsed wall-clock seconds and multiplies by the tier's actual vCPU
 * count. There is deliberately no minimum-duration charge.
 */
export async function buildDevenvUsageEvent(input: DevenvUsageInput): Promise<DevenvUsageResult> {
  if (!isRealUuid(input.tenantId)) {
    return failure("invalid_tenant_uuid", "tenantId", "tenantId must be a non-nil canonical UUID");
  }
  if (!isRealUuid(input.sessionId)) {
    return failure("invalid_session_uuid", "sessionId", "sessionId must be a non-nil canonical UUID");
  }
  if (!Object.hasOwn(DEVENV_TIERS, input.tier)) {
    return failure("invalid_tier", "tier", "tier must be an existing DevEnv tier");
  }
  if (!validTimestamp(input.startedAtMs)) {
    return failure("invalid_started_at", "startedAtMs", "startedAtMs must be a nonnegative safe integer");
  }
  if (!validTimestamp(input.completedAtMs)) {
    return failure("invalid_completed_at", "completedAtMs", "completedAtMs must be a nonnegative safe integer");
  }
  if (input.completedAtMs < input.startedAtMs) {
    return failure("completed_before_started", "completedAtMs", "completedAtMs must be at or after startedAtMs");
  }
  if (typeof input.region !== "string" || !/^[a-z]{3}$/.test(input.region)) {
    return failure("invalid_region", "region", "region must be exactly three lower-case letters");
  }

  const period = billingPeriod(input.completedAtMs);
  const elapsedSeconds = Math.floor((input.completedAtMs - input.startedAtMs) / 1000);
  const qty = elapsedSeconds * DEVENV_TIERS[input.tier].vcpus;
  const event: UsageEvent = {
    tenant_id: input.tenantId.toLowerCase(),
    event_kind: "runner_vcpu_seconds",
    qty,
    billing_period: period,
    region: input.region,
    source: "corelink-runners/spawn-worker",
    time_ms: input.completedAtMs,
    idem_key: await usageIdemKey(`devenv:${input.sessionId.toLowerCase()}`, period),
  };
  if (!Number.isFinite(event.qty) || !Number.isSafeInteger(event.qty) || !Number.isFinite(Date.parse(`${event.billing_period}-01T00:00:00Z`))) {
    return failure("invalid_completed_at", "completedAtMs", "timestamps must produce a finite canonical billing event");
  }
  return { ok: true, event };
}
