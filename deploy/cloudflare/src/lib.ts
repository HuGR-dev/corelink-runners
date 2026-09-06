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
/**
 * CF Access (Inc-3) service-token headers for the gated `/internal/v1/*` control
 * plane (`docs/internal/inc3-cf-access-lockdown.md` in corelink-server). Both
 * present ⇒ the pair; else `{}` (the app-layer key alone, which 403s at the edge
 * when the gate is enforcing). Mirrors the fabricd
 * `crates/corelink-fabric-server/src/cf_access.rs` wiring so BOTH callers of the
 * runner-mint / billing-usage seam send identical CF Access auth — the
 * spawn-worker was the caller Inc-3 (#479) missed.
 */
export function cfAccessHeaders(env: {
  CORELINK_CF_ACCESS_CLIENT_ID?: string;
  CORELINK_CF_ACCESS_CLIENT_SECRET?: string;
}): Record<string, string> {
  const id = env.CORELINK_CF_ACCESS_CLIENT_ID;
  const secret = env.CORELINK_CF_ACCESS_CLIENT_SECRET;
  return id && secret
    ? { "CF-Access-Client-Id": id, "CF-Access-Client-Secret": secret }
    : {};
}

export interface MintEnv {
  // The dedicated `runner_mint` consumer key (Server TL, key-split 2026-06-21): gates
  // ONLY /internal/v1/runner/{mint,revoke} — never signup-mint, erase, or admin (A6,
  // one notch tighter than pat_mint). Sent as `x-corelink-internal-auth` on both calls.
  CORELINK_RUNNER_MINT_AUTH_KEY?: string;
  // "1" ⇒ an unarmed CORELINK_RUNNER_MINT_AUTH_KEY is a HARD DENY instead of a
  // silent cold spawn (★A3.17). DEFAULT-OFF on purpose — see the guard in
  // `buildContainerEnv` for why the loud half ships on and the refusing half is
  // armed deliberately.
  REQUIRE_MINT_KEY?: string;
  CORELINK_MINT_URL?: string;
  // CF Access (Inc-3) service-token pair for the gated `/internal/v1/*` edge. Set
  // as Worker secrets on corelink-spawn-worker; WITHOUT them the runner-mint call
  // 403s at the Cloudflare Access edge (the Inc-3 lockdown strands every spawn).
  CORELINK_CF_ACCESS_CLIENT_ID?: string;
  CORELINK_CF_ACCESS_CLIENT_SECRET?: string;
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
  list?(options: { prefix: string; cursor?: string }): Promise<{
    keys: { name: string }[];
    cursor?: string;
    list_complete?: boolean;
  }>;
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
  // The claim VALUE is the wall-clock ms the claim was taken (was the literal "1").
  // Every reader only tests truthiness — the timestamp adds information without
  // changing any of those verdicts — and it lets the reconciler tell a LIVE spawn
  // from a LEAKED one before force-releasing (see `spawnClaimAgeMs`).
  await kv.put(key, String(Date.now()), { expirationTtl: SPAWN_CLAIM_TTL_S });
  return true;
}

/**
 * Age of a raw spawn-claim value in ms, or `null` when it carries no usable
 * timestamp. Claims written before claims were timestamped store the literal "1"
 * and are UN-AGED; callers must decide that case deliberately (the reconciler
 * treats un-aged as OLD — see redriveOrphanedJobs).
 */
export function spawnClaimAgeMs(raw: string | null | undefined, nowMs: number): number | null {
  if (!raw) return null;
  const takenMs = Number.parseInt(raw, 10);
  if (!Number.isFinite(takenMs)) return null;
  return Math.max(0, nowMs - takenMs);
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

export interface MintParams {
  jobId: string;
  repoFullName: string;
  installationId: string;
  scope?: string;
  ttlSeconds?: number;
  acquiringPat?: string;
  /** Issuer cleanup remains active until the durable Worker adoption ACK. */
  credentialOperationId?: string;
  computeReservationId?: string;
}
export interface MintResult {
  token: string;
  patId: string;
  tenant: string;
  maxConcurrency?: number;
  maxVcpuH?: number;
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
  signal?: AbortSignal,
): Promise<void> {
  const base = env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com";
  const resp = await fetch(`${base}/internal/v1/runner/revoke`, {
    method: "POST",
    signal,
    headers: {
      ...cfAccessHeaders(env),
      "x-corelink-internal-auth": env.CORELINK_RUNNER_MINT_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    // Revoke keys on pat_id; owner_tenant is the SERVER-DERIVED tenant carried from
    // the mint (fallback: wrangler's CLW_TENANT for legacy single-tenant deploys).
    body: JSON.stringify({ pat_id: patId, owner_tenant: ownerTenant ?? env.CLW_TENANT }),
  });
  if (!resp.ok) throw new Error(`D-9 revoke ${resp.status}`);
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
/**
 * Why a spawn came out COLD. Present ONLY on a cold result, and it exists so an
 * operator misconfiguration cannot hide inside the ordinary cold path — the three
 * used to be one silent return. See the guard in `buildContainerEnv`.
 */
export type ColdReason = "mint_key_unarmed" | "no_repo" | "no_installation_or_pat";

export interface ContainerEnvResult {
  /** Durable shared compute reservation, held independently of PAT cleanup. */
  computeReservationId?: string;
  authz: "ok" | "forbidden";
  containerEnv: Record<string, string>;
  /** Present on a 403 so edge-proxy failures are retryable and authz is auditable. */
  forbiddenReason?: "edge_proxy" | "authz";
  /** Set iff the spawn is COLD; absent on a warm mint. */
  coldReason?: ColdReason;
  patId?: string;
  tenant?: string; // server-DERIVED tenant (billed + CLW_TENANT); warm only
  maxConcurrency?: number; // per-tenant ceiling; warm only
  /**
   * Monthly compute allowance in vCPU-HOURS (server #975). Warm only, and
   * ABSENT for a tenant with no metered ceiling — absent means "nothing to warn
   * against", never "zero allowance". ADVISORY: it is not consulted by any
   * admission decision, only by the near-ceiling warning at completion.
   */
  maxVcpuH?: number;
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

/* Required mint/env-0 implementation is exported from ./lib/build_container_env. */
/* The old implementation was removed; this marker keeps the surrounding pure
   credential-stash helpers and their compatibility contracts in this module. */
export {
  buildContainerEnv,
  mintCasPat,
  MintForbiddenError,
  classifyMintForbidden,
} from "./lib/build_container_env";

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
// SCALE (2026-08-19): raised 20 → 250 in lockstep with wrangler.jsonc max_instances.
// The old 20 = one tenant's entitlement, so a single power user saturated the whole
// fleet. 250 is a real cross-tenant global ceiling. Runners sleepAfter-reap to zero
// (no idle cost). The HARD ceiling is the account limit (~343 standard-4 runners
// after the cache fleet's vCPU share); beyond that needs a CF account-limit raise.
export const FLEET_MAX_CONCURRENCY = 250;
// Legacy pure fail-open decision helpers retained for compatibility with their
// historical unit suite. Production admission uses the transactional global
// ContainmentDO authority in lib/admission_budget.ts; these helpers do no storage
// access and are not used by acquireConcurrencySlot.
export const FAILOPEN_WINDOW_S = 60;
export const FAILOPEN_MAX_PER_WINDOW = 5;

/** Legacy bucket key helper; production authority uses a rolling durable record. */
export function failOpenWindowKey(nowMs: number): string {
  return `failopen:${Math.floor(nowMs / (FAILOPEN_WINDOW_S * 1000))}`;
}

/**
 * The pure decision, runtime-agnostic and unit-testable (same shape as
 * `decideSlotAcquire`): given how many fail-open admissions this window has already
 * recorded, may this one be admitted?
 *
 * `null` remains a refusal for the historical pure decision contract.
 */
export function decideFailOpenAdmission(
  countThisWindow: number | null,
  max: number = FAILOPEN_MAX_PER_WINDOW,
): { admitted: boolean; reason?: string } {
  if (countThisWindow === null) {
    return { admitted: false, reason: "slot_failopen_budget_unreadable" };
  }
  if (countThisWindow >= max) {
    return { admitted: false, reason: "slot_failopen_budget_exhausted" };
  }
  return { admitted: true };
}

// The per-repo ceiling for COLD spawns (no derived tenant, so no entitlement).
// Raised 8 → 40 (2026-08-19) so a single busy repo's cold CI isn't throttled at 8
// while the global fleet has room; still well under FLEET_MAX_CONCURRENCY.
export const COLD_REPO_CAP = 40;

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

// ── Rate-limit dead-letter bound (2026-08-02) ────────────────────────────────
//
// The webhook limiter refuses a burst past 30 spawns/60s per repo. That refusal
// used to drop the job permanently (GitHub sends `workflow_job.queued` once and
// does not redeliver a non-2xx), which is the same defect #437 fixed for the
// CEILING refusal — just a different branch. Refused jobs are now dead-lettered
// so the reconciler re-drives them.
//
// But dead-lettering EVERY refusal would be worse than the bug. `driveSpawn`
// mints the per-job CAS PAT BEFORE it checks the concurrency slot, so each
// reconciler retry costs a real HTTP mint against corelink-server even when the
// spawn is then refused at the ceiling. Unbounded dead-lettering therefore turns
// a flood into sustained mint load — and the limiter, whose whole job is to bound
// a flood, would have become the thing that amplifies it.
//
// So the dead-lettering itself is bounded, per repo, per window. Legitimate CI
// bursts fit comfortably under the cap (our own dogfood peak is ~24 jobs); a
// flood does not, and its excess is dropped exactly as before.
//
// ⚠️ Note the limiter is NOT an abuse control against a party holding the webhook
// HMAC secret: its key is `spawn:<repository.full_name>`, a value that party
// controls, so varying the repo string sidesteps it by construction. The real
// containment for a forged repo is server-side — the mint derives the tenant and
// 403s a repo that is not allowlisted for it — plus the fleet-wide slot DO. This
// cap exists to keep the RECOVERY path proportionate, not to replace either.
export const RATE_LIMIT_DEADLETTER_MAX = 60;
export const RATE_LIMIT_DEADLETTER_WINDOW_S = 300;

// ── Near-ceiling warning (2026-08-02) ────────────────────────────────────────
//
// Overage above the tier's included `max_vcpu_h` is billed at 3x COGS. That
// makes crossing the line expensive, and an SMB self-serve customer whose FIRST
// notice is the invoice churns instead of upgrading. So the fleet warns on the
// way up.
//
// Why here and not on the server: the ALLOWANCE lives in D1 (`runners_entitlement`)
// but the CONSUMPTION only exists here — this Worker is the one component that
// sees every job finish. The mint now forwards `max_vcpu_h` precisely so the two
// halves can meet (corelink-server #975).
//
// Deliberately NOT a gate. Crossing the ceiling does not stop a job: the customer
// keeps building and pays the overage. Stopping someone's CI mid-sprint is a
// worse outcome than charging them, which is the whole reason overage exists
// rather than a hard block.

/** Warn once at 80% of the allowance, once more when it is actually crossed. */
export const VCPU_WARN_THRESHOLDS = [0.8, 1.0] as const;

/** Per-tenant cache of the ceiling last seen on a mint (refreshed every spawn). */
export function vcpuCeilingKey(tenant: string): string {
  return `vceil:${tenant}`;
}

/** Accumulated vCPU-SECONDS for a tenant in a billing period. */
export function vcpuUsageKey(tenant: string, period: string): string {
  return `vused:${tenant}:${period}`;
}

/** Marker proving a given threshold was already announced for this period. */
export function vcpuWarnedKey(tenant: string, period: string, threshold: number): string {
  return `vwarn:${tenant}:${period}:${threshold}`;
}

/** The ceiling cache and the period counters outlive a long billing month. */
export const VCPU_KEY_TTL_S = 45 * 24 * 3600; // 45d

export interface VcpuWarning {
  /** Highest threshold newly crossed by this job, or null when none was. */
  crossed: number | null;
  /** Fraction of the allowance consumed AFTER this job (1.0 == exactly at it). */
  fraction: number;
  consumedVcpuH: number;
  ceilingVcpuH: number;
}

/**
 * PURE threshold decision (the KV I/O lives in index.ts).
 *
 * Returns the single HIGHEST threshold newly crossed, never a list: a job big
 * enough to jump from 0% straight past 100% should produce ONE "you are over"
 * message, not a burst of "you are at 80%" followed by "you are over". The
 * customer needs the actionable state, not the history.
 *
 * `alreadyWarned` is the set of thresholds already announced this period, so a
 * tenant sitting at 85% for a thousand jobs is told once — an alert that repeats
 * every job is an alert that gets filtered, which is the same as no alert.
 *
 * A missing/zero/negative ceiling yields `crossed: null`: no allowance on file
 * means nothing to be near, and inventing one would warn every tenant that never
 * bought a metered tier.
 */
export function vcpuWarningStep(
  consumedVcpuSeconds: number,
  ceilingVcpuH: number | null | undefined,
  alreadyWarned: ReadonlySet<number>,
): VcpuWarning {
  const ceiling = typeof ceilingVcpuH === "number" && ceilingVcpuH > 0 ? ceilingVcpuH : 0;
  const consumedVcpuH = Math.max(0, consumedVcpuSeconds) / 3600;
  if (ceiling === 0) {
    return { crossed: null, fraction: 0, consumedVcpuH, ceilingVcpuH: 0 };
  }
  const fraction = consumedVcpuH / ceiling;
  let crossed: number | null = null;
  for (const t of VCPU_WARN_THRESHOLDS) {
    if (fraction >= t && !alreadyWarned.has(t)) crossed = t; // keep the highest
  }
  return { crossed, fraction, consumedVcpuH, ceilingVcpuH: ceiling };
}

/** KV key holding the per-repo count of rate-limit refusals dead-lettered. */
export function rateLimitDeadLetterKey(repo: string): string {
  return `rldl:${repo}`;
}

/**
 * PURE decision for whether a rate-limited job may be dead-lettered (the KV I/O
 * lives in index.ts). Returns the next counter value so the caller can persist it.
 *
 * The count is deliberately best-effort: concurrent webhook invocations can read
 * the same value and both record, so the real bound is approximate. That is fine
 * — this is a proportionality guard, not a quota. Being off by a handful under
 * concurrency changes nothing; being off by 10,000 is what it prevents.
 */
export function rateLimitDeadLetterStep(
  count: number,
  max: number = RATE_LIMIT_DEADLETTER_MAX,
): { record: boolean; nextCount: number } {
  if (count >= max) return { record: false, nextCount: count };
  return { record: true, nextCount: count + 1 };
}

/** Parse the comma/space-separated RECONCILER_REPOS allowlist (owner/repo). */
export function parseReconcilerRepos(csv: string | undefined): string[] {
  if (!csv) return [];
  return csv
    .split(/[,\s]+/)
    .map((s) => s.trim())
    .filter((s) => s.includes("/"));
}

export { installationIdForRepo, tenantPatSecretForRepo } from "./repo_config_lookup";

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
// INSTALLATION_ALLOWLIST="150584374,<customer-install-id>" (150584374 = the
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
  /** Token minted for the verified installation during registry discovery. */
  GITHUB_RECONCILER_TOKEN?: string;
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

// ── Capability claims vs routing labels ──────────────────────────────────────
// The fleet runs exactly ONE box shape: a `standard-4` x86_64 Linux microVM
// (the image is wrangler-pinned per DO class — deploy/cloudflare/README.md §1,
// ADR-0008). Every `corelink-<suffix>` is servable, which is right for ROUTING
// suffixes (`corelink-dogfood`, a team name): the suffix asks WHO, not WHAT, and
// the standard box is the correct answer.
//
// It is NOT right for a suffix that asserts HARDWARE. `runs-on:
// corelink-standard-8` is served today by a `standard-4` box, silently — the
// caller asked for a machine we do not have and got a smaller one with no
// signal. That is precisely the failure USE-SCENARIOS S1.3.4 forbids: "a
// capability gap is a visible 'not yet', not a mis-provisioned wrong box or a
// silent failure". It also undermines billing, which hard-pins the slot cost to
// `standard-4` (see RUNNER_SLOT_VCPU below) on the assumption every box IS one.
//
// ⚠️ Refusing the job is NOT the fix and must not be attempted here.
// `workflow_job.queued` is a one-shot event: returning `null` strands the job
// queued forever with no runner and no error the caller can see. Serving the
// standard box is strictly better for the caller than hanging. So the contract
// is: SERVE, and make the mismatch countable.
export const SERVED_INSTANCE_TYPE = "standard-4";

// Suffix shapes that assert hardware rather than routing. Kept as explicit
// patterns (not "anything unknown") so adding a team/routing label never trips
// a false capability warning.
const CAPABILITY_CLAIM_PATTERNS: readonly RegExp[] = [
  /^corelink-(standard|highcpu|highmem)-\d+$/, // size ladder (ADR-0007 Stage C)
  /^corelink-(gpu|cuda)$/, // accelerator
  /^corelink-(arm64|aarch64|x86_64|amd64)$/, // architecture
  /^corelink-(windows|macos|darwin)$/, // operating system
];

/**
 * The subset of `labels` that assert a hardware capability the fleet does not
 * actually provide. Empty when every label is either a routing suffix or the
 * one shape we really run (`corelink-standard-4`).
 *
 * Callers MUST still serve the job — this is a visibility signal, not a gate.
 */
export function unservedCapabilityClaims(labels: string[]): string[] {
  return labels.filter(
    (l) =>
      l !== `corelink-${SERVED_INSTANCE_TYPE}` &&
      CAPABILITY_CLAIM_PATTERNS.some((re) => re.test(l)),
  );
}

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
        authorization: `Bearer ${env.GITHUB_RECONCILER_TOKEN ?? env.GITHUB_MINT_TOKEN ?? ""}`,
        accept: "application/vnd.github+json",
        "user-agent": "corelink-spawn-worker",
      },
    });
    if (!r.ok) throw new Error(`GH ${path} ${r.status}`);
    return r.json();
  };
  try {
    const runs: GhRun[] = [];
    let runCursor: string | undefined;
    for (let page = 0; page < 20; page++) {
      const query = runCursor
        ? `?status=queued&per_page=30&after=${encodeURIComponent(runCursor)}`
        : "?status=queued&per_page=30";
      const pageBody = (await gh(`/repos/${repo}/actions/runs${query}`)) as {
        workflow_runs?: GhRun[];
        next_cursor?: string | null;
        next_page?: string | null;
      };
      runs.push(...(pageBody.workflow_runs ?? []));
      const next = pageBody.next_cursor ?? pageBody.next_page ?? null;
      if (next == null || next === "") break;
      if (typeof next !== "string" || next === runCursor || page === 19) return [];
      runCursor = next;
    }
    const orphans: { jobId: string; labels: string[] }[] = [];
    for (const run of runs) {
      const age = nowMs - Date.parse(run.created_at);
      if (!Number.isFinite(age) || age < minAgeMs) continue; // too fresh: leave it to the webhook
      const jobs: GhJob[] = [];
      let jobCursor: string | undefined;
      for (let page = 0; page < 20; page++) {
        // Keep the original first-page URL byte-stable; subsequent pages use
        // the provider cursor. This also avoids changing the live seam for
        // installations whose GitHub proxy only recognizes the canonical path.
        const query = jobCursor
          ? `?per_page=100&after=${encodeURIComponent(jobCursor)}`
          : "";
        const pageBody = (await gh(`/repos/${repo}/actions/runs/${run.id}/jobs${query}`)) as {
          jobs?: GhJob[];
          next_cursor?: string | null;
          next_page?: string | null;
        };
        jobs.push(...(pageBody.jobs ?? []));
        const next = pageBody.next_cursor ?? pageBody.next_page ?? null;
        if (next == null || next === "") break;
        if (typeof next !== "string" || next === jobCursor || page === 19) return [];
        jobCursor = next;
      }
      for (const j of jobs) {
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
    return [...new Map(orphans.map((job) => [job.jobId, job])).values()].sort((a, b) =>
      a.jobId.localeCompare(b.jobId, undefined, { numeric: true }),
    );
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

// A spawn REFUSED at the concurrency ceiling — thrown, not returned, so it is
// distinguishable from both success and a genuine failure.
//
// WHY THIS EXISTS (2026-08-02: ~24 jobs pushed at once, 12 ran, 12 sat `queued`
// forever). The ceiling branch used to `return` after logging `spawn_at_ceiling`.
// Two consequences followed, and together they lost the job permanently:
//
//   1. `driveSpawnGuarded` only calls `recordOrphan` from its CATCH. A plain
//      `return` never entered it, so a refused spawn left NO dead-letter and
//      `retryOrphanedSpawns` could not see it.
//   2. Worse, `retryOrphanedSpawns` drives the THROWING `driveSpawn` and reads a
//      normal return as recovery — so even once a dead-letter existed, the first
//      retry tick would be refused again, read that as success, and DELETE the
//      record. The recovery path would have destroyed its own evidence.
//
// GitHub sends `workflow_job.queued` exactly once and never redelivers it, so
// "dropped here" means "the customer's job hangs `queued` forever, and nothing is
// reported as failed".
//
// Refusal is NOT failure — it is BACKPRESSURE, and the expected condition under a
// burst. That distinction is load-bearing downstream: a refusal must not consume
// the 3-strike budget meant for genuine errors, or any burst lasting more than
// three cron ticks would still lose every job behind the ceiling.
export class SpawnRefusedError extends Error {
  constructor(public readonly reason: string) {
    super(`spawn refused at ceiling: ${reason}`);
    this.name = "SpawnRefusedError";
  }
}

// The dead-letter record: the failed spawn's inputs, carried so the reconciler can
// re-drive it WARM (installation_id present ⇒ authorized mint). `attempts` bounds
// the retry.
export interface OrphanRecord {
  repo: string;
  installationId: string;
  labels: string[];
  attempts: number;
  // Wall-clock ms when this dead-letter was FIRST recorded. Bounds the refusal
  // wait as an ABSOLUTE window (see orphanRefusalStep): without it, re-putting the
  // record on each refusal would keep resetting ORPHAN_TTL_S, and a permanently
  // saturated fleet would retry one job forever. Optional — records written before
  // this field existed fall back to the attempt-count bound.
  firstRecordedMs?: number;
  // Wall-clock ms at which this job was classified STRANDED: GitHub reported the
  // job `in_progress` on a runner GitHub itself no longer knows about, i.e. the
  // box died mid-job. Its presence makes the record TERMINAL — the retry
  // reconciler must never re-drive it. Re-driving in-flight work is a separate
  // decision that has not been made; this field exists so the dead-letter can say
  // "recorded, deliberately not retried" in ONE format rather than two.
  stranded?: number;
  // The runner name the stranded classification was made against (diagnostics).
  strandedRunner?: string;
  // Wall-clock ms when a container was last STARTED for this job. Its presence
  // means "we believe this job is placed, but nothing has confirmed it yet" — see
  // the placement-confirmation block below. Absent ⇒ the record is a plain
  // failure/refusal dead-letter and follows the original retry path.
  placedMs?: number;
  /** First failure classification; retained in the same orphan: record for
   * bounded retry/audit and never used as authorization input. */
  failure_class?: "edge_proxy_403" | "authz_403";
}

// ── Placement confirmation (2026-08-03) — a spawn that "succeeded" and produced
// ── nothing was the ONE loss the dead-letter could not see ───────────────────
//
// The dead-letter recovers a spawn that FAILED (an error) or was REFUSED (the
// ceiling). Both are cases where `driveSpawn` throws, which is what routes the job
// into `recordOrphan`. It has no case for a spawn that RETURNS NORMALLY and still
// leaves the job unplaced — and that is the one that actually lost work.
//
// Measured, corelink-server run 30826164339 (2026-08-03T15:09Z, 24-job fan-out):
// the Worker minted a JIT and started a container for all 24 jobs, recorded 8
// ceiling refusals and 3 start failures, and RECOVERED every one of them — 9
// `orphan_retry_recovered`, ZERO `orphan_retry_giveup`, ZERO `orphan_refusal_giveup`.
// The retry budget was never even approached. Yet 11 jobs still sat `queued` with
// no runner until GitHub cancelled them 20 minutes later, because for those 11 the
// box we started never came online and claimed the job. From the Worker's side that
// is indistinguishable from success: it logged `runner_spawned` and moved on.
//
// So the job was lost in the one state nothing watched: believed-placed. There is
// no webhook for "the runner you started never showed up" — GitHub simply keeps the
// job queued, and `workflow_job.queued` is never redelivered.
//
// The fix is to stop treating a started container as proof of placement. A
// successful spawn now writes the SAME dead-letter record carrying `placedMs`, i.e.
// PROVISIONAL: "a box was started; placement unconfirmed". The record is cleared
// only by positive evidence that the job is no longer waiting for us.
//
// ⚠️ Confirmation is deliberately NOT keyed on a `workflow_job.in_progress`
// webhook. That would make correctness depend on a delivery we do not control and
// currently ignore, and getting it wrong re-spawns a HEALTHY running job — burning
// a concurrency slot and real COGS on a job that was never in trouble. Past the
// grace window we instead ASK GitHub about that specific job (one cheap
// `/actions/jobs/{id}` read, and only for a record that is still unconfirmed), and
// re-drive ONLY on an authoritative "still queued, still no runner". Anything else,
// including an API error, leaves the job alone.

/**
 * How long a started container has to come online and claim its job before we
 * treat the placement as lost.
 *
 * Sized off the observed healthy path: in the 30826164339 burst the boxes that DID
 * work were claimed by GitHub 12–50 s after `runner_minted`. 3 minutes leaves a
 * wide margin over the slowest healthy boot, so a re-drive means something really
 * did go wrong — while still leaving ~27 minutes of the 30-minute ORPHAN_TTL_S
 * window to actually recover the job.
 */
export const PLACEMENT_CONFIRM_GRACE_MS = 180_000;

/**
 * PURE: what the reconciler should do with a record that may be a provisional
 * placement (the KV and GitHub I/O live in index.ts).
 *
 *   • "not_placed"   ⇒ no provisional placement — the original failure/refusal
 *     retry path applies unchanged.
 *   • "within_grace" ⇒ a box was started recently; leave it alone entirely. This
 *     must NOT bump attempts and must NOT re-drive: the overwhelming majority of
 *     records are healthy in-flight jobs, and a tick that re-drove them would turn
 *     the reconciler into a spawn amplifier.
 *   • "verify"       ⇒ the grace window elapsed with no confirmation. Ask GitHub
 *     whether the job is still waiting before doing anything.
 */
export function placementConfirmStep(
  rec: OrphanRecord,
  nowMs: number,
  graceMs: number = PLACEMENT_CONFIRM_GRACE_MS,
): { action: "not_placed" | "within_grace" | "verify"; waitedMs: number } {
  if (rec.placedMs == null) return { action: "not_placed", waitedMs: 0 };
  const waitedMs = Math.max(0, nowMs - rec.placedMs);
  return { action: waitedMs < graceMs ? "within_grace" : "verify", waitedMs };
}

/**
 * PURE: read GitHub's view of one job into a placement verdict.
 *
 * Fail-SAFE by construction — every shape that is not an unambiguous "still
 * queued, still no runner" resolves to `placed` (confirmed, drop the record) or
 * `unknown` (leave the record, ask again next tick). Only `lost` re-drives, so no
 * amount of unexpected payload can cause a duplicate spawn of a live job.
 *
 * `runner_id` is 0 (NOT null) for an unassigned queued job on the Actions API —
 * the same live-observed quirk `listOrphanRunnerJobs` documents; both are treated
 * as unassigned here.
 */
export function jobPlacementVerdict(
  job: { status?: string; runner_id?: number | null } | null,
): "lost" | "placed" | "unknown" {
  if (!job || typeof job.status !== "string") return "unknown";
  // Anything past `queued` means a runner took it (or it is already over).
  if (job.status !== "queued") return "placed";
  const runnerless = job.runner_id == null || job.runner_id === 0;
  // Queued but ALREADY assigned a runner: the box is booting and about to claim it.
  return runnerless ? "lost" : "placed";
}

// ── Runner-activity verification (2026-08-03) ────────────────────────────────
//
// What the keep-alive sweep stores per live box, and how it reads GitHub's answer
// about that ONE runner. Both halves are pure so the decision that governs whether
// a customer's box keeps living can be tested without any network at all.
//
// WHY A RECORD AND NOT A BARE HANDLE. `rhandle:<runner_name>` used to hold just the
// DO handle, which is enough to renew a box but not enough to ask GitHub whether it
// SHOULD be renewed. The numeric `runner_id` minted alongside the JIT config is the
// only identifier the runners REST API accepts, and the repo + installation are what
// authorize the read, so all three travel with the handle now.
export interface RunnerBinding {
  /** The RunnerContainer DO handle (what the old bare-string value held). */
  h: string;
  /** GitHub's numeric runner id, from the `generate-jitconfig` response. */
  rid?: number;
  /** `owner/name` — the repo the registration lives on. */
  repo?: string;
  /** Installation id, so the status read uses the same credential as the mint. */
  inst?: string;
  /**
   * The job this box was STARTED for. Recorded so the stranded-job sweep has a
   * candidate job to ask GitHub about.
   *
   * ⛔ It is a CANDIDATE, never a conclusion. `generate-jitconfig` binds a runner
   * to a repo + label set and to nothing else, so GitHub assigns queued jobs to
   * idle runners by LABEL MATCH: the job this box actually ran is frequently NOT
   * this one. Correlating the two without asking GitHub is exactly what SIGKILLed
   * five live customer jobs on 2026-08-02. Every consumer MUST require GitHub to
   * confirm the link (`job.runner_id === rid`) before acting on it.
   */
  jid?: string;
  /** Wall-clock ms when the binding was written (i.e. when the box was started). */
  t?: number;
}

/** One observation of GitHub's view of a single runner. `null` ⇒ the call never
 *  produced an answer (no credential, network throw, unparseable body). */
export interface RunnerObservation {
  httpStatus: number;
  runner?: { status?: string; busy?: boolean } | null;
}

export type RunnerActivity = "busy" | "idle" | "unknown";

/**
 * PURE: encode a runner binding for KV.
 */
export function encodeRunnerBinding(b: RunnerBinding): string {
  return JSON.stringify(b);
}

/**
 * PURE: read a `rhandle:` value in EITHER shape.
 *
 * Records written before this change are a bare handle string. They parse to a
 * binding with no `rid`, which every caller must treat as UNVERIFIABLE — i.e. keep
 * renewing. A deploy must not start reclaiming the boxes that were already in
 * flight when it landed, because those are exactly the ones we know least about.
 */
export function parseRunnerBinding(raw: string | null | undefined): RunnerBinding | null {
  if (!raw) return null;
  if (!raw.startsWith("{")) return { h: raw }; // legacy bare handle
  try {
    const o = JSON.parse(raw) as Partial<RunnerBinding>;
    if (typeof o.h !== "string" || o.h.length === 0) return null;
    return {
      h: o.h,
      rid: typeof o.rid === "number" ? o.rid : undefined,
      repo: typeof o.repo === "string" ? o.repo : undefined,
      inst: typeof o.inst === "string" ? o.inst : undefined,
      jid: typeof o.jid === "string" ? o.jid : undefined,
      t: typeof o.t === "number" ? o.t : undefined,
    };
  } catch {
    return null;
  }
}

/**
 * PURE: turn one `GET /repos/{owner}/{repo}/actions/runners/{runner_id}` result
 * into the only three answers the sweep is allowed to act on.
 *
 * FAIL-SAFE BY CONSTRUCTION, AND THE DIRECTION MATTERS. Leaking a container slot
 * is recoverable — `sleepAfter` still reaps it, and the fleet cap is the only thing
 * that suffers. SIGKILLing a box that is running a customer's job is not: it
 * surfaces ~10 minutes later as GitHub's "the self-hosted runner lost communication
 * with the server", with the customer's work lost. So EVERY shape that is not an
 * unambiguous "GitHub says this specific runner is not working" resolves to
 * `unknown`, and `unknown` means KEEP RENEWING.
 *
 * The mapping, and what GitHub's documentation says about each:
 *
 *   • `busy: true`            ⇒ "busy". Checked FIRST and independently of `status`,
 *     because a runner whose agent has momentarily lost its connection can report
 *     `status: "offline"` while GitHub still has a job assigned to it.
 *   • 404                     ⇒ "idle". An ephemeral runner is de-registered by
 *     GitHub once it has processed its one job, so a runner GitHub no longer knows
 *     about is a runner with nothing left to do.
 *   • `status: "offline"`     ⇒ "idle". This is also the state of a registration
 *     that was created by `generate-jitconfig` and never connected — THE defect
 *     this function exists for: a box that boots and never registers.
 *   • `status: "online"` + `busy: false` ⇒ "idle" (GitHub's UI calls this "Idle":
 *     "The runner is connected to GitHub and is ready to execute jobs.")
 *   • anything else — a non-404 error, a missing body, a non-boolean `busy`, or a
 *     `status` string GitHub has not documented — ⇒ "unknown". An undocumented
 *     status must never be READ as idle; if GitHub adds one, this leaks slots
 *     (visible on the `keepalive_renewed_unverifiable` counter) rather than
 *     killing jobs.
 */
export function runnerActivityVerdict(obs: RunnerObservation | null): RunnerActivity {
  if (!obs) return "unknown"; // no credential / network throw / unparseable
  if (obs.runner && obs.runner.busy === true) return "busy";
  // GitHub has forgotten this runner ⇒ it cannot be executing anything.
  if (obs.httpStatus === 404) return "idle";
  if (obs.httpStatus !== 200 || !obs.runner) return "unknown";
  if (typeof obs.runner.busy !== "boolean") return "unknown"; // ambiguous body
  const status = obs.runner.status;
  if (status === "offline") return "idle"; // never registered, or gone away
  if (status === "online") return "idle"; // busy was false — connected but unused
  return "unknown"; // undocumented status ⇒ refuse to conclude
}

// ── Stranded in-flight jobs (2026-08-23) ─────────────────────────────────────
//
// THE HOLE. `jobPlacementVerdict` above resolves ANYTHING past `queued` to
// "placed" and the reconciler then DROPS the record — "a runner took it (or it is
// already over)". True at the instant it is read, and permanently blind after it:
// a job that was `in_progress` when its box died has no record, no webhook (GitHub
// only redelivers on completion, which never comes for ~600 s) and no sweep. The
// only observation today is GitHub's own timeout surfacing as "the self-hosted
// runner lost communication with the server", ~10 minutes later, to the CUSTOMER.
//
// Meanwhile OUR accounting leaks for hours: the `rhandle:`/`jhandle:`/`jtenant:`
// keys to JOB_PAT_TTL_S (2 h), the concurrency slot to SLOT_TTL_S (45 m), the
// per-job `cas:rw` PAT to its own TTL because revoke fires only on `completed`.
//
// ⛔ THE ONE THING THIS MUST NOT BECOME. On 2026-08-02 a teardown keyed on our own
// bookkeeping SIGKILLed five live customer boxes. So the sweep built on these two
// functions OBSERVES ONLY: it never stops, destroys or signals anything, and it
// may conclude "stranded" ONLY from GitHub's own answers — never from the absence
// or staleness of one of our KV records. Both functions below are therefore
// fail-safe in the SAME direction: every shape that is not an unambiguous
// GitHub-sourced answer is "unknown", and "unknown" means do nothing this tick.

/** One observation of GitHub's view of a single JOB. `null` ⇒ the call never
 *  produced an answer (no credential, network throw, unparseable body). */
export interface JobObservation {
  httpStatus: number;
  job?: { status?: string; runner_id?: number | null; runner_name?: string | null } | null;
}

/**
 * PURE: is the box GONE, from GitHub's point of view?
 *
 * THE AUTHORITY, AND WHY IT CANNOT BE A TRANSPORT ERROR. The only signal accepted
 * is an HTTP **404 on `GET /repos/{owner}/{repo}/actions/runners/{runner_id}`** —
 * GitHub answering, on the wire, that the registration we created no longer
 * exists. `fetchRunnerActivity` turns a throw / no-credential / unparseable body
 * into `null` and every non-200 into its literal status, so a 404 is structurally
 * distinguishable from a timeout (`null`), a rate limit (403/429) and an outage
 * (5xx) — none of which are 404, and all of which land in "unknown".
 *
 *   • 404 ⇒ "gone".    GitHub has no such runner.
 *   • 200 ⇒ "present". The registration is alive; nothing to investigate.
 *   • anything else, including `null` ⇒ "unknown". Ask again next tick.
 *
 * NOTE that "gone" alone is NOT evidence of a problem: GitHub de-registers an
 * ephemeral runner the moment it finishes its one job, so the healthy completion
 * path produces a 404 too. `strandedJobVerdict` is what separates the two.
 */
export function runnerGoneVerdict(obs: RunnerObservation | null): "gone" | "present" | "unknown" {
  if (!obs) return "unknown";
  if (obs.httpStatus === 404) return "gone";
  if (obs.httpStatus === 200) return "present";
  return "unknown";
}

/**
 * PURE: given that the runner is gone, did it take a job down with it?
 *
 * `boundRunnerId` is the runner id from OUR binding. GitHub must CONFIRM the link
 * — `job.runner_id === boundRunnerId` — before this returns "stranded". That check
 * is the whole defence against the 2026-08-02 correlation error: our binding says
 * only which job the box was STARTED for, and GitHub assigns by label match, so
 * the box may well have run somebody else's job. If GitHub reports a different
 * runner id, the answer is "unknown" and the sweep does nothing.
 *
 *   • non-200 / null / no body / non-string status ⇒ "unknown".
 *   • `completed` ⇒ "not_stranded". Ordinary completion; the webhook owns it.
 *   • `queued`    ⇒ "not_stranded" for THIS sweep. Never claimed ⇒ nothing was in
 *     flight to strand, and the placement reconciler already owns that case.
 *   • `in_progress` + `runner_id === boundRunnerId` ⇒ "stranded". A job GitHub
 *     believes is running, on a runner GitHub itself has forgotten.
 *   • `in_progress` on a DIFFERENT runner ⇒ "unknown" (not our box's job).
 *   • an undocumented status ⇒ "unknown". Never read as stranded.
 */
export function strandedJobVerdict(
  obs: JobObservation | null,
  boundRunnerId: number,
): "stranded" | "not_stranded" | "unknown" {
  if (!obs || obs.httpStatus !== 200 || !obs.job) return "unknown";
  const status = obs.job.status;
  if (typeof status !== "string") return "unknown";
  if (status === "completed" || status === "queued" || status === "waiting") return "not_stranded";
  if (status !== "in_progress") return "unknown"; // undocumented ⇒ refuse to conclude
  // GitHub must confirm the job ran on the runner OUR binding names.
  if (typeof obs.job.runner_id !== "number" || obs.job.runner_id !== boundRunnerId) return "unknown";
  return "stranded";
}

/**
 * PURE decision for a dead-letter whose retry was REFUSED at the ceiling
 * (unit-testable; index.ts applies the KV I/O).
 *
 * A refusal does NOT bump `attempts` — a full fleet is not the job's fault, and
 * counting it would turn a burst longer than MAX_ORPHAN_ATTEMPTS ticks back into
 * permanent job loss. The bound is instead the ABSOLUTE ORPHAN_TTL_S window from
 * `firstRecordedMs` — past which GitHub will not usefully assign a runner anyway,
 * which is the same reasoning that sets the TTL itself.
 *
 *   • window exhausted ⇒ "giveup" (delete + log loud; never silently)
 *   • else             ⇒ "wait", carrying the REMAINING ttl, so re-putting the
 *     record never extends the original deadline.
 *
 * `ttlS` is clamped to Cloudflare KV's 60 s minimum; when the true remainder is
 * below that the record just expires on its own, which is the same outcome.
 */
export function orphanRefusalStep(
  rec: OrphanRecord,
  nowMs: number,
  windowS: number = ORPHAN_TTL_S,
): { action: "wait" | "giveup"; ttlS: number; waitedS: number } {
  // A record written before firstRecordedMs existed is treated as fresh rather
  // than infinitely old — it stays bounded by its own KV TTL and by `attempts`.
  const startedMs = rec.firstRecordedMs ?? nowMs;
  const waitedS = Math.max(0, Math.floor((nowMs - startedMs) / 1000));
  const remainingS = windowS - waitedS;
  if (remainingS <= 0) return { action: "giveup", ttlS: 0, waitedS };
  return { action: "wait", ttlS: Math.max(60, remainingS), waitedS };
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
  // CF Access (Inc-3) service-token pair for the gated `/internal/v1/billing/usage`
  // edge — WITHOUT them the usage-push 403s at the Cloudflare Access edge.
  CORELINK_CF_ACCESS_CLIENT_ID?: string;
  CORELINK_CF_ACCESS_CLIENT_SECRET?: string;
}

const BILLING_SOURCE = "corelink-runners/spawn-worker";
// The canonical wire string the Server TL pinned (ASK-2 final, 2026-06-23).
const RUNNER_SLOT_SECONDS_KIND = "runner_slot_seconds";

/**
 * vCPU count of the runner box, and the ONLY place the fleet's shape enters the
 * billing math. Pinned to `RunnerContainer`'s `instance_type` in wrangler.jsonc
 * (`standard-4` = 4 vCPU / 12 GiB / 20 GB — which is also this Cloudflare
 * account's per-deployment ceiling: vcpu_per_deployment = 4).
 *
 * ⚠️ If the fleet ever serves MORE THAN ONE box size (an 8-vCPU or high-memory
 * SKU), this constant becomes wrong for every job that is not standard-4 and
 * MUST be replaced by a per-job value carried from the spawn — not "adjusted".
 * A single global multiplier silently under-bills the bigger SKU on the day it
 * ships, which is the failure this comment exists to prevent.
 */
export const RUNNER_BOX_VCPU = 4;

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
  /** @deprecated Accepted for source compatibility; the wire contract is slot-seconds. */
  vcpu?: number;
}): Promise<UsageEvent> {
  const allocatedS = Math.max(0, Math.floor((opts.completedMs - opts.startedMs) / 1000));
  const period = billingPeriod(opts.completedMs);
  return {
    tenant_id: opts.tenantId,
    // Frozen wire contract: every Worker completion path emits one canonical
    // per-job slot-seconds event. vCPU accounting, where needed, is derived by
    // the owning entitlement path and never changes this event's meaning.
    event_kind: RUNNER_SLOT_SECONDS_KIND,
    qty: allocatedS,
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
export async function pushUsageEvent(env: BillingEnv, ev: UsageEvent, signal?: AbortSignal): Promise<void> {
  const resp = await fetch(env.BILLING_INGEST_URL ?? "", {
    method: "POST",
    headers: {
      ...cfAccessHeaders(env),
      "x-corelink-internal-auth": env.BILLING_INGEST_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    body: JSON.stringify([ev]),
    ...(signal ? { signal } : {}),
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
  readAttribution?: (jobId: string) => Promise<{ jobId: string; tenant: string } | null>,
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
      // A complete usage ledger supplies tenant + execution region. When the
      // completion webhook was lost, T4-W1's ContainmentDO reader supplies the
      // immutable tenant attribution instead.
      let rec: UsageLedgerRecord | null;
      try {
        rec = await readUsageLedger(env.RUNNER_JOB_PATS, job.jobId);
      } catch (error) {
        // A per-job KV read failure must not abort the rest of the sweep. Keep
        // the source untouched and let the next tick retry this same job.
        logEvent("error", "billing_reconcile_ledger_read_failed", {
          jobId: job.jobId,
          error: (error as Error).message,
        });
        continue;
      }
      let tenant = rec?.tenant;
      // GitHub's authenticated completed-job response is the lifecycle source
      // for both timestamps. Durable attribution supplies ownership only.
      const startedMs = rec?.startedMs ?? job.startedMs;
      if (!tenant && readAttribution) {
        try {
          const attribution = await readAttribution(job.jobId);
          if (attribution?.jobId === job.jobId && attribution.tenant.trim()) {
            tenant = attribution.tenant;
          }
        } catch (error) {
          logEvent("error", "billing_reconcile_attribution_read_failed", { jobId: job.jobId, error: (error as Error).message });
        }
      }
      if (!tenant) {
        skipped += 1;
        continue;
      }
      // Prefer the region stored at completion (where the job actually ran); fall
      // back to an explicitly configured BILLING_REGION. Never invent a region.
      const region = rec?.region?.toLowerCase() ?? env.BILLING_REGION?.toLowerCase() ?? "";
      if (!/^[a-z]{3}$/.test(region)) {
        skipped += 1;
        continue;
      }
      if (!rec) {
        // A lost completion webhook has no complete usage row. Freeze the
        // authenticated lifecycle evidence before the first HTTP attempt so a
        // prolonged ingest outage remains recoverable after GitHub lookback.
        if (!env.RUNNER_JOB_PATS) {
          skipped += 1;
          continue;
        }
        try {
          await writeUsageLedger(env.RUNNER_JOB_PATS, {
            jobId: job.jobId,
            tenant,
            startedMs: job.startedMs,
            completedMs: job.completedMs,
            region,
          });
        } catch (error) {
          logEvent("error", "billing_reconcile_ledger_write_failed", { jobId: job.jobId, error: (error as Error).message });
          continue;
        }
      }
      try {
        const ev = await buildUsageEvent({
          tenantId: tenant,
          jobId: job.jobId,
          startedMs,
          completedMs: rec?.completedMs ?? job.completedMs,
          region,
        });
        await pushUsageEvent(env, ev);
        pushed += 1;
      } catch (e) {
        // Fail-open backstop: a push error just means the next tick retries
        // (idem_key makes the re-push safe). Never break the scan.
        logEvent("error", "billing_reconcile_push_failed", {
          jobId: job.jobId,
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
