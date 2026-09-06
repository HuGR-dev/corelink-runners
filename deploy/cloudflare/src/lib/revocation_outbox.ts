import { bumpMetrics } from "../metrics";
import { logEvent, revokeCasPatById, type MintEnv } from "../lib";
import type { CredentialAuthority, CredentialIdentity, CredentialPage, CredentialSelection } from "./credential_authority_contract.js";
import { buildCLW_LEASE_ID } from "./runner_credential_lease.js";

export interface RevocationKv {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void>;
  delete(key: string): Promise<void>;
  list(options?: { prefix?: string; cursor?: string }): Promise<{ keys: { name: string }[]; list_complete?: boolean; cursor?: string }>;
}
export interface RevocationEnv {
  CORELINK_RUNNER_MINT_AUTH_KEY?: string;
  CORELINK_MINT_URL?: string;
  RUNNER_JOB_PATS?: RevocationKv;
  METRICS?: any;
  CRED_STASH?: { idFromName(name: string): unknown; get(id: unknown): { wipe(): Promise<void> } };
}
const RETRY_PREFIX = "revoke-retry:";
const MAX_LIST_PAGES = 128;

async function pendingAll(authority: CredentialAuthority, selection: CredentialSelection): Promise<CredentialIdentity[]> {
  const records: CredentialIdentity[] = [];
  let cursor: string | undefined;
  for (let page = 0; page < MAX_LIST_PAGES; page++) {
    const result: CredentialPage = await authority.pendingCredentials(selection, cursor);
    records.push(...result.records);
    if (result.complete) return records;
    if (!result.cursor || result.cursor === cursor) throw new Error("incomplete credential authority page");
    cursor = result.cursor;
  }
  throw new Error("credential authority page bound exceeded");
}

async function requestedAll(authority: CredentialAuthority): Promise<CredentialIdentity[]> {
  const records: CredentialIdentity[] = [];
  let cursor: string | undefined;
  for (let page = 0; page < MAX_LIST_PAGES; page++) {
    const result = await authority.revocationRequestedCredentials(cursor);
    records.push(...result.records);
    if (result.complete) return records;
    if (!result.cursor || result.cursor === cursor) throw new Error("incomplete credential authority retry page");
    cursor = result.cursor;
  }
  throw new Error("credential authority retry page bound exceeded");
}

function retryKey(identity: CredentialIdentity): string {
  return `${RETRY_PREFIX}${encodeURIComponent(identity.jobId)}:${encodeURIComponent(identity.tenant)}:${encodeURIComponent(identity.patId)}`;
}

async function retainRetry(env: RevocationEnv, identity: CredentialIdentity): Promise<void> {
  if (!env.RUNNER_JOB_PATS) return;
  const key = retryKey(identity);
  if (!(await env.RUNNER_JOB_PATS.get(key))) await env.RUNNER_JOB_PATS.put(key, JSON.stringify({ schema_version: 1, ...identity, attempts: 0 }));
}

async function revokeOne(env: RevocationEnv, authority: CredentialAuthority, identity: CredentialIdentity): Promise<boolean> {
  try {
    await authority.requestCredentialRevocation(identity);
    if (!env.CORELINK_RUNNER_MINT_AUTH_KEY) throw new Error("missing mint auth key");
    const mintEnv: MintEnv = {
      CORELINK_RUNNER_MINT_AUTH_KEY: env.CORELINK_RUNNER_MINT_AUTH_KEY,
      CORELINK_MINT_URL: env.CORELINK_MINT_URL,
    };
    await revokeCasPatById(mintEnv, identity.patId, identity.tenant);
    await authority.confirmCredentialRevoked(identity);
    if (env.CRED_STASH) await env.CRED_STASH.get(env.CRED_STASH.idFromName(buildCLW_LEASE_ID(identity.jobId, identity.tenant, identity.patId))).wipe();
    if (env.RUNNER_JOB_PATS) await env.RUNNER_JOB_PATS.delete(retryKey(identity));
    return true;
  } catch (e) {
    await retainRetry(env, identity);
    await bumpMetrics(env, "revoke_failed");
    logEvent("error", "revoke_failed", { jobId: identity.jobId, patId: identity.patId, tenant: identity.tenant, error: (e as Error).message });
    return false;
  }
}

export async function revokeCompletedJob(env: RevocationEnv, authority: CredentialAuthority, jobId: string, derivedTenant?: string): Promise<boolean> {
  const closed = await authority.closeJobCredentials(jobId);
  if (!closed.known) throw new Error(`credential authority has no migrated obligation for job ${jobId}`);
  const identities = await pendingAll(authority, { kind: "job", jobId });
  if (identities.length === 0) return true;
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY) throw new Error(`credential revoke pending for job ${jobId}`);
  if (derivedTenant && identities.some(identity => identity.tenant !== derivedTenant)) throw new Error("credential tenant attribution conflict");
  let revoked = false;
  for (const identity of identities) {
    if (!(await revokeOne(env, authority, identity))) throw new Error(`credential revoke pending for job ${jobId}`);
    revoked = true;
  }
  return revoked;
}

/** Revoke one issued credential after an individual attempt fails. This path
 * deliberately does not close the job: a later attempt may mint another PAT.
 */
export async function revokeIssuedCredential(env: RevocationEnv, authority: CredentialAuthority, identity: CredentialIdentity): Promise<boolean> {
  // The authority transition is the source of truth. A missing pending-page
  // row can mean terminal, malformed, or unknown state, so absence must not be
  // treated as proof that this exact identity was already revoked.
  return revokeOne(env, authority, identity);
}

export async function retryFailedRevocations(env: RevocationEnv, authority: CredentialAuthority): Promise<number> {
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY) return 0;
  let succeeded = 0;
  // Healthy registered credentials are deliberately excluded from scheduled
  // retry; only the authority's explicit revoke_requested page is eligible.
  for (const identity of await requestedAll(authority)) if (await revokeOne(env, authority, identity)) succeeded++;
  return succeeded;
}

export async function dispatchTenantSuspensionRevocations(env: RevocationEnv, authority: CredentialAuthority, event: { event_id: string; tenant_id: string }): Promise<number> {
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY || !event.event_id || !event.tenant_id) throw new Error("invalid suspension event");
  const marker = `suspend-revoke:${event.event_id}`;
  if (env.RUNNER_JOB_PATS && await env.RUNNER_JOB_PATS.get(marker)) return 0;
  const identities = await pendingAll(authority, { kind: "tenant", tenant: event.tenant_id });
  if (identities.length === 0) throw new Error(`credential authority has no migrated obligations for tenant ${event.tenant_id}`);
  let dispatched = 0;
  for (const identity of identities) {
    if (!(await revokeOne(env, authority, identity))) throw new Error(`credential revoke pending for job ${identity.jobId}`);
    dispatched++;
  }
  if (env.RUNNER_JOB_PATS) await env.RUNNER_JOB_PATS.put(marker, "1");
  return dispatched;
}
