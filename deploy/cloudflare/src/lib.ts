// Pure, runtime-agnostic helpers for the spawn-Worker. NO `cloudflare:workers` /
// `@cloudflare/containers` imports here — so this module is unit-testable in
// plain vitest (node): only `crypto` + `fetch` (Node 20+ globals) are used.

/** The subset of Env the warm-mint path reads. */
export interface MintEnv {
  // The dedicated `runner_mint` consumer key (Server TL, key-split 2026-06-21): gates
  // ONLY /internal/v1/runner/{mint,revoke} — never signup-mint, erase, or admin (A6,
  // one notch tighter than pat_mint). Sent as `x-corelink-internal-auth` on both calls.
  CORELINK_RUNNER_MINT_AUTH_KEY?: string;
  CORELINK_MINT_URL?: string;
  CLW_ENDPOINT?: string;
  CLW_TENANT?: string;
  // Explicit, non-prod ESCAPE HATCH for the pre-env-0 transition ONLY. When env-0
  // (stash + fabricEndpoint) is NOT wired, the raw per-job PAT (`CLW_TOKEN`) is
  // injected into the untrusted container ONLY if this is set to "1". Absent/any
  // other value ⇒ spawn COLD (no PAT in the untrusted env) — fail-closed by
  // default (clw coordinator env-0 review must-fix #1, 2026-07-05). Never set in
  // prod: prod arms `SPAWN_WORKER_PUBLIC_URL` (env-0) instead.
  ALLOW_LEGACY_PAT_ENV?: string;
}

// ── Spawn idempotency (gap #2) — dedup a redelivered queued webhook ──────────
//
// GitHub redelivers a webhook (retries, at-least-once) — two `queued` deliveries
// for the SAME workflow_job.id would otherwise mint+spawn TWICE → double COGS
// (billing dedups on idem_key at completion, but the spawn cost does not). This
// is a per-jobId CLAIM in KV, set BEFORE the expensive mint+spawn: the first
// delivery wins the claim and spawns; a redelivery sees the claim and is a no-op.
//
// The minimal KV subset used (lib.ts is runtime-agnostic — no `KVNamespace`
// import). `RUNNER_JOB_PATS` satisfies this; we reuse it with a `spawn:` prefix
// so NO new wrangler binding is needed (the pat map uses the bare jobId key).
export interface KvLike {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void>;
  delete(key: string): Promise<void>;
  // Optional prefix listing (the real KVNamespace has it) — used ONLY by the
  // best-effort per-tenant concurrency counter. Absent ⇒ the counter fails open
  // (admits), never a gate. Kept optional so the spawn-claim path (which does not
  // list) still satisfies this runtime-agnostic subset.
  list?(options: { prefix: string }): Promise<{ keys: { name: string }[] }>;
}

// How long a spawn claim lives — past the longest CI job, a self-cleaning
// backstop (the claim is normally left to TTL-expire; only a FAILED spawn
// releases it early so a legitimate retry can re-spawn).
export const SPAWN_CLAIM_TTL_S = 7200;

function spawnClaimKey(jobId: string): string {
  return `spawn:${jobId}`;
}

/**
 * Try to CLAIM the spawn for `jobId`. Returns `true` if THIS caller won the claim
 * (it must proceed to mint+spawn), `false` if the job was already claimed (a
 * redelivery → skip, no double spawn).
 *
 * FAIL-OPEN by north-star: with no KV bound we cannot dedup, so we return `true`
 * (spawn) rather than block a job — dedup is an optimization on a correct path,
 * never a gate that can refuse a real job. Residual race: two EXACTLY-concurrent
 * deliveries can both read "absent" before either writes (KV has no atomic CAS);
 * this collapses the common case (retries seconds apart) and is the pragmatic
 * mitigation short of a Durable Object. Documented, not silently capped.
 */
export async function claimSpawn(kv: KvLike | undefined, jobId: string): Promise<boolean> {
  if (!kv) return true; // no dedup infra ⇒ fail-open to spawn (never block a job)
  const key = spawnClaimKey(jobId);
  const existing = await kv.get(key);
  if (existing) return false; // already claimed ⇒ a redelivery, skip
  await kv.put(key, "1", { expirationTtl: SPAWN_CLAIM_TTL_S });
  return true;
}

/**
 * Release a spawn claim — called ONLY when mint/spawn FAILED, so GitHub's retry
 * (or a re-queue) of the same job can claim again and actually spawn. A
 * successful spawn leaves the claim to TTL-expire (it must keep blocking
 * redeliveries for the job's lifetime). Best-effort: a delete failure just means
 * the claim TTL-expires (the job won't re-spawn until then — fail-safe, never a
 * double spawn).
 */
export async function releaseSpawnClaim(kv: KvLike | undefined, jobId: string): Promise<void> {
  if (!kv) return;
  await kv.delete(spawnClaimKey(jobId)).catch(() => {
    /* best-effort: TTL is the backstop */
  });
}

// Constant-time string compare (no early-exit on first mismatch) so a bearer/
// signature check can't be timing-probed. Length may leak (fixed-length,
// high-entropy tokens); the byte loop is constant-time.
export function safeEqual(a: string, b: string): boolean {
  const ea = new TextEncoder().encode(a);
  const eb = new TextEncoder().encode(b);
  if (ea.length !== eb.length) return false;
  let diff = 0;
  for (let i = 0; i < ea.length; i++) diff |= ea[i] ^ eb[i];
  return diff === 0;
}

// Verify GitHub's X-Hub-Signature-256 (HMAC-SHA256 of the raw body) in constant
// time. Fail-closed on a missing/short/mismatched signature.
export async function verifyGithubHmac(secret: string, sig: string, body: string): Promise<boolean> {
  if (!sig.startsWith("sha256=")) return false;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  const hex = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
  return safeEqual(`sha256=${hex}`, sig);
}

// The frozen mint-request inputs (server-derived-tenant seam, 2026-07-04). The
// Worker sends `repo_full_name` + `installation_id`; the SERVER derives the tenant
// (we no longer send `owner_tenant`). `job_id` is the GH workflow_job.id.
export interface MintParams {
  jobId: string;
  repoFullName: string; // evt.repository.full_name
  installationId: string; // evt.installation.id (stringified)
  scope?: string; // default "read-write"
  ttlSeconds?: number; // optional PAT TTL override
}

// The per-job CAS PAT mint result. `tenant` is the SERVER-DERIVED, authoritative
// tenant (injected as CLW_TENANT + used as the billed tenant); `maxConcurrency`
// is the per-tenant runner ceiling (absent ⇒ no ceiling enforced).
export interface MintResult {
  token: string;
  patId: string;
  tenant: string;
  maxConcurrency?: number;
}

// A 403 from the mint is a HARD DENY (installation not mapped / tenant suspended /
// repo not allowlisted / not runner-entitled). It is thrown as a DISTINCT error so
// the caller ABORTS the spawn — it must NEVER fail-open to a wrong-tenant cold
// spawn. Any OTHER failure (5xx, network) is a plain Error ⇒ fail-open to cold.
export class MintForbiddenError extends Error {
  constructor(message = "runner mint unauthorized") {
    super(message);
    this.name = "MintForbiddenError";
  }
}

// Mint a per-job CAS PAT via the runner-mint seam (corelink-server). The server
// DERIVES the tenant from installation_id + repo_full_name (authorization happens
// HERE): a 403 ⇒ MintForbiddenError (hard deny, abort); a 5xx/network ⇒ plain
// Error (caller falls open to a COLD spawn — cache absent ⇒ slow, never broken).
async function mintCasPat(env: MintEnv, params: MintParams): Promise<MintResult> {
  const base = env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com";
  const resp = await fetch(`${base}/internal/v1/runner/mint`, {
    method: "POST",
    headers: {
      "x-corelink-internal-auth": env.CORELINK_RUNNER_MINT_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    // FROZEN request body: NO owner_tenant (server derives the tenant).
    body: JSON.stringify({
      job_id: params.jobId,
      repo_full_name: params.repoFullName,
      installation_id: params.installationId,
      scope: params.scope ?? "read-write",
      ...(params.ttlSeconds != null ? { ttl_seconds: params.ttlSeconds } : {}),
    }),
  });
  // 403 FORBIDDEN ⇒ HARD DENY. Propagate a distinct error so the caller aborts
  // the spawn (no JIT, no container) rather than fail-open to a wrong-tenant cold.
  if (resp.status === 403) {
    throw new MintForbiddenError(
      `runner mint unauthorized (403): ${await resp.text().catch(() => "")}`,
    );
  }
  // Any other non-2xx (5xx D1 error "runner mint unavailable", etc.) ⇒ plain Error
  // ⇒ the caller MAY fail-open to a cold spawn (unchanged discipline).
  if (!resp.ok) throw new Error(`runner mint ${resp.status}`);
  // FROZEN 200 wire: {token_plaintext, pat_id, token_id, tenant (DERIVED,
  // AUTHORITATIVE), expires_ms, max_concurrency}. Keys logged on a miss so any
  // future drift is loud. A malformed 200 fails open to cold (not a hard deny).
  const j = (await resp.json()) as {
    token_plaintext?: string;
    pat_id?: string;
    tenant?: string;
    max_concurrency?: number;
  };
  if (!j.token_plaintext) {
    throw new Error(`runner mint: no token_plaintext (200 keys: ${Object.keys(j).join(",")})`);
  }
  if (!j.pat_id) {
    throw new Error(`runner mint: no pat_id (200 keys: ${Object.keys(j).join(",")})`);
  }
  if (!j.tenant) {
    throw new Error(`runner mint: no tenant (200 keys: ${Object.keys(j).join(",")})`);
  }
  return {
    token: j.token_plaintext,
    patId: j.pat_id,
    tenant: j.tenant,
    maxConcurrency: typeof j.max_concurrency === "number" ? j.max_concurrency : undefined,
  };
}

// Revoke a per-job CAS PAT via D-9 — keyed by `pat_id` (the live /revoke contract:
// `{owner_tenant, job_id}` → 400 "pat_id required"). The pat_id comes from the mint
// response and is carried across the queued→completed gap via the RUNNER_JOB_PATS KV
// (mint+revoke are separate Worker invocations). Throws on failure; caller swallows
// it (the PAT is TTL-bounded, so revoke is best-effort window-shrinking hardening).
export async function revokeCasPatById(
  env: MintEnv,
  patId: string,
  ownerTenant?: string,
): Promise<void> {
  const base = env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com";
  const resp = await fetch(`${base}/internal/v1/runner/revoke`, {
    method: "POST",
    headers: {
      "x-corelink-internal-auth": env.CORELINK_RUNNER_MINT_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    // Revoke keys on pat_id; owner_tenant is the SERVER-DERIVED tenant carried from
    // the mint (fallback: wrangler's CLW_TENANT for legacy single-tenant deploys).
    body: JSON.stringify({ pat_id: patId, owner_tenant: ownerTenant ?? env.CLW_TENANT }),
  });
  if (!resp.ok) throw new Error(`D-9 revoke ${resp.status}: ${await resp.text()}`);
}

// The result of the AUTHORIZE + warm-mint step. `authz` is the gate the caller
// MUST obey BEFORE minting a GitHub JIT:
//   • "forbidden" ⇒ 403 HARD DENY — abort: no JIT, no container.
//   • "ok"        ⇒ proceed. `containerEnv` carries the warm CLW_* (empty on a
//                   cold/fail-open spawn); `tenant`/`patId`/`maxConcurrency` are
//                   present iff the warm mint succeeded.
// NOTE: `containerEnv` is the CLW_* OVERLAY only — it does NOT include the JIT.
// The JIT is minted AFTER authorization and merged by the caller (spawnRunner),
// so an unauthorized repo never even gets a JIT.
export interface ContainerEnvResult {
  authz: "ok" | "forbidden";
  containerEnv: Record<string, string>;
  patId?: string;
  tenant?: string; // server-DERIVED tenant (billed + CLW_TENANT); warm only
  maxConcurrency?: number; // per-tenant ceiling; warm only
}

// ── env-0 (cred-ticket) — keep the CAS PAT OUT of the untrusted container env ──
//
// Instead of injecting CLW_TOKEN (the raw per-job PAT) into the untrusted
// container, the autoscaler STASHES the PAT server-side (a Durable Object latch)
// and injects a single-use, lease-bound CLW_CRED_TICKET. clw redeems it ONCE at
// its trusted boot against POST {CLW_FABRIC_ENDPOINT}/v1/leases/{id}/cas-cred and
// holds the PAT in-process. An `env` / `/proc/self/environ` dump inside the lease
// shows NO PAT — only a ticket that is 410/gone after the boot redemption. This
// mirrors the fabricd env-0 mechanism (crates/corelink-fabric-server/cred_ticket)
// so clw's already-merged CredentialSource redeems against the Worker identically.

/** The per-job credential stashed server-side, returned once on redemption. */
export interface StashedCred {
  token: string; // the per-job CAS PAT plaintext
  endpoint: string; // CLW_ENDPOINT (CAS/AC base)
  tenant: string; // the server-DERIVED CLW_TENANT
}

/**
 * Runtime-agnostic stash — the DO-backed single-use latch lives in index.ts (this
 * module stays free of `cloudflare:workers` imports). `stash` inserts the cred
 * under `ticket`, keyed by `leaseId`, self-cleaning after `ttlMs`.
 */
export interface CredStashLike {
  stash(leaseId: string, ticket: string, cred: StashedCred, ttlMs: number): Promise<void>;
}

// The cred-ticket + stash live as long as the longest CI job (mirrors the PAT/JIT
// TTLs); the DO alarm wipes the stash at this bound.
export const CRED_TICKET_TTL_S = 7200;

/** A 256-bit high-entropy hex ticket — the ticket string IS the secret. */
export function randomTicket(): string {
  const b = new Uint8Array(32);
  crypto.getRandomValues(b);
  return [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
}

/** The stash record persisted in DO storage (the value under key "rec"). */
export interface StashRecord {
  ticket: string;
  cred: StashedCred;
  expiresMs: number;
}

/**
 * PURE redeem decision (unit-testable without a DO runtime — the DO is a thin
 * wrapper that applies `consume`/`wipe` to its storage). Mirrors fabricd's
 * handlers/cas_cred order: 200 first-valid (returns the cred, `consume` the latch),
 * 401 bad ticket (constant-time), 410 already-redeemed (tombstone) OR expired
 * (`wipe`), 404 never-stashed. A bad ticket does NOT consume — only a correct one
 * spends the single use.
 */
export function decideRedeem(
  rec: StashRecord | undefined,
  consumedTombstone: boolean,
  nowMs: number,
  ticket: string,
): { status: number; cred?: StashedCred; consume?: boolean; wipe?: boolean } {
  if (!rec) return { status: consumedTombstone ? 410 : 404 };
  if (nowMs > rec.expiresMs) return { status: 410, wipe: true };
  if (!safeEqual(ticket, rec.ticket)) return { status: 401 };
  return { status: 200, cred: rec.cred, consume: true };
}

// AUTHORIZE the runner + build the cache-warm CLW_* overlay. Opt-in: only when the
// runner-mint key is configured AND we have the authz inputs (repo + installation).
// A 403 ⇒ authz:"forbidden" (HARD DENY — never a wrong-tenant cold spawn). A 5xx/
// network/malformed-200 ⇒ authz:"ok" with an EMPTY overlay (FAIL-OPEN to cold —
// the job still runs, uncached). CLW_TENANT is the SERVER-DERIVED tenant, never
// wrangler's CLW_TENANT var. Returns pat_id/tenant/max_concurrency on a warm mint.
//
// env-0: when `deps.stash` + `deps.fabricEndpoint` are provided, the PAT is
// STASHED and a single-use CLW_CRED_TICKET is injected INSTEAD of CLW_TOKEN — the
// untrusted container never sees the raw PAT. When they're absent, the default is
// FAIL-CLOSED (spawn COLD, no PAT) unless `env.ALLOW_LEGACY_PAT_ENV === "1"` is
// explicitly set (the non-prod pre-env-0 escape hatch). A stash FAILURE also never
// falls back to CLW_TOKEN — it spawns COLD (the whole point is no PAT in the untrusted env).
export async function buildContainerEnv(
  env: MintEnv,
  params: MintParams,
  deps?: { stash?: CredStashLike; fabricEndpoint?: string },
): Promise<ContainerEnvResult> {
  // No mint key, or not enough to authorize ⇒ COLD (legacy fail-open). We do NOT
  // authorize and do NOT warm — the job spawns without CLW_* under no tenant.
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY || !params.repoFullName || !params.installationId) {
    return { authz: "ok", containerEnv: {} };
  }
  try {
    const m = await mintCasPat(env, params);
    const endpoint = env.CLW_ENDPOINT ?? "https://corelink-api.humangr.com";
    // env-0 ON (stash + fabric endpoint configured): stash the PAT, inject a
    // single-use ticket — NEVER CLW_TOKEN. A stash failure spawns COLD (no leak).
    if (deps?.stash && deps?.fabricEndpoint) {
      const ticket = randomTicket();
      try {
        await deps.stash.stash(
          params.jobId,
          ticket,
          { token: m.token, endpoint, tenant: m.tenant },
          CRED_TICKET_TTL_S * 1000,
        );
      } catch (e) {
        // Stash failed ⇒ we CANNOT do env-0. Never fall back to CLW_TOKEN — spawn
        // COLD (the minted PAT is undelivered and TTL-expires). No PAT ever leaks.
        console.log(`cred-stash failed, spawning COLD (no token leaked): ${(e as Error).message}`);
        return { authz: "ok", containerEnv: {} };
      }
      return {
        authz: "ok",
        containerEnv: {
          CLW_ENDPOINT: endpoint,
          CLW_TENANT: m.tenant, // server-DERIVED, authoritative (NEVER wrangler's var)
          CLW_CRED_TICKET: ticket, // single-use; redeemed once at clw boot
          CLW_LEASE_ID: params.jobId, // the redemption key (= GH jobId)
          CLW_FABRIC_ENDPOINT: deps.fabricEndpoint, // where clw redeems the ticket
          CLW_REF_DOMAIN: "runner",
        },
        patId: m.patId,
        tenant: m.tenant,
        maxConcurrency: m.maxConcurrency,
      };
    }
    // env-0 NOT configured. FAIL-CLOSED by default: never silently inject the raw
    // PAT (`CLW_TOKEN`) into the untrusted container. The legacy PAT overlay is a
    // pre-env-0 transition escape hatch, gated behind an EXPLICIT non-prod flag
    // (`ALLOW_LEGACY_PAT_ENV="1"`) — coordinator env-0 review must-fix #1. Without
    // it we spawn COLD: the minted PAT is undelivered (TTL-expires), no leak. In
    // prod, env-0 (`SPAWN_WORKER_PUBLIC_URL`) is armed, so this branch is dead.
    if (env.ALLOW_LEGACY_PAT_ENV !== "1") {
      console.log(
        "env-0 not configured and ALLOW_LEGACY_PAT_ENV not set: spawning COLD " +
          "(no raw PAT in the untrusted container env)",
      );
      return { authz: "ok", containerEnv: {} };
    }
    // Legacy (explicit non-prod opt-in) — pre-launch transition only: inject CLW_TOKEN.
    console.log("ALLOW_LEGACY_PAT_ENV=1: injecting legacy CLW_TOKEN (non-prod transition path)");
    return {
      authz: "ok",
      containerEnv: {
        CLW_ENDPOINT: endpoint,
        CLW_TENANT: m.tenant, // server-DERIVED, authoritative (NEVER wrangler's var)
        CLW_TOKEN: m.token, // per-job; never logged
        CLW_REF_DOMAIN: "runner",
      },
      patId: m.patId,
      tenant: m.tenant,
      maxConcurrency: m.maxConcurrency,
    };
  } catch (e) {
    if (e instanceof MintForbiddenError) {
      // 403 HARD DENY ⇒ ABORT. An unauthorized repo must not run at all — never
      // let a 403 degrade into a (wrong-tenant) cold spawn.
      console.log(`runner mint FORBIDDEN (aborting spawn): ${(e as Error).message}`);
      return { authz: "forbidden", containerEnv: {} };
    }
    // 5xx ("runner mint unavailable") / network / malformed-200 ⇒ FAIL-OPEN to
    // cold. The entrypoint's cache-warm hook is also fail-open — slow, never broken.
    console.log(`warm-mint failed, spawning COLD: ${(e as Error).message}`);
    return { authz: "ok", containerEnv: {} };
  }
}

// ── Per-tenant concurrency ceiling (max_concurrency) — best-effort fairness ────
//
// One KV key per in-flight (tenant, job): `conc:<tenant>:<jobId>`, TTL-bounded so
// a MISSED release SELF-HEALS (the slot expires) — it can never become a permanent
// gate. The live count is the number of keys under the tenant prefix. Best-effort
// + FAIL-OPEN by north-star: no KV / no `list` / any error ⇒ ADMIT. Fairness must
// never refuse a legitimately-under-ceiling job. Residual: two exactly-concurrent
// admits can both see `< max` (KV has no atomic CAS) ⇒ a transient +1 overshoot
// that self-heals; the common case (bursts seconds apart) is collapsed.
export const TENANT_SLOT_TTL_S = 2700; // 45m — matches the container sleepAfter backstop

function tenantSlotKey(tenant: string, jobId: string): string {
  return `conc:${tenant}:${jobId}`;
}

/**
 * Try to ACQUIRE a runner slot for `tenant`. Returns `true` (admit → spawn) if the
 * tenant is under `max`, `false` (refuse → no spawn) if already at the ceiling.
 * FAIL-OPEN (admit) with no KV, no `list`, or any KV error.
 */
export async function acquireTenantSlot(
  kv: KvLike | undefined,
  tenant: string,
  jobId: string,
  max: number,
): Promise<boolean> {
  if (!kv || !kv.list) return true; // no infra ⇒ fail-open (admit)
  try {
    const { keys } = await kv.list({ prefix: `conc:${tenant}:` });
    if (keys.length >= max) return false; // at ceiling ⇒ refuse (no spawn)
    await kv.put(tenantSlotKey(tenant, jobId), "1", { expirationTtl: TENANT_SLOT_TTL_S });
    return true;
  } catch {
    return true; // KV error ⇒ fail-open (never block a real job)
  }
}

/**
 * Release a tenant's runner slot (called at job completion, and on a spawn failure
 * after acquiring). Best-effort: a delete failure just leaves the slot to TTL-expire.
 */
export async function releaseTenantSlot(
  kv: KvLike | undefined,
  tenant: string,
  jobId: string,
): Promise<void> {
  if (!kv) return;
  await kv.delete(tenantSlotKey(tenant, jobId)).catch(() => {
    /* best-effort: the slot TTL-expires */
  });
}

// ── Autoscaler re-drive reconciler — recover jobs orphaned by a failed spawn ───
//
// GitHub fires workflow_job.queued ONCE (at-least-once, but no periodic re-drive).
// If the webhook's spawn transiently fails (a CF DO-start reset, even past the
// #268 retry+waitUntil), the job sits QUEUED forever — no runner, no re-delivery
// (observed 2026-07-04: probes/CI stuck queued for hours until a manual dispatch).
// This scheduled reconciler lists queued+labeled+runnerLESS jobs older than a
// grace window (so it never races the normal webhook path) and re-drives their
// spawn. It re-drives COLD (the jobs API carries no installation_id, so no CAS-PAT
// mint / no per-job authz) — a runner that RUNS beats an orphan — so it is scoped
// to an explicit first-party allowlist (RECONCILER_REPOS); only trusted repos
// belong there. The claim-KV dedups it against the webhook + other ticks.

// Grace window: a job younger than this is left to the webhook path (avoid racing
// a spawn that's still in-flight in ctx.waitUntil). Past it, an un-runnered job is
// treated as orphaned and re-driven.
export const RECONCILE_MIN_AGE_MS = 90_000;

/** Parse the comma/space-separated RECONCILER_REPOS allowlist (owner/repo). */
export function parseReconcilerRepos(csv: string | undefined): string[] {
  if (!csv) return [];
  return csv
    .split(/[,\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.includes("/"));
}

/** The subset of Env the reconciler's GitHub listing reads. */
export interface ReconcilerEnv {
  GITHUB_MINT_TOKEN?: string;
}

interface GhRun {
  id: number;
  created_at: string;
}
interface GhJob {
  id: number;
  status: string;
  runner_id: number | null;
  labels: string[];
}

/**
 * List queued+labeled jobs with NO runner assigned, older than `minAgeMs`, in
 * `repo`. Returns their jobIds (stringified — the autoscaler's stable id). Goes
 * via queued RUNS (which carry created_at) → their jobs. Best-effort: any GitHub
 * error returns [] (the reconciler is a backstop, never itself a gate).
 */
export async function listOrphanRunnerJobs(
  env: ReconcilerEnv,
  repo: string,
  label: string,
  minAgeMs: number,
  nowMs: number,
): Promise<string[]> {
  const gh = async (path: string): Promise<unknown> => {
    const r = await fetch(`https://api.github.com${path}`, {
      headers: {
        authorization: `Bearer ${env.GITHUB_MINT_TOKEN ?? ""}`,
        accept: "application/vnd.github+json",
        "user-agent": "corelink-spawn-worker",
      },
    });
    if (!r.ok) throw new Error(`GH ${path} ${r.status}`);
    return r.json();
  };
  try {
    const runs = (await gh(`/repos/${repo}/actions/runs?status=queued&per_page=30`)) as {
      workflow_runs?: GhRun[];
    };
    const orphans: string[] = [];
    for (const run of runs.workflow_runs ?? []) {
      const age = nowMs - Date.parse(run.created_at);
      if (!Number.isFinite(age) || age < minAgeMs) continue; // too fresh: leave it to the webhook
      const jobs = (await gh(`/repos/${repo}/actions/runs/${run.id}/jobs`)) as { jobs?: GhJob[] };
      for (const j of jobs.jobs ?? []) {
        if (j.status === "queued" && j.runner_id == null && (j.labels ?? []).includes(label)) {
          orphans.push(String(j.id));
        }
      }
    }
    return orphans;
  } catch (e) {
    console.log(`reconciler list failed for ${repo} (backstop, skipping): ${(e as Error).message}`);
    return [];
  }
}

// ── Billing usage-push (ASK-2) — per-completed-job runner_slot_seconds ────────
//
// The prod (all-Cloudflare) home for billing: the Rust `corelink-fabricd`
// billing-push is the DEV path; in prod the spawn-Worker IS the runner path, so
// the usage event is emitted here on `workflow_job:completed`. Wire: a JSON batch
// of one `UsageEvent` to corelink-billing ingest, dedicated `BILLING_INGEST_AUTH_KEY`
// (never the shared key). The aggregator owns rollup/chain/dedup — we send raw,
// at-least-once, idempotent by `idem_key`. FAIL-OPEN: any error is swallowed by the
// caller (billing never breaks the webhook).

/** The subset of Env the billing usage-push reads. */
export interface BillingEnv {
  // corelink-billing ingest endpoint, e.g.
  // https://corelink-api.humangr.com/internal/v1/billing/usage. Absent ⇒ no push.
  BILLING_INGEST_URL?: string;
  // Dedicated `x-corelink-internal-auth` value for billing ingest (NEVER the
  // shared internal key, nor the runner_mint key). Worker secret. Absent ⇒ no push.
  BILLING_INGEST_AUTH_KEY?: string;
  // 3-char region stamped on the event (Cloudflare colo by default — ADR-0008).
  // Falls back to the request's CF colo when unset.
  BILLING_REGION?: string;
  // The owner tenant the job was minted under = the billed tenant_id.
  CLW_TENANT?: string;
}

const BILLING_SOURCE = "corelink-runners/spawn-worker";
// The canonical wire string the Server TL pinned (ASK-2 final, 2026-06-23).
const RUNNER_SLOT_SECONDS_KIND = "runner_slot_seconds";

/** `"YYYY-MM"` (UTC) for an epoch-ms instant. */
export function billingPeriod(atMs: number): string {
  const d = new Date(atMs);
  return `${d.getUTCFullYear()}-${String(d.getUTCMonth() + 1).padStart(2, "0")}`;
}

/**
 * Deterministic 64-hex idempotency key = SHA-256(jobId ‖ period). The ingest
 * dedups on it opaquely, so any stable 64-hex is valid (the contract says
 * "e.g. BLAKE3"); SHA-256 is what the Workers runtime provides natively.
 */
export async function usageIdemKey(jobId: string, period: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`${jobId}|${period}`));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** One raw usage event — the per-event wire shape the aggregator ingests. */
export interface UsageEvent {
  tenant_id: string;
  event_kind: string;
  qty: number;
  billing_period: string;
  region: string;
  source: string;
  time_ms: number;
  idem_key: string;
}

/**
 * Pure builder (vitest-testable, no fetch): the `runner_slot_seconds` event for
 * one completed job. `qty = floor((completedMs − startedMs)/1000)`, clamped ≥ 0
 * (a clock skew / missing start never bills negative). `idem_key` is per
 * (job, period) so an at-least-once re-send dedups.
 */
export async function buildUsageEvent(opts: {
  tenantId: string;
  jobId: string;
  startedMs: number;
  completedMs: number;
  region: string;
}): Promise<UsageEvent> {
  const qty = Math.max(0, Math.floor((opts.completedMs - opts.startedMs) / 1000));
  const period = billingPeriod(opts.completedMs);
  return {
    tenant_id: opts.tenantId,
    event_kind: RUNNER_SLOT_SECONDS_KIND,
    qty,
    billing_period: period,
    region: opts.region,
    source: BILLING_SOURCE,
    time_ms: opts.completedMs,
    idem_key: await usageIdemKey(opts.jobId, period),
  };
}

/**
 * POST a batch of one usage event to corelink-billing ingest with the dedicated
 * key. Throws on a non-2xx (the caller swallows it — fail-open). The ingest
 * returns `{accepted, deduped, total}`; we only need the 2xx (the aggregator
 * reconciles, and idem_key makes a retry safe).
 */
export async function pushUsageEvent(env: BillingEnv, ev: UsageEvent): Promise<void> {
  const resp = await fetch(env.BILLING_INGEST_URL ?? "", {
    method: "POST",
    headers: {
      "x-corelink-internal-auth": env.BILLING_INGEST_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    body: JSON.stringify([ev]),
  });
  if (!resp.ok) throw new Error(`billing usage-push ${resp.status}`);
}
