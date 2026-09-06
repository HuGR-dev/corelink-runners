import {
  cfAccessHeaders,
  CRED_TICKET_TTL_S,
  randomTicket,
  type MintEnv,
  type MintParams,
  type CredStashLike,
  type MintResult,
  type ContainerEnvResult,
  type ColdReason,
} from "../lib";
import { buildCLW_LEASE_ID } from "./runner_credential_lease";

export class MintForbiddenError extends Error {
  readonly source: "edge_proxy" | "authz";
  readonly patId?: string;
  readonly tenant?: string;
  readonly maxConcurrency?: number;
  constructor(
    message = "runner mint unauthorized",
    source: "edge_proxy" | "authz" = "authz",
    metadata?: Partial<Pick<MintResult, "patId" | "tenant" | "maxConcurrency">>,
  ) {
    super(message);
    this.name = "MintForbiddenError";
    this.source = source;
    this.patId = metadata?.patId;
    this.tenant = metadata?.tenant;
    this.maxConcurrency = metadata?.maxConcurrency;
  }
}

export function classifyMintForbidden(body: string): "edge_proxy" | "authz" {
  const normalized = body.toLowerCase();
  return normalized.includes("cloudflare") || normalized.includes("cf-access")
    || normalized.includes("access denied") || normalized.includes("<!doctype html")
    ? "edge_proxy" : "authz";
}

function forbidden(
  coldReason?: ColdReason,
  metadata?: Partial<Pick<MintResult, "patId" | "tenant" | "maxConcurrency">>,
  forbiddenReason: "edge_proxy" | "authz" = "authz",
): ContainerEnvResult {
  const safeMetadata = metadata ? {
    ...(metadata.patId ? { patId: metadata.patId } : {}),
    ...(metadata.tenant ? { tenant: metadata.tenant } : {}),
    ...(metadata.maxConcurrency !== undefined ? { maxConcurrency: metadata.maxConcurrency } : {}),
  } : {};
  return { authz: "forbidden", containerEnv: {}, ...(coldReason ? { coldReason } : {}), forbiddenReason, ...safeMetadata };
}

/** Mint a server-derived per-job credential. No response body is logged. */
export async function mintCasPat(env: MintEnv, params: MintParams): Promise<MintResult> {
  const optionC = !!params.acquiringPat;
  const headers: Record<string, string> = {
    ...cfAccessHeaders(env),
    "x-corelink-internal-auth": env.CORELINK_RUNNER_MINT_AUTH_KEY ?? "",
    "content-type": "application/json",
    "user-agent": "corelink-spawn-worker",
  };
  if (optionC) headers.authorization = `Bearer ${params.acquiringPat}`;
  let resp: Response;
  try {
    resp = await fetch(`${env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com"}/internal/v1/runner/mint`, {
      method: "POST",
      headers,
      body: JSON.stringify({
        job_id: params.jobId,
        repo_full_name: params.repoFullName,
        ...(optionC ? {} : { installation_id: params.installationId }),
        scope: params.scope ?? (optionC ? "cas:rw" : "read-write"),
        ...(params.ttlSeconds != null ? { ttl_seconds: params.ttlSeconds } : {}),
      }),
    });
  } catch {
    throw new MintForbiddenError();
  }
  if (resp.status === 403) {
    // Classification is based on the body, but the body itself never leaves this function.
    const body = await resp.text().catch(() => "");
    throw new MintForbiddenError("runner mint unauthorized", classifyMintForbidden(body));
  }
  if (!resp.ok) throw new MintForbiddenError();
  let value: unknown;
  try { value = await resp.json(); } catch { throw new MintForbiddenError(); }
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new MintForbiddenError();
  const j = value as Record<string, unknown>;
  const token = typeof j.token_plaintext === "string" ? j.token_plaintext : "";
  const patId = typeof j.pat_id === "string" ? j.pat_id : "";
  const tenant = typeof j.tenant === "string" ? j.tenant : "";
  const maxConcurrency = j.max_concurrency;
  const maxVcpuH = j.max_vcpu_h;
  const validMaxConcurrency = typeof maxConcurrency === "number"
    && Number.isFinite(maxConcurrency) && Number.isSafeInteger(maxConcurrency) && maxConcurrency > 0;
  const metadata = patId.trim() && tenant.trim()
    ? { patId, tenant, ...(validMaxConcurrency ? { maxConcurrency } : {}) } : undefined;
  if (!token.trim() || !patId.trim() || !tenant.trim() || typeof maxConcurrency !== "number"
    || !Number.isFinite(maxConcurrency) || !Number.isSafeInteger(maxConcurrency) || maxConcurrency <= 0
    || (maxVcpuH !== undefined && (typeof maxVcpuH !== "number" || !Number.isFinite(maxVcpuH) || maxVcpuH < 0))) {
    throw new MintForbiddenError("runner mint response invalid", "authz", metadata);
  }
  return {
    token,
    patId,
    tenant,
    maxConcurrency,
    ...(maxVcpuH === undefined ? {} : { maxVcpuH }),
  };
}

export async function buildContainerEnv(
  env: MintEnv,
  params: MintParams,
  deps?: { stash?: CredStashLike; fabricEndpoint?: string },
): Promise<ContainerEnvResult> {
  if (typeof params.jobId !== "string" || !params.jobId.trim()) return forbidden();
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY) return forbidden("mint_key_unarmed");
  if (!params.repoFullName) return forbidden("no_repo");
  if (!params.installationId && !params.acquiringPat) return forbidden("no_installation_or_pat");
  if (!deps?.stash || !deps.fabricEndpoint) return forbidden();

  let m: MintResult;
  try {
    m = await mintCasPat(env, params);
  } catch (e) {
    const source = e instanceof MintForbiddenError ? e.source : "authz";
    console.log(JSON.stringify({ level: "error", event: "runner_mint_forbidden", source }));
    return forbidden(undefined, e instanceof MintForbiddenError ? e : undefined, source);
  }
  const endpoint = env.CLW_ENDPOINT ?? "https://corelink-api.humangr.com";
  const leaseId = buildCLW_LEASE_ID(params.jobId, m.tenant, m.patId);
  // Production and all successful authorization require env-0. The raw PAT is
  // never placed in an untrusted container, including when legacy is set.
  try {
    const ticket = await deps.stash.stash(
      leaseId,
      randomTicket(),
      { token: m.token, endpoint, tenant: m.tenant },
      CRED_TICKET_TTL_S * 1000,
    );
    if (!ticket) return forbidden(undefined, m);
    return {
      authz: "ok",
      containerEnv: {
        CLW_ENDPOINT: endpoint,
        CLW_TENANT: m.tenant,
        CLW_CRED_TICKET: ticket,
        CLW_LEASE_ID: leaseId,
        CLW_FABRIC_ENDPOINT: deps.fabricEndpoint,
        CLW_REF_DOMAIN: "runner",
      },
      patId: m.patId,
      tenant: m.tenant,
      maxConcurrency: m.maxConcurrency,
      maxVcpuH: m.maxVcpuH,
    };
  } catch {
    return forbidden(undefined, m);
  }
}
