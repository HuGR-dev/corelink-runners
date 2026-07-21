// CoreLink spawn-Worker + Container DO (ADR-0008).
//
// Cloudflare side of the frozen seam (docs/spec/cloudflare-spawn-worker-contract.md).
// The Rust `CloudflareEngine` (corelink-cloud-engine) calls these endpoints:
// runner spawn (/webhook, /v1/spawn), check-host spawn (/v1/spawn mode:"check"),
// check-exec (/v1/exec, rota A), status/teardown/egress-cutoff, and the env-0
// cred-ticket route (/v1/leases/{id}/cas-cred).
//
// Test state: unit-tested against a mocked @cloudflare/containers SDK — the full
// route surface incl. the native check-exec path (test/check-host.test.ts) and
// the mint/env-0 surface (test/index.test.ts, test/cred-stash-do.test.ts). The
// remaining gate is a LIVE-account smoke (SDK behavior against real Containers),
// owner-gated at deploy — the mocks assert our contract, not Cloudflare's runtime.

import { Container, getContainer } from "@cloudflare/containers";
import { DurableObject } from "cloudflare:workers";

// ── G2 metadata-exposure denylist (O7 hardening) — BEST-EFFORT, NOT G2-closing ─
// Hosts the container is blocked from reaching via the SDK's `deniedHosts`. The
// SDK enforces deniedHosts even with `enableInternet: true` (a denied host is
// blocked "even when enableInternet is true or a catch-all outbound handler is
// set" — @cloudflare/containers container.d.ts:121-123).
//
// ⚠️ IMPORTANT — this does NOT close G2 by itself. The SDK matches deniedHosts
// with `simpleGlobMatch`: pure literal / `*`-glob string matching, with NO CIDR
// math. So only EXACT-HOST entries below actually block anything, and only for
// egress that traverses the SDK's outbound proxy — RAW SOCKETS bypass it. Real
// IMDS / link-local blocking needs platform-network-layer filtering, not this.
// See docs/adr/0009 (G2 is tracked as best-effort / not-yet-verified).
//   • 169.254.169.254            — canonical AWS/GCP/Azure IMDS address (EXACT —
//                                  matches, proxied egress only).
//   • metadata.google.internal   — GCP metadata hostname alias (EXACT — matches).
const METADATA_DENYLIST: string[] = [
  "169.254.169.254",
  "metadata.google.internal",
  // TODO(G2, needs platform-network filtering): the CIDR ranges below are INERT
  // here — `simpleGlobMatch` does NO CIDR math, so these never match a request
  // and give false assurance. Left as a documented TODO, NOT enabled:
  //   "169.254.0.0/16"  — IPv4 link-local range (alternate IMDS IPs)
  //   "fe80::/10"       — IPv6 link-local
  //   "fd00::/8"        — IPv6 unique-local
  // Blocking these ranges requires filtering at the platform network layer
  // (outside this Worker/SDK). Do NOT re-add them as deniedHosts entries
  // expecting range-matching — they will silently no-op. A live-account smoke
  // is still owed even for the exact-host entries above.
];
import {
  safeEqual,
  verifyGithubHmac,
  buildContainerEnv,
  revokeCasPatById,
  buildUsageEvent,
  pushUsageEvent,
  writeUsageLedger,
  claimSpawn,
  releaseSpawnClaim,
  claimCompletion,
  decideSlotAcquire,
  releaseSlotByJob,
  SLOT_TTL_S,
  FLEET_MAX_CONCURRENCY,
  COLD_REPO_CAP,
  decideRedeem,
  parseReconcilerRepos,
  installationIdForRepo,
  tenantPatSecretForRepo,
  installationAllowlistArmed,
  isInstallationAllowlisted,
  matchManagedLabels,
  listOrphanRunnerJobs,
  reconcileCompletedJobBilling,
  RECONCILE_MIN_AGE_MS,
  orphanRetryStep,
  ORPHAN_TTL_S,
  MAX_ORPHAN_ATTEMPTS,
  logEvent,
  type ContainerEnvResult,
  type SlotRecord,
  type StashedCred,
  type StashRecord,
  type CredStashLike,
  type OrphanRecord,
} from "./lib";
import { bumpMetrics, snapshotMetrics, MetricsDO } from "./metrics";
import { installationToken } from "./github_app";

// Re-export the counter Durable Object so wrangler resolves `MetricsDO` from
// this main module (its class + migration are in wrangler.jsonc). Defined in
// ./metrics.ts to keep the counter surface self-contained.
// (ConcurrencySlotsDO + CredStashDO are declared in THIS module below, so the
// Worker runtime already resolves them from the main entrypoint — no re-export
// needed for those.)
export { MetricsDO };

export interface Env {
  RUNNER_CONTAINER: DurableObjectNamespace<RunnerContainer>;
  // The Container DO for a check-host lease (CF-native check-host, campaign B).
  // A `mode:"check"` /v1/spawn routes HERE (not RUNNER_CONTAINER); /v1/exec dials
  // its in-container exec-server on port 8080. See docs/spec/cf-check-host-contract.md.
  CHECK_HOST_CONTAINER: DurableObjectNamespace<CheckHostContainer>;
  // Worker secret (`wrangler secret put`). Must match the fabric's
  // CLOUDFLARE_SPAWN_AUTH_TOKEN. Missing/mismatch ⇒ 401.
  CLOUDFLARE_SPAWN_AUTH_TOKEN: string;
  // Track-C C2b: the bearer the in-container check-host exec-server requires on
  // /exec. Injected into the check-host container env at spawn and presented on
  // the /v1/exec containerFetch. Set via `wrangler secret put`.
  //
  // O7: now REQUIRED for a mode==="check" spawn — an unset secret FAILS CLOSED
  // (503), mirroring the CLOUDFLARE_SPAWN_AUTH_TOKEN fail-closed discipline
  // (authed() returns false when the token is empty). Previously optional
  // (serve-unauthenticated back-compat); that default is removed so a check-host
  // exec-server is never spawned without its auth gate.
  EXEC_SERVER_AUTH_TOKEN?: string;
  // The deploy-time pinned image digest (README wrinkle #1): a RUNNER-mode spawn
  // request's image_digest must equal this, else 409. OPTIONAL by construction:
  // when unset, the runner-mode assertion at the spawn handler is INERT (the
  // container image is wrangler-bound regardless, so this is defense-in-depth, not
  // the isolation floor). To ARM it, set PINNED_IMAGE_DIGEST in wrangler `vars` to
  // the EXACT string fabricd sends as image_digest for a runner spawn (verify the
  // format — full `registry.cloudflare.com/…@sha256:` ref vs bare digest — against
  // the fabricd CloudflareEngine payload at runner-path activation; a mismatch here
  // would 409 every runner spawn). Owner-gated with the runner fleet (autoscaler /
  // GitHub App), which is not yet active — so the inert guard affects no live path.
  // The type is OPTIONAL to reflect reality (it was `: string` = required, which
  // silently lied: it is not set, so the `&&` short-circuited the guard dead).
  PINNED_IMAGE_DIGEST?: string;
  // ── Autoscaler (POST /webhook) — all-Cloudflare, no external fabric ──
  // GitHub webhook HMAC secret (X-Hub-Signature-256). Absent ⇒ /webhook is
  // disabled (the route returns 503), so the autoscaler is opt-in.
  GITHUB_WEBHOOK_SECRET?: string;
  // A GitHub token with repo Administration:write — used to mint the JIT runner
  // config (POST generate-jitconfig). Worker secret. Absent ⇒ /webhook 503.
  // This is the STATIC first-party dogfood credential: it only has rights on
  // HumanGuardrail repos. A CUSTOMER repo's mint uses a GitHub-App installation
  // token instead (GITHUB_APP_ID + GITHUB_APP_PRIVATE_KEY below); this stays the
  // fallback when App creds are absent (byte-identical to the pre-App behaviour).
  GITHUB_MINT_TOKEN?: string;
  // ── GitHub-App installation-token minting (external customer repos) ──────────
  // The App's numeric id + PKCS#8 RSA private-key PEM. When BOTH are set AND a
  // spawn has an installation_id, `mintJit` mints a per-installation access token
  // (scoped to THAT customer's repo) instead of the first-party GITHUB_MINT_TOKEN.
  // Absent ⇒ the App path is INERT and every mint uses GITHUB_MINT_TOKEN exactly
  // as before (default-safe). `wrangler secret put`. See src/github_app.ts.
  GITHUB_APP_ID?: string;
  GITHUB_APP_PRIVATE_KEY?: string;
  // Label a queued workflow_job must carry to be served (default corelink-dogfood).
  AUTOSCALER_LABEL?: string;
  // Per-spawn rate limit (native CF binding) — caps the autoscaler blast radius
  // if the webhook secret is ever leaked. Enforced when bound (see wrangler).
  WEBHOOK_LIMITER?: RateLimit;
  // ── Warm moat (cache-warm) — mint a per-job CAS PAT (D-9) + inject CLW_* ──
  // D-9 internal-auth key (`x-corelink-internal-auth`). Worker secret. Absent ⇒
  // the runner spawns COLD (no cache-warm) — fail-open, north star.
  CORELINK_RUNNER_MINT_AUTH_KEY?: string;
  // D-9 mint base URL (default the public on-net hostname; Option B).
  CORELINK_MINT_URL?: string;
  // The CAS API base URL injected as CLW_ENDPOINT (default same host).
  CLW_ENDPOINT?: string;
  // The owner tenant the per-job CAS PAT + CLW_TENANT are scoped to (dogfood ee30f7ba).
  CLW_TENANT?: string;
  // job_id → pat_id map (written at mint/queued, read+deleted at completion) so
  // revoke can key on pat_id (the live /revoke contract). Absent ⇒ no revoke
  // (PAT TTL-expires; fail-open). See wrangler kv_namespaces.
  RUNNER_JOB_PATS?: KVNamespace;
  // ── Billing usage-push (ASK-2) — per-completed-job runner_slot_seconds ──
  // corelink-billing ingest endpoint (e.g. .../internal/v1/billing/usage).
  // Absent ⇒ no usage-push (fail-open; billing simply not captured).
  BILLING_INGEST_URL?: string;
  // Dedicated `x-corelink-internal-auth` for billing ingest (NEVER the shared
  // key, NEVER the runner_mint key). Worker secret. Absent ⇒ no usage-push.
  BILLING_INGEST_AUTH_KEY?: string;
  // 3-char region stamped on the event; defaults to the request's CF colo.
  BILLING_REGION?: string;
  // ── Re-drive reconciler (scheduled) — recover spawn-orphaned jobs ──────────
  // Comma/space-separated first-party allowlist (owner/repo) the cron scans for
  // queued+labeled+runnerless jobs to re-drive. Absent ⇒ the reconciler is OFF.
  // Cold re-spawn skips per-job authz, so ONLY trusted repos belong here.
  RECONCILER_REPOS?: string;
  // repo_full_name → installation_id JSON map. A plain *repo* webhook payload has
  // no `installation.id` (only a GitHub *App* webhook does), so the server-derived
  // mint (#283) can't derive the tenant and the runner spawns COLD. For known
  // first-party repos we inject the installation_id from this map so the mint runs
  // WARM (server derives the tenant) without requiring an App webhook. e.g.
  // {"HumanGuardrail/corelink-runners":"144561227"}. Absent/unmatched ⇒ COLD.
  REPO_INSTALLATION_MAP?: string;
  // ── Option-C per-tenant-PAT dispatch (server-confirmed live 2026-07-21) ───────
  // JSON `{ "<owner/repo>": "<SECRET_ENV_NAME>" }` mapping a repo to the NAME of the
  // secret binding holding that tenant's acquiring PAT. When a workflow_job repo
  // matches AND that secret is bound, the mint resolves the tenant by INTROSPECTING
  // the PAT (installation_id omitted) instead of deriving it from the installation.
  // The GitHub JIT/box still registers via the installation — only the CAS-tenant
  // changes. Absent/unmatched/unbound ⇒ default installation-derived mint (no-op).
  // e.g. {"HumanGuardrail/corelink-cold-organic-e2e":"COLD_ORGANIC_TENANT_PAT"}.
  REPO_TENANT_PAT_MAP?: string;
  // The acquiring PAT secret(s) referenced by REPO_TENANT_PAT_MAP (bound via
  // `wrangler secret put`; never in wrangler.jsonc). Indexed by name at runtime.
  COLD_ORGANIC_TENANT_PAT?: string;
  // ── External-GA installation allowlist (WP-D) — the pre-mint identity gate ────
  // A comma/whitespace-separated list of GitHub App installation ids permitted to
  // drive a spawn. OPT-IN + FAIL-CLOSED-WHEN-ARMED: unset/blank ⇒ NOT armed ⇒
  // today's exact behavior is preserved (never breaks the live deploy). When
  // armed (≥1 id), a webhook whose resolved installation id is not in the list is
  // refused at the Worker edge BEFORE any mint / spawn / spawn-claim / COLD_REPO_CAP
  // slot / dead-letter orphan — closing the hole where a foreign App-installed but
  // un-entitled repo can churn/DoS the shared FLEET_MAX_CONCURRENCY before the
  // server's post-cold-spawn 403 ever fires. Arm for GA with:
  //   INSTALLATION_ALLOWLIST="144561227,<customer-install-id>"
  // where 144561227 is the dogfood installation (MUST stay served).
  INSTALLATION_ALLOWLIST?: string;
  // ── env-0 (cred-ticket) — keep the CAS PAT OUT of the untrusted container env ──
  // The single-use stash latch (one DO instance per lease_id = GH jobId).
  CRED_STASH: DurableObjectNamespace<CredStashDO>;
  // ── Concurrency slots (W7/F7) — the ATOMIC per-key + fleet concurrency cap ──
  // A SINGLETON DO (always addressed by the fixed id "global") holds the one
  // authoritative in-flight slot list; its single-threaded input-gating makes the
  // read-modify-write atomic (no KV race). Warm mints cap on the per-tenant
  // entitlement (clamped to FLEET), cold spawns on COLD_REPO_CAP per repo; both
  // under the global FLEET cap. Always present (bound in wrangler); the acquire is
  // FAIL-OPEN only on a THROWN DO/infra error, never on a clean at-capacity refusal.
  CONCURRENCY_SLOTS: DurableObjectNamespace<ConcurrencySlotsDO>;
  // Golden-signal counters for the direct fleet (src/metrics.ts). Optional:
  // absent ⇒ bumpMetrics is a no-op + GET /internal/v1/metrics returns {} (the
  // counters are additive/default-safe).
  METRICS?: DurableObjectNamespace<MetricsDO>;
  // Dedicated observability key gating GET /internal/v1/metrics (X-Corelink-
  // Internal-Auth). Default-off: unset ⇒ the route 404s. Separate from the
  // spawn-control CLOUDFLARE_SPAWN_AUTH_TOKEN. `wrangler secret put`.
  METRICS_OBSERVABILITY_KEY?: string;
  // The Worker's OWN public base URL, injected into the container as
  // CLW_FABRIC_ENDPOINT so clw redeems its cred-ticket here at boot. Its PRESENCE
  // enables env-0 (a single-use ticket is injected instead of CLW_TOKEN — the raw
  // PAT never enters the untrusted env). Absent ⇒ FAIL-CLOSED (spawn COLD, no PAT)
  // unless ALLOW_LEGACY_PAT_ENV="1" is explicitly set (non-prod escape hatch). Set
  // this to arm env-0. wrangler var.
  SPAWN_WORKER_PUBLIC_URL?: string;
  // Explicit non-prod escape hatch — see MintEnv.ALLOW_LEGACY_PAT_ENV. Never in prod.
  ALLOW_LEGACY_PAT_ENV?: string;
}

// ── env-0 cred-stash Durable Object — the Worker-native single-use latch ──────
// One instance per lease_id (= GH jobId). The autoscaler stashes the per-job CAS
// PAT here and injects only a CLW_CRED_TICKET into the untrusted container; clw
// redeems it ONCE at boot via POST /v1/leases/{id}/cas-cred. Mirrors fabricd's
// in-process pending_cred + take_cred latch (crates/corelink-fabric-server), so
// clw's CredentialSource redeems against the Worker byte-identically. Storage is
// the DO's own strongly-consistent store — the take is atomic (no CLW_TOKEN race).
export class CredStashDO extends DurableObject<Env> {
  // Stash the PAT under a high-entropy ticket, with a self-cleaning TTL alarm.
  // IDEMPOTENT per lease: if a live stash already exists (an earlier/concurrent
  // spawn attempt for this jobId — the spawn-reliability retries re-run env-0), the
  // existing ticket is KEPT and returned, not overwritten with a fresh one. Returns
  // the EFFECTIVE ticket to inject, so whichever container actually registers
  // redeems a ticket the DO still recognizes.
  async stash(ticket: string, cred: StashedCred, ttlMs: number): Promise<string> {
    const existing = await this.ctx.storage.get<StashRecord>("rec");
    const now = Date.now();
    if (existing && now <= existing.expiresMs) return existing.ticket; // reuse — don't clobber
    const expiresMs = now + ttlMs;
    await this.ctx.storage.put("rec", { ticket, cred, expiresMs });
    await this.ctx.storage.setAlarm(expiresMs);
    return ticket;
  }

  // MULTI-USE redeem (lease-scoped). `{status, cred?}`: 200 (live + correct ticket,
  // every time), 401 (bad ticket), 410 (lease expired), 404 (never stashed). The
  // runner needs the cred for BOTH its boot `clw hydrate` AND the job's `clw run`
  // (corelink-memoize); a single-use latch was consumed by the first, starving the
  // second. The cred is served on every redeem until the lease TTL expires; the
  // decision is the PURE `decideRedeem` (lib, unit-tested), this wrapper only
  // applies the `wipe` at expiry to strongly-consistent DO storage.
  async redeem(ticket: string): Promise<{ status: number; cred?: StashedCred }> {
    const rec = await this.ctx.storage.get<StashRecord>("rec");
    const d = decideRedeem(rec, false, Date.now(), ticket);
    if (d.wipe) await this.ctx.storage.deleteAll();
    return { status: d.status, cred: d.cred };
  }

  // TTL cleanup — wipe the stash at lease expiry (multi-use until then).
  async alarm(): Promise<void> {
    await this.ctx.storage.deleteAll();
  }

  // F2-3 (W3): explicit wipe, called at job COMPLETION to close the credential
  // window immediately instead of waiting for the lease-TTL alarm. After this a
  // redeem of the ticket returns 404 (no stash), so the per-job cas:rw PAT is no
  // longer retrievable by in-lease code once the job ends. Idempotent (deleteAll
  // on an already-empty store is a no-op); also clears the pending TTL alarm.
  async wipe(): Promise<void> {
    await this.ctx.storage.deleteAll();
    await this.ctx.storage.deleteAlarm();
  }
}

// ── Concurrency slots DO (W7/F7) — the ATOMIC per-key + fleet concurrency cap ──
// A SINGLETON (always addressed via idFromName("global")) so every spawn shares
// ONE global count (same pattern as the singleton MetricsDO). It holds the whole
// in-flight slot list under a single "slots" key; the DO's single-threaded
// input-gating serializes the read-modify-write, so — unlike the old KV
// read-then-write — two concurrent admits can NEVER both see `< cap` and both +1.
// The DECISION is the pure `decideSlotAcquire`/`releaseSlotByJob` (lib, unit-
// tested); this wrapper only persists the resulting slot list.
export class ConcurrencySlotsDO extends DurableObject<Env> {
  // ATOMIC acquire: prune-expired → decide (per-key cap THEN fleet cap; idempotent
  // per jobId) → persist. Returns the clean admit/refuse decision — the caller
  // fail-opens ONLY on a THROWN error (infra hiccup), never on a `{admitted:false}`.
  async acquire(
    key: string,
    jobId: string,
    perKeyCap: number,
    fleetCap: number,
    ttlMs: number,
  ): Promise<{ admitted: boolean; reason?: string }> {
    const slots = (await this.ctx.storage.get<SlotRecord[]>("slots")) ?? [];
    const d = decideSlotAcquire(slots, key, jobId, perKeyCap, fleetCap, Date.now(), ttlMs);
    await this.ctx.storage.put("slots", d.slots);
    return { admitted: d.admitted, reason: d.reason };
  }

  // Release a slot by jobId (globally unique — no key needed). Also prunes expired
  // slots. Idempotent: releasing an unknown/already-released jobId is a safe no-op.
  async release(jobId: string): Promise<void> {
    const slots = (await this.ctx.storage.get<SlotRecord[]>("slots")) ?? [];
    await this.ctx.storage.put("slots", releaseSlotByJob(slots, jobId, Date.now()));
  }
}

// Per-job runner container. One DO instance per spawned runner (keyed by handle).
export class RunnerContainer extends Container<Env> {
  // standard-4; the GH-Actions agent is the image ENTRYPOINT (runner-direct, v0).
  // No inbound port — the runner dials OUT to GitHub (the GH-Actions agent is
  // the image entrypoint; runner-direct, v0). `defaultPort` is left unset.
  // Orphan-leak backstop; the DO sleeps (and the container stops) after this
  // IDLE window. Reduced 45m→15m (2026-07-06): a completed job is torn down
  // immediately (teardownCompletedRunner), so sleepAfter only governs FAILED/stuck
  // containers — at 45m those hold account container-instance capacity long enough
  // to starve new spawns under load. 15m still comfortably exceeds any legit
  // between-jobs idle (a runner is ephemeral/one-shot) while freeing capacity ~3×
  // faster. A running job keeps the container active, so this never cuts a live job.
  sleepAfter = "15m";
  // The runner needs egress (git clone, GH API, CAS hydration). ADR-0003 bounds
  // it (no-free-tier + scoped short-TTL PAT + ephemeral box).
  enableInternet = true;
  // O7 / G2: the `deniedHosts` class-property was REMOVED (2026-07-04, coordinator
  // root-cause of the #273 registration regression). On @cloudflare/containers
  // 0.3.x, setting `deniedHosts` AT ALL breaks the container's outbound egress to
  // GitHub — the runner agent can't reach api.github.com to register. It never
  // closed G2 anyway (no CIDR match, raw-socket bypass), so removing it costs
  // nothing on posture. G2 is settled by the metadata probe; a REAL network-layer
  // control (allowlist) lands only if the probe shows metadata reachable. The
  // on-demand cutEgress() kill-switch below (setDeniedHosts at runtime) is unaffected.

  // Start the per-job container with the JIT config + CLW_* injected at runtime
  // (@cloudflare/containers 0.3.x: env arrives via `start({ envVars })`, not baked).
  async startWithEnv(envVars: Record<string, string>): Promise<void> {
    await this.start({ envVars, enableInternet: true });
  }

  // Liveness for GET /v1/status: a running container ⇒ alive.
  async isAlive(): Promise<boolean> {
    const state = await this.getState();
    return state.status === "running" || state.status === "healthy";
  }

  // O7 egress kill-switch: cut ALL outbound egress at runtime WITHOUT a full
  // destroy() (operator-reachable via POST /v1/egress-cutoff, admin-authed).
  // Uses the SDK setter (container.d.ts:120,setDeniedHosts) with a catch-all so
  // every host is denied — the metadata denylist stays in place and "*" blankets
  // the rest. Lets an operator sever a misbehaving lease's network while keeping
  // the container alive for forensics, instead of tearing it down blind.
  // CAVEAT (O7): this operates at the SDK's outbound-proxy layer — it denies
  // HTTP(S) egress that traverses the proxy, but RAW SOCKETS bypass it (same
  // limitation as the boot-time denylist above). For a hard sever, teardown()/
  // destroy() is the fail-closed control; this is the keep-alive soft-cut.
  async cutEgress(): Promise<void> {
    await this.setDeniedHosts([...METADATA_DENYLIST, "*"]);
  }

  // Idempotent teardown for POST /v1/teardown (SIGKILL via destroy()).
  async teardown(): Promise<void> {
    await this.destroy();
  }
}

// Per-lease check-host container (CF-native check-host, campaign B). One DO
// instance per check-host lease (keyed by handle). UNLIKE RunnerContainer, this
// container exposes an HTTP exec-server on port 8080 (C4) that /v1/exec dials via
// `containerFetch`; the toolchain is hydrated once at start from the injected
// TOOLCHAIN_DIGEST (C2/C5). See docs/spec/cf-check-host-contract.md.
export class CheckHostContainer extends Container<Env> {
  // The in-container exec-server listens here (C4); `containerFetch(req, 8080)`
  // and this default both target it.
  defaultPort = 8080;
  // Orphan-leak backstop, mirroring RunnerContainer: the DO sleeps (and the
  // container stops) after this if no exec/teardown arrives.
  sleepAfter = "45m";
  // The check-host needs egress to hydrate the toolchain from CAS at start (C2).
  enableInternet = true;
  // O7 / G2: `deniedHosts` class-property REMOVED — same reason as RunnerContainer
  // (it broke GitHub egress on @cloudflare/containers 0.3.x; never closed G2). The
  // on-demand cutEgress() kill-switch is unaffected.

  // Start the per-lease container with the check env injected at runtime
  // (TOOLCHAIN_DIGEST + CLW_*), enabling egress for the start-time clw hydrate.
  async startWithEnv(envVars: Record<string, string>): Promise<void> {
    await this.start({ envVars, enableInternet: true });
  }

  // Liveness for GET /v1/status?mode=check (audit r4): mirrors RunnerContainer so
  // the status route can query a check-host handle in its own DO namespace.
  async isAlive(): Promise<boolean> {
    const state = await this.getState();
    return state.status === "running" || state.status === "healthy";
  }

  // O7 egress kill-switch, mirroring RunnerContainer.cutEgress: sever outbound
  // egress at runtime (setDeniedHosts + catch-all) without a full destroy().
  async cutEgress(): Promise<void> {
    await this.setDeniedHosts([...METADATA_DENYLIST, "*"]);
  }

  // Idempotent teardown (SIGKILL via destroy()), mirroring RunnerContainer.
  async teardown(): Promise<void> {
    await this.destroy();
  }
}

interface SpawnBody {
  image_digest: string;
  jitconfig: string;
  env: Record<string, string>;
  labels: string[];
  expiry_ms: number;
  // CF-native check-host (C2): "runner" (default, back-compat) | "check". When
  // "check" the spawn routes to CHECK_HOST_CONTAINER and toolchain_digest is
  // required. Absent ⇒ the runner path, byte-unchanged.
  mode?: "runner" | "check";
  // The clw snapshot manifest digest of the toolchain to hydrate at start (C2).
  // Required when mode==="check"; injected as TOOLCHAIN_DIGEST.
  toolchain_digest?: string;
}

// POST /v1/exec request (C3): run argv in an already-spawned check-host lease.
interface ExecBody {
  handle: string;
  argv: string[];
  timeout_ms: number;
}

function unauthorized(): Response {
  return new Response(JSON.stringify({ error: "unauthorized" }), {
    status: 401,
    headers: { "content-type": "application/json" },
  });
}

// safeEqual / verifyGithubHmac / buildContainerEnv (+ the per-job CAS-PAT mint)
// live in ./lib — pure, runtime-agnostic, unit-tested in test/index.test.ts.

function authed(request: Request, env: Env): boolean {
  const tok = env.CLOUDFLARE_SPAWN_AUTH_TOKEN ?? "";
  if (tok.length === 0) return false; // fail-closed: no secret configured ⇒ deny
  const h = request.headers.get("authorization") ?? "";
  return safeEqual(h, `Bearer ${tok}`);
}

// ── Autoscaler (POST /webhook) — GitHub workflow_job → mint JIT → spawn ──────

// Select the credential `mintJit` presents to `generate-jitconfig`:
//   • App creds present (GITHUB_APP_ID + GITHUB_APP_PRIVATE_KEY) AND an
//     installationId in hand ⇒ a GitHub-App INSTALLATION token, scoped to THAT
//     customer's repo (the only credential that can mint a JIT on a foreign repo).
//     `installationToken` THROWS on failure — the mint then fails (never a silent
//     fallback to the first-party token for a foreign repo, which would 404 and
//     mask the real cause). [I4]
//   • Else ⇒ the static first-party GITHUB_MINT_TOKEN. When App creds are absent
//     this is the ONLY branch taken, byte-identical to the pre-App behaviour. [I1]
export async function mintJitAuthToken(env: Env, installationId: string): Promise<string> {
  if (env.GITHUB_APP_ID && env.GITHUB_APP_PRIVATE_KEY && installationId) {
    const { token } = await installationToken(env, installationId, Date.now());
    return token;
  }
  return env.GITHUB_MINT_TOKEN ?? "";
}

// Mint a one-shot JIT runner config for `repoFullName` via the GitHub API. The
// credential is chosen by `mintJitAuthToken`: a per-installation App token for a
// customer repo (when App creds + installationId are present), else the static
// first-party GITHUB_MINT_TOKEN. Returns the encoded JIT.
async function mintJit(
  env: Env,
  repoFullName: string,
  labels: string[],
  installationId: string,
): Promise<string> {
  const authToken = await mintJitAuthToken(env, installationId);
  const name = `cf-runner-${crypto.randomUUID().slice(0, 8)}`;
  const resp = await fetch(
    `https://api.github.com/repos/${repoFullName}/actions/runners/generate-jitconfig`,
    {
      method: "POST",
      headers: {
        authorization: `Bearer ${authToken}`,
        accept: "application/vnd.github+json",
        "user-agent": "corelink-spawn-worker",
      },
      body: JSON.stringify({
        name,
        runner_group_id: 1,
        // The FULL subset-gated label set the job requested — the runner must
        // advertise all of them for GitHub to assign the job (webhook gate proved
        // every one is a servable corelink label).
        labels,
        work_folder: "_work",
      }),
    },
  );
  if (!resp.ok) throw new Error(`generate-jitconfig ${resp.status}: ${await resp.text()}`);
  const j = (await resp.json()) as { encoded_jit_config?: string };
  if (!j.encoded_jit_config) throw new Error("generate-jitconfig: no encoded_jit_config");
  return j.encoded_jit_config;
}

// How long a job_id→pat_id entry lives in KV — a self-cleaning backstop well
// past the longest CI job (the entry is normally deleted at completion).
const JOB_PAT_TTL_S = 7200;

// job_id → server-DERIVED tenant. Stashed at spawn so completion can (a) release
// the per-tenant concurrency slot and (b) bill the CORRECT tenant (not wrangler's
// CLW_TENANT). A distinct `jtenant:` namespace, never colliding with the bare
// jobId (pat map) or `spawn:`/`conc:` keys.
function jobTenantKey(jobId: string): string {
  return `jtenant:${jobId}`;
}

// job_id → the spawned RunnerContainer DO handle (a random UUID minted at spawn).
// Stashed so the `workflow_job:completed` webhook can DESTROY the container
// immediately, instead of leaving it to idle out `sleepAfter` (45m). Without this
// a finished job's container lingers, consuming account container-instance
// capacity — which starves NEW spawns (observed 2026-07-05: dogfood jobs queued
// with no runner while completed-job containers sat in their 45m sleep window).
// A distinct `jhandle:` namespace, never colliding with the other job keys.
function jobHandleKey(jobId: string): string {
  return `jhandle:${jobId}`;
}

// Container-start retry (root-caused 2026-07-03): Cloudflare Container DO
// `start()` intermittently fails with a TRANSIENT platform error — e.g.
// "Internal error while starting up Durable Object storage caused object to be
// reset" — where the SAME call succeeds moments later on a fresh DO (observed:
// a 201 spawn at 14:40, a 502 on the same path at 16:34). It is a CF blip, not a
// config error. Retry a bounded number of times, each with a FRESH handle (a new
// DO, side-stepping a reset one); surface the last error only after exhausting
// attempts, so a PERSISTENT misconfiguration still fails closed (never a silent
// non-spawn). Small linear backoff stays well inside the webhook's ~10s budget
// (a real spawn is ~3s).
const SPAWN_MAX_ATTEMPTS = 3;
// A single `start()` attempt is abandoned after this so a HUNG DO start (the
// transient can hang, not just throw) is retried on a fresh DO instead of
// stalling forever. Kept short so 3 attempts + backoff fit the background budget.
const SPAWN_ATTEMPT_TIMEOUT_MS = 8000;

async function startWithRetry(start: (handle: string) => Promise<void>): Promise<string> {
  let lastErr: unknown;
  for (let attempt = 1; attempt <= SPAWN_MAX_ATTEMPTS; attempt++) {
    const handle = crypto.randomUUID();
    try {
      // Race the start against a timeout — a hung start rejects and is retried.
      await Promise.race([
        start(handle),
        new Promise<never>((_, reject) =>
          setTimeout(
            () => reject(new Error(`start timed out after ${SPAWN_ATTEMPT_TIMEOUT_MS}ms`)),
            SPAWN_ATTEMPT_TIMEOUT_MS,
          ),
        ),
      ]);
      return handle;
    } catch (e) {
      lastErr = e;
      logEvent("info", "container_start_retry", {
        attempt,
        maxAttempts: SPAWN_MAX_ATTEMPTS,
        error: (e as Error).message,
      });
      if (attempt < SPAWN_MAX_ATTEMPTS) {
        await new Promise((r) => setTimeout(r, 300 * attempt));
      }
    }
  }
  throw new Error(
    `container start failed after ${SPAWN_MAX_ATTEMPTS} attempts: ${(lastErr as Error).message}`,
  );
}

// Spawn one runner container with the JIT injected + the ALREADY-authorized
// cache-warm CLW_* overlay (`mint`, from buildContainerEnv, computed BEFORE the
// JIT was minted). On a warm mint we stash job_id→pat_id (revoke keys on pat_id)
// AND job_id→tenant (completion bills/releases the DERIVED tenant). The overlay's
// CLW_TENANT is the server-derived tenant, never wrangler's var.
async function spawnRunner(
  env: Env,
  jit: string,
  jobId: string,
  mint: ContainerEnvResult,
): Promise<string> {
  const containerEnv: Record<string, string> = {
    CORELINK_RUNNER_JITCONFIG: jit,
    ...mint.containerEnv, // CLW_* overlay (empty on a cold spawn)
  };
  const handle = await startWithRetry((h) =>
    getContainer(env.RUNNER_CONTAINER, h).startWithEnv(containerEnv),
  );
  // (The jobId->patId revoke-key is now written at MINT time in driveSpawn, BEFORE
  // the spawn — F2/W3 — so a start failure can revoke the PAT rather than orphan it.
  // Intentionally NOT re-written here.)
  if (mint.tenant && env.RUNNER_JOB_PATS) {
    // Stash the derived tenant for completion (concurrency-slot release + billing).
    await env.RUNNER_JOB_PATS.put(jobTenantKey(jobId), mint.tenant, {
      expirationTtl: JOB_PAT_TTL_S,
    }).catch((e) => logEvent("error", "kv_put_job_tenant_failed", { jobId, error: (e as Error).message }));
  }
  if (env.RUNNER_JOB_PATS) {
    // Stash the DO handle so `completed` can tear the container down immediately
    // (vs the 45m sleepAfter idle-out that starves new spawns). Best-effort: a
    // miss just falls back to sleepAfter (fail-safe, never blocks the spawn).
    await env.RUNNER_JOB_PATS.put(jobHandleKey(jobId), handle, {
      expirationTtl: JOB_PAT_TTL_S,
    }).catch((e) => logEvent("error", "kv_put_job_handle_failed", { jobId, error: (e as Error).message }));
  }
  return handle;
}

// Best-effort revoke of a completed job's per-job CAS PAT, by pat_id (looked up
// from KV). No-op when the mint isn't configured or no pat_id was stored. Fail-
// OPEN: any error is swallowed (the PAT TTL-expires) — never breaks the webhook.
async function revokeCompletedJob(
  env: Env,
  jobId: string,
  derivedTenant?: string,
): Promise<boolean> {
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY || !env.RUNNER_JOB_PATS) return false;
  try {
    const patId = await env.RUNNER_JOB_PATS.get(jobId);
    if (!patId) return false; // cold job, or already revoked/expired
    await revokeCasPatById(env, patId, derivedTenant ?? env.CLW_TENANT);
    await env.RUNNER_JOB_PATS.delete(jobId);
    return true;
  } catch (e) {
    logEvent("error", "revoke_failed", { jobId, error: (e as Error).message });
    return false;
  }
}

// Tear down a completed job's runner container by the DO handle stashed at spawn.
// A finished ephemeral runner's container otherwise idles until `sleepAfter` (45m),
// holding account container-instance capacity and starving new spawns. No-op when
// no handle is on file (legacy/cold spawn, or the KV entry TTL-expired) — sleepAfter
// is the backstop. Fail-OPEN: a destroy() throw is swallowed (teardown() is
// idempotent and the provider deadline is the final backstop), never breaking the
// webhook. Returns true only when a teardown was actually issued.
async function teardownCompletedRunner(env: Env, jobId: string): Promise<boolean> {
  if (!env.RUNNER_JOB_PATS) return false;
  let handle: string | null = null;
  try {
    handle = await env.RUNNER_JOB_PATS.get(jobHandleKey(jobId));
  } catch {
    return false; // KV read failed ⇒ sleepAfter is the backstop
  }
  if (!handle) return false; // cold/legacy job, or already torn down
  try {
    await getContainer(env.RUNNER_CONTAINER, handle).teardown();
  } catch (e) {
    logEvent("error", "teardown_failed", { jobId, error: (e as Error).message });
    // fall through: still drop the handle key so we don't retry a dead handle
  }
  await env.RUNNER_JOB_PATS.delete(jobHandleKey(jobId)).catch(() => {
    /* best-effort: the key TTL-expires */
  });
  return true;
}

// One completed job's workflow_job fields we read for billing.
interface CompletedJob {
  started_at?: string;
  completed_at?: string;
}

// The 3-char region stamped on a usage event: the configured BILLING_REGION, else
// the request's CF colo (the substrate's natural 3-char region, ADR-0008), else
// "". Shared by the live push AND the durable usage-ledger write so both stamp the
// SAME region (the reconciler later validates it is 3-char).
function resolveBillingRegion(env: Env, request: Request): string {
  const colo = (request as unknown as { cf?: { colo?: string } }).cf?.colo;
  return env.BILLING_REGION ?? colo ?? "";
}

// WP-F: durably record this completed job's usage (server-derived tenant + timings
// + region) to the `usage:<jobId>` ledger. Written REGARDLESS of whether the
// billing push is armed — so with the push OFF the ledger still fills and a LATER-
// armed push can backfill it tenant-safely (the reconciler reads this record; the
// GitHub jobs API has no tenant). SKIP when there is no derived tenant (under-bill-
// NEVER-mis-bill: a tenant-less record could never be safely billed) or the timings
// aren't finite (nothing billable). Best-effort + fail-open: a write failure logs
// and never breaks the webhook. MUST run BEFORE the `jtenant:` stash is deleted.
async function recordCompletedJobUsage(
  env: Env,
  jobId: string,
  wj: CompletedJob | undefined,
  derivedTenant: string | undefined,
  region: string,
): Promise<boolean> {
  if (!derivedTenant || !env.RUNNER_JOB_PATS) return false;
  const startedMs = wj?.started_at ? Date.parse(wj.started_at) : NaN;
  const completedMs = wj?.completed_at ? Date.parse(wj.completed_at) : NaN;
  if (!Number.isFinite(startedMs) || !Number.isFinite(completedMs)) return false;
  try {
    await writeUsageLedger(env.RUNNER_JOB_PATS, {
      jobId,
      tenant: derivedTenant,
      startedMs,
      completedMs,
      region,
    });
    return true;
  } catch (e) {
    logEvent("error", "usage_ledger_write_failed", { jobId, error: (e as Error).message });
    return false;
  }
}

// Push the `runner_slot_seconds` usage event for a completed job to corelink-
// billing. No-op (returns false) unless billing is configured AND we can compute
// a slot·seconds duration AND we have a 3-char region. Best-effort + FAIL-OPEN:
// any error is swallowed (billing never breaks the webhook). `region` defaults to
// the request's CF colo (the substrate's natural 3-char region, ADR-0008).
async function maybeBillCompletedJob(
  env: Env,
  jobId: string,
  wj: CompletedJob | undefined,
  request: Request,
  derivedTenant?: string,
): Promise<boolean> {
  // F6 (W7): bill ONLY the SERVER-DERIVED tenant (stashed at spawn). The old
  // `?? env.CLW_TENANT` fallback mis-attributed a customer's runner_slot_seconds to
  // the wrangler CLW_TENANT (dogfood) on a `jtenant:` KV-miss — exactly what the
  // reconciler's I2 rule forbids (lib.ts reconcileCompletedJobBilling emits 0, not a
  // CLW_TENANT bill). No derived tenant ⇒ NO push (under-bill, NEVER mis-bill). Billing
  // is OFF today (BILLING_INGEST_URL unset); this makes the path correct BEFORE
  // multi-tenant billing is armed (3-lens audit F6/Lens C).
  const billedTenant = derivedTenant;
  if (!env.BILLING_INGEST_URL || !env.BILLING_INGEST_AUTH_KEY || !billedTenant) return false;
  try {
    const startedMs = wj?.started_at ? Date.parse(wj.started_at) : NaN;
    const completedMs = wj?.completed_at ? Date.parse(wj.completed_at) : NaN;
    if (!Number.isFinite(startedMs) || !Number.isFinite(completedMs)) return false;
    const region = resolveBillingRegion(env, request);
    if (region.length !== 3) return false; // ingest validates 3-char; skip if unknown
    const ev = await buildUsageEvent({
      tenantId: billedTenant,
      jobId,
      startedMs,
      completedMs,
      region,
    });
    await pushUsageEvent(env, ev);
    return true;
  } catch (e) {
    logEvent("error", "billing_push_failed", { jobId, error: (e as Error).message });
    return false;
  }
}

// The fixed singleton id for the ConcurrencySlotsDO — every spawn shares ONE
// global count (mirrors the singleton MetricsDO). Kept as a helper so both the
// acquire and release call sites address the SAME instance.
function concurrencySlots(env: Env): DurableObjectStub<ConcurrencySlotsDO> {
  return env.CONCURRENCY_SLOTS.get(env.CONCURRENCY_SLOTS.idFromName("global"));
}

// Best-effort release of a spawn's concurrency slot (by globally-unique jobId).
// Fully guarded: swallows BOTH a synchronous throw (an unbound binding in a
// partial/test env) AND an async DO error — a missed release self-heals at the
// slot TTL, so a release failure must NEVER break the webhook / spawn-fail path.
async function releaseConcurrencySlot(env: Env, jobId: string): Promise<void> {
  try {
    await concurrencySlots(env).release(jobId);
  } catch (e) {
    logEvent("error", "concurrency_slot_release_failed", { jobId, error: (e as Error).message });
  }
}

// Atomically acquire a concurrency slot for THIS spawn — warm OR cold:
//   • warm (server-derived tenant + entitlement): key = the tenant, perKeyCap =
//     min(entitlement, FLEET) so a tenant never exceeds what it bought NOR the
//     physical fleet.
//   • cold (no derived tenant): key = `repo:<repo>`, perKeyCap = COLD_REPO_CAP —
//     the old KV path skipped cold spawns entirely (unlimited runners); they are
//     now capped per-repo AND under the same global FLEET cap.
// FAIL-OPEN ONLY on a THROWN DO/infra error (never block a legit job on an infra
// hiccup); a clean `{admitted:false}` is a REAL at-capacity refusal and is honored.
async function acquireConcurrencySlot(
  env: Env,
  jobId: string,
  mint: ContainerEnvResult,
  repo: string,
): Promise<{ admitted: boolean; reason?: string }> {
  const warm = mint.tenant != null && mint.maxConcurrency != null;
  const key = warm ? (mint.tenant as string) : `repo:${repo}`;
  const perKeyCap = warm
    ? Math.min(mint.maxConcurrency as number, FLEET_MAX_CONCURRENCY)
    : COLD_REPO_CAP;
  try {
    return await concurrencySlots(env).acquire(
      key,
      jobId,
      perKeyCap,
      FLEET_MAX_CONCURRENCY,
      SLOT_TTL_S * 1000,
    );
  } catch (e) {
    // Infra hiccup ⇒ ADMIT (never block a legitimate job on a DO error). A clean
    // at-capacity decision above is NOT an error and is honored as a real refusal.
    logEvent("error", "concurrency_slot_acquire_error_failopen", {
      jobId,
      key,
      error: (e as Error).message,
    });
    return { admitted: true };
  }
}

// The spawn drive shared by the webhook path AND the re-drive reconciler:
// AUTHORIZE + warm-mint (env-0 aware) → per-tenant concurrency → GitHub JIT →
// spawn. Assumes the caller ALREADY won the spawn claim. Throws on JIT/spawn
// failure (the guarded wrapper releases the claim). A 403 authz / at-ceiling
// refusal releases the claim inline and returns (no throw — a definitive no-op).
async function driveSpawn(
  env: Env,
  opts: { jobId: string; repo: string; installationId: string; labels: string[] },
): Promise<void> {
  const { jobId, repo, installationId, labels } = opts;
  // env-0: when the Worker's public URL is configured, stash the PAT in the
  // CRED_STASH DO and inject a single-use ticket instead of CLW_TOKEN.
  const env0 = env.SPAWN_WORKER_PUBLIC_URL
    ? {
        stash: {
          stash: (leaseId, ticket, cred, ttlMs) =>
            env.CRED_STASH.get(env.CRED_STASH.idFromName(leaseId)).stash(ticket, cred, ttlMs),
        } satisfies CredStashLike,
        fabricEndpoint: env.SPAWN_WORKER_PUBLIC_URL,
      }
    : undefined;
  // Option-C: if this repo is mapped to a tenant-PAT secret AND that secret is
  // bound, present the PAT so the mint resolves the tenant by introspection
  // (installation_id omitted). Unmapped/unbound ⇒ acquiringPat undefined ⇒ the
  // default installation-derived mint (unchanged). The installationId still flows
  // for the GitHub JIT/box registration below — only the CAS-tenant changes.
  const patSecretName = tenantPatSecretForRepo(env.REPO_TENANT_PAT_MAP, repo);
  const acquiringPat = patSecretName
    ? (env as unknown as Record<string, string | undefined>)[patSecretName]
    : undefined;
  if (patSecretName && acquiringPat) {
    logEvent("info", "mint_option_c_pat_dispatch", { jobId, repo, patSecret: patSecretName });
  }
  const mint = await buildContainerEnv(
    env,
    { jobId, repoFullName: repo, installationId, acquiringPat },
    env0,
  );
  if (mint.authz === "forbidden") {
    await releaseSpawnClaim(env.RUNNER_JOB_PATS, jobId);
    await bumpMetrics(env, "spawn_forbidden");
    logEvent("error", "mint_forbidden", { jobId, repo });
    return;
  }
  // F2 (W3): register the revoke-key jobId->patId at MINT time — BEFORE the spawn.
  // Previously it was written only AFTER a successful container start (spawnRunner),
  // so a start failure orphaned the minted cas:rw PAT to its 2h TTL (and the 60s
  // reconciler re-minted a fresh orphan each tick — 3-lens audit F2/Lens A). Writing
  // it here lets the spawn-failure catch below revoke it immediately.
  if (mint.patId && env.RUNNER_JOB_PATS) {
    await env.RUNNER_JOB_PATS.put(jobId, mint.patId, { expirationTtl: JOB_PAT_TTL_S }).catch((e) =>
      logEvent("error", "kv_put_job_pat_failed", { jobId, error: (e as Error).message }),
    );
  }
  // Concurrency ceiling (W7/F7) — ATOMIC, and enforced for BOTH warm AND cold
  // spawns (the old KV path skipped cold ⇒ unlimited runners). At-capacity ⇒ no
  // spawn (a clean refusal; fail-open is only on a thrown DO error).
  {
    const slot = await acquireConcurrencySlot(env, jobId, mint, repo);
    if (!slot.admitted) {
      await releaseSpawnClaim(env.RUNNER_JOB_PATS, jobId);
      await bumpMetrics(env, "spawn_at_ceiling");
      logEvent("info", "spawn_at_ceiling", {
        jobId,
        repo,
        tenant: mint.tenant,
        maxConcurrency: mint.maxConcurrency,
        reason: slot.reason,
      });
      // W3/F2 (validation-campaign SJ-2 finding): the CAS PAT was already minted (its revoke-key
      // was stored at mint time) but we're REFUSING the spawn — revoke it NOW instead of orphaning
      // it to its ~2h TTL, exactly as the spawn-failure paths do. Fail-open (revokeCompletedJob
      // swallows its own errors); reads jobId->patId, revokes by pat_id, deletes the key. No-op on
      // a cold spawn (no patId stored).
      await revokeCompletedJob(env, jobId, mint.tenant);
      return;
    }
  }
  // Authorized ⇒ mint the GitHub JIT and spawn.
  try {
    const jit = await mintJit(env, repo, labels, installationId);
    await bumpMetrics(env, "jit_minted");
    await spawnRunner(env, jit, jobId, mint);
    await bumpMetrics(env, "runner_spawned");
  } catch (e) {
    // Release the concurrency slot on a spawn failure (the guard releases the claim).
    // Release by jobId ONLY (globally unique) — works for warm AND cold; best-effort
    // (a miss self-heals at the slot TTL). Fully guarded: never mask the spawn error.
    await releaseConcurrencySlot(env, jobId);
    // F2 (W3): the PAT was minted (revoke-key stored above) but the spawn failed —
    // REVOKE it now instead of leaking a live cas:rw PAT to its TTL. revokeCompletedJob
    // reads the jobId->patId stored at mint time, revokes by pat_id, deletes the key,
    // and swallows its own errors (fail-open — never masks the original spawn error).
    await revokeCompletedJob(env, jobId, mint.tenant);
    throw e;
  }
}

// ── Dead-letter orphan store (W7/F8) — records a WARM-recoverable failed spawn ──
// A distinct `orphan:` namespace in RUNNER_JOB_PATS, never colliding with the bare
// jobId (pat map) or `spawn:`/`done:`/`conc:`/`jtenant:`/`jhandle:` keys. The value
// is a JSON `OrphanRecord`; `retryOrphanedSpawns` (scheduled) re-drives it WARM.
const ORPHAN_KEY_PREFIX = "orphan:";
function orphanKey(jobId: string): string {
  return `${ORPHAN_KEY_PREFIX}${jobId}`;
}

// Record the FIRST failure of a WARM-recoverable spawn as a dead-letter so the
// scheduled reconciler retries it WARM (for ANY repo, not just RECONCILER_REPOS).
// Only records when an installation_id was in hand — a cold spawn (no
// installation_id) can't be warm-retried and stays covered by the first-party
// GitHub scan. IDEMPOTENT: records only if no `orphan:<jobId>` already exists (the
// idempotent record of the FIRST failure; attempts is NOT bumped here — the
// reconciler owns the attempt count). Best-effort: a KV error is swallowed so this
// never breaks the spawn path.
export async function recordOrphan(
  env: Env,
  opts: { jobId: string; repo: string; installationId: string; labels: string[] },
): Promise<void> {
  if (!env.RUNNER_JOB_PATS || !opts.installationId) return; // cold ⇒ not warm-recoverable
  try {
    const key = orphanKey(opts.jobId);
    if (await env.RUNNER_JOB_PATS.get(key)) return; // FIRST-failure record only (don't clobber/bump)
    const rec: OrphanRecord = {
      repo: opts.repo,
      installationId: opts.installationId,
      labels: opts.labels,
      attempts: 1,
    };
    await env.RUNNER_JOB_PATS.put(key, JSON.stringify(rec), { expirationTtl: ORPHAN_TTL_S });
    logEvent("info", "orphan_recorded", { jobId: opts.jobId, repo: opts.repo });
  } catch (e) {
    // Best-effort: never break the (already-failed) spawn path on a KV hiccup.
    logEvent("error", "orphan_record_failed", { jobId: opts.jobId, error: (e as Error).message });
  }
}

// driveSpawn wrapped so ANY failure RELEASES the spawn claim — a GitHub redelivery
// or a later reconciler tick can then re-drive the job (never a silent orphan).
async function driveSpawnGuarded(
  env: Env,
  opts: { jobId: string; repo: string; installationId: string; labels: string[] },
): Promise<void> {
  try {
    await driveSpawn(env, opts);
  } catch (e) {
    await releaseSpawnClaim(env.RUNNER_JOB_PATS, opts.jobId);
    await bumpMetrics(env, "spawn_failed");
    logEvent("error", "spawn_drive_failed", { jobId: opts.jobId, error: (e as Error).message });
    // W7/F8: record the WARM-recoverable failure as a dead-letter so the scheduled
    // reconciler retries it WARM (for ANY repo). driveSpawnGuarded is the "first
    // attempt" context (webhook + first-party GitHub scan) — the retry path calls
    // the THROWING driveSpawn directly, so it never re-enters this recording catch.
    await recordOrphan(env, opts);
  }
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    // Top-level guard: every route below already has its OWN try/catch around
    // its failure-prone step, but there was no backstop for an uncaught throw
    // outside those (a routing bug, a malformed URL, a future route missing its
    // own guard) — which would otherwise surface as Cloudflare's raw, un-
    // structured Workers 500. Wrap the whole routing body so ANY uncaught error
    // still returns our structured JSON shape (never leaking the message/stack
    // to the caller) instead of an opaque platform 500.
    try {
      return await handleFetch(request, env, ctx);
    } catch (e) {
      logEvent("error", "fetch_uncaught", { error: (e as Error).message });
      return json({ error: "internal error" }, 500);
    }
  },

  // ── scheduled() — the re-drive + billing reconcilers (cron) ──────────────
  // GitHub fires workflow_job.queued/completed ONCE each; a transient failure
  // (spawn OR the completed-webhook's usage-push) permanently loses that job
  // (an orphaned queue, or silently-dropped billing) with no re-delivery. Both
  // reconcilers below re-scan the SAME `RECONCILER_REPOS` allowlist on the ONE
  // cron trigger (no second trigger added) and re-drive/re-push what the live
  // webhook path missed; each is independently default-off and wrapped so a
  // failure in one never blocks or throws out of the other.
  async scheduled(_event: ScheduledEvent, env: Env, ctx: ExecutionContext): Promise<void> {
    // Family-aware (mirrors the webhook gate): the reconcilers scan for the
    // `corelink` label family, not a fixed default, so an orphaned/unbilled
    // `runs-on: corelink` customer job is recovered too. `AUTOSCALER_LABEL`, if
    // set, pins to the exact label. Passed as the `configured` arg.
    const configured = env.AUTOSCALER_LABEL;
    const now = Date.now();
    await redriveOrphanedJobs(env, ctx, configured, now);
    try {
      // W7/F8: retry the dead-letter WARM (ANY repo). Runs AFTER the first-party
      // GitHub scan — the two are complementary (that scan covers first-party
      // LOST-webhook orphans the dead-letter can't see; the dead-letter covers a
      // WARM-recoverable spawn FAILURE for any repo). Wrapped so it never throws
      // out of scheduled().
      await retryOrphanedSpawns(env, ctx, now);
    } catch (e) {
      logEvent("error", "orphan_retry_failed", { error: (e as Error).message });
    }
    try {
      const pushed = await reconcileCompletedJobBilling(env, configured, now);
      if (pushed > 0) {
        logEvent("info", "billing_reconcile_pushed", { count: pushed });
      }
    } catch (e) {
      // Never let the billing reconciler throw out of scheduled() — it is a
      // backstop, not a gate; a failure here just means next tick retries.
      logEvent("error", "billing_reconcile_failed", { error: (e as Error).message });
    }
  },
};

// The actual route table, factored out of `fetch` so the top-level guard above
// can wrap it uniformly. Behavior is byte-identical to before the guard was
// added — only the outer catch is new.
async function handleFetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const url = new URL(request.url);
  const { pathname } = url;

    // ── GET /internal/v1/metrics — direct-fleet golden-signal snapshot ───────
    // Gated by a DEDICATED observability key (X-Corelink-Internal-Auth), mirroring
    // fabricd's /internal/v1/status — NOT the shared CLOUDFLARE_SPAWN_AUTH_TOKEN
    // (that's the spawn-CONTROL credential; ops-READ is a separate domain, and a
    // shared secret can't be rotated for observability without breaking spawn).
    // Default-off, fail-closed: key unset → 404 (invisible); header mismatch →
    // 401; match → 200. Non-tenant, non-secret counts. The fabricd counters cover
    // the check-exec/moat lease path; THIS covers the autoscaler/direct-fleet path
    // the dogfood product runs on.
    if (request.method === "GET" && pathname === "/internal/v1/metrics") {
      const key = env.METRICS_OBSERVABILITY_KEY ?? "";
      if (key.length === 0) return json({ error: "not found" }, 404);
      const presented = request.headers.get("x-corelink-internal-auth") ?? "";
      if (!safeEqual(presented, key)) return unauthorized();
      return json({ counters: await snapshotMetrics(env) }, 200);
    }

    // ── POST /webhook (GitHub autoscaler) — HMAC-authed, NOT bearer ──────────
    // A queued workflow_job with our label ⇒ mint a JIT + spawn a runner. This
    // is the all-Cloudflare autoscaler: no external fabric. Opt-in (disabled
    // unless both the webhook secret and the mint token are configured).
    if (request.method === "POST" && pathname === "/webhook") {
      if (!env.GITHUB_WEBHOOK_SECRET || !env.GITHUB_MINT_TOKEN) {
        return json({ error: "autoscaler not configured" }, 503);
      }
      const raw = await request.text();
      const sig = request.headers.get("x-hub-signature-256") ?? "";
      if (!(await verifyGithubHmac(env.GITHUB_WEBHOOK_SECRET, sig, raw))) {
        return unauthorized();
      }
      // Only act on workflow_job:queued carrying our managed label.
      if (request.headers.get("x-github-event") !== "workflow_job") {
        return json({ ok: true, ignored: "not workflow_job" }, 200);
      }
      const evt = JSON.parse(raw) as {
        action?: string;
        workflow_job?: {
          labels?: string[];
          id?: number;
          started_at?: string;
          completed_at?: string;
        };
        repository?: { full_name?: string };
        // GitHub-App delivery: the installation whose id the server maps to a
        // tenant. Present on App-authed webhooks (required for the runner mint).
        installation?: { id?: number | string };
      };
      // Serve the CoreLink managed-label FAMILY (bare `corelink` + `corelink-
      // <suffix>`, minus RESERVED like `corelink-builder`), SUBSET-gated: refuse
      // a job that also needs a label we don't provide. AUTOSCALER_LABEL, when
      // set, pins to an exact label. `mintLabels` is the servable corelink label
      // set the runner must advertise so GitHub assigns exactly this job.
      const jobLabels = evt.workflow_job?.labels ?? [];
      const mintLabels = matchManagedLabels(jobLabels, env.AUTOSCALER_LABEL);
      if (!mintLabels) {
        return json({ ok: true, ignored: "not our label" }, 200);
      }
      // The stable correlation id across queued→completed for THIS job. The PAT
      // is minted under it (job_id) so completion can revoke the SAME PAT.
      const jobId = String(evt.workflow_job?.id ?? "");
      if (!jobId) return json({ error: "no workflow_job.id in payload" }, 400);

      // ── workflow_job:completed ⇒ revoke the per-job CAS PAT (hardening) ──────
      // Shrinks the post-job window the PAT is valid (TTL is the backstop).
      // Best-effort + fail-open: a revoke failure never breaks the webhook.
      if (evt.action === "completed") {
        // Look up the DERIVED tenant stashed at spawn (fallback: wrangler's
        // CLW_TENANT for legacy/cold jobs). Used for revoke, the concurrency-slot
        // release, AND billing — so every completion acts on the RIGHT tenant.
        let derivedTenant: string | undefined;
        if (env.RUNNER_JOB_PATS) {
          derivedTenant = (await env.RUNNER_JOB_PATS.get(jobTenantKey(jobId))) ?? undefined;
        }
        const revoked = await revokeCompletedJob(env, jobId, derivedTenant);
        // Release the concurrency slot (W7/F7) — by jobId ONLY, so it releases a
        // warm OR cold spawn's slot without needing the derived tenant. Best-effort
        // (the slot TTL self-heals a missed release, so this never permanently
        // blocks a tenant/repo). Fully guarded (never breaks the webhook).
        await releaseConcurrencySlot(env, jobId);
        // WP-F: durably record this job's usage to the `usage:<jobId>` ledger NOW —
        // BEFORE the `jtenant:` stash is dropped below and while derivedTenant + the
        // workflow_job timings are still in hand. Written even when the push is off
        // (ledger fills ⇒ a later-armed push backfills tenant-safely); skipped when
        // there's no derived tenant. Independent of the push, so history exists to
        // backfill (the reconciler reads this record, not the tenant-less GitHub API).
        const region = resolveBillingRegion(env, request);
        const ledgered = await recordCompletedJobUsage(
          env,
          jobId,
          evt.workflow_job,
          derivedTenant,
          region,
        );
        // Drop the derived-tenant stash (still needed for revoke + billing above;
        // the usage ledger above already captured the tenant durably for backfill).
        if (derivedTenant && env.RUNNER_JOB_PATS) {
          await env.RUNNER_JOB_PATS.delete(jobTenantKey(jobId)).catch(() => {
            /* best-effort: TTL is the backstop */
          });
        }
        // ASK-2: emit the per-job runner_slot_seconds usage event (prod billing
        // lives here, not the dev-only Rust fabricd). Best-effort, fail-open.
        const billed = await maybeBillCompletedJob(
          env,
          jobId,
          evt.workflow_job,
          request,
          derivedTenant,
        );
        // Tear the runner container DOWN immediately (vs the 45m sleepAfter idle-
        // out). A finished ephemeral runner's container otherwise lingers, holding
        // account container-instance capacity and starving NEW spawns (root cause
        // of the 2026-07-05 dogfood spawn stall). Best-effort + fail-open: no handle
        // on file (legacy/cold job, or a KV miss) ⇒ sleepAfter is the backstop; a
        // destroy() throw is swallowed (idempotent teardown, deadline backstop).
        const tornDown = await teardownCompletedRunner(env, jobId);
        // F2-3 (W3): wipe the env-0 cred-stash so the per-job cas:rw PAT window
        // closes at COMPLETION, not at the 2h lease-TTL. After this a ticket redeem
        // by any in-lease code returns 404 (stash gone) — the credential dies with
        // the job. Security action (NOT dedup-gated); best-effort + fail-open (a
        // wipe failure just falls back to the TTL alarm). CRED_STASH is keyed by
        // leaseId == jobId (CLW_LEASE_ID = jobId, lib.ts; + the cas-cred route).
        if (env.CRED_STASH) {
          await env.CRED_STASH.get(env.CRED_STASH.idFromName(jobId)).wipe().catch((e) =>
            logEvent("error", "cred_stash_wipe_failed", { jobId, error: (e as Error).message }),
          );
        }
        // 2c completed-leg dedup: GitHub redelivers `completed` (at-least-once).
        // Claim the completion so the `webhook_job_completed` counter is bumped
        // EXACTLY once — a redelivery is a counter no-op. This gates ONLY the
        // metric; the security actions above (revoke / slot-release / teardown)
        // are NOT gated by it — they already ran and are each independently
        // idempotent/self-healing, so a redelivery re-runs them safely.
        const firstCompletion = await claimCompletion(env.RUNNER_JOB_PATS, jobId);
        // Golden signals for the completion leg (fire-and-forget; never delays
        // the GitHub webhook response).
        const completedSignals: string[] = [];
        if (firstCompletion) completedSignals.push("webhook_job_completed");
        if (revoked) completedSignals.push("cas_pat_revoked");
        if (billed) completedSignals.push("billing_pushed");
        if (tornDown) completedSignals.push("runner_torn_down");
        if (completedSignals.length > 0) ctx?.waitUntil?.(bumpMetrics(env, ...completedSignals));
        return json({ ok: true, revoked, billed, ledgered, tornDown, deduped: !firstCompletion, job_id: jobId }, 200);
      }

      if (evt.action !== "queued") {
        return json({ ok: true, ignored: `action ${evt.action}` }, 200);
      }
      // Rate-limit real spawn attempts (defense-in-depth vs a leaked webhook
      // secret). Ignored events above are free; only queued+labeled jobs count.
      // 2a per-tenant key: bucket by the REPO (`spawn:<repoFullName>`) so ONE busy
      // repo can no longer starve every other tenant's spawns (the global
      // `key:"spawn"` was a single cross-tenant bucket). The repo is known here
      // (evt.repository.full_name); when absent we still rate-limit under the
      // literal `spawn:` bucket — NEVER fail-open to unbounded spawns (I1).
      if (env.WEBHOOK_LIMITER) {
        const rateKey = `spawn:${evt.repository?.full_name ?? ""}`;
        const { success } = await env.WEBHOOK_LIMITER.limit({ key: rateKey });
        if (!success) {
          ctx?.waitUntil?.(bumpMetrics(env, "webhook_rate_limited"));
          return json({ error: "rate limited" }, 429);
        }
      }
      // The repo is the webhook's repository (full_name).
      const repo = evt.repository?.full_name ?? "";
      if (!repo) return json({ error: "no repository in payload" }, 400);
      // ── Multi-tenant runner-mint authorization inputs ────────────────────────
      // The server DERIVES the tenant from installation_id + repo_full_name (we no
      // longer send owner_tenant). `installation.id` is present ONLY on GitHub
      // *App* webhook deliveries — a plain *repo* webhook (this repo's autoscaler
      // hook) NEVER includes it. #283 originally 400-rejected a queued event with
      // no installation_id when the mint key was armed; on a repo webhook that
      // rejects EVERY spawn (observed 2026-07-06: workflow_job.queued → 400, jobs
      // never spawn). FAIL-OPEN TO COLD instead (the north star: slow, never
      // broken). With installationId == "" the mint is skipped downstream
      // (`buildContainerEnv` returns an empty overlay), so the runner spawns COLD
      // — no tenant, no CAS, no cache-warm, and CRUCIALLY no wrong-tenant WARM
      // spawn (the only thing the 400 actually protected against). Server-derived
      // tenant + env-0 cache-warm require the *App* webhook (which carries
      // installation.id); until that's wired, repo-webhook spawns are COLD.
      // installation.id comes only on App-webhook deliveries. On a repo webhook it
      // is absent; inject the known installation_id for first-party repos from
      // REPO_INSTALLATION_MAP so the server-derived mint (#283) runs WARM. If the
      // repo isn't mapped, installationId stays "" ⇒ the mint is skipped downstream
      // and the runner spawns COLD (fail-open, north star — never a 400).
      let installationId = evt.installation?.id != null ? String(evt.installation.id) : "";
      if (!installationId) {
        installationId = installationIdForRepo(env.REPO_INSTALLATION_MAP, repo);
      }
      if (env.CORELINK_RUNNER_MINT_AUTH_KEY && !installationId) {
        logEvent("info", "installation_id_missing", { jobId, repo });
      }
      // ── External-GA installation allowlist gate (WP-D) ───────────────────────
      // MUST run here — after the installation id is resolved (App id, or the
      // REPO_INSTALLATION_MAP injection for first-party repo-webhooks) and BEFORE
      // `claimSpawn` below (the first consumer of a spawn-claim) and therefore
      // before `driveSpawnGuarded` (mint + COLD_REPO_CAP slot + `recordOrphan`).
      // OPT-IN: unset/blank INSTALLATION_ALLOWLIST ⇒ not armed ⇒ this is a no-op
      // (today's exact behavior). Armed + id not in the list ⇒ refuse EARLY with a
      // clean ack (202, NOT 5xx — a 5xx makes GitHub retry the same rejected id),
      // having taken NO claim / NO slot / NO orphan.
      if (installationAllowlistArmed(env.INSTALLATION_ALLOWLIST)) {
        if (!isInstallationAllowlisted(env.INSTALLATION_ALLOWLIST, installationId)) {
          logEvent("info", "webhook_installation_not_allowlisted", { jobId, repo, installationId });
          ctx?.waitUntil?.(bumpMetrics(env, "webhook_installation_not_allowlisted"));
          return json(
            { ok: true, ignored: "installation not allowlisted", job_id: jobId },
            202,
          );
        }
      }
      // ── Spawn idempotency (gap #2): claim this jobId BEFORE the expensive
      // mint+spawn. A redelivered queued webhook (GitHub at-least-once) for the
      // same job loses the claim and is a no-op — no double mint+spawn / double
      // COGS. Fail-open when no KV is bound (dedup is an optimization, never a
      // gate that refuses a real job).
      if (!(await claimSpawn(env.RUNNER_JOB_PATS, jobId))) {
        ctx?.waitUntil?.(bumpMetrics(env, "webhook_spawn_deduped"));
        return json({ ok: true, deduped: true, job_id: jobId }, 200);
      }
      // Respond to GitHub FAST (202) and do the mint+spawn in the BACKGROUND:
      // awaiting container.start() inline risks GitHub's 10s webhook timeout →
      // 504 whenever a DO start HANGS on a transient reset (observed 2026-07-03).
      // `startWithRetry` (per-attempt timeout + fresh DO) then abandons a hung
      // start and retries instead of stalling the webhook. On terminal failure we
      // RELEASE the claim so a GitHub redelivery / re-queue can spawn.
      // Respond FAST (202); AUTHORIZE + warm-mint (env-0) + JIT + spawn run in the
      // BACKGROUND (driveSpawnGuarded): awaiting start() inline risks GitHub's 10s
      // webhook timeout when a DO start hangs on a transient reset (2026-07-03).
      // The guard releases the claim on failure so a redelivery / the scheduled
      // reconciler can re-drive the job (never a silent orphan).
      ctx?.waitUntil?.(bumpMetrics(env, "webhook_spawn_claimed"));
      ctx.waitUntil(driveSpawnGuarded(env, { jobId, repo, installationId, labels: mintLabels }));
      return json({ ok: true, spawning: true, job_id: jobId }, 202);
    }

    // ── POST /v1/leases/{lease_id}/cas-cred — env-0 cred-ticket redemption ────
    // TICKET-authed (NOT bearer): clw, inside the untrusted container, redeems its
    // single-use CLW_CRED_TICKET here for the per-job CAS PAT. Mounted BEFORE the
    // bearer gate because the ticket IS the credential. Contract is byte-identical
    // to fabricd's handlers/cas_cred (200 {cas_pat, clw_endpoint, clw_tenant,
    // clw_ref_domain}; 401 bad ticket; 410 already-redeemed/expired; 404 no lease)
    // so clw's CredentialSource redeems against the Worker or fabricd identically.
    {
      const cred = pathname.match(/^\/v1\/leases\/([^/]+)\/cas-cred$/);
      if (request.method === "POST" && cred) {
        const leaseId = decodeURIComponent(cred[1]);
        let body: { ticket?: string };
        try {
          body = (await request.json()) as { ticket?: string };
        } catch (e) {
          return json({ error: `invalid JSON body: ${(e as Error).message}` }, 400);
        }
        if (!body.ticket) return json({ error: "ticket required" }, 400);
        const r = await env.CRED_STASH.get(env.CRED_STASH.idFromName(leaseId)).redeem(body.ticket);
        if (r.status === 200 && r.cred) {
          return json(
            {
              cas_pat: r.cred.token,
              clw_endpoint: r.cred.endpoint,
              clw_tenant: r.cred.tenant,
              clw_ref_domain: "runner",
            },
            200,
          );
        }
        if (r.status === 401) return json({ error: "invalid ticket" }, 401);
        if (r.status === 410) return json({ error: "ticket already redeemed" }, 410);
        return json({ error: "no such lease" }, 404);
      }
    }

    // ── /v1/* routes — bearer-authed (the fabric/Engine seam) ────────────────
    if (!authed(request, env)) return unauthorized();

    // POST /v1/spawn
    if (request.method === "POST" && pathname === "/v1/spawn") {
      let body: SpawnBody;
      try {
        body = (await request.json()) as SpawnBody;
      } catch (e) {
        return json({ error: `invalid JSON body: ${(e as Error).message}` }, 400);
      }

      // README wrinkle #1: image is wrangler-bound; image_digest is an ASSERTION.
      // Guard the type too — an absent/non-string image_digest would throw on
      // `.includes` and surface as an opaque 500 rather than a clean 400.
      if (typeof body.image_digest !== "string" || !body.image_digest.includes("@sha256:")) {
        return json({ error: "image_digest must be a content-pinned string (@sha256:)" }, 400);
      }

      // The container spawn (SDK `start()`) is the failure-prone step: a bad image
      // build/push or an SDK error would otherwise throw UNCAUGHT and Cloudflare
      // returns an opaque 500 with no diagnostic. Wrap it so the real cause is
      // surfaced as a structured 502 (fail-closed — the fabric's CloudflareEngine
      // sees a diagnosable Err, never a fabricated success). Mirrors the
      // /webhook + /v1/exec + /v1/teardown error discipline already in this file.
      try {
        // ── Check-mode (C2): route to CHECK_HOST_CONTAINER (NOT the runner DO) ──
        // Additive + back-compat: mode absent OR "runner" ⇒ the unchanged runner
        // path below. mode==="check" requires toolchain_digest; injected as
        // TOOLCHAIN_DIGEST so the container hydrates the toolchain at start (C2/C5).
        if (body.mode === "check") {
          if (!body.toolchain_digest) {
            return json({ error: "toolchain_digest required when mode==check" }, 400);
          }
          // O7 (fail-closed): the exec-server bearer is REQUIRED for a check-host
          // spawn. Without it the exec-server would serve unauthenticated, so we
          // refuse to spawn one — mirroring authed()'s "no secret ⇒ deny" gate
          // (index.ts fail-closed on an empty CLOUDFLARE_SPAWN_AUTH_TOKEN). 503:
          // a config/service-not-ready condition, not the caller's fault.
          //
          // ⚠️ DEPLOY-ORDERING (breaking): this secret was the back-compat-unset
          // default and is now MANDATORY. Provision it BEFORE deploying this
          // Worker version — `wrangler secret put EXEC_SERVER_AUTH_TOKEN` → then
          // `wrangler deploy` — or every check-mode spawn 503s until it is set.
          // See deploy/cloudflare/README.md "Deploy-ordering" note.
          const execAuthToken = env.EXEC_SERVER_AUTH_TOKEN;
          if (!execAuthToken) {
            return json(
              { error: "EXEC_SERVER_AUTH_TOKEN is not configured; check-host spawn refused" },
              503,
            );
          }
          // Retry the DO start on a transient CF reset (fresh handle each try).
          const handle = await startWithRetry((h) =>
            getContainer(env.CHECK_HOST_CONTAINER, h).start({
              envVars: {
                ...body.env,
                TOOLCHAIN_DIGEST: body.toolchain_digest!,
                // Track-C C2b (now REQUIRED, guaranteed present by the check above):
                // inject the exec-server bearer so the in-container /exec requires
                // it; the SAME value is presented on the /v1/exec containerFetch.
                EXEC_SERVER_AUTH_TOKEN: execAuthToken,
              },
              enableInternet: true,
            }),
          );
          return json({ handle }, 201);
        }

        // ── Runner mode (default / absent) — byte-unchanged ──────────────────
        if (env.PINNED_IMAGE_DIGEST && body.image_digest !== env.PINNED_IMAGE_DIGEST) {
          return json(
            { error: "image_digest does not match the deployed pinned image" },
            409,
          );
        }

        // Inject the per-job env (JIT config + CLW_*) at start (runtime, not baked);
        // retry the DO start on a transient CF reset (fresh handle each attempt).
        const handle = await startWithRetry((h) =>
          getContainer(env.RUNNER_CONTAINER, h).startWithEnv(body.env),
        );
        return json({ handle }, 201);
      } catch (e) {
        return json({ error: `spawn failed: ${(e as Error).message}` }, 502);
      }
    }

    // ── POST /v1/exec (C3) — run argv in an already-spawned check-host lease ──
    // Relays the container's exec-server JSON {exit_code, stdout, stderr} back as
    // 200. A non-2xx from the container is FAIL-CLOSED (502/503; never a
    // fabricated success) so CloudflareEngine::exec_captured returns Err.
    if (request.method === "POST" && pathname === "/v1/exec") {
      let body: ExecBody;
      try {
        body = (await request.json()) as ExecBody;
      } catch {
        // Match /v1/spawn + /cas-cred: a malformed/empty body is a clean 400,
        // not an opaque 500 (trusted bearer caller, but diagnosable > opaque).
        return json({ error: "invalid JSON body" }, 400);
      }
      if (!body.handle) return json({ error: "missing handle" }, 400);
      const container = getContainer(env.CHECK_HOST_CONTAINER, body.handle);
      let resp: Response;
      try {
        resp = await container.containerFetch(
          new Request("http://check/exec", {
            method: "POST",
            headers: {
              "content-type": "application/json",
              // Track-C C2b: present the exec-server bearer (the same value
              // injected at spawn). Absent secret ⇒ header omitted. NOTE: this
              // no-auth fallback is now UNREACHABLE for any live check-host — the
              // O7 change makes check-mode spawn hard-require EXEC_SERVER_AUTH_TOKEN
              // (fail-closed 503), so no check container can exist without it. The
              // spread is kept only so the request shape is uniform; it is not a
              // live fail-open.
              ...(env.EXEC_SERVER_AUTH_TOKEN
                ? { authorization: `Bearer ${env.EXEC_SERVER_AUTH_TOKEN}` }
                : {}),
            },
            body: JSON.stringify({ argv: body.argv, timeout_ms: body.timeout_ms }),
          }),
          8080,
        );
      } catch (e) {
        // The container is unreachable (gone / not started / dial failure) ⇒
        // fail-closed (503), never a fabricated CmdOutput.
        return json({ error: `check-host unreachable: ${(e as Error).message}` }, 503);
      }
      if (!resp.ok) {
        // The exec-server returned a non-2xx ⇒ fail-closed (502). The fabric must
        // NOT see a CmdOutput; run_check fails closed.
        const detail = await resp.text().catch(() => "");
        return json({ error: `check-host exec failed: ${resp.status} ${detail}` }, 502);
      }
      // Relay the byte-faithful {exit_code, stdout, stderr} verbatim as 200 (C3).
      const out = (await resp.json()) as {
        exit_code: number | null;
        stdout: string;
        stderr: string;
      };
      return json(out, 200);
    }

    // GET /v1/status/{handle}?mode=check|runner
    if (request.method === "GET" && pathname.startsWith("/v1/status/")) {
      const handle = pathname.slice("/v1/status/".length);
      if (!handle) return json({ error: "missing handle" }, 400);
      // Route by mode (audit r4): a check-host handle lives in CHECK_HOST_CONTAINER,
      // NOT RUNNER_CONTAINER. Querying the wrong DO namespace returns a fresh
      // never-started stub (isAlive()=false → false 404). Default 'runner' is
      // back-compat. Mirrors the spawn/exec routing.
      // Branch the getContainer call (not a `ns` var) — the two DO types differ,
      // so a union would not typecheck.
      const checkMode = url.searchParams.get("mode") === "check";
      const container = checkMode
        ? getContainer(env.CHECK_HOST_CONTAINER, handle)
        : getContainer(env.RUNNER_CONTAINER, handle);
      const alive = await container.isAlive();
      return alive
        ? json({ status: "alive" }, 200)
        : json({ status: "gone" }, 404);
    }

    // POST /v1/teardown  (idempotent) — body: { handle, mode?: "check"|"runner" }
    if (request.method === "POST" && pathname === "/v1/teardown") {
      let body: { handle: string; mode?: string };
      try {
        body = (await request.json()) as { handle: string; mode?: string };
      } catch (e) {
        return json({ error: `invalid JSON body: ${(e as Error).message}` }, 400);
      }
      const handle = body.handle;
      if (!handle) return json({ error: "missing handle" }, 400);
      // Route by mode (audit r4): without this, a check-host teardown hit
      // RUNNER_CONTAINER (wrong namespace) → a silent no-op, leaking the live
      // CheckHostContainer until its 45m sleepAfter backstop. Default 'runner'.
      const container =
        body.mode === "check"
          ? getContainer(env.CHECK_HOST_CONTAINER, handle)
          : getContainer(env.RUNNER_CONTAINER, handle);
      // Idempotent SIGKILL teardown; already-gone is success for the caller. A
      // destroy() throw must NOT 500 — log loud and still return 204 (the
      // provider deadline is the backstop). Mirrors the /v1/exec error discipline.
      try {
        await container.teardown();
      } catch (e) {
        logEvent("error", "teardown_route_failed", {
          handle,
          mode: body.mode ?? "runner",
          error: String(e),
        });
      }
      return new Response(null, { status: 204 });
    }

    // ── POST /v1/egress-cutoff — O7 operator egress kill-switch ───────────────
    // body: { handle, mode?: "check"|"runner" }. Sever a live lease's OUTBOUND
    // egress WITHOUT a full destroy() — the container stays up (for forensics /
    // an orderly wind-down) while its network is cut. Bearer-authed like the rest
    // of /v1/* (an operator/admin path, reached through the same fabric bearer).
    // Routed by mode exactly like /v1/teardown so a check-host handle hits its
    // own DO namespace. Idempotent + fail-soft: a setter throw is logged and
    // still returns 204 (teardown remains the hard backstop). Wires the SDK
    // setDeniedHosts() setter (container.d.ts:120) via each container's cutEgress.
    if (request.method === "POST" && pathname === "/v1/egress-cutoff") {
      let body: { handle: string; mode?: string };
      try {
        body = (await request.json()) as { handle: string; mode?: string };
      } catch (e) {
        return json({ error: `invalid JSON body: ${(e as Error).message}` }, 400);
      }
      const handle = body.handle;
      if (!handle) return json({ error: "missing handle" }, 400);
      const container =
        body.mode === "check"
          ? getContainer(env.CHECK_HOST_CONTAINER, handle)
          : getContainer(env.RUNNER_CONTAINER, handle);
      try {
        await container.cutEgress();
      } catch (e) {
        logEvent("error", "egress_cutoff_failed", {
          handle,
          mode: body.mode ?? "runner",
          error: String(e),
        });
      }
      return new Response(null, { status: 204 });
    }

    return json({ error: "not found" }, 404);
}

// ── the re-drive reconciler (cron, part 1 of 2 — see scheduled() above) ─────
// GitHub fires workflow_job.queued ONCE; a transient spawn failure orphans the
// job forever. Each tick lists queued+labeled+runnerless jobs older than the
// grace window in the RECONCILER_REPOS allowlist and re-drives their spawn
// (COLD — no installation_id from the jobs API; a running runner beats an
// orphan). The claim-KV dedups against the webhook + prior ticks. OFF unless
// RECONCILER_REPOS is set AND the autoscaler is configured.
async function redriveOrphanedJobs(
  env: Env,
  ctx: ExecutionContext,
  configured: string | undefined,
  now: number,
): Promise<void> {
  const repos = parseReconcilerRepos(env.RECONCILER_REPOS);
  if (repos.length === 0) return; // opt-in: no allowlist ⇒ reconciler off
  if (!env.GITHUB_WEBHOOK_SECRET || !env.GITHUB_MINT_TOKEN) return; // autoscaler not configured
  for (const repo of repos) {
    const orphans = await listOrphanRunnerJobs(env, repo, configured, RECONCILE_MIN_AGE_MS, now);
    // Each orphan carries its OWN matched family label so the redrive mints the
    // JIT with exactly what the job requested (family-aware).
    for (const { jobId, labels } of orphans) {
      // `listOrphanRunnerJobs` already proved this job is queued ≥ MIN_AGE,
      // labeled, and has NO runner — genuinely orphaned. A spawn claim can LEAK
      // when the background `driveSpawnGuarded` (waitUntil) is killed by the
      // platform before its catch releases the claim (a slow mint+start
      // exceeding the waitUntil budget). A leaked claim then blocks the
      // reconciler FOREVER (`claimSpawn` → false → skip), so the recovery path
      // never recovers — the exact deadlock observed 2026-07-05 (stuck `spawn:`
      // claims, jobs queued with no runner, no self-heal). CLEAR any stale claim
      // first, then re-claim fresh (concurrent ticks still dedup on the fresh
      // claim). This turns "stuck forever" into "retry each tick until a spawn
      // succeeds".
      //
      // WARM re-drive (2026-07-06): use the installation_id from
      // REPO_INSTALLATION_MAP (same as the webhook), so a reconciler-recovered
      // job is WARM (cache-warm), not COLD — otherwise every job that fell to the
      // reconciler silently lost cache-warm. RECONCILER_REPOS is a trusted
      // first-party allowlist, so authorizing the mint on re-drive is safe. An
      // unmapped repo ⇒ installationId "" ⇒ COLD (unchanged fallback).
      const reInstallationId = installationIdForRepo(env.REPO_INSTALLATION_MAP, repo);
      await releaseSpawnClaim(env.RUNNER_JOB_PATS, jobId);
      if (await claimSpawn(env.RUNNER_JOB_PATS, jobId)) {
        logEvent("info", "reconciler_redrive", {
          jobId,
          repo,
          warm: !!reInstallationId,
        });
        ctx.waitUntil(
          driveSpawnGuarded(env, { jobId, repo, installationId: reInstallationId, labels }),
        );
      }
    }
  }
}

// ── the dead-letter orphan retry (cron, part 3 of 3 — see scheduled() above) ─────
// W7/F8: retry the WARM-recoverable failed spawns recorded by `recordOrphan` (the
// `orphan:<jobId>` dead-letter). UNLIKE `redriveOrphanedJobs` (first-party GitHub
// scan, RECONCILER_REPOS-scoped, cold), this re-drives WARM (the record carries the
// installation_id ⇒ buildContainerEnv authorizes+mints) and works for ANY repo,
// including external customers. Bounded (MAX_ORPHAN_ATTEMPTS), idempotent
// (claimSpawn dedups vs the live path), self-healing (ORPHAN_TTL_S).
//
// `drive` is injected (defaults to the THROWING `driveSpawn`, NOT driveSpawnGuarded
// — so a retry FAILURE does NOT re-enter the recording catch and re-create the
// dead-letter) so the reconciler is unit-testable with a mocked drive.
export async function retryOrphanedSpawns(
  env: Env,
  _ctx: ExecutionContext,
  _now: number,
  drive: (
    env: Env,
    opts: { jobId: string; repo: string; installationId: string; labels: string[] },
  ) => Promise<void> = driveSpawn,
): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) return; // no dead-letter store bound ⇒ nothing to retry
  let listed: { keys: { name: string }[] };
  try {
    listed = await kv.list({ prefix: ORPHAN_KEY_PREFIX });
  } catch (e) {
    logEvent("error", "orphan_retry_list_failed", { error: (e as Error).message });
    return;
  }
  for (const { name } of listed.keys) {
    const jobId = name.slice(ORPHAN_KEY_PREFIX.length);
    // Parse the record (a malformed/absent value ⇒ null ⇒ the "missing" branch).
    let rec: OrphanRecord | null = null;
    try {
      const raw = await kv.get(name);
      rec = raw ? (JSON.parse(raw) as OrphanRecord) : null;
    } catch {
      rec = null;
    }
    const step = orphanRetryStep(rec, MAX_ORPHAN_ATTEMPTS);
    if (step.action === "missing") continue; // TTL-expired between list and get — skip
    if (step.action === "giveup") {
      // Bounded: never retry forever. Delete the dead-letter + log loud.
      await kv.delete(name).catch(() => {
        /* best-effort: the key TTL-expires */
      });
      logEvent("error", "orphan_retry_giveup", {
        jobId,
        repo: rec!.repo,
        attempts: rec!.attempts,
      });
      continue;
    }
    // retry: bump the attempt count (same TTL), then claim + WARM re-drive.
    const bumped: OrphanRecord = { ...(rec as OrphanRecord), attempts: step.nextAttempts };
    await kv
      .put(name, JSON.stringify(bumped), { expirationTtl: ORPHAN_TTL_S })
      .catch(() => {
        /* best-effort: a failed bump just means next tick re-reads the old count */
      });
    // Idempotent: if the job is already claimed (a live path / another tick won
    // it), skip this tick and LEAVE the record for later.
    if (!(await claimSpawn(kv, jobId))) continue;
    try {
      await drive(env, {
        jobId,
        repo: bumped.repo,
        installationId: bumped.installationId,
        labels: bumped.labels,
      });
      // Recovered ⇒ drop the dead-letter (the spawn claim is left to TTL-expire,
      // blocking redeliveries for the job's lifetime, same as the live path).
      await kv.delete(name).catch(() => {
        /* best-effort: the key TTL-expires */
      });
      logEvent("info", "orphan_retry_recovered", {
        jobId,
        repo: bumped.repo,
        attempts: bumped.attempts,
      });
    } catch (e) {
      // Retry failed ⇒ release the claim so a later tick (or the live path) can
      // re-drive, and LEAVE the (bumped) record for the next tick.
      await releaseSpawnClaim(kv, jobId);
      await bumpMetrics(env, "spawn_failed");
      logEvent("error", "orphan_retry_drive_failed", {
        jobId,
        repo: bumped.repo,
        attempts: bumped.attempts,
        error: (e as Error).message,
      });
    }
  }
}

function json(obj: unknown, status: number): Response {
  return new Response(JSON.stringify(obj), {
    status,
    headers: { "content-type": "application/json" },
  });
}
