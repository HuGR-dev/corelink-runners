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

// The per-job CAS PAT mint result: the plaintext token (injected as CLW_TOKEN)
// + its pat_id (the handle /revoke keys on at completion).
export interface MintResult {
  token: string;
  patId: string;
}

// Mint a per-job CAS PAT via D-9 (corelink-server). Scope cas:rw, tenant-scoped
// (A6: per-job, never the tenant PAT). Throws on any failure — the caller falls
// open to a COLD spawn (north star: cache absent ⇒ slow, never broken).
async function mintCasPat(env: MintEnv, jobId: string): Promise<MintResult> {
  const base = env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com";
  const resp = await fetch(`${base}/internal/v1/runner/mint`, {
    method: "POST",
    headers: {
      "x-corelink-internal-auth": env.CORELINK_RUNNER_MINT_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    body: JSON.stringify({ owner_tenant: env.CLW_TENANT, job_id: jobId, scope: "cas:rw" }),
  });
  if (!resp.ok) throw new Error(`D-9 mint ${resp.status}`);
  // LIVE wire shape (verified 2026-06-21): 200 → {token_plaintext, pat_id, token_id,
  // principal, tenant, expires_ms}. The PAT value is `token_plaintext` (NOT `token`,
  // which the relay doc mis-stated). Keys logged on miss so any future drift is loud.
  const j = (await resp.json()) as { token_plaintext?: string; pat_id?: string };
  if (!j.token_plaintext) {
    throw new Error(`D-9 mint: no token_plaintext (200 keys: ${Object.keys(j).join(",")})`);
  }
  if (!j.pat_id) {
    throw new Error(`D-9 mint: no pat_id (200 keys: ${Object.keys(j).join(",")})`);
  }
  return { token: j.token_plaintext, patId: j.pat_id };
}

// Revoke a per-job CAS PAT via D-9 — keyed by `pat_id` (the live /revoke contract:
// `{owner_tenant, job_id}` → 400 "pat_id required"). The pat_id comes from the mint
// response and is carried across the queued→completed gap via the RUNNER_JOB_PATS KV
// (mint+revoke are separate Worker invocations). Throws on failure; caller swallows
// it (the PAT is TTL-bounded, so revoke is best-effort window-shrinking hardening).
export async function revokeCasPatById(env: MintEnv, patId: string): Promise<void> {
  const base = env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com";
  const resp = await fetch(`${base}/internal/v1/runner/revoke`, {
    method: "POST",
    headers: {
      "x-corelink-internal-auth": env.CORELINK_RUNNER_MINT_AUTH_KEY ?? "",
      "content-type": "application/json",
      "user-agent": "corelink-spawn-worker",
    },
    body: JSON.stringify({ pat_id: patId, owner_tenant: env.CLW_TENANT }),
  });
  if (!resp.ok) throw new Error(`D-9 revoke ${resp.status}: ${await resp.text()}`);
}

// The result of building the per-job container env: the env to inject, plus the
// pat_id (present iff the warm-mint succeeded) so the caller can persist it for
// revoke-on-completion.
export interface ContainerEnvResult {
  containerEnv: Record<string, string>;
  patId?: string;
}

// Build the per-job container env: always the JIT; cache-warm CLW_* WHEN the
// D-9 mint is configured AND succeeds. Any mint failure ⇒ cold env (fail-open).
// Returns the pat_id on a warm mint so the caller can stash job_id→pat_id for
// revoke-on-completion (the /revoke contract keys on pat_id, not job_id).
export async function buildContainerEnv(
  env: MintEnv,
  jit: string,
  jobId: string,
): Promise<ContainerEnvResult> {
  const containerEnv: Record<string, string> = { CORELINK_RUNNER_JITCONFIG: jit };
  if (env.CORELINK_RUNNER_MINT_AUTH_KEY && env.CLW_TENANT) {
    try {
      const { token, patId } = await mintCasPat(env, jobId);
      containerEnv.CLW_ENDPOINT = env.CLW_ENDPOINT ?? "https://corelink-api.humangr.com";
      containerEnv.CLW_TENANT = env.CLW_TENANT;
      containerEnv.CLW_TOKEN = token; // per-job; never logged
      containerEnv.CLW_REF_DOMAIN = "runner";
      return { containerEnv, patId };
    } catch (e) {
      // Fail-OPEN: spawn cold (no CLW_*). The entrypoint's cache-warm hook is
      // also fail-open, so a job ALWAYS runs — slow, never broken.
      console.log(`warm-mint failed, spawning COLD: ${(e as Error).message}`);
    }
  }
  return { containerEnv };
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
