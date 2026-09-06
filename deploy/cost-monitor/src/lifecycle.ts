import type { LifecycleAuthority, LifecyclePayload, SourceRegistration } from "./types.js";

const MAX_ID_LENGTH = 256;
const MAX_NONCE_LENGTH = 256;
const MAX_SAMPLE_AGE_MS = 120_000;

export type LifecycleStatus = "healthy" | "failed" | "unknown";

export interface LifecycleDecision {
  status: LifecycleStatus;
  reason: string;
  authority: LifecycleAuthority;
  nonce: string;
}

/** A producer fact that cannot be accepted into the lifecycle cursor. */
export class LifecycleRefusalError extends Error {
  readonly code = "lifecycle_refused" as const;

  constructor(reason: string) {
    super(reason);
    this.name = "LifecycleRefusalError";
  }
}

function refusal(reason: string): never {
  throw new LifecycleRefusalError(reason);
}

function boundedIdentity(value: unknown, label: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_ID_LENGTH) {
    return refusal(`${label}_invalid`);
  }
  return value;
}

function safeMs(value: unknown, label: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) return refusal(`${label}_invalid`);
  return value as number;
}

function safeSequence(value: unknown): number {
  if (!Number.isSafeInteger(value) || (value as number) < 1) return refusal("monotonic_seq_invalid");
  return value as number;
}

function lifecycleState(value: unknown): LifecycleStatus {
  if (value === "healthy" || value === "failed" || value === "unknown") return value;
  return refusal("state_invalid");
}

function authorityEqual(a: LifecycleAuthority, b: LifecycleAuthority): boolean {
  return a.source_id === b.source_id
    && a.monotonic_seq === b.monotonic_seq
    && a.transition_id === b.transition_id
    && a.state === b.state
    && a.transition_at === b.transition_at
    && a.source_version === b.source_version;
}

/**
 * Classify one already-authenticated lifecycle payload.
 *
 * This is deliberately a pure policy function. Envelope authentication, lane
 * CAS, cursor persistence, and missing-source scheduling remain caller-owned.
 */
export function classifyLifecycle(
  payload: LifecyclePayload,
  registration: SourceRegistration,
  previousAuthority: LifecycleAuthority | null,
  previousNonce: string | null,
  nowMs: number,
): LifecycleDecision {
  const sourceId = boundedIdentity(payload.source_id, "source_id");
  const sourceVersion = boundedIdentity(payload.source_version, "source_version");
  const transitionId = boundedIdentity(payload.transition_id, "transition_id");
  const nonce = boundedIdentity(payload.nonce, "nonce");
  const sequence = safeSequence(payload.monotonic_seq);
  const transitionAt = safeMs(payload.transition_at, "transition_at");
  const sampledAt = safeMs(payload.sampled_at, "sampled_at");
  const now = safeMs(nowMs, "now");
  const state = lifecycleState(payload.state);
  const status = state;

  if (registration.authoritySourceId !== sourceId) return refusal("source_id_not_pinned");
  if (registration.sourceVersion !== sourceVersion) return refusal("source_version_not_pinned");
  if (previousNonce !== null && nonce === previousNonce) return refusal("nonce_replayed");
  if (transitionAt > sampledAt) return refusal("transition_in_future");
  if (sampledAt > now) return refusal("sample_in_future");

  const authority = {
    source_id: sourceId,
    monotonic_seq: sequence,
    transition_id: transitionId,
    state,
    transition_at: transitionAt,
    source_version: sourceVersion,
  } satisfies LifecycleAuthority;

  if (previousAuthority !== null) {
    if (sequence < previousAuthority.monotonic_seq) return refusal("sequence_regressed");
    if (sequence === previousAuthority.monotonic_seq && !authorityEqual(authority, previousAuthority)) {
      return refusal("same_sequence_changed");
    }
  }

  if (now - sampledAt > MAX_SAMPLE_AGE_MS) {
    return { status: "unknown", reason: "sample_stale", authority, nonce };
  }
  return { status, reason: status === "healthy" ? "healthy" : status, authority, nonce };
}
