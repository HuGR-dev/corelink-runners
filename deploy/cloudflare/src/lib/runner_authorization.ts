import { cfAccessHeaders, type MintEnv, type MintParams } from "../lib";

export type RunnerAuthorization = {
  tenant: string;
  maxConcurrency: number;
  maxVcpuH?: number;
  computeGrant?: string;
};

export class RunnerAuthorizationError extends Error {
  constructor() {
    super("runner authorization unavailable");
    this.name = "RunnerAuthorizationError";
  }
}
export type RunnerAuthorizationAttempt =
  | { kind: "authorized"; authorization: RunnerAuthorization }
  | { kind: "refused"; status: 401 | 403 }
  | { kind: "unknown"; status?: number };

function required(value: unknown): value is string {
  return typeof value === "string" && value.trim() === value && value.length > 0;
}

function validResponse(value: unknown): RunnerAuthorization | null {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const tenant = record.tenant;
  const maxConcurrency = record.max_concurrency;
  const maxVcpuH = record.max_vcpu_h;
  const computeGrant = record.compute_grant;
  if (!required(tenant) || typeof maxConcurrency !== "number" || !Number.isSafeInteger(maxConcurrency) || maxConcurrency <= 0) return null;
  if (maxVcpuH !== undefined && (typeof maxVcpuH !== "number" || !Number.isSafeInteger(maxVcpuH) || maxVcpuH <= 0 || maxVcpuH > 0xffffffff)) return null;
  if (maxVcpuH !== undefined && (typeof computeGrant !== "string" || computeGrant.length < 1 || computeGrant.length > 8192)) return null;
  if (maxVcpuH === undefined && computeGrant !== undefined) return null;
  return {
    ...(typeof computeGrant === "string" ? { computeGrant } : {}),
    tenant,
    maxConcurrency,
    ...(maxVcpuH === undefined ? {} : { maxVcpuH }),
  };
}

export async function inspectRunnerAuthorization(env: MintEnv, params: MintParams): Promise<RunnerAuthorizationAttempt> {
  const key = env.CORELINK_RUNNER_MINT_AUTH_KEY;
  const jobId = params.jobId;
  const repo = params.repoFullName;
  const installationId = params.installationId;
  const acquiringPat = params.acquiringPat;
  if (!required(key) || !required(jobId) || !required(repo)
    || (!required(installationId) && !required(acquiringPat))) return { kind: "unknown" };
  const optionC = required(acquiringPat);
  const headers: Record<string, string> = {
    ...cfAccessHeaders(env),
    "x-corelink-internal-auth": key,
    "content-type": "application/json",
    "user-agent": "corelink-spawn-worker",
  };
  if (optionC) headers.authorization = `Bearer ${acquiringPat}`;
  try {
    const response = await fetch(`${env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com"}/internal/v1/runner/authorize`, {
      method: "POST",
      headers,
      body: JSON.stringify({
        job_id: jobId,
        ...(params.computeReservationId ? { compute_reservation_id: params.computeReservationId } : {}),
        repo_full_name: repo,
        ...(installationId ? { installation_id: installationId } : {}),
      }),
    });
    if (response.status === 401 || response.status === 403) return { kind: "refused", status: response.status };
    if (!response.ok) return { kind: "unknown", status: response.status };
    let value: unknown;
    try { value = await response.json(); } catch { return { kind: "unknown", status: response.status }; }
    const result = validResponse(value);
    if (!result) return { kind: "unknown", status: response.status };
    return { kind: "authorized", authorization: result };
  } catch (error) {
    return { kind: "unknown" };
  }
}

export async function authorizeRunner(env: MintEnv, params: MintParams): Promise<RunnerAuthorization> {
  const result = await inspectRunnerAuthorization(env, params);
  if (result.kind !== "authorized") throw new RunnerAuthorizationError();
  return result.authorization;
}
