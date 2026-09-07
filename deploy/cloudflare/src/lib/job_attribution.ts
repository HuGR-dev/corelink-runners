/**
 * Durable, tenant-safe identity carried from the authorized mint to completion.
 *
 * This deliberately stores only the server-resolved tenant and job id. The
 * per-job PAT remains in the existing PAT/revoke record and is never copied
 * into attribution. A job's attribution is immutable for its lifetime: a
 * retry for the same tenant is idempotent, while a conflicting tenant is a
 * hard failure so cleanup and billing cannot silently change owners.
 */

export interface JobAttributionStore {
  get(key: string): Promise<string | null>;
  /** Atomic insert-or-read, serialized by the durable authority. */
  putIfAbsent(key: string, value: string): Promise<string>;
  delete(key: string): Promise<void>;
}

export interface JobAttribution {
  jobId: string;
  tenant: string;
}

const JOB_ATTRIBUTION_PREFIX = "job-attribution:";

export class JobAttributionConflictError extends Error {
  readonly code = "job_attribution_conflict" as const;

  constructor(
    readonly jobId: string,
    readonly existingTenant: string,
    readonly requestedTenant: string,
  ) {
    super(`immutable tenant attribution conflict for job ${jobId}`);
    this.name = "JobAttributionConflictError";
  }
}

export class JobAttributionInvalidError extends Error {
  readonly code = "job_attribution_invalid" as const;

  constructor(readonly jobId: string) {
    super(`invalid durable tenant attribution for job ${jobId}`);
    this.name = "JobAttributionInvalidError";
  }
}

export function jobAttributionKey(jobId: string): string {
  return `${JOB_ATTRIBUTION_PREFIX}${jobId}`;
}

export function decodeJobAttribution(raw: string, jobId: string): JobAttribution {
  try {
    const value = JSON.parse(raw) as Partial<JobAttribution>;
    if (value.jobId !== jobId || typeof value.tenant !== "string" || value.tenant.trim() === "") {
      throw new Error("invalid shape");
    }
    return { jobId, tenant: value.tenant };
  } catch {
    throw new JobAttributionInvalidError(jobId);
  }
}

/** Persist the verified tenant before any billable or container side effect. */
export async function persistJobAttribution(
  store: JobAttributionStore | undefined,
  attribution: JobAttribution,
): Promise<JobAttribution | null> {
  if (!store) return null;
  if (!attribution.jobId || !attribution.tenant || attribution.tenant.trim() === "") {
    throw new JobAttributionInvalidError(attribution.jobId);
  }
  const key = jobAttributionKey(attribution.jobId);
  const existingRaw = await store.get(key);
  if (existingRaw) {
    const existing = decodeJobAttribution(existingRaw, attribution.jobId);
    if (existing.tenant !== attribution.tenant) {
      throw new JobAttributionConflictError(attribution.jobId, existing.tenant, attribution.tenant);
    }
    return existing;
  }
  const persistedRaw = await store.putIfAbsent(key, JSON.stringify(attribution));
  if (!persistedRaw) throw new JobAttributionInvalidError(attribution.jobId);
  const persisted = decodeJobAttribution(persistedRaw, attribution.jobId);
  if (persisted.tenant !== attribution.tenant) {
    throw new JobAttributionConflictError(attribution.jobId, persisted.tenant, attribution.tenant);
  }
  return persisted;
}

/** Read only the verified durable identity used by completion cleanup/billing. */
export async function readJobAttribution(
  store: JobAttributionStore | undefined,
  jobId: string,
): Promise<JobAttribution | null> {
  if (!store) return null;
  const raw = await store.get(jobAttributionKey(jobId));
  return raw ? decodeJobAttribution(raw, jobId) : null;
}

export async function deleteJobAttribution(
  store: JobAttributionStore | undefined,
  jobId: string,
): Promise<void> {
  if (store) await store.delete(jobAttributionKey(jobId));
}
