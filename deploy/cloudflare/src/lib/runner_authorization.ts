import { cfAccessHeaders, type MintEnv, type MintParams } from "../lib";

export type RunnerAuthorization = {
  tenant: string;
  maxConcurrency: number;
  maxVcpuH?: number;
};

export class RunnerAuthorizationError extends Error {
  constructor() {
    super("runner authorization unavailable");
    this.name = "RunnerAuthorizationError";
  }
}

function required(value: unknown): value is string {
  return typeof value === "string" && value.trim() === value && value.length > 0;
}

function validResponse(value: unknown): RunnerAuthorization | null {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const tenant = record.tenant;
  const maxConcurrency = record.max_concurrency;
  const maxVcpuH = record.max_vcpu_h;
  if (!required(tenant) || typeof maxConcurrency !== "number" || !Number.isSafeInteger(maxConcurrency) || maxConcurrency <= 0) return null;
  if (maxVcpuH !== undefined && (typeof maxVcpuH !== "number" || !Number.isFinite(maxVcpuH) || maxVcpuH < 0)) return null;
  return {
    tenant,
    maxConcurrency,
    ...(maxVcpuH === undefined ? {} : { maxVcpuH }),
  };
}

export async function authorizeRunner(env: MintEnv, params: MintParams): Promise<RunnerAuthorization> {
  const key = env.CORELINK_RUNNER_MINT_AUTH_KEY;
  const jobId = params.jobId;
  const repo = params.repoFullName;
  const installationId = params.installationId;
  const acquiringPat = params.acquiringPat;
  if (!required(key) || !required(jobId) || !required(repo)
    || (!required(installationId) && !required(acquiringPat))) throw new RunnerAuthorizationError();
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
        repo_full_name: repo,
        ...(installationId ? { installation_id: installationId } : {}),
      }),
    });
    if (!response.ok) throw new RunnerAuthorizationError();
    let value: unknown;
    try { value = await response.json(); } catch { throw new RunnerAuthorizationError(); }
    const result = validResponse(value);
    if (!result) throw new RunnerAuthorizationError();
    return result;
  } catch (error) {
    if (error instanceof RunnerAuthorizationError) throw error;
    throw new RunnerAuthorizationError();
  }
}
