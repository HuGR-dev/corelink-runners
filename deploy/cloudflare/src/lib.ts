// Pure, runtime-agnostic helpers for the spawn-Worker. NO `cloudflare:workers` /
// `@cloudflare/containers` imports here — so this module is unit-testable in
// plain vitest (node): only `crypto` + `fetch` (Node 20+ globals) are used.

// ── Structured worker log — single-line JSON, queryable in CF Logs ───────────
//
// Swaps the historical bare `console.log`/`console.error` free-text calls for a
// single-line JSON event so a log query (Logpush/Tail) can filter/aggregate on
// `event` + fields instead of regexing prose. Same information, just structured.
// `Date.now()` is fine here (this runs in the Worker at request/scheduled time,
// not at module-eval / cold-start).
export function logEvent(
  level: "info" | "error",
  event: string,
  fields?: Record<string, unknown>,
): void {
  const line = JSON.stringify({ level, event, ...fields, ts: Date.now() });
  if (level === "error") console.error(line);
  else console.log(line);
}

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
  // F2-5 (W3): the PROD marker. Its presence means env-0 is armed in this deploy, so
  // the legacy raw-PAT overlay is refused as defense-in-depth even if
  // ALLOW_LEGACY_PAT_ENV="1" were mis-set — a raw CLW_TOKEN can NEVER reach an
  // untrusted container in a prod deploy (3-lens audit F2/Lens B).
  SPAWN_WORKER_PUBLIC_URL?: string;
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
  // Optional prefix listing (the real KVNamespace has it). No longer consumed by
  // the spawn-Worker (the per-tenant concurrency counter that used it moved to the
  // atomic ConcurrencySlotsDO in W7/F7); kept optional for KVNamespace shape
  // parity so the real binding still satisfies this runtime-agnostic subset.
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

// ── Completed-leg dedup (WP-2 2c) — dedup a redelivered `completed` webhook ────
//
// GitHub redelivers `workflow_job:completed` too (at-least-once). Without a guard
// each redelivery re-bumps the `webhook_job_completed` golden signal → an inflated
// counter (a false "N completions" reading). This is a per-jobId COMPLETION claim
// in KV, mirroring `claimSpawn`: the FIRST completion wins the claim (counts once);
// a redelivery sees the claim and is a COUNTER no-op.
//
// CRUCIAL: this gates ONLY the metric bump. The SECURITY actions on the completed
// leg (revoke the per-job PAT, release the concurrency slot, tear the container
// down) are NOT gated by this — they are each independently idempotent and
// self-healing, so a redelivery still safely re-runs them (revoke of an already-
// deleted PAT is a no-op, teardown() is idempotent, slot-release self-heals).
//
// Short TTL: a `completed` redelivery lands within GitHub's retry window (minutes),
// so the claim need only outlive that — NOT the whole job (unlike the spawn claim,
// which must block redeliveries for the job's entire lifetime). Distinct `done:`
// prefix, never colliding with the `spawn:`/`conc:`/`jtenant:`/bare-jobId keys.
export const COMPLETION_CLAIM_TTL_S = 3600; // 1h — spans GitHub's redelivery window

function completionClaimKey(jobId: string): string {
  return `done:${jobId}`;
}

/**
 * Try to CLAIM the completion counter for `jobId`. Returns `true` if THIS is the
 * FIRST completion (the caller SHOULD bump `webhook_job_completed`), `false` if the
 * job was already counted (a redelivery → skip the bump so the counter is a no-op).
 *
 * FAIL-OPEN by north-star: with no KV bound we cannot dedup, so we return `true`
 * (count) rather than drop a real completion — the security actions run regardless,
 * and dedup is an optimization on a correct path, never a gate. Residual race: two
 * EXACTLY-concurrent redeliveries can both read "absent" (KV has no atomic CAS);
 * this collapses the common case (retries seconds apart), same as `claimSpawn`.
 */
export async function claimCompletion(kv: KvLike | undefined, jobId: string): Promise<boolean> {
  if (!kv) return true; // no dedup infra ⇒ count (never drop a real completion)
  const key = completionClaimKey(jobId);
  const existing = await kv.get(key);
  if (existing) return false; // already counted ⇒ a redelivery, skip the bump
  await kv.put(key, "1", { expirationTtl: COMPLETION_CLAIM_TTL_S });
  return true;
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
  // Option-C (per-tenant-PAT dispatch, server confirmed live 2026-07-21): when set,
  // the mint resolves the tenant by INTROSPECTING this acquiring PAT instead of
  // deriving it from installation_id. Presented as `Authorization: Bearer <pat>`
  // ALONGSIDE the dispatcher's `x-corelink-internal-auth` (both required — the
  // internal-auth is still the trust boundary; the PAT only names the tenant), and
  // `installation_id` is OMITTED from the body (server rejects a null/"" as
  // malformed → 400). Used for repos in REPO_TENANT_PAT_MAP; empty map ⇒ never set.
  acquiringPat?: string;
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
  // Option-C (per-tenant-PAT dispatch): resolve the tenant by introspecting the
  // acquiring PAT. The dispatcher trust boundary (x-corelink-internal-auth) is
  // UNCHANGED — the Bearer PAT is additive and only names the tenant (server
  // runner_mint.ts:407-427). `installation_id` MUST be omitted entirely (a null/""
  // is rejected as malformed → 400); server-confirmed scope for this path is cas:rw.
  const optionC = !!params.acquiringPat;
  const headers: Record<string, string> = {
    "x-corelink-internal-auth": env.CORELINK_RUNNER_MINT_AUTH_KEY ?? "",
    "content-type": "application/json",
    "user-agent": "corelink-spawn-worker",
  };
  if (optionC) headers["authorization"] = `Bearer ${params.acquiringPat}`;
  const resp = await fetch(`${base}/internal/v1/runner/mint`, {
    method: "POST",
    headers,
    // FROZEN request body: NO owner_tenant (server derives the tenant). Option-C
    // OMITS installation_id (tenant comes from PAT introspection) and pins scope
    // cas:rw; the default installation-derived path is byte-identical to before.
    body: JSON.stringify({
      job_id: params.jobId,
      repo_full_name: params.repoFullName,
      ...(optionC ? {} : { installation_id: params.installationId }),
      scope: params.scope ?? (optionC ? "cas:rw" : "read-write"),
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
// and injects a lease-bound CLW_CRED_TICKET. clw redeems the ticket against POST
// {CLW_FABRIC_ENDPOINT}/v1/leases/{id}/cas-cred to obtain the PAT. An `env` /
// `/proc/self/environ` dump inside the lease shows NO raw PAT — only the ticket.
//
// MULTI-USE within the lease (NOT single-use — honest correction, 3-lens audit
// F2/Lens B flagged the old "410/gone after boot" claim as false): the runner has
// TWO clw processes that each redeem — the boot `clw hydrate` + the job's `clw run`
// (corelink-memoize) — so the cred is served on EVERY redeem until the lease TTL
// wipes the stash (see `decideRedeem` + CredStashDO). This is a DELIBERATE,
// coordinator-ACKed envelope: the cred is a per-job, tenant-scoped `cas:rw` PAT
// with no escalation over the job's OWN cache access, so an in-lease redeem grants
// nothing the job doesn't already hold. The exposure window is further bounded by
// wiping the stash at job completion (see the workflow_job:completed handler).
// Mirrors the fabricd env-0 mechanism (crates/corelink-fabric-server/cred_ticket).

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
  // Returns the EFFECTIVE ticket: idempotent per lease — if a live stash already
  // exists for `leaseId` (an earlier/concurrent spawn attempt for the same jobId),
  // the existing ticket is kept and returned, NOT overwritten. This is what makes
  // env-0 survive the spawn-reliability retries: every attempt's container is given
  // the SAME ticket, so whichever container actually registers redeems a ticket the
  // CRED_STASH DO still recognizes (a fresh ticket per attempt would orphan it).
  stash(leaseId: string, ticket: string, cred: StashedCred, ttlMs: number): Promise<string>;
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
 * wrapper that applies `wipe` to its storage).
 *
 * MULTI-USE within the lease (2026-07-06): a live lease returns its cred on EVERY
 * redeem — NOT single-use. Root cause it fixes: the runner has TWO clw processes
 * that each need the cred (the boot `clw hydrate` + the job's `clw run` via
 * corelink-memoize, the product path); a single-use ticket was consumed by the
 * boot hydrate, starving the job → "missing token" → COLD. Each clw now redeems
 * IN-PROCESS (the PAT never persists in the env or on disk — env-0's goal). The
 * ticket stays lease-bound + short-lived (410 the moment the lease expires).
 * Order: 200 (live + correct ticket, returns the cred), 401 bad ticket
 * (constant-time), 410 expired (`wipe`), 404 never-stashed.
 */
export function decideRedeem(
  rec: StashRecord | undefined,
  _consumedTombstone: boolean, // unused under multi-use; kept for wrapper compat
  nowMs: number,
  ticket: string,
): { status: number; cred?: StashedCred; wipe?: boolean } {
  if (!rec) return { status: 404 };
  if (nowMs > rec.expiresMs) return { status: 410, wipe: true };
  if (!safeEqual(ticket, rec.ticket)) return { status: 401 };
  return { status: 200, cred: rec.cred }; // MULTI-USE: no consume — cred served until expiry
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
  // Authorizable when we have EITHER an installation_id (tenant derived from it) OR
  // an acquiring PAT (Option-C: tenant derived by introspection).
  if (
    !env.CORELINK_RUNNER_MINT_AUTH_KEY ||
    !params.repoFullName ||
    (!params.installationId && !params.acquiringPat)
  ) {
    return { authz: "ok", containerEnv: {} };
  }
  try {
    const m = await mintCasPat(env, params);
    const endpoint = env.CLW_ENDPOINT ?? "https://corelink-api.humangr.com";
    // env-0 ON (stash + fabric endpoint configured): stash the PAT, inject a
    // single-use ticket — NEVER CLW_TOKEN. A stash failure spawns COLD (no leak).
    if (deps?.stash && deps?.fabricEndpoint) {
      // The stash is idempotent per lease: it returns the EFFECTIVE ticket (the
      // existing one if a prior spawn attempt for this jobId already stashed, else
      // the fresh one). Inject whatever it returns so retries converge on one ticket.
      let ticket: string;
      try {
        ticket = await deps.stash.stash(
          params.jobId,
          randomTicket(),
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
          CLW_CRED_TICKET: ticket, // multi-use, lease-scoped; redeemed in-process by each clw
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
    // F2-5 (W3): refuse the legacy raw-PAT overlay whenever the PROD marker
    // (SPAWN_WORKER_PUBLIC_URL) is present — even if ALLOW_LEGACY_PAT_ENV="1" was
    // mis-set and the env-0 deps weren't passed. In prod the env-0 branch above
    // already wins; this closes the residual "deps missing + flag mis-set in prod"
    // hole so a raw PAT can NEVER reach an untrusted container in a prod deploy.
    const legacyRefusedInProd = env.ALLOW_LEGACY_PAT_ENV === "1" && !!env.SPAWN_WORKER_PUBLIC_URL;
    if (env.ALLOW_LEGACY_PAT_ENV !== "1" || legacyRefusedInProd) {
      console.log(
        legacyRefusedInProd
          ? "ALLOW_LEGACY_PAT_ENV=1 REFUSED (SPAWN_WORKER_PUBLIC_URL set ⇒ prod env-0 armed): spawning COLD"
          : "env-0 not configured and ALLOW_LEGACY_PAT_ENV not set: spawning COLD (no raw PAT in the untrusted container env)",
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

// ── Concurrency slots (W7/F7) — ATOMIC per-key + fleet cap, DO-backed ─────────
//
// Concurrency is the BILLING SKU, so the ceiling must be REAL — not a best-effort
// KV read-then-write (which was NON-atomic: two concurrent admits both saw
// `< max` → +1 overshoot; and FAIL-OPEN, so a customer could exceed what they
// bought). The count now lives in a single `ConcurrencySlotsDO` key; the DO's
// single-threaded input-gating makes the read-modify-write below ATOMIC (no CAS
// race). The pure decision lives HERE (runtime-agnostic, unit-testable); the DO
// (index.ts) is a thin wrapper that persists `decideSlotAcquire(...).slots`.
//
// The cap is enforced on BOTH warm mints (per-tenant entitlement) AND cold spawns
// (per-repo `COLD_REPO_CAP`) — the old KV path skipped cold spawns entirely, so a
// COLD spawn had NO ceiling → unlimited runners. A fleet cap (`FLEET_MAX_CONCURRENCY`,
// mirroring the RunnerContainer max_instances) bounds the global physical fleet.

// A live slot lives no longer than this even if its release is missed — the TTL is
// the self-heal backstop (matches the old TENANT_SLOT_TTL_S 45m container backstop).
export const SLOT_TTL_S = 2700;
// The physical fleet cap — mirrors the RunnerContainer `max_instances` in
// wrangler.jsonc. Warm admission clamps the per-tenant entitlement to this, and it
// is the global ceiling across ALL keys (tenants + cold repos).
export const FLEET_MAX_CONCURRENCY = 20;
// The per-repo ceiling for COLD spawns (no derived tenant, so no entitlement).
export const COLD_REPO_CAP = 8;

export interface SlotRecord {
  key: string;
  jobId: string;
  expiresMs: number;
}
export interface SlotAcquireDecision {
  admitted: boolean;
  reason?: "over_key_cap" | "over_fleet_cap";
  slots: SlotRecord[];
}

/**
 * PURE slot-acquire decision (unit-testable; the DO applies `slots` to storage).
 * Prunes expired slots first, then decides. IDEMPOTENT per jobId: if this jobId
 * already holds a LIVE slot (a retry), it is re-admitted WITHOUT double-counting.
 * Enforces the per-key cap FIRST (a tenant/repo can't exceed its own ceiling),
 * THEN the fleet cap (the global physical bound). On admit, appends the slot.
 */
export function decideSlotAcquire(
  slots: SlotRecord[],
  key: string,
  jobId: string,
  perKeyCap: number,
  fleetCap: number,
  nowMs: number,
  ttlMs: number,
): SlotAcquireDecision {
  const live = slots.filter((s) => s.expiresMs > nowMs);
  // Idempotent re-admit: a retry for a jobId already holding a live slot must not
  // double-count (the DO read-modify-write can be re-driven by a redelivery).
  if (live.some((s) => s.jobId === jobId)) {
    return { admitted: true, slots: live };
  }
  const perKey = live.filter((s) => s.key === key).length;
  if (perKey >= perKeyCap) return { admitted: false, reason: "over_key_cap", slots: live };
  if (live.length >= fleetCap) return { admitted: false, reason: "over_fleet_cap", slots: live };
  return {
    admitted: true,
    slots: [...live, { key, jobId, expiresMs: nowMs + ttlMs }],
  };
}

/**
 * Release a slot by jobId ONLY (jobId is globally unique, so the key is not needed
 * at release — completion/failure can release without re-deriving the key). Also
 * prunes expired slots (self-heal, same as the acquire path).
 */
export function releaseSlotByJob(
  slots: SlotRecord[],
  jobId: string,
  nowMs: number,
): SlotRecord[] {
  return slots.filter((s) => s.expiresMs > nowMs && s.jobId !== jobId);
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

/**
 * Look up a repo's installation_id from the REPO_INSTALLATION_MAP JSON. A repo
 * webhook carries no `installation.id`; for known first-party repos we inject it
 * so the server-derived mint (#283) runs WARM. Returns "" when the map is
 * absent/malformed or the repo isn't listed (⇒ COLD, fail-open — never throws).
 */
export function installationIdForRepo(json: string | undefined, repoFullName: string): string {
  if (!json || !repoFullName) return "";
  try {
    const map = JSON.parse(json) as Record<string, unknown>;
    const v = map[repoFullName];
    return typeof v === "string" ? v : typeof v === "number" ? String(v) : "";
  } catch {
    return "";
  }
}

/**
 * Option-C per-tenant-PAT dispatch (server-confirmed live 2026-07-21). Look up a
 * repo in REPO_TENANT_PAT_MAP — a JSON `{ "<owner/repo>": "<SECRET_ENV_NAME>" }`
 * that maps a repo to the NAME of the secret binding holding that tenant's
 * acquiring PAT (the raw PAT is a Worker secret, never in this var). Returns the
 * secret NAME, or "" when the map is absent/malformed or the repo isn't listed
 * (⇒ default installation-derived mint). Never throws. The caller reads
 * `env[<name>]` to get the PAT, so a mapped-but-unbound secret still falls back to
 * the default path (no PAT ⇒ no Option-C).
 */
export function tenantPatSecretForRepo(json: string | undefined, repoFullName: string): string {
  if (!json || !repoFullName) return "";
  try {
    const map = JSON.parse(json) as Record<string, unknown>;
    const v = map[repoFullName];
    return typeof v === "string" ? v : "";
  } catch {
    return "";
  }
}

// ── External-GA installation allowlist (WP-D) ────────────────────────────────
//
// The webhook's ONLY identity-authz today is the server's post-cold-spawn 403
// (lib mint seam) / JIT-404 — both fire AFTER a cold spawn attempt has already
// burned a spawn-claim + a COLD_REPO_CAP slot + (on failure) a dead-letter
// orphan. A foreign repo where the GitHub App is installed but NOT entitled can
// therefore churn/DoS the shared FLEET_MAX_CONCURRENCY before the server ever
// says no. This allowlist is the pre-mint gate that stops an un-entitled
// installation at the Worker edge.
//
// OPT-IN, FAIL-CLOSED-WHEN-ARMED: unset/blank ⇒ NOT armed ⇒ preserve today's
// exact behavior (never breaks the live deploy). When armed (≥1 id parsed), an
// installation id absent from the list is refused BEFORE any mint/spawn/claim.
// Comma- OR whitespace-separated ids. e.g. arm with
// INSTALLATION_ALLOWLIST="144561227,<customer-install-id>" (144561227 = the
// dogfood installation — it MUST stay served).

/** Parse the comma/whitespace-separated installation-id list. Non-throwing;
 *  returns the trimmed, non-empty ids (order/dupes irrelevant to membership). */
export function parseInstallationAllowlist(raw: string | undefined): string[] {
  if (!raw) return [];
  return raw
    .split(/[,\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** True iff the allowlist is ARMED — i.e. it parses to ≥1 id. An unset/blank/
 *  whitespace-only value is NOT armed (fail-safe: today's behavior is preserved). */
export function installationAllowlistArmed(raw: string | undefined): boolean {
  return parseInstallationAllowlist(raw).length > 0;
}

/** True iff `installationId` may proceed under the allowlist. When NOT armed,
 *  everything proceeds (returns true). When armed, only a non-empty id that is a
 *  member of the list proceeds (an empty/unknown id is refused). */
export function isInstallationAllowlisted(raw: string | undefined, installationId: string): boolean {
  const allow = parseInstallationAllowlist(raw);
  if (allow.length === 0) return true; // not armed ⇒ preserve current behavior
  if (!installationId) return false; // armed + unknown/empty id ⇒ refuse
  return allow.includes(installationId);
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
// The managed-label prefix + bare root — the CoreLink runner fleet's label
// family. A job is served if it carries the bare `corelink` label OR any
// `corelink-<suffix>` (covers the internal `corelink-dogfood` AND the product
// labels `corelink` / `corelink-<size>`). Single source of truth so the webhook
// gate + both reconciler scans agree on what "our fleet" means.
export const MANAGED_LABEL_ROOT = "corelink";
export const MANAGED_LABEL_PREFIX = "corelink-";

// RESERVED labels the autoscaler must NEVER mint an ephemeral runner for, even
// though they are in the `corelink-*` prefix space. `corelink-builder` is the
// PERSISTENT self-hosted builder pool — minting an ephemeral runner that
// advertises it would RACE the always-on builder (and let `runs-on:
// [corelink, corelink-builder]` be assigned a privileged builder job on our
// ephemeral box). The sibling Rust webhook gate excludes it for exactly this
// reason (crates/corelink-fabric-server/src/handlers/webhook.rs:82-83,508-518).
const RESERVED_LABELS = new Set<string>(["corelink-builder"]);

// Labels a job may carry ALONGSIDE a corelink label without us refusing it —
// `self-hosted` is the GitHub runner-group label a customer commonly combines
// with a fleet label; it does not route to a foreign pool. Any OTHER non-corelink
// label means the job needs a runner we don't provide.
const PASSTHROUGH_LABELS = new Set<string>(["self-hosted"]);

function isServableCorelinkLabel(l: string): boolean {
  return (l === MANAGED_LABEL_ROOT || l.startsWith(MANAGED_LABEL_PREFIX)) && !RESERVED_LABELS.has(l);
}

/**
 * The corelink labels to mint the JIT runner with for `jobLabels`, or `null` to
 * REFUSE the job. A SUBSET gate mirroring the Rust webhook (webhook.rs:508-518):
 *
 * - `configured` set (AUTOSCALER_LABEL) → serve iff the job carries EXACTLY that
 *   pin (plus passthrough labels); returns `[configured]`.
 * - `configured` unset → the `corelink` FAMILY minus RESERVED: serve iff the job
 *   has ≥1 servable corelink label AND EVERY other label is a passthrough
 *   (`self-hosted`). Returns ALL the servable corelink labels so the minted
 *   runner advertises exactly what the job requested and GitHub can assign it.
 *
 * Refusing (null) when a job also carries a NON-corelink, non-passthrough label
 * (e.g. `runs-on: [corelink, gpu]`) is deliberate: minting a `corelink`-only
 * runner for such a job would never be assigned by GitHub → the job hangs and the
 * orphan reconciler re-spawns it forever. And RESERVED labels (`corelink-builder`)
 * are never servable, so we can't poach the persistent builder pool.
 */
export function matchManagedLabels(
  jobLabels: string[],
  configured: string | undefined,
): string[] | null {
  if (jobLabels.length === 0) return null;
  const cfg = configured?.trim();
  if (cfg) {
    if (RESERVED_LABELS.has(cfg) || !jobLabels.includes(cfg)) return null;
    return jobLabels.every((l) => l === cfg || PASSTHROUGH_LABELS.has(l)) ? [cfg] : null;
  }
  const corelink = jobLabels.filter(isServableCorelinkLabel);
  if (corelink.length === 0) return null;
  const allServable = jobLabels.every(
    (l) => isServableCorelinkLabel(l) || PASSTHROUGH_LABELS.has(l),
  );
  return allServable ? corelink : null;
}

export async function listOrphanRunnerJobs(
  env: ReconcilerEnv,
  repo: string,
  configured: string | undefined,
  minAgeMs: number,
  nowMs: number,
): Promise<{ jobId: string; labels: string[] }[]> {
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
    const orphans: { jobId: string; labels: string[] }[] = [];
    for (const run of runs.workflow_runs ?? []) {
      const age = nowMs - Date.parse(run.created_at);
      if (!Number.isFinite(age) || age < minAgeMs) continue; // too fresh: leave it to the webhook
      const jobs = (await gh(`/repos/${repo}/actions/runs/${run.id}/jobs`)) as { jobs?: GhJob[] };
      for (const j of jobs.jobs ?? []) {
        const matched = matchManagedLabels(j.labels ?? [], configured);
        // "runnerless" = no runner assigned. GitHub's Actions jobs API reports an
        // unassigned queued job as `runner_id: 0` (observed live 2026-07-20 — NOT
        // the `null` we originally assumed); a real assignment is a positive id.
        // Treat BOTH 0 and null as unassigned, else EVERY orphan is silently skipped
        // and the re-drive reconciler never recovers a spawn-orphaned job (jobs sit
        // `queued` forever — the exact prod stall observed 2026-07-20 ~18:00Z).
        const runnerless = j.runner_id == null || j.runner_id === 0;
        if (j.status === "queued" && runnerless && matched) {
          // Carry the MATCHED label set (subset-gated) so the redrive mints the
          // JIT runner advertising exactly what this job requested.
          orphans.push({ jobId: String(j.id), labels: matched });
        }
      }
    }
    return orphans;
  } catch (e) {
    console.log(`reconciler list failed for ${repo} (backstop, skipping): ${(e as Error).message}`);
    return [];
  }
}

// ── Dead-letter orphan retry (W7/F8) — WARM re-drive of a failed spawn, ANY repo ─
//
// GitHub fires workflow_job.queued ONCE. If the webhook's spawn transiently fails
// AFTER the retry+waitUntil budget, the job sits queued forever. The first-party
// GitHub scan (`listOrphanRunnerJobs` + RECONCILER_REPOS) only re-drives COLD and
// only for the trusted allowlist — a CUSTOMER repo's transiently-failed spawn has
// NO recovery, and a cold re-drive would skip per-job authz/mint anyway.
//
// The fix is a DEAD-LETTER: on a spawn failure where an installation_id was in
// hand (⇒ warm-recoverable), record the failed spawn WITH its installation_id in
// KV under an `orphan:<jobId>` key. The scheduled reconciler then retries the
// dead-letter WARM (installation_id present ⇒ buildContainerEnv authorizes+mints),
// for ANY repo, idempotently (claimSpawn dedups vs the live path) and bounded to
// MAX_ORPHAN_ATTEMPTS. A cold spawn (no installation_id) is NOT recorded — it
// can't be warm-retried and stays covered by the first-party GitHub scan.

// Past this the queued job is stale/gone (GitHub won't assign a runner minted so
// late), so the dead-letter self-expires — the retry never chases a dead job.
export const ORPHAN_TTL_S = 1800; // 30 min
// The retry is bounded: after this many attempts the dead-letter is given up
// (deleted + logged) so it never retries forever.
export const MAX_ORPHAN_ATTEMPTS = 3;

// The dead-letter record: the failed spawn's inputs, carried so the reconciler can
// re-drive it WARM (installation_id present ⇒ authorized mint). `attempts` bounds
// the retry.
export interface OrphanRecord {
  repo: string;
  installationId: string;
  labels: string[];
  attempts: number;
}

/**
 * PURE dead-letter decision (unit-testable; index.ts applies the KV I/O). Given
 * the current record and the attempt ceiling, decide the reconciler's branch:
 *   • null record        ⇒ "missing"  (the key TTL-expired between list and get) — skip.
 *   • attempts >= max     ⇒ "giveup"  (bounded — delete + log, never retry forever).
 *   • else                ⇒ "retry"   (bump to nextAttempts, then claim + WARM re-drive).
 * `nextAttempts` is the attempts value to persist on a retry (attempts + 1), or the
 * current attempts on giveup (for the log), or 0 when missing.
 */
export function orphanRetryStep(
  rec: OrphanRecord | null,
  max: number,
): { action: "giveup" | "retry" | "missing"; nextAttempts: number } {
  if (!rec) return { action: "missing", nextAttempts: 0 };
  if (rec.attempts >= max) return { action: "giveup", nextAttempts: rec.attempts };
  return { action: "retry", nextAttempts: rec.attempts + 1 };
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
 *
 * WP-C billing-emit disjointness invariant — READ BEFORE CHANGING:
 * The Rust fabricd-native emitter (crates/corelink-fabric-server/corelink_billing.rs)
 * ALSO pushes `runner_slot_seconds`, but keyed BLAKE3(lease_id ‖ period) on its
 * OWN `lease-<uuid>` lease ids. This spawn-worker path keys SHA-256(jobId ‖ period)
 * on the decimal GH `workflow_job.id`. The same billable job never emits from both:
 *   1. runner-path — a job is served by exactly one path (in prod clw redeems the
 *      cred at the Worker's own /v1/leases/{id}/cas-cred, not fabricd);
 *   2. id-space — `lease-<uuid>` and a pure-decimal job id never overlap.
 * The idem_key does NOT and CANNOT dedup across the two paths (BLAKE3 vs SHA-256 →
 * different key even for the same input); it is at-least-once safety WITHIN this
 * path only. If a future change lets one billable unit emit from BOTH paths, that
 * is a double-count — do NOT unify the algos (an owner-signed billing change);
 * restore disjointness or escalate. Disjointness tests: `usageIdemKey` /
 * `buildUsageEvent` describe block in test/index.test.ts.
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

// ── Durable per-completed-job usage ledger (WP-F) ────────────────────────────
//
// The fail-open billing loss WP-E surfaced: with the push OFF (BILLING_INGEST_URL
// unset) a completed job's usage was NEVER persisted — `maybeBillCompletedJob`
// returns before it can compute anything, the `jtenant:` tenant map is TTL'd 2h +
// deleted at completion, and the reconciler's GitHub source carries no
// installation_id (→ no tenant). So once a job completed with the push off, its
// usage was UNRECOVERABLE.
//
// This ledger closes that gap: at COMPLETION (index.ts) we durably record the
// job's usage — tenant (server-derived), timings, region — under `usage:<jobId>`,
// REGARDLESS of whether the push is armed. It is the tenant-safe backfill source
// the reconciler reads: the GitHub jobs API has no tenant, but this record DOES.
// Reuses the existing RUNNER_JOB_PATS binding with a `usage:` prefix (no new
// wrangler binding = no owner-gated namespace provisioning). TTL 60d — a wide
// arm-later window so turning billing ON weeks after a job ran still recovers it.
export const USAGE_LEDGER_TTL_S = 60 * 24 * 3600; // 60d

/** One completed job's durable usage record (the backfill source of truth). */
export interface UsageLedgerRecord {
  jobId: string;
  tenant: string;
  startedMs: number;
  completedMs: number;
  region: string;
}

function usageLedgerKey(jobId: string): string {
  return `usage:${jobId}`;
}

/**
 * Durably record a completed job's usage under `usage:<jobId>`. No-op when no KV
 * is bound. The CALLER guarantees a derived tenant + finite timings (under-bill-
 * never-mis-bill); this only serializes + persists with the 60d TTL.
 */
export async function writeUsageLedger(kv: KvLike | undefined, rec: UsageLedgerRecord): Promise<void> {
  if (!kv) return;
  await kv.put(usageLedgerKey(rec.jobId), JSON.stringify(rec), { expirationTtl: USAGE_LEDGER_TTL_S });
}

/**
 * Read a completed job's durable usage record. Returns null when no KV is bound,
 * the record is absent (cold/no-tenant job, or pre-ledger), or it fails the
 * billable-shape invariant (non-empty tenant + finite timings) — so a malformed
 * or tenant-less record is treated as "not backfillable" rather than mis-billed.
 */
export async function readUsageLedger(
  kv: KvLike | undefined,
  jobId: string,
): Promise<UsageLedgerRecord | null> {
  if (!kv) return null;
  const raw = await kv.get(usageLedgerKey(jobId));
  if (!raw) return null;
  try {
    const rec = JSON.parse(raw) as UsageLedgerRecord;
    if (!rec || typeof rec.tenant !== "string" || rec.tenant.length === 0) return null;
    if (!Number.isFinite(rec.startedMs) || !Number.isFinite(rec.completedMs)) return null;
    return rec;
  } catch {
    return null; // malformed ⇒ not backfillable (never throw out of the backstop)
  }
}

// ── Billing reconciler — closes the fail-open billing-loss window ────────────
//
// `maybeBillCompletedJob` (index.ts) fires ONCE per `workflow_job:completed`
// webhook delivery and is fail-open: any error (network blip, a transient
// ingest 5xx, the delivery never arriving at all) silently drops that job's
// `runner_slot_seconds` — with no backstop. This mirrors the EXISTING orphan
// re-drive reconciler shape (`listOrphanRunnerJobs` + `RECONCILER_REPOS`,
// scheduled via the same cron): each tick, list recently-COMPLETED jobs in the
// trusted allowlist and re-push their usage event through the SAME
// `buildUsageEvent`/`pushUsageEvent` path the webhook uses. Re-emitting a job
// already billed is SAFE — the aggregator dedups on `idem_key = SHA-256(jobId
// | period)` (at-least-once by design), so this only ever *recovers* missed
// revenue, never double-bills.

// How far back a tick looks for completed jobs — bounds the GitHub API scan
// (and the dedup-safe re-push cost) to a reasonable recovery window; a job
// completed longer ago than this is left as a permanent (rare, already-logged)
// loss rather than re-scanned forever.
export const BILLING_RECONCILE_LOOKBACK_MS = 6 * 60 * 60 * 1000; // 6h

/** The subset of Env the billing reconciler reads (GH listing + billing push). */
export interface BillingReconcileEnv extends ReconcilerEnv, BillingEnv {
  // Reuses the SAME first-party allowlist as the orphan re-drive — no new
  // binding, no new config surface.
  RECONCILER_REPOS?: string;
  // WP-F: the durable `usage:<jobId>` ledger lives in the existing RUNNER_JOB_PATS
  // KV (KvLike subset). This is the tenant-safe backfill source — the reconciler
  // reads it per completed job to recover the DERIVED tenant the GitHub jobs API
  // never carries. Optional (absent ⇒ ledger empty ⇒ prior no-op behavior).
  RUNNER_JOB_PATS?: KvLike;
}

interface GhCompletedJob {
  id: number;
  status: string;
  started_at: string | null;
  completed_at: string | null;
  labels: string[];
}

/**
 * List completed+labeled jobs in `repo` whose `completed_at` is OLDER than
 * `settleMs` (so an in-flight `completed` webhook for the same job isn't
 * double-raced) but no older than `lookbackMs` (bounds the scan). Mirrors
 * `listOrphanRunnerJobs`'s shape (completed RUNS → their jobs), just over
 * completed runs instead of queued ones. Best-effort: any GitHub error returns
 * `[]` for this repo (the reconciler is a backstop, never itself a gate).
 */
export async function listCompletedRunnerJobs(
  env: ReconcilerEnv,
  repo: string,
  configured: string | undefined,
  lookbackMs: number,
  settleMs: number,
  nowMs: number,
): Promise<{ jobId: string; startedMs: number; completedMs: number }[]> {
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
    const runs = (await gh(`/repos/${repo}/actions/runs?status=completed&per_page=30`)) as {
      workflow_runs?: GhRun[];
    };
    const out: { jobId: string; startedMs: number; completedMs: number }[] = [];
    for (const run of runs.workflow_runs ?? []) {
      const jobs = (await gh(`/repos/${repo}/actions/runs/${run.id}/jobs`)) as {
        jobs?: GhCompletedJob[];
      };
      for (const j of jobs.jobs ?? []) {
        if (j.status !== "completed" || !matchManagedLabels(j.labels ?? [], configured)) continue;
        const completedMs = j.completed_at ? Date.parse(j.completed_at) : NaN;
        const startedMs = j.started_at ? Date.parse(j.started_at) : NaN;
        if (!Number.isFinite(completedMs) || !Number.isFinite(startedMs)) continue;
        const age = nowMs - completedMs;
        if (age < settleMs || age > lookbackMs) continue; // too fresh (racing the webhook), or too old
        out.push({ jobId: String(j.id), startedMs, completedMs });
      }
    }
    return out;
  } catch (e) {
    console.log(
      `billing reconciler list failed for ${repo} (backstop, skipping): ${(e as Error).message}`,
    );
    return [];
  }
}

/**
 * The scheduled billing reconciler — TENANT-SAFE backstop (WP-2 2b, WP-F).
 *
 * For each repo in `RECONCILER_REPOS`, it lists recently-completed+labeled jobs
 * (settled past the race window, within the lookback). The GitHub jobs-list API
 * carries NO `installation_id` (→ no tenant), so it can never bill from GitHub
 * alone. WP-F closes this: each listed job is looked up in the durable
 * `usage:<jobId>` ledger (written at completion, which HAS the server-derived
 * tenant + timings + region). When a ledger record exists, we build + push its
 * usage event keyed on the DERIVED tenant — so a LATER-armed push backfills
 * tenant-safely. A job with NO ledger record (cold/no-tenant, or pre-ledger) is
 * SKIPPED-and-counted, exactly as before (never mis-billed to `CLW_TENANT`).
 *
 * Re-push safety: `buildUsageEvent`'s `idem_key = SHA-256(jobId|period)` is
 * unchanged, so the webhook live-push and this backfill dedup against each other
 * at the aggregator (at-least-once by design — recovers revenue, never doubles).
 *
 * Still default-off (mirrors the orphan reconciler): short-circuits to 0 unless
 * `RECONCILER_REPOS` + `GITHUB_MINT_TOKEN` + `BILLING_INGEST_URL` +
 * `BILLING_INGEST_AUTH_KEY` are configured (so it never scans GitHub when billing
 * isn't wired). Region is resolved PER RECORD (the region the job actually ran in,
 * stored at completion), falling back to `BILLING_REGION`; a record with no 3-char
 * region is skipped. Never throws (best-effort backstop).
 */
export async function reconcileCompletedJobBilling(
  env: BillingReconcileEnv,
  configured: string | undefined,
  nowMs: number,
): Promise<number> {
  const repos = parseReconcilerRepos(env.RECONCILER_REPOS);
  if (repos.length === 0) return 0; // opt-in: no allowlist ⇒ off (mirrors the orphan re-drive)
  if (!env.GITHUB_MINT_TOKEN || !env.BILLING_INGEST_URL || !env.BILLING_INGEST_AUTH_KEY) return 0;
  let pushed = 0;
  let skipped = 0;
  for (const repo of repos) {
    const jobs = await listCompletedRunnerJobs(
      env,
      repo,
      configured,
      BILLING_RECONCILE_LOOKBACK_MS,
      RECONCILE_MIN_AGE_MS,
      nowMs,
    );
    for (const job of jobs) {
      // The ledger is the tenant-safe source of truth: it carries the DERIVED
      // tenant the GitHub jobs API never does. No record ⇒ not backfillable
      // (preserves the prior skip-and-count behavior — never mis-bill).
      const rec = await readUsageLedger(env.RUNNER_JOB_PATS, job.jobId);
      if (!rec) {
        skipped += 1;
        continue;
      }
      // Prefer the region stored at completion (where the job actually ran); fall
      // back to BILLING_REGION. Ingest validates 3-char — skip if neither is one.
      const region = rec.region.length === 3 ? rec.region : (env.BILLING_REGION ?? "");
      if (region.length !== 3) {
        skipped += 1;
        continue;
      }
      try {
        const ev = await buildUsageEvent({
          tenantId: rec.tenant,
          jobId: rec.jobId,
          startedMs: rec.startedMs,
          completedMs: rec.completedMs,
          region,
        });
        await pushUsageEvent(env, ev);
        pushed += 1;
      } catch (e) {
        // Fail-open backstop: a push error just means the next tick retries
        // (idem_key makes the re-push safe). Never break the scan.
        logEvent("error", "billing_reconcile_push_failed", {
          jobId: rec.jobId,
          error: (e as Error).message,
        });
      }
    }
  }
  if (skipped > 0) {
    logEvent("info", "billing_reconcile_skipped_no_tenant", { skipped });
  }
  return pushed;
}
