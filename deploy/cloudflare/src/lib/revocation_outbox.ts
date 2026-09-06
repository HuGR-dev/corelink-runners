import { bumpMetrics } from "../metrics";
import { logEvent, revokeCasPatById } from "../lib";

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
}

export interface SuspensionAuthority {
  listJobAttributions(tenantId: string, cursor?: string): Promise<{
    records: Array<{ jobId: string; tenant: string }>;
    cursor?: string;
    complete: boolean;
  }>;
}

interface RevokeRecord {
  schema_version: 1;
  job_id: string;
  pat_id: string;
  tenant: string;
  attempts: number;
}

const RETRY_PREFIX = "revoke-retry:";
const RECEIPT_PREFIX = "revoke-receipt:";
const MAX_LIST_PAGES = 128;

function identityKey(prefix: string, jobId: string, tenant: string, patId: string): string {
  return `${prefix}${encodeURIComponent(jobId)}:${encodeURIComponent(tenant)}:${encodeURIComponent(patId)}`;
}

function retryKey(jobId: string, tenant: string, patId: string): string {
  return identityKey(RETRY_PREFIX, jobId, tenant, patId);
}

function receiptKey(jobId: string, tenant: string, patId: string): string {
  return identityKey(RECEIPT_PREFIX, jobId, tenant, patId);
}

async function listKeys(kv: RevocationKv, prefix: string): Promise<string[]> {
  const keys: string[] = [];
  let cursor: string | undefined;
  for (let page = 0; page < MAX_LIST_PAGES; page++) {
    const listed = await kv.list({ prefix, cursor });
    keys.push(...listed.keys.map(key => key.name));
    if (listed.list_complete !== false) return keys;
    if (!listed.cursor) throw new Error(`KV list incomplete for ${prefix}`);
    cursor = listed.cursor;
  }
  throw new Error(`KV list page bound exceeded for ${prefix}`);
}

async function writeReceipt(env: RevocationEnv, jobId: string, tenant: string, patId: string): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) throw new Error("revocation receipt authority unavailable");
  await kv.put(receiptKey(jobId, tenant, patId), JSON.stringify({
    schema_version: 1, job_id: jobId, tenant, pat_id: patId, confirmed_at_ms: Date.now(),
  }));
}

async function hasReceipt(kv: RevocationKv, jobId: string, tenant: string): Promise<boolean> {
  const prefix = `${RECEIPT_PREFIX}${encodeURIComponent(jobId)}:${encodeURIComponent(tenant)}:`;
  return (await listKeys(kv, prefix)).length > 0;
}

async function retainRetry(env: RevocationEnv, jobId: string, patId: string, tenant: string): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) throw new Error("revocation outbox unavailable");
  const key = retryKey(jobId, tenant, patId);
  if (await kv.get(key)) return;
  await kv.put(key, JSON.stringify({ schema_version: 1, job_id: jobId, pat_id: patId, tenant, attempts: 0 } satisfies RevokeRecord));
}

export async function revokeCompletedJob(env: RevocationEnv, jobId: string, derivedTenant?: string): Promise<boolean> {
  const kv = env.RUNNER_JOB_PATS;
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY || !kv) return false;
  const patId = await kv.get(jobId);
  if (!patId) return false;
  if (!derivedTenant) {
    await bumpMetrics(env, "revoke_missing_tenant");
    logEvent("error", "revoke_missing_tenant", { jobId, patId });
    throw new Error("revoke refused: server-derived tenant is missing");
  }
  try {
    await revokeCasPatById(env as never, patId, derivedTenant);
    // The receipt is the durable proof that permits later cleanup/skip.
    await writeReceipt(env, jobId, derivedTenant, patId);
    if (await kv.get(jobId) === patId) await kv.delete(jobId);
    await kv.delete(retryKey(jobId, derivedTenant, patId));
    return true;
  } catch (e) {
    await retainRetry(env, jobId, patId, derivedTenant);
    await bumpMetrics(env, "revoke_failed");
    logEvent("error", "revoke_failed", { jobId, patId, tenant: derivedTenant, error: (e as Error).message });
    return false;
  }
}

export async function retryFailedRevocations(env: RevocationEnv): Promise<number> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv || !env.CORELINK_RUNNER_MINT_AUTH_KEY) return 0;
  let succeeded = 0;
  for (const name of await listKeys(kv, RETRY_PREFIX)) {
    const raw = await kv.get(name);
    if (!raw) continue;
    let rec: RevokeRecord;
    try {
      rec = JSON.parse(raw) as RevokeRecord;
      if (rec.schema_version !== 1 || !rec.job_id || !rec.pat_id || !rec.tenant) continue;
    } catch { continue; }
    try {
      await revokeCasPatById(env as never, rec.pat_id, rec.tenant);
      await writeReceipt(env, rec.job_id, rec.tenant, rec.pat_id);
      if (await kv.get(rec.job_id) === rec.pat_id) await kv.delete(rec.job_id);
      await kv.delete(name);
      succeeded++;
    } catch (e) {
      await kv.put(name, JSON.stringify({ ...rec, attempts: rec.attempts + 1 }));
      await bumpMetrics(env, "revoke_failed");
      logEvent("error", "revoke_failed", { jobId: rec.job_id, patId: rec.pat_id, tenant: rec.tenant, retry: true, error: (e as Error).message });
    }
  }
  return succeeded;
}

export async function dispatchTenantSuspensionRevocations(
  env: RevocationEnv,
  event: { event_id: string; tenant_id: string },
  authority: SuspensionAuthority,
): Promise<number> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv || !event.event_id || !event.tenant_id) throw new Error("invalid suspension event");
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY) throw new Error("runner mint revoke authority unavailable");
  const marker = `suspend-revoke:${event.event_id}`;
  if (await kv.get(marker)) return 0;
  let dispatched = 0;
  let cursor: string | undefined;
  for (;;) {
    const page = await authority.listJobAttributions(event.tenant_id, cursor);
    for (const record of page.records) {
      if (record.tenant !== event.tenant_id) throw new Error(`tenant attribution mismatch for active job ${record.jobId}`);
      const patId = await kv.get(record.jobId);
      if (!patId) {
        if (await hasReceipt(kv, record.jobId, event.tenant_id)) continue;
        throw new Error(`active tenant job ${record.jobId} has no durable pat_id or confirmed receipt`);
      }
      if (!(await revokeCompletedJob(env, record.jobId, event.tenant_id))) throw new Error(`active tenant job ${record.jobId} revoke was not confirmed`);
      dispatched++;
    }
    if (page.complete) break;
    if (!page.cursor || page.cursor === cursor) throw new Error("incomplete tenant attribution page");
    cursor = page.cursor;
  }
  await kv.put(marker, "1");
  return dispatched;
}
