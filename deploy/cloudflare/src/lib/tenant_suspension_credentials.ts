import type { CredentialAuthority, CredentialIdentity, CredentialPage } from "./credential_authority_contract";

export interface TenantSuspensionInput {
  event_id: string;
  tenant_id: string;
  lifecycle_generation: string;
}

export interface TenantSuspensionConsumerDependencies {
  authority: CredentialAuthority & {
    beginTenantSuspension(input: TenantSuspensionInput): Promise<{ complete: boolean; cursor?: string }>;
    checkpointTenantSuspension(
      input: TenantSuspensionInput,
      expectedCursor: string | undefined,
      nextCursor: string | undefined,
      complete: boolean,
    ): Promise<boolean>;
  };
  revokeCredential(identity: CredentialIdentity): Promise<void>;
  isLegacyCoverageComplete(tenant: string, throughGeneration: string): Promise<boolean>;
}

const MAX_EVENT_ID = 512;
const MAX_PAGE_RECORDS = 101;
const MAX_RESPONSE_BYTES = 4096;
const MAX_GENERATION = 9_223_372_036_854_775_807n;
const UUID = /^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function validInput(input: TenantSuspensionInput): boolean {
  if (!input || typeof input !== "object") return false;
  if (typeof input.event_id !== "string" || input.event_id.length === 0 || input.event_id.length > MAX_EVENT_ID) return false;
  if (typeof input.tenant_id !== "string" || !UUID.test(input.tenant_id)) return false;
  if (typeof input.lifecycle_generation !== "string" || !/^(?:0|[1-9][0-9]*)$/.test(input.lifecycle_generation)) return false;
  try { return BigInt(input.lifecycle_generation) <= MAX_GENERATION; } catch { return false; }
}

function sameInput(a: TenantSuspensionInput, b: TenantSuspensionInput): boolean {
  return a.event_id === b.event_id && a.tenant_id === b.tenant_id && a.lifecycle_generation === b.lifecycle_generation;
}

async function readBoundedBody(response: Response): Promise<unknown> {
  const length = response.headers.get("content-length");
  if (length !== null && (!/^\d+$/.test(length) || Number(length) > MAX_RESPONSE_BYTES)) throw new Error("suspension response too large");
  const text = await response.text();
  if (new TextEncoder().encode(text).byteLength > MAX_RESPONSE_BYTES) throw new Error("suspension response too large");
  try { return JSON.parse(text); } catch { throw new Error("invalid suspension response"); }
}

async function closeGeneration(env: { CORELINK_MINT_URL?: string; CORELINK_RUNNER_MINT_AUTH_KEY?: string }, input: TenantSuspensionInput): Promise<boolean> {
  const key = env.CORELINK_RUNNER_MINT_AUTH_KEY;
  if (typeof key !== "string" || key.length === 0) throw new Error("runner mint auth key unavailable");
  const origin = env.CORELINK_MINT_URL;
  if (typeof origin !== "string" || origin.length === 0) throw new Error("mint origin unavailable");
  let url: URL;
  try { url = new URL("/internal/v1/runner/credentials/close-generation", origin); } catch { throw new Error("invalid mint origin"); }
  if (url.protocol !== "https:") throw new Error("mint origin must use HTTPS");
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 5_000);
  let response: Response;
  try {
    response = await fetch(url, {
      method: "POST", redirect: "error", signal: controller.signal,
      headers: { "content-type": "application/json", "x-corelink-internal-auth": key },
      body: JSON.stringify(input),
    });
  } finally { clearTimeout(timer); }
  if (response.status !== 200 && response.status !== 202) throw new Error(`suspension close returned ${response.status}`);
  const value = await readBoundedBody(response);
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid suspension response");
  const body = value as Record<string, unknown>;
  if (Object.keys(body).length !== 4 || body.event_id !== input.event_id || body.tenant_id !== input.tenant_id || body.lifecycle_generation !== input.lifecycle_generation || typeof body.complete !== "boolean") throw new Error("suspension response identity mismatch");
  const expected = response.status === 200;
  if (body.complete !== expected) throw new Error("suspension response completion mismatch");
  return body.complete;
}

export async function consumeTenantSuspensionCredentials(
  env: { CORELINK_MINT_URL?: string; CORELINK_RUNNER_MINT_AUTH_KEY?: string },
  input: TenantSuspensionInput,
  deps: TenantSuspensionConsumerDependencies,
): Promise<{ complete: boolean }> {
  if (!validInput(input)) throw new Error("invalid tenant suspension input");
  const receipt = await deps.authority.beginTenantSuspension(input);
  if (receipt.complete) {
    if (!(await deps.isLegacyCoverageComplete(input.tenant_id, input.lifecycle_generation))) return { complete: false };
    return { complete: true };
  }
  const producerComplete = await closeGeneration(env, input);
  if (!producerComplete) return { complete: false };
  const page = await deps.authority.pendingCredentials({ kind: "tenant", tenant: input.tenant_id, throughGeneration: input.lifecycle_generation }, receipt.cursor);
  if (!Array.isArray(page.records) || page.records.length > MAX_PAGE_RECORDS) throw new Error("suspension credential page too large");
  for (const identity of page.records) {
    if (identity.tenant !== input.tenant_id) throw new Error("suspension credential tenant mismatch");
    if (identity.lifecycleGeneration !== undefined && BigInt(identity.lifecycleGeneration) > BigInt(input.lifecycle_generation)) throw new Error("suspension credential generation mismatch");
    await deps.revokeCredential(identity);
  }
  const coverage = page.complete && await deps.isLegacyCoverageComplete(input.tenant_id, input.lifecycle_generation);
  const nextCursor = page.complete ? undefined : page.cursor;
  if (!page.complete && typeof nextCursor !== "string") throw new Error("suspension page missing cursor");
  if (page.complete && !coverage) return { complete: false };
  const committed = await deps.authority.checkpointTenantSuspension(input, receipt.cursor, nextCursor, page.complete && coverage);
  return { complete: committed && page.complete && coverage };
}
