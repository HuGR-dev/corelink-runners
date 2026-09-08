import { ComputeBudgetClient } from "./lib/compute_budget_client";
import { ComputeObligations, type ComputeBinding } from "./lib/compute_budget_obligation";
import { NormalIntakeInbox, installationTombstoneKey, type NormalIntakeInput, type NormalIntakeRecord } from "./lib/normal_intake_inbox";
import { JobAttributionAuthority } from "./lib/job_attribution_authority";
import { CredentialObligationAuthority } from "./lib/credential_obligation_authority";
import { RetryEpochAuthority } from "./lib/retry_epoch_authority";
import { retryEpochClient, type RetryEpochAuthorityRpc } from "./lib/retry_epoch_client";
import { controlAuthed } from "./lib/control_auth";
import { authorizeRunner, RunnerAuthorizationError } from "./lib/runner_authorization";
import { adoptIssuedRunnerCredential } from "./lib/runner_credential_adoption";
import { runnerCredentialLeaseId } from "./lib/runner_credential_lease";
import { ConcurrencyAuthority } from "./lib/concurrency_authority";
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

import { CRED_STASH_CLOSED_KEY, expireCredential, stashCredential, wipeCredential } from "./lib/cred_stash_lifecycle.js";
import { Container, getContainer } from "@cloudflare/containers";
import { DurableObject } from "cloudflare:workers";
import { EXEC_SERVER_AUTH_TOKEN_FILE } from "./lib/clw";
export { RunnerDevEnvDO } from "./durable_objects/runner_dev_env";

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
  spawnClaimAgeMs,
  SPAWN_CLAIM_TTL_S,
  claimCompletion,
  SLOT_TTL_S,
  FLEET_MAX_CONCURRENCY,
  COLD_REPO_CAP,
  decideRedeem,
  parseReconcilerRepos,
  vcpuCeilingKey,
  vcpuUsageKey,
  vcpuWarnedKey,
  vcpuWarningStep,
  billingPeriod,
  RUNNER_BOX_VCPU,
  VCPU_KEY_TTL_S,
  VCPU_WARN_THRESHOLDS,
  canonicalInstallationId,
  installationIdForRepo,
  tenantPatSecretForRepo,
  installationAllowlistArmed,
  isInstallationAllowlisted,
  matchManagedLabels,
  unservedCapabilityClaims,
  SERVED_INSTANCE_TYPE,
  listOrphanRunnerJobs,
  reconcileCompletedJobBilling,
  RECONCILE_MIN_AGE_MS,
  orphanRetryStep,
  orphanRefusalStep,
  SpawnRefusedError,
  ORPHAN_TTL_S,
  MAX_ORPHAN_ATTEMPTS,
  placementConfirmStep,
  jobPlacementVerdict,
  PLACEMENT_CONFIRM_GRACE_MS,
  encodeRunnerBinding,
  runnerGoneVerdict,
  strandedJobVerdict,
  type JobObservation,
  parseRunnerBinding,
  runnerActivityVerdict,
  type RunnerBinding,
  type RunnerObservation,
  type RunnerActivity,
  logEvent,
  type ContainerEnvResult,
  type StashedCred,
  type StashRecord,
  type CredStashLike,
  type OrphanRecord,
} from "./lib";
import { flushBillingUsageBacklog } from "./billing_recovery";
import { spendAdmissionBudget, type AdmissionBudgetVerdict } from "./lib/admission_budget";
import { bumpMetrics, snapshotMetrics, MetricsDO } from "./metrics";
import {
  dispatchTenantSuspensionRevocations as dispatchTenantSuspensionRevocationsOwned,
  revokeCompletedJob as revokeCompletedJobOwned,
  revokeIssuedCredential,
  retryFailedRevocations as retryFailedRevocationsOwned,
} from "./lib/revocation_outbox.js";
import type { CredentialIdentity, CredentialPage, CredentialSelection } from "./lib/credential_authority_contract.js";
import { TenantSuspensionAuthority, type TenantSuspensionInput } from "./lib/tenant_suspension_authority.js";
import { consumeTenantSuspensionCredentials, type TenantSuspensionConsumerDependencies } from "./lib/tenant_suspension_credentials.js";
import { installationToken } from "./github_app";
import {
  claimReconcileHandoff,
  discoverAuthorizationCandidates,
  releaseReconcileHandoff,
  type ReconcilerRepository,
} from "./reconciler";
import { confirmInstallationRepositories } from "./reconciler_membership";
import {
  ContainmentEffectLedger,
  containmentEffectPointerKey,
  type ContainmentEffectAttempt,
  type ContainmentEffectBinding,
  type ContainmentEffectIdentity,
  type ContainmentEffectPermit,
  type ContainmentEffectPrepareInput,
  type ContainmentEffectReapInput,
  type ContainmentEffectReceipt,
  type ContainmentEffectResult,
  type ContainmentEffectTransition,
  type OwnerResult,
  type SpawnOwnerRequest,
  type SpawnMirrorObservation,
} from "./containment_effect_ledger";
import { admitDrainOwnerInTransaction } from "./containment_drain_owner_reap";
import { drainOwnerTuple, intakeOwnerTuple, redriveOwnerTuple, runCanonicalEffect, type ProviderDriveReceipt } from "./containment_effect_route";
import {
  containmentEventKey,
  containmentJobIndexKey,
  containmentJobIndexMarkerKey,
  containmentPauseKey,
  containmentReservationKey,
  emptyContainmentMeta,
  isValidJobIndex,
  isValidJobIndexMeta,
  isValidJobIndexMarker,
  MAX_ACTIVE_INDEX_EVENTS,
  normalizeRedriveIdentity,
  redriveEffectId,
  reservationPermit,
  reservationTupleMatches,
  type ContainmentJobIndex,
  type ContainmentJobIndexMarker,
  type ContainmentJobIndexMeta,
} from "./containment_authority_helpers";
import {
  acknowledgeInvalidConfigInStorage,
  canonicalContainmentEvidence,
  CONTAINMENT_EFFECT_WITNESS_KINDS,
  containmentEffectEvidenceKey,
  containmentEffectJobKey,
  CONTAINMENT_INDEX_META_KEY,
  CONTAINMENT_META_KEY,
  DRAIN_LEASE_TTL_MS,
  DRAIN_RENEW_THRESHOLD_MS,
  isCurrentHead,
  leaseMatches,
  markInvalidConfigAttemptInStorage,
  pendingInvalidConfigInStorage,
  recordInvalidConfigInStorage, REDRIVE_RESERVATION_TTL_MS, validateInvalidConfigIdentity,
  type ContainmentEffectEvidence,
  type ContainmentEffectWitnessKind,
  type ContainmentEvent,
  type ContainmentMeta,
  type ContainmentOutboxRecord,
  type ContainmentPause,
  type ContainmentRedrivePermit,
  type ContainmentRedriveReservation,
} from "./containment_authority_records";
import { canonicalWorkflowJobIdFromRaw } from "./workflow_job_id";
import {
  persistJobAttribution,
  readJobAttribution,
  type JobAttribution,
  type JobAttributionStore,
} from "./lib/job_attribution.js";
export {
  ContainmentEffectLedger,
  containmentEffectMirrorFromAttempt,
  containmentEffectMirrorKey,
  containmentEffectPointerKey,
  readContainmentEffectMirror,
} from "./containment_effect_ledger";
export type {
  ContainmentEffectAttempt,
  ContainmentEffectBinding,
  ContainmentEffectIdentity,
  ContainmentEffectPermit,
  ContainmentEffectPrepareInput,
  ContainmentEffectReapInput,
  ContainmentEffectReceipt,
  ContainmentEffectResult,
  ContainmentEffectState,
  ContainmentEffectTransition,
} from "./containment_effect_ledger";
export type { ContainmentEffectEvidence, ContainmentEffectWitnessKind, ContainmentEvent, ContainmentMeta,
  ContainmentOutboxRecord, ContainmentPause, ContainmentRedrivePermit, ContainmentRedriveReservation, ContainmentState, InvalidConfigRecord } from "./containment_authority_records";
export { DRAIN_LEASE_TTL_MS, REDRIVE_RESERVATION_TTL_MS };

// Re-export the counter Durable Object so wrangler resolves `MetricsDO` from
// this main module (its class + migration are in wrangler.jsonc). Defined in
// ./metrics.ts to keep the counter surface self-contained.
// (ConcurrencySlotsDO + CredStashDO are declared in THIS module below, so the
// Worker runtime already resolves them from the main entrypoint — no re-export
// needed for those.)
export { MetricsDO };

export interface Env {
  FABRIC_COMPUTE_URL?: string;
  FABRIC_COMPUTE_TERMINAL_AUTHORITY?: string;
  FABRIC_COMPUTE_TERMINAL_PUBLIC_KEY?: string;
  FABRIC_COMPUTE_TERMINAL_RECEIPT_VERSION?: string;
  FABRIC_COMPUTE_TERMINAL_KEY_ID?: string;
  RUNNER_CONTAINER: DurableObjectNamespace<RunnerContainer>;
  // The Container DO for a check-host lease (CF-native check-host, campaign B).
  // A `mode:"check"` /v1/spawn routes HERE (not RUNNER_CONTAINER); /v1/exec dials
  // its in-container exec-server on port 8080. See docs/spec/cf-check-host-contract.md.
  CHECK_HOST_CONTAINER: DurableObjectNamespace<CheckHostContainer>;
  // Worker secret (`wrangler secret put`). Must match the fabric's
  // CLOUDFLARE_SPAWN_AUTH_TOKEN. Missing/mismatch ⇒ 401.
  CLOUDFLARE_SPAWN_AUTH_TOKEN: string;
  // Independent control domains; all three keys must differ. No shared-key fallback.
  CLOUDFLARE_EXEC_AUTH_TOKEN?: string;
  CLOUDFLARE_LIFECYCLE_AUTH_TOKEN?: string;
  // Shared emergency edge freeze. Absent or exact "0" is fail-open; every
  // other value pauses new admission/spawn work (fail-closed on bad config).
  FABRIC_ADMISSION_PAUSED?: string;
  AUTOSCALER_INTAKE_PAUSED?: string;
  AUTOSCALER_REDRIVE_PAUSED?: string;
  CONTAINMENT_ADMIN_KEY?: string;
  CONTAINMENT?: DurableObjectNamespace<ContainmentDO>;
  // Track-C C2b: the bearer the in-container check-host exec-server requires on
  // /exec. Provider ingress is passed only to the short-lived entrypoint; the
  // entrypoint writes EXEC_SERVER_AUTH_TOKEN_FILE and unsets this variable
  // before starting the durable server. The Worker presents the same bearer on
  // the /v1/exec containerFetch. Set via `wrangler secret put`.
  //
  // O7: now REQUIRED for a mode==="check" spawn — an unset secret FAILS CLOSED
  // (503), mirroring the CLOUDFLARE_SPAWN_AUTH_TOKEN fail-closed discipline
  // (controlAuthed() refuses missing or overlapping domain tokens). Previously optional
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
  // Optional repository-hook secret. The CoreLink App uses GITHUB_WEBHOOK_SECRET;
  // a first-party repository hook may use this separate secret so rotating or
  // restoring the repo delivery path never changes the App's credential.
  GITHUB_WEBHOOK_REPO_SECRET?: string;
  // A GitHub token with repo Administration:write — used to mint the JIT runner
  // config (POST generate-jitconfig). Worker secret. Absent ⇒ /webhook 503.
  // This is the STATIC first-party dogfood credential: it only has rights on
  // HuGR-Labs repos. A CUSTOMER repo's mint uses a GitHub-App installation
  // token instead (GITHUB_APP_ID + GITHUB_APP_PRIVATE_KEY below); this stays the
  // fallback when App creds are absent (byte-identical to the pre-App behaviour).
  GITHUB_MINT_TOKEN?: string;
  // Per-installation token used only for the authoritative reconciler scan.
  GITHUB_RECONCILER_TOKEN?: string;
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
  // D-9 internal-auth key (`x-corelink-internal-auth`). Worker secret. Required
  // spawn preparation refuses missing authorization before JIT/provider work.
  CORELINK_RUNNER_MINT_AUTH_KEY?: string;
  // D-9 mint base URL (default the public on-net hostname; Option B).
  CORELINK_MINT_URL?: string;
  // The CAS API base URL injected as CLW_ENDPOINT (default same host).
  CLW_ENDPOINT?: string;
  // The owner tenant the per-job CAS PAT + CLW_TENANT are scoped to (dogfood ee30f7ba).
  CLW_TENANT?: string;
  // Compatibility projections and lifecycle indexes. CredentialObligationAuthority
  // owns exact-PAT revocation. Bare job→PAT metadata expires by TTL; a KV
  // read/delete cannot safely remove a concurrent replacement. See kv_namespaces.
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
  // Authoritative external-repo registry. When armed, this replaces the static
  // first-party list for recovery and fails closed if its snapshot is unclear.
  RECONCILER_REGISTRY_URL?: string;
  RECONCILER_REGISTRY_AUTH_KEY?: string;
  // repo_full_name → installation_id JSON map. A plain *repo* webhook payload has
  // no `installation.id` (only a GitHub *App* webhook does), so the server-derived
  // mint (#283) can't derive the tenant and the runner spawns COLD. For known
  // first-party repos we inject the installation_id from this map so the mint runs
  // WARM (server derives the tenant) without requiring an App webhook. e.g.
  // {"HuGR-Labs/corelink-runners":"150584374"}. Absent/unmatched ⇒ COLD.
  REPO_INSTALLATION_MAP?: string;
  // ── Option-C per-tenant-PAT dispatch (server-confirmed live 2026-07-21) ───────
  // JSON `{ "<owner/repo>": "<SECRET_ENV_NAME>" }` mapping a repo to the NAME of the
  // secret binding holding that tenant's acquiring PAT. When a workflow_job repo
  // matches AND that secret is bound, the mint resolves the tenant by INTROSPECTING
  // the PAT (installation_id omitted) instead of deriving it from the installation.
  // The GitHub JIT/box still registers via the installation — only the CAS-tenant
  // changes. Absent/unmatched/unbound ⇒ default installation-derived mint (no-op).
  // e.g. {"HuGR-Labs/corelink-cold-organic-e2e":"COLD_ORGANIC_TENANT_PAT"}.
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
  //   INSTALLATION_ALLOWLIST="150584374,<customer-install-id>"
  // where 150584374 is the dogfood installation (MUST stay served).
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
  // Dedicated ops-READ key gating GET /internal/v1/fleet/busy (X-Corelink-
  // Internal-Auth) — the pre-roll deploy gate's only authority on "is a box
  // executing customer work". Deliberately its OWN credential: ops-READ is a
  // separate domain from spawn-CONTROL (CLOUDFLARE_SPAWN_AUTH_TOKEN) and from
  // observability (METRICS_OBSERVABILITY_KEY), so it can be rotated — or leaked
  // and revoked — without breaking spawn or the canary. It is held by a GitHub
  // Actions secret, which is a wider blast radius than either of those, and that
  // is precisely why it must not be shared. Default-off: unset ⇒ the route 404s.
  // `wrangler secret put`.
  FLEET_BUSY_READ_KEY?: string;
  // The Worker's OWN public base URL, injected into the container as
  // CLW_FABRIC_ENDPOINT so clw redeems its cred-ticket here at boot. Its PRESENCE
  // enables env-0 (a single-use ticket is injected instead of CLW_TOKEN — the raw
  // PAT never enters the untrusted env). Absent ⇒ FAIL-CLOSED (spawn COLD, no PAT)
  // unless ALLOW_LEGACY_PAT_ENV="1" is explicitly set (non-prod escape hatch). Set
  // this to arm env-0. wrangler var.
  SPAWN_WORKER_PUBLIC_URL?: string;
  // Explicit non-prod escape hatch — see MintEnv.ALLOW_LEGACY_PAT_ENV. Never in prod.
  ALLOW_LEGACY_PAT_ENV?: string;
  // ── Orphan-box reconciliation (platform-truth sweep, B-002) ──────────────────
  // The ENABLE flag for `reconcileOrphanBoxes`. Default-OFF + FAIL-SAFE: unset/
  // blank/"0"/"false" ⇒ the sweep is a NO-OP (it does not even enumerate the
  // platform), so this whole feature lands INERT — no behaviour change on any live
  // path — mirroring the opt-in `crash_probe_config_from_env` posture. Set to a
  // truthy value ("1"/"true") to turn on OBSERVE-ONLY reconciliation. Enabling the
  // reconciler alone NEVER destroys anything; it only logs orphan candidates and
  // bumps `orphan_box_detected`. `wrangler var` / `wrangler secret put`.
  RECONCILE_ORPHAN_BOXES?: string;
  // The SEPARATE, owner-gated teardown flag. Default-OFF ⇒ pure dry-run. Declared
  // now so the Env shape is stable, but the teardown path itself is NOT wired in
  // this landing (B-002 is observe-only): flipping it today changes nothing. When
  // the teardown half lands (like the B-038 audit lease), it will re-read the
  // `sbox:` set immediately before each destroy (TOCTOU guard) and bump
  // `orphan_box_reaped`. Never enable before the join key is confirmed live.
  RECONCILE_ORPHAN_TEARDOWN?: string;
  // ── Cloudflare Containers API creds — the "platform truth" enumeration ────────
  // Needed by `reconcileOrphanBoxes` to enumerate the ACTUAL running instances
  // per-app (the account-level /containers/instances endpoint is dead — it returns
  // {instances:[]} unconditionally — so enumeration MUST be per-application). BOTH
  // the account id AND a containers-read token must be bound or the sweep no-ops
  // (this is what keeps it inert until an owner wires the creds via `wrangler
  // secret put`). Token precedence mirrors scripts/container-instances.sh:
  // CLOUDFLARE_CONTAINERS_API_TOKEN preferred, plain CLOUDFLARE_API_TOKEN accepted.
  CLOUDFLARE_ACCOUNT_ID?: string;
  CLOUDFLARE_CONTAINERS_API_TOKEN?: string;
  CLOUDFLARE_API_TOKEN?: string;
}

export interface SpawnClaimRecord {
  jobId: string;
  generation: number;
  ownerToken: string;
  phase: "claimed" | "active";
  claimedAtMs: number;
  expiresAtMs: number;
  /** The provider identity that GitHub echoes on a completion webhook. */
  providerIdentity?: string;
}

export type SpawnClaimResult =
  | { status: "acquired"; generation: number; ownerToken: string }
  | { status: "held"; generation: number }
  | { status: "invalid" };

export type SpawnClaimRelease = "released" | "stale" | "missing";

/**
 * Durable post-start ownership.  This lives beside the generation claim rather
 * than in KV: KV is a projection with a TTL and is therefore never evidence
 * that a provider start may be repeated or that a slot may be released.
 */
export interface ActiveSpawnAttempt {
  schemaVersion: 1;
  jobId: string;
  generation: number;
  ownerToken: string;
  handle: string;
  runnerName: string;
  runnerId?: number;
  repo: string;
  installationId: string;
  jitAttempt: number;
  preparationId?: string;
  createdAtMs: number;
  /** Written before start; retained until exact teardown is confirmed. */
  teardownIntent: true;
  /** Exact handle was observed down; capacity cleanup may still be pending. */
  teardownConfirmedAtMs?: number;
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
  async stash(ticket: string, cred: StashedCred, ttlMs: number, absoluteExpiresAtMs?: number): Promise<string> {
    return this.ctx.blockConcurrencyWhile(() => stashCredential(this.ctx.storage, ticket, cred, ttlMs, absoluteExpiresAtMs));
  }

  // MULTI-USE redeem (lease-scoped). `{status, cred?}`: 200 (live + correct ticket,
  // every time), 401 (bad ticket), 410 (lease expired), 404 (never stashed). The
  // runner needs the cred for BOTH its boot `clw hydrate` AND the job's `clw run`
  // (corelink-memoize); a single-use latch was consumed by the first, starving the
  // second. The cred is served on every redeem until the lease TTL expires; the
  // decision is the PURE `decideRedeem` (lib, unit-tested), this wrapper only
  // applies the `wipe` at expiry to strongly-consistent DO storage.
  async redeem(ticket: string): Promise<{ status: number; cred?: StashedCred }> {
    return this.ctx.blockConcurrencyWhile(async () => {
      if (await this.ctx.storage.get(CRED_STASH_CLOSED_KEY) !== undefined) return { status: 404 };
      const rec = await this.ctx.storage.get<StashRecord>("rec");
      const d = decideRedeem(rec, false, Date.now(), ticket);
      if (d.wipe) await this.ctx.storage.deleteAll();
      return { status: d.status, cred: d.cred };
    });
  }

  // Keep DevEnv closure tombstones until expiry, including on a stale queued alarm.
  async alarm(): Promise<void> {
    await this.ctx.blockConcurrencyWhile(() => expireCredential(this.ctx.storage));
  }

  // Normal job completion wipes the stash and alarm. DevEnv passes the absolute
  // PAT deadline to retain a closure tombstone: a delayed stash RPC cannot make
  // a credential redeemable after cleanup was confirmed. Both forms are idempotent.
  async wipe(absoluteExpiresAtMs?: number): Promise<void> {
    await this.ctx.blockConcurrencyWhile(() => wipeCredential(this.ctx.storage, absoluteExpiresAtMs));
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
  private authority(): ConcurrencyAuthority { return new ConcurrencyAuthority(this.ctx.storage); }
  private async spawnClaimTx<T>(fn: (storage: DurableObjectTransaction) => Promise<T>): Promise<T> {
    return this.ctx.storage.transaction(fn);
  }

  /**
   * The spawn claim is separate from capacity slots.  A generation and owner
   * token make every release conditional, so a late failed attempt cannot
   * release a claim acquired by a later retry.
   */
  async acquireSpawnClaim(jobId: string, ttlMs: number): Promise<SpawnClaimResult> {
    if (!jobId || !Number.isFinite(ttlMs) || ttlMs <= 0) return { status: "invalid" };
    return this.spawnClaimTx(async storage => {
      const now = Date.now();
      const key = `spawn-claim:${jobId}`;
      const current = await storage.get<SpawnClaimRecord>(key);
      if (current && current.expiresAtMs > now) return { status: "held", generation: current.generation };
      // Keep a monotonic counter after release. Reusing generation 1 after a
      // completed/failed attempt makes audits ambiguous and weakens the exact
      // attempt proof carried into completion.
      const generationKey = `spawn-claim-generation:${jobId}`;
      const generation = ((await storage.get<number>(generationKey)) ?? 0) + 1;
      const record: SpawnClaimRecord = {
        jobId,
        generation,
        ownerToken: crypto.randomUUID(),
        phase: "claimed",
        claimedAtMs: now,
        expiresAtMs: now + ttlMs,
      };
      await storage.put(key, record);
      await storage.put(generationKey, generation);
      return { status: "acquired", generation, ownerToken: record.ownerToken };
    });
  }

  async renewSpawnClaim(jobId: string, generation: number, ownerToken: string, ttlMs: number): Promise<boolean> {
    return this.spawnClaimTx(async storage => {
      const key = `spawn-claim:${jobId}`;
      const current = await storage.get<SpawnClaimRecord>(key);
      if (!current || current.generation !== generation || current.ownerToken !== ownerToken) return false;
      await storage.put(key, { ...current, expiresAtMs: Date.now() + ttlMs });
      return true;
    });
  }

  async markSpawnClaimActive(jobId: string, generation: number, ownerToken: string): Promise<boolean> {
    return this.spawnClaimTx(async storage => {
      const key = `spawn-claim:${jobId}`;
      const current = await storage.get<SpawnClaimRecord>(key);
      if (!current || current.generation !== generation || current.ownerToken !== ownerToken) return false;
      // Once provider dispatch is fenced, the claim lives until the matching
      // completion/teardown releases this exact generation. A wall-clock TTL
      // here would permit a duplicate spawn during a long running job.
      await storage.put(key, { ...current, phase: "active", expiresAtMs: Number.MAX_SAFE_INTEGER });
      return true;
    });
  }

  /**
   * Bind the provider identity after dispatch. Completion supplies this value,
   * so it can release only the exact attempt that actually owned that runner.
   */
  async bindSpawnClaimProvider(jobId: string, generation: number, ownerToken: string, providerIdentity: string): Promise<boolean> {
    if (!providerIdentity) return false;
    return this.spawnClaimTx(async storage => {
      const key = `spawn-claim:${jobId}`;
      const current = await storage.get<SpawnClaimRecord>(key);
      if (!current || current.generation !== generation || current.ownerToken !== ownerToken || current.phase !== "active") return false;
      if (current.providerIdentity && current.providerIdentity !== providerIdentity) return false;
      await storage.put(key, { ...current, providerIdentity });
      return true;
    });
  }

  private attemptKey(jobId: string): string { return `spawn-active-attempt:v1:${jobId}`; }

  /**
   * Commit the exact handle, JIT runner identity and cleanup intent before the
   * provider start RPC.  A second delivery can only see this durable record and
   * cannot mint another JIT/slot/start after KV has expired.
   */
  async persistActiveAttempt(
    jobId: string, handle: string, runnerName: string, runnerId: number | undefined,
    jitAttempt: number, repo: string, installationId: string, preparationId?: string,
  ): Promise<boolean> {
    if (!jobId || !handle || !runnerName || !Number.isSafeInteger(jitAttempt) || jitAttempt < 1
      || (installationId !== "" && canonicalInstallationId(installationId) !== installationId)) return false;
    return this.spawnClaimTx(async storage => {
      const claim = await storage.get<SpawnClaimRecord>(`spawn-claim:${jobId}`);
      if (!claim || claim.phase !== "active" || claim.providerIdentity !== runnerName) return false;
      const key = this.attemptKey(jobId);
      const prior = await storage.get<ActiveSpawnAttempt>(key);
      if (prior) {
        return prior.generation === claim.generation && prior.ownerToken === claim.ownerToken
          && prior.handle === handle && prior.runnerName === runnerName && prior.jitAttempt === jitAttempt;
      }
      await storage.put(key, {
        schemaVersion: 1, jobId, generation: claim.generation, ownerToken: claim.ownerToken,
        handle, runnerName, ...(typeof runnerId === "number" ? { runnerId } : {}), jitAttempt,
        repo, installationId, ...(preparationId ? { preparationId } : {}), createdAtMs: Date.now(), teardownIntent: true,
      } satisfies ActiveSpawnAttempt);
      return true;
    });
  }

  async readActiveAttempt(jobId: string): Promise<ActiveSpawnAttempt | null> {
    return (await this.ctx.storage.get<ActiveSpawnAttempt>(this.attemptKey(jobId))) ?? null;
  }

  async pendingActiveAttempts(limit = 25): Promise<ActiveSpawnAttempt[]> {
    const page = await this.ctx.storage.list<ActiveSpawnAttempt>({ prefix: "spawn-active-attempt:v1:", limit });
    return [...page.values()];
  }

  /**
   * Checkpoint a successful exact-handle teardown before local capacity cleanup.
   * A retry after a slot-release outage therefore never issues a second destroy,
   * while a replacement generation still fences this stale callback completely.
   */
  async markAttemptTeardownConfirmed(jobId: string, generation: number, ownerToken: string, handle: string): Promise<boolean> {
    return this.spawnClaimTx(async storage => {
      const key = this.attemptKey(jobId);
      const attempt = await storage.get<ActiveSpawnAttempt>(key);
      if (!attempt || attempt.generation !== generation || attempt.ownerToken !== ownerToken || attempt.handle !== handle) return false;
      const claim = await storage.get<SpawnClaimRecord>(`spawn-claim:${jobId}`);
      if (claim && (claim.generation !== generation || claim.ownerToken !== ownerToken)) return false;
      if (!attempt.teardownConfirmedAtMs) await storage.put(key, { ...attempt, teardownConfirmedAtMs: Date.now() });
      return true;
    });
  }

  /** Terminalize only the generation that owns this exact provider handle. */
  async confirmAttemptTeardown(jobId: string, generation: number, ownerToken: string, handle: string): Promise<ActiveSpawnAttempt | null> {
    return this.spawnClaimTx(async storage => {
      const key = this.attemptKey(jobId);
      const attempt = await storage.get<ActiveSpawnAttempt>(key);
      if (!attempt || attempt.generation !== generation || attempt.ownerToken !== ownerToken || attempt.handle !== handle) return null;
      const claim = await storage.get<SpawnClaimRecord>(`spawn-claim:${jobId}`);
      // A replacement generation is never harmed by a stale cleanup callback.
      if (claim && (claim.generation !== generation || claim.ownerToken !== ownerToken)) return null;
      await storage.delete(key);
      if (claim) await storage.delete(`spawn-claim:${jobId}`);
      return attempt;
    });
  }

  /**
   * Retire one confirmed-dead start attempt so the same claim may mint a fresh
   * JIT runner and retry. The slot and claim belong to the workflow job; the
   * provider identity belongs only to the single-use runner just destroyed.
   */
  async retireActiveAttemptForRetry(jobId: string, generation: number, ownerToken: string, handle: string): Promise<boolean> {
    return this.spawnClaimTx(async storage => {
      const key = this.attemptKey(jobId);
      const attempt = await storage.get<ActiveSpawnAttempt>(key);
      if (!attempt || attempt.generation !== generation || attempt.ownerToken !== ownerToken || attempt.handle !== handle) return false;
      const claimKey = `spawn-claim:${jobId}`;
      const claim = await storage.get<SpawnClaimRecord>(claimKey);
      if (!claim || claim.generation !== generation || claim.ownerToken !== ownerToken || claim.phase !== "active") return false;
      await storage.delete(key);
      // Clear only the retired runner identity, so attempt B can bind its own
      // JIT identity without weakening the generation/owner fence.
      await storage.put(claimKey, { ...claim, providerIdentity: undefined });
      return true;
    });
  }

  /**
   * Completion deliberately accepts a provider identity, never an unqualified
   * job id. A stale completion for runner A therefore cannot release a later
   * generation B for the same job.
   */
  async releaseSpawnClaimForCompletion(jobId: string, providerIdentity: string): Promise<SpawnClaimRelease> {
    if (!providerIdentity) return "stale";
    return this.spawnClaimTx(async storage => {
      const key = `spawn-claim:${jobId}`;
      const current = await storage.get<SpawnClaimRecord>(key);
      if (!current) return "missing";
      if (current.phase !== "active" || current.providerIdentity !== providerIdentity) return "stale";
      const attempt = await storage.get<ActiveSpawnAttempt>(this.attemptKey(jobId));
      // The completion caller reaches this only after exact-handle teardown has
      // been confirmed.  Require the durable start identity too, then retire the
      // terminal tombstone and claim together.  A mismatched completion cannot
      // remove a replacement attempt.
      if (attempt && (attempt.generation !== current.generation || attempt.ownerToken !== current.ownerToken || attempt.runnerName !== providerIdentity)) return "stale";
      if (attempt) await storage.delete(this.attemptKey(jobId));
      await storage.delete(key);
      return "released";
    });
  }

  async releaseSpawnClaim(jobId: string, generation: number, ownerToken: string): Promise<SpawnClaimRelease> {
    return this.spawnClaimTx(async storage => {
      const key = `spawn-claim:${jobId}`;
      const current = await storage.get<SpawnClaimRecord>(key);
      if (!current) return "missing";
      if (current.generation !== generation || current.ownerToken !== ownerToken) return "stale";
      // An active provider attempt is released only by confirmAttemptTeardown.
      // This permits pre-start preparation failures to release normally while
      // making a post-start projection failure durable and retryable.
      if (await storage.get<ActiveSpawnAttempt>(this.attemptKey(jobId))) return "stale";
      await storage.delete(key);
      return "released";
    });
  }

  async readSpawnClaim(jobId: string): Promise<SpawnClaimRecord | null> {
    return (await this.ctx.storage.get<SpawnClaimRecord>(`spawn-claim:${jobId}`)) ?? null;
  }
  // ATOMIC acquire: prune-expired → decide (per-key cap THEN fleet cap; idempotent
  // per jobId) → persist. Returns the clean admit/refuse decision — the caller
  // fail-opens ONLY on a THROWN error (infra hiccup), never on a `{admitted:false}`.
  async acquire(
    key: string,
    jobId: string,
    perKeyCap: number,
    fleetCap: number,
    ttlMs: number,
    preparationId?: string,
  ): Promise<{ admitted: boolean; reason?: string }> {
    return this.authority().acquire(key, jobId, perKeyCap, fleetCap, Date.now(), ttlMs, preparationId);
  }

  // Release a slot by jobId (globally unique — no key needed). Also prunes expired
  // slots. Idempotent: releasing an unknown/already-released jobId is a safe no-op.
  async release(jobId: string, nowMs = Date.now()): Promise<void> {
    await this.authority().release(jobId, nowMs);
  }

  async releasePreparation(jobId: string, preparationId: string): Promise<boolean> {
    return this.authority().releasePreparation(jobId, preparationId);
  }

  /** Prune leases even when no acquire/release request arrives. */
  async pruneExpired(nowMs = Date.now()): Promise<number> {
    return this.authority().prune(nowMs);
  }

  async getRefusal(jobId: string): Promise<object | null> { return this.authority().getRefusal(jobId); }

  async renew(jobId: string, ttlMs: number): Promise<boolean> {
    return this.authority().renew(jobId, Date.now(), ttlMs);
  }

  async recordRetry(jobId: string, epochId: string, legacyFloor = 0): Promise<{ attempts: number; recorded: boolean }> {
    return new RetryEpochAuthority(this.ctx.storage).record(jobId, epochId, legacyFloor);
  }

  async readRetry(jobId: string): Promise<number> {
    return new RetryEpochAuthority(this.ctx.storage).read(jobId);
  }
}

// ── T3-W17 containment authority ────────────────────────────────────────────
// The singleton is the sole authority for ordered pause records, cursors,
// leases, fencing and effect permits. Sequencing lives exclusively in this DO;
// immutable effect evidence lives in KV and is revalidated by this DO before a
// recovery/commit can advance the cursor.
export class ContainmentDO extends DurableObject<Env> {
  private computeObligations(): ComputeObligations {
    const terminalConfig = {
      terminalAuthority: this.env.FABRIC_COMPUTE_TERMINAL_AUTHORITY ?? "",
      terminalPublicKey: this.env.FABRIC_COMPUTE_TERMINAL_PUBLIC_KEY ?? "",
      receiptVersion: this.env.FABRIC_COMPUTE_TERMINAL_RECEIPT_VERSION ?? "",
      terminalKeyId: this.env.FABRIC_COMPUTE_TERMINAL_KEY_ID ?? "",
    };
    return new ComputeObligations(this.ctx.storage, new ComputeBudgetClient(this.env.FABRIC_COMPUTE_URL ?? "", fetch, terminalConfig), terminalConfig);
  }

  async prepareCompute(binding: ComputeBinding): Promise<void> {
    if (binding.workloadKind !== "spawn_worker_runner" || binding.vcpuCount !== 4 || binding.maximumWallMs !== 28_800_000) {
      throw new Error("COMPUTE_BINDING_INVALID");
    }
    return this.ctx.blockConcurrencyWhile(() => this.computeObligations().prepare(binding, Date.now()));
  }

  async claimComputeProvider(reservationId: string, jobId: string): Promise<void> {
    return this.ctx.blockConcurrencyWhile(() => this.computeObligations().claimProvider(reservationId, jobId, Date.now()));
  }

  async abandonUnusedCompute(reservationId: string): Promise<void> {
    return this.ctx.blockConcurrencyWhile(() => this.computeObligations().abandonUnused(reservationId));
  }

  async drainUnusedCompute(): Promise<void> {
    if (!this.env.FABRIC_COMPUTE_URL) return;
    return this.ctx.blockConcurrencyWhile(async () => {
      const durableState = await this.ctx.storage.get<{ cursor?: string; retryRequired: boolean }>("compute:drain-state");
      const cursor = durableState?.cursor ?? await this.ctx.storage.get<string>("compute:drain-cursor");
      const result = await this.computeObligations().drainUnused(Date.now(), cursor);
      const previousRetry = durableState?.retryRequired ?? await this.ctx.storage.get<boolean>("compute:drain-retry") === true;
      // A prior-page failure must survive the rest of that bounded sweep. Once
      // the cursor is gone, the next invocation is a fresh full pass; a clean
      // pass may then retire the retry marker.
      const retryRequired = result.retryRequired || (cursor !== undefined && previousRetry);
      // The state record is the recovery authority. Commit it before the
      // compatibility mirror keys so a crash between writes cannot lose the
      // retry intent or advance a cursor without its associated obligation.
      await this.ctx.storage.put("compute:drain-state", { ...(result.cursor ? { cursor: result.cursor } : {}), retryRequired });
      if (result.cursor) {
        // Preserve failures from an earlier page while the bounded sweep
        // advances. A short final page must not erase that obligation.
        await this.ctx.storage.put("compute:drain-cursor", result.cursor);
        await this.ctx.storage.put("compute:drain-retry", retryRequired);
      } else {
        await this.ctx.storage.delete("compute:drain-cursor");
        if (retryRequired) await this.ctx.storage.put("compute:drain-retry", true);
        else await this.ctx.storage.delete("compute:drain-retry");
      }
      if (!result.cursor && !retryRequired) await this.ctx.storage.delete("compute:drain-state");
    });
  }

  private tx<T>(fn: (storage: any) => Promise<T>): Promise<T> {
    return this.ctx.storage.transaction(fn);
  }

  /**
   * Spend the bounded exceptional-admission budget in the same durable
   * authority that serializes all callers. A missing or unreadable authority
   * is a refusal; a healthy missing record is a fresh budget.
   */
  async spendAdmissionBudget(): Promise<AdmissionBudgetVerdict> {
    const nowMs = Date.now();
    return this.tx((storage) => spendAdmissionBudget(storage, nowMs));
  }

  async snapshot(): Promise<ContainmentMeta> {
    return (await this.ctx.storage.get<ContainmentMeta>(CONTAINMENT_META_KEY)) ?? emptyContainmentMeta();
  }

  async normalIntakeEnqueue(input: NormalIntakeInput, delayMs = 0) {
    return new NormalIntakeInbox(this.ctx.storage).enqueue(input, Date.now(), delayMs);
  }

  async normalIntakePending(limit = 25): Promise<NormalIntakeRecord[]> {
    return new NormalIntakeInbox(this.ctx.storage).pending(Date.now(), limit);
  }

  async normalIntakeSettle(eventId: string, bodySha: string, outcome: "complete" | "uncertain" | "retry"): Promise<void> {
    return new NormalIntakeInbox(this.ctx.storage).settle(eventId, bodySha, outcome, Date.now());
  }

  async tombstoneInstallation(installationId: string, eventId: string, bodySha: string): Promise<"accepted" | "duplicate" | "conflict"> {
    return new NormalIntakeInbox(this.ctx.storage).tombstoneInstallation(installationId, eventId, bodySha, Date.now());
  }

  async installationTombstoned(installationId: string): Promise<boolean> {
    return new NormalIntakeInbox(this.ctx.storage).installationTombstoned(installationId);
  }

  async getEvent(eventId: string): Promise<ContainmentEvent | null> {
    return (await this.ctx.storage.get<ContainmentEvent>(containmentEventKey(eventId))) ?? null;
  }

  private effectLedger(): ContainmentEffectLedger {
    return new ContainmentEffectLedger(this.ctx.storage as never, this.env.RUNNER_JOB_PATS);
  }

  async readJobAttribution(key: string): Promise<string | null> { return new JobAttributionAuthority(this.ctx.storage).readJobAttribution(key); }

  async putJobAttributionIfAbsent(key: string, value: string): Promise<string> { return new JobAttributionAuthority(this.ctx.storage).putJobAttributionIfAbsent(key, value); }

  async deleteJobAttribution(key: string): Promise<void> { return new JobAttributionAuthority(this.ctx.storage).deleteJobAttribution(key); }

  /**
   * Enumerate durable ownership for settlement/reconciliation. The scan reads
   * one look-ahead key so a full page never pretends to be complete. The cursor
   * is the last scanned key, including non-matches, so callers cannot loop over
   * a tenant-sparse prefix forever.
   */
  async listJobAttributions(
    tenantId: string,
    cursor?: string,
  ): Promise<{ records: JobAttribution[]; cursor?: string; complete: boolean }> { return new JobAttributionAuthority(this.ctx.storage).listJobAttributions(tenantId, cursor); }

  async registerCredential(identity: CredentialIdentity): Promise<void> { return new CredentialObligationAuthority(this.ctx.storage).registerCredential(identity); }

  async requestCredentialRevocation(identity: CredentialIdentity): Promise<void> { return new CredentialObligationAuthority(this.ctx.storage).requestCredentialRevocation(identity); }

  async revocationRequestedCredentials(cursor?: string): Promise<CredentialPage> { return new CredentialObligationAuthority(this.ctx.storage).revocationRequestedCredentials(cursor); }

  async pendingCredentials(selection: CredentialSelection, cursor?: string, requestedStatus?: string): Promise<CredentialPage> { return new CredentialObligationAuthority(this.ctx.storage).pendingCredentials(selection, cursor, requestedStatus); }

  async confirmCredentialRevoked(identity: CredentialIdentity): Promise<void> { return new CredentialObligationAuthority(this.ctx.storage).confirmCredentialRevoked(identity); }

  async closeJobCredentials(jobId: string): Promise<{ known: boolean }> { return new CredentialObligationAuthority(this.ctx.storage).closeJobCredentials(jobId); }
  async closeTenantCredentials(tenant: string, throughGeneration: string): Promise<void> {
    return new CredentialObligationAuthority(this.ctx.storage).closeTenantCredentials(tenant, throughGeneration);
  }
  async beginTenantSuspension(input: TenantSuspensionInput): Promise<{ complete: boolean; cursor?: string }> {
    return new TenantSuspensionAuthority(this.ctx.storage as never).begin(input);
  }
  async checkpointTenantSuspension(input: TenantSuspensionInput, expectedCursor: string | undefined, nextCursor: string | undefined, complete: boolean): Promise<boolean> {
    return new TenantSuspensionAuthority(this.ctx.storage as never).checkpoint(input, expectedCursor, nextCursor, complete);
  }

  async getEffectAttempt(identity: ContainmentEffectIdentity, nonce: string): Promise<ContainmentEffectAttempt | null> {
    return this.effectLedger().getEffectAttempt(identity, nonce);
  }
  async prepareEffect(input: ContainmentEffectPrepareInput): Promise<ContainmentEffectResult> {
    return this.effectLedger().prepareEffect(input);
  }
  async acquireEffectClaim(input: ContainmentEffectTransition): Promise<ContainmentEffectResult> {
    return this.effectLedger().acquireEffectClaim(input);
  }
  async issueEffectPermit(input: ContainmentEffectTransition): Promise<ContainmentEffectResult> {
    return this.effectLedger().issueEffectPermit(input);
  }
  async bindEffect(input: ContainmentEffectTransition & { permit_id: string; binding: ContainmentEffectBinding }): Promise<ContainmentEffectResult> {
    return this.effectLedger().bindEffect(input);
  }
  async beginEffectDrive(input: ContainmentEffectTransition & { permit_id: string }): Promise<ContainmentEffectResult> {
    return this.effectLedger().beginEffectDrive(input);
  }
  async commitEffect(input: ContainmentEffectTransition & { permit_id: string; receipt: ContainmentEffectReceipt }): Promise<ContainmentEffectResult>;
  async commitEffect(input: SpawnOwnerRequest, permitId: string, proofId: string, receipt: ContainmentEffectReceipt): Promise<OwnerResult>;
  async commitEffect(input: SpawnOwnerRequest | (ContainmentEffectTransition & { permit_id: string; receipt: ContainmentEffectReceipt }), permitId?: string, proofId?: string, receipt?: ContainmentEffectReceipt): Promise<OwnerResult | ContainmentEffectResult> {
    return permitId && proofId && receipt
      ? this.effectLedger().commitEffect(input as SpawnOwnerRequest, permitId, proofId, receipt)
      : this.effectLedger().commitEffect(input as ContainmentEffectTransition & { permit_id: string; receipt: ContainmentEffectReceipt });
  }
  async abortEffect(input: ContainmentEffectTransition): Promise<ContainmentEffectResult> {
    return this.effectLedger().abortEffect(input);
  }
  async reapEffect(input: ContainmentEffectReapInput): Promise<ContainmentEffectResult> {
    return this.effectLedger().reapEffect(input);
  }

  // Canonical owner-ledger RPCs have unique names. The legacy beginEffect
  // seam below is intentionally event-shaped and can never dispatch a
  // canonical SpawnOwnerRequest by accident.
  async ownerPrepare(input: SpawnOwnerRequest): Promise<OwnerResult> { return this.effectLedger().prepare(input); }
  async ownerAcquire(input: SpawnOwnerRequest): Promise<OwnerResult> { return this.effectLedger().acquire(input); }
  async ownerMirror(input: SpawnOwnerRequest, result?: "acquired" | "owned"): Promise<SpawnMirrorObservation> { return this.effectLedger().mirror(input, result); }
  async ownerConfirm(input: SpawnOwnerRequest, mirrorDigest: string, readbackDigest: string, permitId?: string): Promise<OwnerResult> { return this.effectLedger().confirm(input, mirrorDigest, readbackDigest, permitId); }
  async ownerBegin(input: SpawnOwnerRequest, permitId: string): Promise<OwnerResult> { return this.effectLedger().beginEffect(input, permitId); }
  async ownerBind(input: SpawnOwnerRequest, permitId: string, proofId: string, binding: ContainmentEffectBinding): Promise<OwnerResult> { return this.effectLedger().bind(input, permitId, proofId, binding); }
  async ownerMarkDriving(input: SpawnOwnerRequest, permitId: string, proofId: string): Promise<OwnerResult> { return this.effectLedger().markDriving(input, permitId, proofId); }
  async ownerCommit(input: SpawnOwnerRequest, permitId: string, proofId: string, receipt: ContainmentEffectReceipt): Promise<OwnerResult> { return this.effectLedger().commitEffect(input, permitId, proofId, receipt) as Promise<OwnerResult>; }
  async ownerObserve(pointerKey: string, attemptKey: string): Promise<OwnerResult> { return this.effectLedger().observe(pointerKey, attemptKey); }
  async ownerAbort(input: SpawnOwnerRequest): Promise<OwnerResult> { return this.effectLedger().abort(input); } async ownerFreeze(input: SpawnOwnerRequest): Promise<OwnerResult> { return this.effectLedger().freezeUnknown(input); }
  async admitDrainOwner(eventId: string, tuple: Awaited<ReturnType<typeof drainOwnerTuple>>, now = Date.now()): Promise<boolean> { return this.tx(s => admitDrainOwnerInTransaction(s, eventId, tuple, now)); }
  async beginEffect(eventId: string, owner: string, epoch: number, now = Date.now(), permitId?: string): Promise<ContainmentEvent["effect_permit"]> {
    return this.beginContainmentEventEffect(eventId, owner, epoch, now, permitId);
  }

  private async ensureJobIndex(
    s: any,
    repo: string,
    jobId: string,
  ): Promise<ContainmentJobIndex> {
    const meta = await s.get(CONTAINMENT_INDEX_META_KEY);
    if (meta === undefined) throw new Error("containment job index missing");
    if (!isValidJobIndexMeta(meta)) throw new Error("containment job index meta divergent");
    const key = containmentJobIndexKey(repo, jobId);
    const existing = await s.get(key) as ContainmentJobIndex | undefined;
    if (existing === undefined) throw new Error("containment job index missing");
    if (!isValidJobIndex(existing, repo, jobId)) throw new Error("containment job index divergent");
    if (!isValidJobIndexMarker(await s.get(containmentJobIndexMarkerKey(repo, jobId)), repo, jobId)) throw new Error("containment job index marker divergent");
    return existing;
  }

  async bootstrapContainedEventIndex(
    repoInput: string,
    jobIdInput: string,
    now = Date.now(),
  ): Promise<{ status: "bootstrapped" | "already_present" | "blocked" | "invalid" }> {
    const identity = normalizeRedriveIdentity(repoInput, jobIdInput);
    if (!identity) return { status: "invalid" };
    const { repo, job_id: jobId } = identity;
    return this.tx(async (s) => {
      const meta = await s.get(CONTAINMENT_INDEX_META_KEY);
      if (meta !== undefined && !isValidJobIndexMeta(meta)) return { status: "blocked" as const };
      const key = containmentJobIndexKey(repo, jobId);
      const existing = await s.get(key) as ContainmentJobIndex | undefined;
      const marker = await s.get(containmentJobIndexMarkerKey(repo, jobId));
      if (existing !== undefined) return isValidJobIndex(existing, repo, jobId) && isValidJobIndexMarker(marker, repo, jobId) ? { status: "already_present" as const } : { status: "blocked" as const };
      if (marker !== undefined) return { status: "blocked" as const };
      // The pair can only be initialized when the authority proves that no
      // active queue or reservation owns it. Never scan broad event/owner
      // namespaces: an initialized-but-missing pair remains fail-closed.
      const reservation = await s.get(containmentReservationKey(repo, jobId));
      const pointer = await s.get(containmentEffectPointerKey({ repo, job_id: jobId, effect_id: redriveEffectId(repo, jobId) }));
      const legacyJobState = await s.get(containmentEffectJobKey(jobId));
      const legacyIndex = await s.get(`containment:v1:job-index:${repo}/${jobId}`);
      if (reservation !== undefined || pointer !== undefined || legacyJobState !== undefined || legacyIndex !== undefined) return { status: "blocked" as const };
      if (meta === undefined) await s.put(CONTAINMENT_INDEX_META_KEY, { schema_version: 1, initialized: true } satisfies ContainmentJobIndexMeta);
      await s.put(key, { schema_version: 1, repo, job_id: jobId, active_event_ids: [], active_count: 0, updated_at_ms: now } satisfies ContainmentJobIndex);
      await s.put(containmentJobIndexMarkerKey(repo, jobId), { schema_version: 1, repo, job_id: jobId, bootstrapped_at_ms: now } satisfies ContainmentJobIndexMarker);
      return { status: "bootstrapped" as const };
    });
  }

  private async containedEventExists(s: any, repo: string, jobId: string): Promise<boolean> {
    const index = await this.ensureJobIndex(s, repo, jobId);
    for (const eventId of index.active_event_ids) {
      const event = await s.get(containmentEventKey(eventId)) as ContainmentEvent | undefined;
      if (!event) throw new Error("containment job index references missing event");
      const identity = normalizeRedriveIdentity(event.repo, event.job_id);
      if (!identity || identity.repo !== repo || identity.job_id !== jobId) throw new Error("containment job index identity divergent");
    }
    return index.active_count > 0;
  }

  async reserveRedriveCandidate(
    repoInput: string,
    jobIdInput: string,
    now = Date.now(),
  ): Promise<{ status: "reserved" | "contained" | "busy" | "effect_eligible" | "completed" | "invalid"; reservation?: ContainmentRedriveReservation }> {
    const identity = normalizeRedriveIdentity(repoInput, jobIdInput);
    if (!identity) return { status: "invalid" };
    const { repo, job_id: jobId } = identity;
    const key = containmentReservationKey(repo, jobId);
    return this.tx(async (s) => {
      // An unacknowledged contained intake owns the job before a redrive can
      // enter its first mutable seam. The direct index is inside the deciding
      // DO tx; never scan the full event collection per candidate.
      if (await this.containedEventExists(s, repo, jobId)) return { status: "contained" as const };
      const prior = (await s.get(key)) as ContainmentRedriveReservation | undefined;
      if (prior) {
        if (prior.schema_version !== 1 || prior.repo !== repo || prior.job_id !== jobId || typeof prior.owner !== "string" || typeof prior.token !== "string" || prior.path !== "redrive" || prior.effect_id !== redriveEffectId(repo, jobId) || !Number.isSafeInteger(prior.epoch) || prior.epoch < 1 || !Number.isFinite(prior.expires_ms) || !["HELD", "EFFECT_ELIGIBLE", "COMPLETED"].includes(prior.state) || (prior.event_id !== null && typeof prior.event_id !== "string") || typeof prior.completion_observed !== "boolean") return { status: "busy" as const };
        if (prior.state === "EFFECT_ELIGIBLE") return { status: "effect_eligible" as const, reservation: prior };
        if (prior.state === "COMPLETED") return { status: "completed" as const, reservation: prior };
        if (prior.state !== "HELD" || prior.expires_ms > now) return { status: "busy" as const, reservation: prior };
        // Only this exact HELD state may be reclaimed. EFFECT_ELIGIBLE has no
        // timer/reset path, so no existing effect can ever be reissued.
        const reclaimed: ContainmentRedriveReservation = {
          ...prior,
          owner: crypto.randomUUID(),
          token: crypto.randomUUID(),
          epoch: prior.epoch + 1,
          state: "HELD",
          expires_ms: now + REDRIVE_RESERVATION_TTL_MS,
          event_id: null,
          completion_observed: false,
        };
        await s.put(key, reclaimed);
        return { status: "reserved" as const, reservation: reclaimed };
      }
      const reservation: ContainmentRedriveReservation = {
        schema_version: 1,
        repo,
        job_id: jobId,
        owner: crypto.randomUUID(),
        token: crypto.randomUUID(),
        epoch: 1,
        path: "redrive",
        state: "HELD",
        expires_ms: now + REDRIVE_RESERVATION_TTL_MS,
        event_id: null,
        effect_id: redriveEffectId(repo, jobId),
        completion_observed: false,
      };
      await s.put(key, reservation);
      return { status: "reserved" as const, reservation };
    });
  }

  async beginReservedEffect(
    repoInput: string,
    jobIdInput: string,
    owner: string,
    token: string,
    epoch: number,
    path: "redrive" = "redrive",
    effectId?: string,
    now = Date.now(),
  ): Promise<{ status: "eligible" | "stale" | "ineligible" | "invalid"; permit?: ContainmentRedrivePermit }> {
    const identity = normalizeRedriveIdentity(repoInput, jobIdInput);
    if (!identity || !Number.isSafeInteger(epoch) || epoch < 1 || path !== "redrive") return { status: "invalid" };
    const { repo, job_id: jobId } = identity;
    const expectedEffectId = redriveEffectId(repo, jobId);
    if (effectId !== undefined && effectId !== expectedEffectId) return { status: "invalid" };
    return this.tx(async (s) => {
      const reservation = (await s.get(containmentReservationKey(repo, jobId))) as ContainmentRedriveReservation | undefined;
      if (!reservation || !reservationTupleMatches(reservation, repo, jobId, owner, token, epoch, path, expectedEffectId)) return { status: "stale" as const };
      // Expiry is a fence, not a hint. A worker that read a HELD tuple before
      // its deadline must not promote it after the deadline; it has to reclaim
      // a fresh tuple through reserveRedriveCandidate first.
      if (reservation.state !== "HELD" || !Number.isFinite(reservation.expires_ms) || reservation.expires_ms <= now) return { status: "ineligible" as const };
      const eligible: ContainmentRedriveReservation = { ...reservation, state: "EFFECT_ELIGIBLE" };
      await s.put(containmentReservationKey(repo, jobId), eligible);
      return { status: "eligible" as const, permit: reservationPermit(eligible) };
    });
  }

  async completeRedrive(
    repoInput: string,
    jobIdInput: string,
    owner: string,
    token: string,
    epoch: number,
    effectId: string,
  ): Promise<{ status: "completed" | "cleared_after_completion" | "stale" | "incomplete" | "invalid" }> {
    const identity = normalizeRedriveIdentity(repoInput, jobIdInput);
    if (!identity || !Number.isSafeInteger(epoch) || epoch < 1 || effectId !== redriveEffectId(identity.repo, identity.job_id)) return { status: "invalid" };
    const { repo, job_id: jobId } = identity;
    const key = containmentReservationKey(repo, jobId);
    return this.tx(async (s) => {
      const reservation = (await s.get(key)) as ContainmentRedriveReservation | undefined;
      if (!reservation || !reservationTupleMatches(reservation, repo, jobId, owner, token, epoch, "redrive", effectId)) return { status: "stale" as const };
      if (reservation.state === "HELD") return { status: "incomplete" as const };
      if (reservation.state === "COMPLETED") return { status: "completed" as const };
      if (reservation.state !== "EFFECT_ELIGIBLE") return { status: "stale" as const };
      // A verified completion may have arrived while the external continuation
      // was in flight. It never clears eligibility itself; this matching owner
      // resolves the latched terminal transition without leaking a tombstone.
      if (reservation.completion_observed) {
        await s.delete(key);
        return { status: "cleared_after_completion" as const };
      }
      await s.put(key, { ...reservation, state: "COMPLETED" });
      return { status: "completed" as const };
    });
  }

  async clearCompletedRedrive(
    repoInput: string,
    jobIdInput: string,
    effectId: string,
  ): Promise<{ status: "cleared" | "latched" | "terminal" | "not_completed" | "invalid" }> {
    const identity = normalizeRedriveIdentity(repoInput, jobIdInput);
    if (!identity || effectId !== redriveEffectId(identity.repo, identity.job_id)) return { status: "invalid" };
    const { repo, job_id: jobId } = identity;
    const key = containmentReservationKey(repo, jobId);
    return this.tx(async (s) => {
      const reservation = (await s.get(key)) as ContainmentRedriveReservation | undefined;
      if (!reservation || reservation.schema_version !== 1 || reservation.repo !== repo || reservation.job_id !== jobId || reservation.path !== "redrive" || reservation.effect_id !== effectId || !Number.isFinite(reservation.expires_ms) || typeof reservation.completion_observed !== "boolean") return { status: "not_completed" as const };
      if (reservation.state === "COMPLETED") {
        await s.delete(key);
        return { status: "cleared" as const };
      }
      if (reservation.state === "HELD") {
        // Verified completion wins over an owner that has not crossed
        // eligibility. Removing the record atomically fences that tuple: its
        // later begin sees no key and cannot touch KV/effects.
        await s.delete(key);
        return { status: "terminal" as const };
      }
      if (reservation.state === "EFFECT_ELIGIBLE") {
        if (!reservation.completion_observed) await s.put(key, { ...reservation, completion_observed: true });
        return { status: "latched" as const };
      }
      return { status: "not_completed" as const };
    });
  }

  private async appendInTransaction(
    s: any,
    event: Omit<ContainmentEvent, "pause_seq" | "state" | "claim" | "effect_permit">,
    meta: ContainmentMeta,
  ): Promise<{ status: "appended" | "duplicate" | "conflict"; event?: ContainmentEvent }> {
    // Validate the pair authority before looking at delivery identity. A
    // duplicate must not bypass a missing/corrupt marker.
    const index = await this.ensureJobIndex(s, event.repo, event.job_id);
    const key = containmentEventKey(event.event_id);
    const prior = (await s.get(key)) as ContainmentEvent | undefined;
    if (prior) return prior.body_sha256 === event.body_sha256 ? { status: "duplicate", event: prior } : { status: "conflict" };
    // Build field-by-field: no RPC caller can seed claim, permit, or proof state.
    const next: ContainmentEvent = {
      schema_version: 1,
      event_id: event.event_id,
      pause_seq: meta.next_pause_seq,
      received_at_ms: event.received_at_ms,
      body_sha256: event.body_sha256,
      raw_payload: event.raw_payload,
      action: event.action,
      job_id: event.job_id,
      repo: event.repo,
      installation_id: event.installation_id,
      labels: event.labels,
      effect_id: event.effect_id,
      state: "QUEUED",
      claim: null,
      effect_permit: null,
    };
    const pause: ContainmentPause = { schema_version: 1, event_id: event.event_id, pause_seq: next.pause_seq };
    if (index.active_count >= MAX_ACTIVE_INDEX_EVENTS) throw new Error("containment job index bound exceeded");
    await s.put(key, next);
    await s.put(containmentPauseKey(next.pause_seq), pause);
    await s.put(containmentJobIndexKey(event.repo, event.job_id), { ...index, active_event_ids: [...index.active_event_ids, event.event_id], active_count: index.active_count + 1, updated_at_ms: Date.now() });
    await s.put(CONTAINMENT_META_KEY, { ...meta, next_pause_seq: next.pause_seq + 1, backlog_count: meta.backlog_count + 1 });
    return { status: "appended", event: next };
  }

  async append(event: Omit<ContainmentEvent, "pause_seq" | "state" | "claim" | "effect_permit">): Promise<{ status: "appended" | "duplicate" | "conflict"; event?: ContainmentEvent }> {
    const identity = normalizeRedriveIdentity(event.repo, event.job_id);
    if (!identity) throw new TypeError("invalid containment repo/job identity");
    const normalizedEvent = { ...event, repo: identity.repo, job_id: identity.job_id };
    return this.tx(async (s) => {
      const rawMeta = await s.get(CONTAINMENT_META_KEY) as ContainmentMeta | undefined;
      return this.appendInTransaction(s, normalizedEvent, rawMeta ?? emptyContainmentMeta());
    });
  }

  async admitQueued(
    event: Omit<ContainmentEvent, "pause_seq" | "state" | "claim" | "effect_permit">,
    intakeState: "normal" | "paused" | "invalid",
  ): Promise<{ status: "continued" | "appended" | "duplicate" | "conflict" | "authority-busy" | "redrive_owned"; event?: ContainmentEvent }> {
    // The reservation identity is normalized before *any* containment storage
    // access. This leaves the normal/empty-backlog path outside reservation
    // arbitration while refusing malformed identities before a DO read could
    // accidentally give them a durable interpretation.
    const identity = normalizeRedriveIdentity(event.repo, event.job_id);
    if (!identity) return { status: "authority-busy" };
    const normalizedEvent = { ...event, repo: identity.repo, job_id: identity.job_id };
    return this.tx(async (s) => {
      const rawMeta = await s.get(CONTAINMENT_META_KEY) as ContainmentMeta | undefined;
      const meta = rawMeta ?? emptyContainmentMeta();
      if (intakeState === "normal" && meta.backlog_count === 0) return { status: "continued" as const };
      // A duplicate is already durable and never needs reservation arbitration.
      const prior = (await s.get(containmentEventKey(event.event_id))) as ContainmentEvent | undefined;
      if (prior) return prior.body_sha256 === event.body_sha256 ? { status: "duplicate" as const, event: prior } : { status: "conflict" as const };
      const reservationKey = containmentReservationKey(identity.repo, identity.job_id);
      const reservation = (await s.get(reservationKey)) as ContainmentRedriveReservation | undefined;
      if (reservation) {
        // The intake write takes over an expired *pre-effect* hold atomically:
        // deleting it and appending below leave no interval in which its old
        // tuple can become eligible. Eligible/completed redrives own the job.
        const wellFormed = reservation.repo === identity.repo
          && reservation.job_id === identity.job_id
          && reservation.path === "redrive"
          && reservation.effect_id === redriveEffectId(identity.repo, identity.job_id)
          && typeof reservation.completion_observed === "boolean";
        if (!wellFormed) return { status: "authority-busy" as const };
        if (reservation.state === "HELD") {
          if (reservation.expires_ms > Date.now()) return { status: "authority-busy" as const };
          await s.delete(reservationKey);
        } else if (reservation.state === "EFFECT_ELIGIBLE" || reservation.state === "COMPLETED") {
          return { status: "redrive_owned" as const };
        } else {
          return { status: "authority-busy" as const };
        }
      }
      return this.appendInTransaction(s, normalizedEvent, meta);
    });
  }

  async recordInvalidConfig(switchName: string, rawValue: string, rawValueSha256: string): Promise<ContainmentOutboxRecord> {
    const signalId = await validateInvalidConfigIdentity(switchName, rawValue, rawValueSha256, sha256Hex);
    return this.tx((storage) => recordInvalidConfigInStorage(storage, switchName, rawValueSha256, signalId));
  }

  async pendingInvalidConfig(): Promise<ContainmentOutboxRecord[]> {
    return pendingInvalidConfigInStorage(this.ctx.storage, sha256Hex);
  }

  async markInvalidConfigAttempt(signalId: string): Promise<void> {
    await this.tx((storage) => markInvalidConfigAttemptInStorage(storage, signalId));
  }

  async acknowledgeInvalidConfig(signalId: string): Promise<void> {
    await this.tx((storage) => acknowledgeInvalidConfigInStorage(storage, signalId));
  }

  async requestDrain(): Promise<ContainmentMeta> {
    return this.tx(async (s) => {
      const meta = ((await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined) ?? emptyContainmentMeta();
      const indexMeta = await s.get(CONTAINMENT_INDEX_META_KEY);
      if (indexMeta === undefined) await s.put(CONTAINMENT_INDEX_META_KEY, { schema_version: 1, initialized: true } satisfies ContainmentJobIndexMeta);
      else if (!isValidJobIndexMeta(indexMeta)) throw new Error("containment job index meta divergent");
      const next = { ...meta, drain_requested: meta.backlog_count > 0 };
      await s.put(CONTAINMENT_META_KEY, next);
      return next;
    });
  }

  async acquireLease(owner: string, now = Date.now()): Promise<{ owner: string; epoch: number; expires_ms: number } | null> {
    return this.tx(async (s) => {
      const meta = ((await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined) ?? emptyContainmentMeta();
      const old = meta.lease;
      if (old && old.expires_ms > now) {
        if (old.owner !== owner) return null;
        if (old.expires_ms - now > DRAIN_RENEW_THRESHOLD_MS) return old;
        const renewed = { ...old, expires_ms: now + DRAIN_LEASE_TTL_MS };
        await s.put(CONTAINMENT_META_KEY, { ...meta, lease: renewed });
        return renewed;
      }
      // An expired lease is a reclaim even if the random owner happens to be
      // the same string. Reusing its epoch would let an old fenced holder race
      // the new lease, so every acquire/reclaim advances the fence.
      const next = { owner, epoch: meta.lease_epoch + 1, expires_ms: now + DRAIN_LEASE_TTL_MS };
      await s.put(CONTAINMENT_META_KEY, { ...meta, lease_epoch: next.epoch, lease: next });
      return next;
    });
  }

  async renewLease(owner: string, epoch: number, now = Date.now()): Promise<boolean> {
    return this.tx(async (s) => {
      const meta = (await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined;
      if (!meta || !leaseMatches(meta, owner, epoch, now)) return false;
      if (meta.lease!.expires_ms - now > DRAIN_RENEW_THRESHOLD_MS) return true;
      await s.put(CONTAINMENT_META_KEY, { ...meta, lease: { ...meta.lease!, expires_ms: now + DRAIN_LEASE_TTL_MS } });
      return true;
    });
  }

  async claimNext(owner: string, epoch: number, now = Date.now()): Promise<{ event: ContainmentEvent; committed: boolean } | null> {
    return this.tx(async (s) => {
      const meta = (await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined;
      if (!meta || !leaseMatches(meta, owner, epoch, now) || meta.backlog_count === 0) return null;
      const pause = (await s.get(containmentPauseKey(meta.drain_cursor + 1))) as ContainmentPause | undefined;
      if (!pause) return null;
      const key = containmentEventKey(pause.event_id);
      const event = (await s.get(key)) as ContainmentEvent | undefined;
      if (!event || !isCurrentHead(meta, event) || event.pause_seq !== pause.pause_seq || pause.event_id !== event.event_id) return null;
      if (event.state === "EFFECT_COMMITTED") {
        // The effect permit remains immutable, but the current fenced claim must
        // move to the reclaimer so only the current lease can acknowledge it.
        const committed = { ...event, claim: { owner, lease_epoch: epoch } };
        if (event.claim?.owner !== owner || event.claim.lease_epoch !== epoch) await s.put(key, committed);
        return { event: committed, committed: true };
      }
      const claimed = { ...event, state: "CLAIMED" as const, claim: { owner, lease_epoch: epoch } };
      if (event.state !== "CLAIMED" || event.claim?.owner !== owner || event.claim.lease_epoch !== epoch) await s.put(key, claimed);
      return { event: claimed, committed: false };
    });
  }

  private async beginContainmentEventEffect(eventId: string, owner: string, epoch: number, now = Date.now(), permitId?: string): Promise<ContainmentEvent["effect_permit"]> {
    return this.tx(async (s) => {
      const meta = (await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined;
      const event = (await s.get(containmentEventKey(eventId))) as ContainmentEvent | undefined;
      if (!meta || !event || !isCurrentHead(meta, event) || !leaseMatches(meta, owner, epoch, now) || event.state !== "CLAIMED" || event.claim?.owner !== owner || event.claim.lease_epoch !== epoch) return null;
      // The permit is one-shot and is never handed to a new owner. A reclaiming
      // owner with an old permit has only the proof-bound recovery path.
      if (event.effect_permit) {
        return event.effect_permit.issued_to_owner === owner && event.effect_permit.issued_to_epoch === epoch
          ? event.effect_permit
          : null;
      }
      const permit = { permit_id: permitId ?? crypto.randomUUID(), issued_to_owner: owner, issued_to_epoch: epoch };
      await s.put(containmentEventKey(eventId), { ...event, effect_permit: permit });
      return permit;
    });
  }

  private async validatedEffectProof(event: ContainmentEvent): Promise<string | null> {
    const kv = this.env.RUNNER_JOB_PATS;
    const permit = event.effect_permit;
    if (!kv || !permit) return null;
    try {
      const rawEvidence = await Promise.all(CONTAINMENT_EFFECT_WITNESS_KINDS.map((kind) => kv.get(containmentEffectEvidenceKey(event.effect_id, kind))));
      const evidence = rawEvidence.map((raw) => {
        if (!raw) return null;
        try {
          const parsed = JSON.parse(raw) as ContainmentEffectEvidence;
          return canonicalContainmentEvidence(parsed) === raw ? parsed : null;
        } catch {
          return null;
        }
      });
      if (evidence.some((item) => !item)) return null;
      const records = evidence as ContainmentEffectEvidence[];
      const sourceHashes = await Promise.all(records.map((rec) => sha256Hex(rec.source_value)));
      for (let i = 0; i < records.length; i++) {
        const rec = records[i];
        if (rec.schema_version !== 1 || rec.kind !== CONTAINMENT_EFFECT_WITNESS_KINDS[i] || rec.effect_id !== event.effect_id || rec.event_id !== event.event_id || rec.job_id !== event.job_id || rec.permit_id !== permit.permit_id || typeof rec.source_value !== "string" || !/^[0-9a-f]{64}$/.test(rec.source_sha256) || rec.source_sha256 !== sourceHashes[i]) return null;
      }
      const [claim, attempt, placement, lease, result] = records;
      if (claim.source_key !== `spawn:${event.job_id}` || attempt.source_key !== "github:generate-jitconfig" || placement.source_key !== orphanKey(event.job_id) || lease.source_key !== jobHandleKey(event.job_id) || result.source_key !== "result" || !Number.isSafeInteger(attempt.attempt_count) || attempt.attempt_count! < 1 || result.terminal !== "DELIVERED" || result.attempt_count !== attempt.attempt_count) return null;
      // These source values were captured at their actual external-effect seams
      // and are part of the canonical, write-once witness. Do not re-read the
      // mutable `spawn:`, `orphan:`, or `jhandle:` keys here: completion is
      // allowed to remove them before a fenced recovery obtains the DO lease.
      if (!/^\d+$/.test(claim.source_value) || claim.source_value.length === 0 || lease.source_value.length === 0) return null;
      let attemptValue: { runner_name?: unknown; runner_id?: unknown; attempt?: unknown } | null = null;
      let placementValue: (OrphanRecord & { effect_id?: unknown }) | null = null;
      try {
        attemptValue = JSON.parse(attempt.source_value) as { runner_name?: unknown; runner_id?: unknown; attempt?: unknown };
        placementValue = JSON.parse(placement.source_value) as OrphanRecord & { effect_id?: unknown };
      } catch {
        return null;
      }
      if (!attemptValue || typeof attemptValue.runner_name !== "string" || attemptValue.runner_name.length === 0 || !Number.isSafeInteger(attemptValue.runner_id) || attemptValue.attempt !== attempt.attempt_count || JSON.stringify({ runner_name: attemptValue.runner_name, runner_id: attemptValue.runner_id, attempt: attemptValue.attempt }) !== attempt.source_value || !placementValue || placementValue.effect_id !== event.effect_id || !Number.isFinite(placementValue.placedMs) || JSON.stringify(placementValue) !== placement.source_value) return null;
      const expectedResultSource = JSON.stringify({
        terminal: "DELIVERED",
        attempt_count: attempt.attempt_count,
        spawn_claim_sha256: claim.source_sha256,
        attempt_sha256: attempt.source_sha256,
        placement_sha256: placement.source_sha256,
        lease_sha256: lease.source_sha256,
      });
      if (result.source_value !== expectedResultSource) return null;
      return sha256Hex(JSON.stringify(records.map(canonicalContainmentEvidence)));
    } catch {
      return null;
    }
  }

  private async readEffectCommitSnapshot(eventId: string): Promise<{ meta: ContainmentMeta; event: ContainmentEvent } | null> {
    // Do not issue KV reads while a storage transaction is open. Durable Object
    // input gates may yield around non-storage I/O, so witness validation is an
    // optimistic phase whose storage view is fenced and rechecked below.
    const [meta, event] = await Promise.all([
      this.ctx.storage.get<ContainmentMeta>(CONTAINMENT_META_KEY),
      this.ctx.storage.get<ContainmentEvent>(containmentEventKey(eventId)),
    ]);
    return meta && event ? { meta, event } : null;
  }

  private matchesEffectCommitSnapshot(
    meta: ContainmentMeta,
    event: ContainmentEvent,
    snapshot: { meta: ContainmentMeta; event: ContainmentEvent },
  ): boolean {
    // A queued append may advance next_pause_seq while evidence is being read;
    // it cannot affect this head. The event, head cursor, and fenced lease must
    // nevertheless be byte-for-byte/equivalent to the validation snapshot.
    return meta.drain_cursor === snapshot.meta.drain_cursor
      && meta.lease?.owner === snapshot.meta.lease?.owner
      && meta.lease?.epoch === snapshot.meta.lease?.epoch
      && meta.lease?.expires_ms === snapshot.meta.lease?.expires_ms
      && JSON.stringify(event) === JSON.stringify(snapshot.event);
  }

  private canCommitEffect(
    meta: ContainmentMeta,
    event: ContainmentEvent,
    owner: string,
    epoch: number,
    recovery: boolean,
  ): boolean {
    if (!isCurrentHead(meta, event) || !leaseMatches(meta, owner, epoch, Date.now()) || event.state !== "CLAIMED" || !event.effect_permit || event.claim?.owner !== owner || event.claim.lease_epoch !== epoch) return false;
    // An issued permit is a capability, never a transferable work item. Its
    // holder performs the normal commit; a later claimant can only recover the
    // DO-revalidated immutable evidence.
    return recovery
      ? event.effect_permit.issued_to_owner !== owner || event.effect_permit.issued_to_epoch !== epoch
      : event.effect_permit.issued_to_owner === owner && event.effect_permit.issued_to_epoch === epoch;
  }

  async markEffectCommitted(eventId: string, owner: string, epoch: number): Promise<boolean> {
    const snapshot = await this.readEffectCommitSnapshot(eventId);
    if (!snapshot || !this.canCommitEffect(snapshot.meta, snapshot.event, owner, epoch, false)) return false;
    const proofDigest = await this.validatedEffectProof(snapshot.event);
    if (!proofDigest) return false;
    return this.tx(async (s) => {
      const meta = (await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined;
      const event = (await s.get(containmentEventKey(eventId))) as ContainmentEvent | undefined;
      if (!meta || !event || !this.matchesEffectCommitSnapshot(meta, event, snapshot) || !this.canCommitEffect(meta, event, owner, epoch, false)) return false;
      // `proofDigest` was recomputed by this DO from immutable, effect-bound KV
      // records. The rechecked event includes the exact permit those records
      // name, so no caller-supplied evidence can cross the transaction fence.
      void proofDigest;
      await s.put(containmentEventKey(eventId), { ...event, state: "EFFECT_COMMITTED" });
      return true;
    });
  }

  async recoverEffectCommitted(effectId: string, owner: string, epoch: number, proofDigest: string): Promise<boolean> {
    const prefix = "containment:v1:";
    if (!effectId.startsWith(prefix)) return false;
    const eventId = effectId.slice(prefix.length);
    const snapshot = await this.readEffectCommitSnapshot(eventId);
    if (!snapshot || snapshot.event.effect_id !== effectId || !this.canCommitEffect(snapshot.meta, snapshot.event, owner, epoch, true)) return false;
    const actualProof = await this.validatedEffectProof(snapshot.event);
    if (!actualProof || proofDigest !== actualProof) return false;
    return this.tx(async (s) => {
      const meta = (await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined;
      const event = (await s.get(containmentEventKey(eventId))) as ContainmentEvent | undefined;
      if (!meta || !event || !this.matchesEffectCommitSnapshot(meta, event, snapshot) || !this.canCommitEffect(meta, event, owner, epoch, true)) return false;
      // The caller may supply only the digest it observed. This DO performed
      // the authoritative five-record validation and requires exact equality.
      if (proofDigest !== actualProof) return false;
      await s.put(containmentEventKey(eventId), { ...event, state: "EFFECT_COMMITTED" });
      return true;
    });
  }

  async acknowledge(eventId: string, owner: string, epoch: number): Promise<boolean> {
    const acknowledged = await this.tx(async (s) => {
      const meta = (await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined;
      const event = (await s.get(containmentEventKey(eventId))) as ContainmentEvent | undefined;
      if (!meta || !event || !isCurrentHead(meta, event) || !leaseMatches(meta, owner, epoch, Date.now()) || event.state !== "EFFECT_COMMITTED" || event.claim?.owner !== owner || event.claim.lease_epoch !== epoch) return null;
      if (!Number.isSafeInteger(meta.backlog_count) || meta.backlog_count < 1) return null;
      const pause = (await s.get(containmentPauseKey(event.pause_seq))) as ContainmentPause | undefined;
      if (!pause || pause.event_id !== event.event_id || pause.pause_seq !== event.pause_seq) return null;
      const indexKey = containmentJobIndexKey(event.repo, event.job_id);
      const index = await s.get(indexKey) as ContainmentJobIndex | undefined;
      const marker = await s.get(containmentJobIndexMarkerKey(event.repo, event.job_id));
      if (!index || !isValidJobIndex(index, event.repo, event.job_id) || !isValidJobIndexMarker(marker, event.repo, event.job_id) || !index.active_event_ids.includes(event.event_id)) return null;
      const backlog = meta.backlog_count - 1;
      await s.delete(containmentEventKey(eventId));
      await s.delete(containmentPauseKey(event.pause_seq));
      const remaining = index.active_event_ids.filter((id) => id !== event.event_id);
      if (remaining.length === 0) await s.delete(indexKey);
      else await s.put(indexKey, { ...index, active_event_ids: remaining, active_count: remaining.length, updated_at_ms: Date.now() });
      await s.put(CONTAINMENT_META_KEY, { ...meta, drain_cursor: event.pause_seq, backlog_count: backlog, drain_requested: backlog > 0 });
      return event;
    });
    if (!acknowledged) return false;
    // Cloudflare storage transactions must not issue external KV I/O. The
    // cursor/event commit above is the authority. Cleanup is deliberately
    // best-effort afterwards: a stale mapping is safe because its lookup finds
    // no live DO event, while a failed transaction can never erase the gate.
    const kv = this.env.RUNNER_JOB_PATS;
    if (kv) {
      try {
        await Promise.all([
          kv.delete(containmentEffectJobKey(acknowledged.job_id)),
          ...CONTAINMENT_EFFECT_WITNESS_KINDS.map((kind) => kv.delete(containmentEffectEvidenceKey(acknowledged.effect_id, kind))),
        ]);
      } catch {
        // Lazy cleanup on a later operational pass is safe; never roll back an
        // already-acknowledged DO head because external cleanup was unavailable.
      }
    }
    return true;
  }

  /**
   * A deletion tombstone is a terminal outcome for a head that never received
   * an effect permit.  The same EFFECT_COMMITTED → acknowledge recovery path
   * makes a crash between these two writes deterministic without issuing work.
   */
  async terminalizeTombstonedEvent(eventId: string, owner: string, epoch: number): Promise<boolean> {
    return this.tx(async (s) => {
      const meta = (await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined;
      const event = (await s.get(containmentEventKey(eventId))) as ContainmentEvent | undefined;
      if (!meta || !event || !isCurrentHead(meta, event) || !leaseMatches(meta, owner, epoch, Date.now())
        || event.state !== "CLAIMED" || event.claim?.owner !== owner || event.claim.lease_epoch !== epoch
        || event.effect_permit !== null || canonicalInstallationId(event.installation_id) !== event.installation_id) return false;
      if ((await s.get(installationTombstoneKey(event.installation_id))) === undefined) return false;
      await s.put(containmentEventKey(eventId), { ...event, state: "EFFECT_COMMITTED" });
      return true;
    });
  }

  async releaseLease(owner: string, epoch: number): Promise<void> {
    await this.tx(async (s) => {
      const meta = (await s.get(CONTAINMENT_META_KEY)) as ContainmentMeta | undefined;
      if (meta?.lease?.owner === owner && meta.lease.epoch === epoch) await s.put(CONTAINMENT_META_KEY, { ...meta, lease: null });
    });
  }
}

// ── Durable idle backstop (2026-08-23 incident) ──────────────────────────────
// The SDK's own idle deadline cannot be trusted to expire. `sleepAfterMs` is a
// bare in-memory field (@cloudflare/containers 0.3.7 container.js:1024) and the
// Container constructor calls `renewActivityTimeout()` UNCONDITIONALLY inside
// `blockConcurrencyWhile` (container.js:348-360). So any re-instantiation of the
// DO — an eviction, a Worker redeploy, or merely a `/v1/status` poll touching a
// cold stub — silently rearms the full window with zero real activity. The alarm
// TIME is durable (`ctx.storage.setAlarm`); the DEADLINE is not, so the two can
// disagree indefinitely. On top of that the SDK's `alarm()` renews the timeout
// immediately after firing `onActivityExpired()` (container.js:1566-1569), making
// the idle alarm a self-perpetuating loop that never concludes anything — which
// is how three boxes reached 10.5 h against a 15-minute window while `stop()` was
// called about forty times.
//
// This is a BACKSTOP, not the primary control. The SDK path plus the keep-alive
// sweep stay exactly as they are; this only catches the case where they failed.
// Hence the deliberately generous window: it must never be the thing that ends a
// legitimate job. A cron outage that stopped renewals would take this long to
// bite, by which point a stuck box has cost more than a late one.
const DURABLE_IDLE_BACKSTOP_MS = 45 * 60 * 1000;
// Key names are namespaced so they cannot collide with SDK-owned storage keys.
const LAST_ACTIVITY_KEY = "corelink:lastActivityAt";
const SOFT_STOP_COUNT_KEY = "corelink:softStopCount";
// After this many consecutive backstop expiries on a container that is STILL
// running, stop asking politely. `stop()` is SIGTERM-only and never escalates on
// its own, so without a ceiling here a stop()-defeating bug has no cost bound —
// exactly the shape of the incident this came from.
const MAX_SOFT_STOPS_BEFORE_DESTROY = 2;

// Per-job runner container. One DO instance per spawned runner (keyed by handle).
export class RunnerContainer extends Container<Env> {
  // standard-4; the GH-Actions agent is the image ENTRYPOINT (runner-direct, v0).
  // No inbound port — the runner dials OUT to GitHub (the GH-Actions agent is
  // the image entrypoint; runner-direct, v0). `defaultPort` is left unset.
  // Orphan-leak backstop: the DO sleeps (and the container stops) this long after
  // the last observed ACTIVITY. Reduced 45m→15m (2026-07-06) because a completed
  // job is torn down immediately, so this only governs FAILED/stuck containers, and
  // at 45m those hold account container-instance capacity long enough to starve new
  // spawns under load.
  //
  // ⚠️ 2026-08-02: the note that used to sit here — "A running job keeps the
  // container active, so this never cuts a live job" — was FALSE, and made this a
  // hard 15-minute cap on job DURATION rather than an idle timeout.
  //
  // In @cloudflare/containers 0.3.x, `sleepAfterMs` only moves forward via
  // `renewActivityTimeout()`, and `isActivityExpired()` renews only while
  // `inflightRequests > 0` — a counter incremented SOLELY inside `containerFetch`.
  // This container has no `defaultPort` and is never `containerFetch`ed (the GH
  // Actions agent is the image entrypoint and dials OUT; nothing dials in). So
  // `inflightRequests` stayed 0 forever, the deadline froze at container-start +
  // 900 s, and `alarm()` → `onActivityExpired()` → `stop()` SIGTERMed the box
  // mid-job. Consistent with the longest fabric job that ever succeeded: 864 s.
  //
  // The SDK's own contract for that method is "Call this method whenever there is
  // activity on the container", so the cron supplies the activity signal (see
  // `keepAliveLiveRunners`) and this stays a real IDLE window rather than becoming
  // a lifetime cap. Raising the number instead would have turned every stuck box
  // into a multi-hour hold on `max_instances` — trading a job-killer for a
  // fleet-starver.
  //
  // ⚠️ 2026-08-03: the sentence that used to finish that paragraph — "the
  // spawn-Worker is precisely what knows: a box with a live `rhandle:` binding has
  // a job on it" — was ALSO false, and it is why the first fix leaked. The binding
  // is written at SPAWN, before anything has registered with GitHub, and lives
  // `JOB_PAT_TTL_S` (2 h). A box that booted and never registered therefore never
  // gets a completion event naming it, so it held a `rhandle:` for two hours and
  // the sweep renewed it every single minute — defeating this 15-minute window
  // entirely, on a standard-4 out of `max_instances: 20`. The binding proves a box
  // was STARTED for a job; only GitHub can say whether that box is working. The
  // sweep now asks it.
  sleepAfter = "15m";

  /**
   * Keep this box's idle window open while its job is still running.
   *
   * Called once per cron tick for every container the sweep has VERIFIED is doing
   * work — GitHub reports that box's own runner as `busy` — or could not verify at
   * all (fail-safe: renew). A box GitHub reports as idle, offline or unknown is not
   * called and falls through to `sleepAfter`.
   *
   * Renewing sets the deadline to now + `sleepAfter`, so "stop renewing" is never
   * "kill now": with a 1-minute cron against a 15-minute window a box has to be
   * continuously unrenewed for ~14 minutes to actually sleep, and any tick in that
   * span that finds it busy puts the full window back. That ratio is what makes it
   * safe to ever stop — not the accuracy of a single observation.
   *
   * The activity signal comes from OUR knowledge of the job, not from traffic the
   * SDK can see, because there is no inbound traffic to see.
   */
  keepAlive(): { ok: true } {
    this.renewActivityTimeout();
    // Also record the activity DURABLY. `renewActivityTimeout()` writes only to
    // the SDK's in-memory field, which does not survive re-instantiation; this is
    // what the backstop below reads, and it is deliberately the same call site so
    // the two can never drift apart.
    void this.noteActivity();
    return { ok: true };
  }

  /**
   * Record that this box was observed doing real work, durably.
   *
   * Also clears the soft-stop counter: a box that is working again has not been
   * ignoring anything, and letting a stale count carry over would eventually
   * destroy a healthy container. The two storage ops are independent (the delete
   * consumes nothing from the put), so they run concurrently — same end state,
   * half the round-trips.
   */
  async noteActivity(): Promise<void> {
    await Promise.all([
      this.ctx.storage.put(LAST_ACTIVITY_KEY, Date.now()),
      this.ctx.storage.delete(SOFT_STOP_COUNT_KEY),
    ]);
  }

  /**
   * The backstop. Runs on every alarm, BEFORE the SDK's own handler.
   *
   * Reads the durable last-activity stamp rather than the SDK's in-memory
   * deadline, so a DO re-instantiation cannot rearm it. Escalates to `destroy()`
   * once a soft `stop()` has demonstrably failed to end the container.
   *
   * Fail-safe in the quiet direction: no stamp yet, an unreadable state, or a
   * container that is not running ⇒ do nothing. This code can destroy a customer's
   * running job, so every uncertain branch leaves the box alone and lets the
   * normal paths handle it.
   */
  async enforceDurableIdleBackstop(): Promise<void> {
    const last = await this.ctx.storage.get<number>(LAST_ACTIVITY_KEY);
    if (typeof last !== "number") {
      // First alarm on a box spawned before this shipped, or before any activity
      // was recorded. Stamp it now and judge from here — never from an assumed
      // start time, which would make the very first alarm a potential killer.
      await this.ctx.storage.put(LAST_ACTIVITY_KEY, Date.now());
      return;
    }
    const idleMs = Date.now() - last;
    if (idleMs < DURABLE_IDLE_BACKSTOP_MS) return;

    let running = false;
    try {
      const state = await this.getState();
      running = state.status === "running" || state.status === "healthy";
    } catch (e) {
      // Cannot see the container ⇒ cannot justify killing it.
      logEvent("error", "idle_backstop_state_unreadable", { error: String(e) });
      return;
    }
    if (!running) return;

    const softStops = (await this.ctx.storage.get<number>(SOFT_STOP_COUNT_KEY)) ?? 0;
    if (softStops >= MAX_SOFT_STOPS_BEFORE_DESTROY) {
      logEvent("error", "idle_backstop_destroy", {
        idle_minutes: String(Math.round(idleMs / 60000)),
        soft_stops: String(softStops),
        note: "stop() did not end this container; escalating to destroy()",
      });
      await this.destroy();
      await this.ctx.storage.delete(SOFT_STOP_COUNT_KEY);
      return;
    }

    logEvent("error", "idle_backstop_stop", {
      idle_minutes: String(Math.round(idleMs / 60000)),
      soft_stops: String(softStops + 1),
      note: "durable idle window elapsed while the container is still running",
    });
    await this.ctx.storage.put(SOFT_STOP_COUNT_KEY, softStops + 1);
    try {
      await this.stop();
    } catch (e) {
      // A stop() throw must not swallow the alarm; the next tick escalates.
      logEvent("error", "idle_backstop_stop_threw", { error: String(e) });
    }
  }

  /**
   * Run the backstop first, then hand off to the SDK's alarm.
   *
   * Order matters: the SDK's handler renews its own timeout as a side effect, so
   * anything that needs to observe the pre-renewal state has to run before it.
   * A throw in the backstop must never prevent the SDK alarm from running, or a
   * bug here would break container lifecycle management wholesale.
   */
  async alarm(alarmProps?: Parameters<Container<Env>["alarm"]>[0]): Promise<void> {
    try {
      await this.enforceDurableIdleBackstop();
    } catch (e) {
      logEvent("error", "idle_backstop_threw", { error: String(e) });
    }
    return super.alarm(alarmProps);
  }
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
    // Stamp the durable clock at boot so the backstop measures from a real event
    // rather than from whenever the first alarm happened to land.
    await this.noteActivity();
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

async function sha256Hex(data: string | ArrayBuffer): Promise<string> {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function hexBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i++) bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return bytes;
}

async function verifyGithubHmacBytes(secret: string, signature: string, body: ArrayBuffer): Promise<boolean> {
  const match = /^sha256=([0-9a-f]{64})$/i.exec(signature.trim());
  if (!match) return false;
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
  return crypto.subtle.verify("HMAC", key, hexBytes(match[1]), body);
}

type ContainmentSwitch = "normal" | "paused" | "invalid";
export function parseContainmentSwitch(raw: string | undefined): ContainmentSwitch {
  if (raw === undefined || raw === "0") return "normal";
  if (raw === "1") return "paused";
  return "invalid";
}

/**
 * Shared cross-worker emergency freeze. Only an absent binding or exact "0"
 * leaves new admissions open; malformed and whitespace-padded values pause
 * them so a config mistake cannot silently re-enable spawning.
 */
export function admissionPaused(raw: string | undefined): boolean {
  return raw !== undefined && raw !== "0";
}

const ADMISSION_PAUSE_RETRY_AFTER_SECONDS = "60";

function admissionPausedResponse(): Response {
  return new Response(JSON.stringify({ error: "fabric admission paused" }), {
    status: 503,
    headers: {
      "content-type": "application/json",
      "cache-control": "no-store",
      "retry-after": ADMISSION_PAUSE_RETRY_AFTER_SECONDS,
    },
  });
}
function trimAsciiWhitespace(value: string): string {
  // HTTP field-value whitespace is ASCII-only here. Include VT (0x0b), which
  // JavaScript's `\\s` would hide among non-ASCII whitespace we must preserve.
  return value.replace(/^[\t\n\v\f\r ]+|[\t\n\v\f\r ]+$/g, "");
}
function containmentAuthority(env: Env): DurableObjectStub<ContainmentDO> {
  if (!env.CONTAINMENT) throw new Error("containment authority unavailable");
  return env.CONTAINMENT.get(env.CONTAINMENT.idFromName("global"));
}

async function installationIsTombstoned(env: Env, installationId: string): Promise<boolean> {
  if (canonicalInstallationId(installationId) !== installationId || !env.CONTAINMENT) return false;
  return containmentAuthority(env).installationTombstoned(installationId);
}

function resolveWebhookInstallationId(
  value: unknown,
  repo: string,
  repositoryMap: string | undefined,
): { installationId: string; invalid: boolean } {
  if (value !== undefined && value !== null) {
    const installationId = canonicalInstallationId(value);
    return installationId ? { installationId, invalid: false } : { installationId: "", invalid: true };
  }
  return { installationId: installationIdForRepo(repositoryMap, repo), invalid: false };
}

// T4-W2 consumes T4-W1's immutable ContainmentDO attribution authority. The
// cast keeps this commit compatible with the pre-W1 local class; the ordered
// W1 integration supplies the RPC method and its exact validation.
async function readBillingJobAttribution(
  env: Env,
  jobId: string,
): Promise<{ jobId: string; tenant: string } | null> {
  if (!env.CONTAINMENT) return null;
  const rpc = containmentAuthority(env) as unknown as {
    readJobAttribution(key: string): Promise<string | null>;
  };
  const raw = await rpc.readJobAttribution(`job-attribution:${jobId}`);
  if (!raw) return null;
  const value = JSON.parse(raw) as { jobId?: unknown; tenant?: unknown };
  if (value.jobId !== jobId || typeof value.tenant !== "string" || value.tenant.trim() === "") return null;
  return { jobId, tenant: value.tenant };
}
async function containmentRedriveAuthorityReadable(env: Env): Promise<boolean> {
  // An unavailable authority refuses every reconciler before external effects.
  try {
    await containmentAuthority(env).snapshot();
    return true;
  } catch {
    return false;
  }
}
async function observeInvalidConfig(env: Env, switchName: string, raw: string): Promise<void> {
  const digest = await sha256Hex(raw);
  const authority = containmentAuthority(env);
  await authority.recordInvalidConfig(switchName, raw, digest);
  await deliverInvalidConfig(env);
}
async function deliverInvalidConfig(env: Env): Promise<void> {
  if (!env.METRICS) return;
  const authority = containmentAuthority(env);
  let pending: ContainmentOutboxRecord[];
  try { pending = await authority.pendingInvalidConfig(); } catch { return; }
  const metrics = env.METRICS.get(env.METRICS.idFromName("singleton")) as unknown as { bumpOnce?: (signalId: string, name: string) => Promise<void> };
  if (!metrics.bumpOnce) return;
  for (const rec of pending) {
    try {
      await authority.markInvalidConfigAttempt(rec.signal_id);
      await metrics.bumpOnce(rec.signal_id, "containment_config_invalid");
      await authority.acknowledgeInvalidConfig(rec.signal_id);
    } catch {
      // Keep the same outbox record pending for the next scheduled tick.
    }
  }
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

// One minted JIT registration: the encoded config a box boots with, plus the two
// identifiers GitHub knows it by — the NAME (echoed back on the job webhooks) and
// the numeric ID (the only handle that can DELETE the registration again).
interface MintedJit {
  jit: string;
  runnerName: string;
  // Absent only if GitHub ever omits `runner.id` from the response; every
  // consumer treats that as "cannot delete", never as an error.
  runnerId?: number;
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
): Promise<MintedJit> {
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
  const j = (await resp.json()) as { encoded_jit_config?: string; runner?: { id?: number } };
  if (!j.encoded_jit_config) throw new Error("generate-jitconfig: no encoded_jit_config");
  // Return the NAME too. `generate-jitconfig` binds the runner to a repo + label
  // set and to NOTHING ELSE — not to the job whose webhook prompted it. GitHub
  // then assigns queued jobs to idle runners by LABEL MATCH, so with N identical
  // jobs and N identical runners the assignment is a PERMUTATION: the box minted
  // for job A routinely runs job B. The name is the only identifier that survives
  // that mapping (GitHub reports it back as `workflow_job.runner_name`), so it —
  // not the spawn-request job id — is the correct key for teardown.
  return { jit: j.encoded_jit_config, runnerName: name, runnerId: j.runner?.id };
}

// Delete a runner REGISTRATION we minted but are not going to place a working box
// on (a superseded container-start attempt — see `startWithRetry`).
//
// This is not tidiness. `generate-jitconfig` creates a real runner entity on the
// repo the moment it is called, and that entity is assignable BY LABEL to any
// queued job the instant something registers with its config. An abandoned
// attempt's box that comes up late would therefore be able to CLAIM a customer's
// job — on a container no `rhandle:`/`jhandle:` binding points at, so the
// keep-alive sweep never renews it. What happens NEXT is weaker than this comment
// used to claim: it said `sleepAfter` "SIGKILLs it mid-job 15 minutes later", and
// neither half was true. Automatic expiry runs through the SDK's `stop()`, which
// is SIGTERM-only and never escalates to `destroy()`; and until #487 this image's
// PID 1 discarded SIGTERM outright. Deleting the registration is therefore the
// load-bearing half, not a nicety: the box can boot, but it can never be given
// work. The container destroy that follows is what actually ends it.
//
// Never throws so callers can preserve their original failure, but false is an
// uncertain ownership result. Durable retry/teardown callers must retain their
// attempt until this returns true (including 404-already-gone).
async function deleteRunnerRegistration(
  env: Env,
  repoFullName: string,
  runnerId: number | undefined,
  installationId: string,
): Promise<boolean> {
  if (typeof runnerId !== "number") return false;
  try {
    const authToken = await mintJitAuthToken(env, installationId);
    const resp = await fetch(
      `https://api.github.com/repos/${repoFullName}/actions/runners/${runnerId}`,
      {
        method: "DELETE",
        headers: {
          authorization: `Bearer ${authToken}`,
          accept: "application/vnd.github+json",
          "user-agent": "corelink-spawn-worker",
        },
      },
    );
    // 404 ⇒ already gone (an ephemeral runner self-deletes) — the desired end state.
    if (resp.ok || resp.status === 404) return true;
    logEvent("error", "runner_registration_delete_failed", {
      repo: repoFullName,
      runnerId,
      status: resp.status,
    });
    return false;
  } catch (e) {
    logEvent("error", "runner_registration_delete_failed", {
      repo: repoFullName,
      runnerId,
      error: (e as Error).message,
    });
    return false;
  }
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

function jobAttributionStore(env: Env): JobAttributionStore | undefined {
  if (!env.CONTAINMENT) return undefined;
  const authority = containmentAuthority(env);
  return {
    get: key => authority.readJobAttribution(key),
    putIfAbsent: (key, value) => authority.putJobAttributionIfAbsent(key, value),
    delete: key => authority.deleteJobAttribution(key),
  };
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

// runner_name → the spawned RunnerContainer DO handle. THE authoritative teardown
// key (2026-08-02).
//
// `jhandle:<jobId>` above records which box we STARTED for a given job's webhook.
// That is not the same thing as which box RAN that job: GitHub assigns queued jobs
// to idle ephemeral runners by label match, so with N identical jobs in flight the
// mapping is a permutation. Tearing down by `jhandle:` therefore SIGKILLed a box
// that was still executing somebody else's job — observed 2026-08-02: five jobs
// killed mid-step (one inside `cargo clippy`, one during `Complete job` with its
// work already finished), each surfacing as GitHub's "The self-hosted runner lost
// communication with the server" ~600 s later. Zero deaths among 59 SOLO jobs and
// five among 27 that had at least one other box alive, with NO concurrency
// threshold — the signature of a correlation bug, not a capacity limit.
//
// `runner_name` is minted by us at `generate-jitconfig` and echoed back by GitHub
// on `workflow_job.in_progress` / `.completed`, so it correlates the box to the
// job that ACTUALLY ran on it, whichever permutation GitHub chose.
// ── The stale-box reaper's durable record ────────────────────────────────────
// `rhandle:` above is the KEEP-ALIVE binding, and it TTLs out with the job PAT
// (`JOB_PAT_TTL_S`, 2 h). That is correct for its purpose and WRONG as the fleet's
// only record of a running box: once it expires the sweep can no longer see the
// box at all — not to renew it, and not to stop it. Termination then rests
// entirely on the DO's own `sleepAfter` alarm, a single point of failure.
//
// Measured 2026-08-23: three `standard-4` boxes were `running` for 10.2 h against
// a 15-minute idle window, with every `keepalive_*` counter flat — i.e. invisible
// to the sweep and never stopped by the alarm. ~120 vCPU-hours of nothing.
//
// `sbox:` is the second record, deliberately outliving the keep-alive binding, so
// a box that outlives its own bookkeeping is still FINDABLE and can be actively
// destroyed instead of waited on.
const SPAWNED_BOX_PREFIX = "sbox:";
function spawnedBoxKey(runnerName: string): string {
  return `${SPAWNED_BOX_PREFIX}${runnerName}`;
}
// Long enough that it always outlives the keep-alive binding it backstops; short
// enough to self-clean if the reaper itself is ever broken.
const SPAWNED_BOX_TTL_S = 86400; // 24 h

// A box older than this has, by this system's OWN assumption, outlived any
// legitimate job: `JOB_PAT_TTL_S` is the lifetime the spawn path gives a job's
// credential, so nothing is expected to still be working past it.
const STALE_BOX_AGE_MS = JOB_PAT_TTL_S * 1000;

const RUNNER_HANDLE_PREFIX = "rhandle:";
function runnerHandleKey(runnerName: string): string {
  return `${RUNNER_HANDLE_PREFIX}${runnerName}`;
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

// ── Ghost containers (2026-08-03) ────────────────────────────────────────────
//
// A retry attempt that fails is not an attempt that did nothing. `Container.start`
// issues `this.ctx.container.start(...)` and only THEN polls for up to 8 s to see
// the instance come up (@cloudflare/containers 0.3.7,
// dist/lib/container.js:1378-1421) — so both failure shapes leave a container
// behind:
//
//   • the SDK throws `NO_CONTAINER_INSTANCE_ERROR` after its own poll window, but
//     the platform start was already issued and may still be provisioning;
//   • our `SPAWN_ATTEMPT_TIMEOUT_MS` race rejects while the DO-side RPC keeps
//     running to completion — Workers does not cancel an RPC because the caller
//     stopped awaiting it.
//
// Before this change the loop then minted a FRESH handle (a different DO, a
// different container) and simply dropped the old one on the floor. Nothing ever
// referenced it again: no `jhandle:`/`rhandle:` binding is written for a failed
// attempt, so no teardown path can reach it and the keep-alive sweep never sees
// it. Its ONLY reaper was `sleepAfter` — 15 minutes of a standard-4 (4 vCPU /
// 12 GiB) instance held out of `max_instances: 20`, per failed attempt, up to 2
// per spawn. That is the mechanism by which a nominal fleet of 20 behaves like ~7
// under exactly the burst it is sized for.
//
// It was worse than a wasted slot. Every attempt shared ONE JIT config, so the
// abandoned box booted with the SAME single-use registration as its successor: at
// most one of them could ever register (GitHub: "A session for this runner already
// exists"), and which one won was a race. The fix is both halves together — a
// fresh registration per attempt, and the superseded attempt CANCELLED rather than
// abandoned: its registration deleted so it can never claim work, its container
// destroyed, and a `ghost:` record left behind so the cron re-destroys it if this
// destroy raced a still-provisioning start.
const GHOST_KEY_PREFIX = "ghost:";
function ghostKey(handle: string): string {
  return `${GHOST_KEY_PREFIX}${handle}`;
}
// How long a ghost record survives if the sweep can never confirm the box is
// down. Comfortably past `sleepAfter` (15m), so the record outlives the thing it
// is tracking rather than the reverse. NOTE: `sleepAfter` is NOT the reliable
// backstop this comment used to call it — its deadline lives in an in-memory SDK
// field that any DO re-instantiation rearms, and its expiry path is SIGTERM-only.
// The durable idle backstop on `RunnerContainer` is what actually bounds a box.
const GHOST_TTL_S = 3600;

// Which DO namespace an abandoned handle lives in. Recorded with the handle
// because the sweep runs later, from the cron, with only the record to go on —
// and destroying a check-host handle in the runner namespace would silently do
// nothing at all.
type GhostNs = "runner" | "check";
interface GhostRecord {
  ns: GhostNs;
  reason: string;
}

// Durably record an abandoned container handle for `sweepGhostContainers`.
// Written BEFORE the inline destroy, because the inline destroy is the attempt
// that can lose a race (destroying a container the platform has not finished
// creating is a no-op, and it then comes up anyway). Best-effort: without KV the
// inline destroy still runs and `sleepAfter` is still the backstop.
async function recordGhostContainer(
  env: Env,
  handle: string,
  ns: GhostNs,
  reason: string,
): Promise<void> {
  if (!env.RUNNER_JOB_PATS) return;
  await env.RUNNER_JOB_PATS.put(ghostKey(handle), JSON.stringify({ ns, reason } as GhostRecord), {
    expirationTtl: GHOST_TTL_S,
  }).catch((e) => logEvent("error", "ghost_record_failed", { handle, error: (e as Error).message }));
}

// Cancel an abandoned container-start attempt at the CONTAINER level: record it
// durably, then destroy it. Shared by the autoscaler path (which additionally
// deletes the GitHub registration first — see `cancelSpawnAttempt`) and the
// `/v1/spawn` fabric contract, where the JIT is supplied by the caller and there
// is no registration for us to revoke.
//
// Best-effort by contract: this runs on an already-failing path and must never
// mask the start error that caused it.
async function abandonContainer(
  env: Env,
  handle: string,
  ns: GhostNs,
  reason: string,
): Promise<boolean> {
  await recordGhostContainer(env, handle, ns, reason);
  let container: { teardown(): Promise<void>; isAlive(): Promise<boolean> } | undefined;
  let teardownSucceeded = false;
  try {
    container = (
      ns === "check"
        ? getContainer(env.CHECK_HOST_CONTAINER, handle)
        : getContainer(env.RUNNER_CONTAINER, handle)
    ) as unknown as { teardown(): Promise<void>; isAlive(): Promise<boolean> };
    await container.teardown();
    teardownSucceeded = true;
  } catch (e) {
    // Expected when the DO never materialised. The `ghost:` record above is what
    // makes this survivable: the cron re-destroys and confirms.
    logEvent("info", "abandon_teardown_threw", { handle, error: (e as Error).message });
  }
  await bumpMetrics(env, "container_start_abandoned");
  if (!teardownSucceeded || !container) return false;
  try {
    return !(await container.isAlive());
  } catch (e) {
    logEvent("info", "abandon_liveness_threw", { handle, error: (e as Error).message });
    return false;
  }
}

/**
 * Destroy the containers left behind by abandoned start attempts, and CONFIRM
 * they are down before forgetting them.
 *
 * Runs on the 1-minute cron. The inline destroy at abandon time is the fast path;
 * this is the one that is allowed to be sure. A record is deleted only once the DO
 * reports the container not alive — a destroy that raced a still-provisioning
 * start leaves the box running and is retried next tick, which is precisely the
 * case the inline destroy cannot handle on its own.
 *
 * A box that is still alive after a destroy is LOGGED AT ERROR and kept for the
 * next tick; `ghost_container_reaped` counts only CONFIRMED reclaims, so the
 * counter can never report capacity we did not actually get back. The whole point
 * of this sweep is that a container which boots and never does any work is a
 * named event instead of an unexplained 15-minute hole in the fleet's capacity.
 *
 * Best-effort throughout — a backstop, never a gate. It must never throw into the
 * cron tick that also drives orphan recovery and billing.
 */
export async function sweepGhostContainers(env: Env): Promise<number> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) return 0;
  let listed: { keys: { name: string }[] };
  try {
    listed = await kv.list({ prefix: GHOST_KEY_PREFIX });
  } catch (e) {
    logEvent("error", "ghost_sweep_list_failed", { error: (e as Error).message });
    return 0;
  }
  let reaped = 0;
  for (const { name } of listed.keys) {
    const handle = name.slice(GHOST_KEY_PREFIX.length);
    // A record written by an older deploy (or a malformed one) is treated as the
    // runner namespace — the only one that existed when this was written, and the
    // only one whose ghosts contend for the fleet's `max_instances`.
    let ns: GhostNs = "runner";
    try {
      const raw = await kv.get(name);
      if (raw) ns = (JSON.parse(raw) as GhostRecord).ns === "check" ? "check" : "runner";
    } catch {
      /* keep the default */
    }
    const container =
      ns === "check"
        ? getContainer(env.CHECK_HOST_CONTAINER, handle)
        : getContainer(env.RUNNER_CONTAINER, handle);
    try {
      await container.teardown();
    } catch (e) {
      // A handle whose DO never materialised throws here; that is a box that is
      // already down, so fall through to the liveness check rather than retrying.
      logEvent("info", "ghost_teardown_threw", { handle, error: (e as Error).message });
    }
    let alive: boolean;
    try {
      alive = await container.isAlive();
    } catch {
      alive = false; // unreachable DO ⇒ nothing is running
    }
    if (alive) {
      // Still up after a destroy: keep the record and try again next tick. Loud,
      // because this is the failure mode that eats the fleet.
      logEvent("error", "ghost_container_still_alive", { handle });
      continue;
    }
    await kv.delete(name).catch(() => {
      /* best-effort: the record TTL-expires */
    });
    reaped++;
  }
  if (reaped > 0) {
    await bumpMetrics(env, ...Array(reaped).fill("ghost_container_reaped"));
    logEvent("info", "ghost_containers_reaped", { count: reaped });
  }
  return reaped;
}

/**
 * Start a container, retrying on the transient CF platform failure, with each
 * attempt independently provisioned and each SUPERSEDED attempt cancelled.
 *
 * @param provision  Per-attempt setup that must not be shared across attempts —
 *                   for the autoscaler path, minting that attempt's own GitHub JIT
 *                   registration. Runs BEFORE the start; a throw here propagates
 *                   immediately (nothing has been started yet, so there is nothing
 *                   to clean up and no reason to burn the retry budget).
 * @param start      Issues the container start for this attempt's fresh handle.
 * @param abandon    Cancels an attempt whose start failed or timed out. Called with
 *                   the handle AND what `provision` produced, so the caller can
 *                   revoke the registration as well as destroy the box. Best-effort
 *                   by contract: it must swallow its own errors.
 */
async function startWithRetry<T>(
  provision: (attempt: number) => Promise<T>,
  start: (handle: string, provisioned: T) => Promise<void>,
  abandon: (handle: string, provisioned: T, reason: string) => Promise<boolean | void>,
  maxAttempts = SPAWN_MAX_ATTEMPTS,
): Promise<{ handle: string; provisioned: T; attempt: number }> {
  let lastErr: unknown;
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    const provisioned = await provision(attempt);
    const handle = crypto.randomUUID();
    try {
      // Race the start against a timeout — a hung start rejects and is retried.
      await Promise.race([
        start(handle, provisioned),
        new Promise<never>((_, reject) =>
          setTimeout(
            () => reject(new Error(`start timed out after ${SPAWN_ATTEMPT_TIMEOUT_MS}ms`)),
            SPAWN_ATTEMPT_TIMEOUT_MS,
          ),
        ),
      ]);
      return { handle, provisioned, attempt };
    } catch (e) {
      lastErr = e;
      logEvent("info", "container_start_retry", {
        attempt,
        maxAttempts,
        error: (e as Error).message,
      });
      // Cancel it. Losing the race to a timeout does NOT mean nothing started —
      // see the ghost-container note above. This runs on EVERY failed attempt,
      // including the last one, so an exhausted spawn leaves no box behind either.
      // Fabric callers retain their established best-effort ghost cleanup. The
      // autoscaler returns false when it cannot prove its exact attempt is down;
      // in that case a replacement JIT must never be minted.
      const safeToRetry = await abandon(handle, provisioned, (e as Error).message);
      if (safeToRetry === false) break;
      if (attempt < maxAttempts) {
        await new Promise((r) => setTimeout(r, 300 * attempt));
      }
    }
  }
  throw new Error(
    `container start failed after ${maxAttempts} attempts: ${(lastErr as Error).message}`,
  );
}

// Cancel one abandoned container-start attempt: kill the registration first (so a
// late boot can never be given a customer's job), then the box, then leave the
// durable `ghost:` record for the cron to confirm. Order is deliberate — the
// registration delete is the only step that bounds the WORST outcome (a stray box
// running real work untracked), so it goes first and does not depend on the
// destroy succeeding.
//
// Fully guarded: it runs inside the retry loop of an already-failing spawn and
// must never mask the start error that caused it.
async function cancelSpawnAttempt(
  env: Env,
  opts: {
    jobId: string;
    repo: string;
    installationId: string;
    handle: string;
    minted?: MintedJit;
    // `startWithEnv` is issued only after this durable fence resolves. If the
    // fence itself failed, no provider start occurred and a fresh retry does
    // not depend on proving a container liveness result.
    activeAttemptPersisted?: boolean;
    reason: string;
  },
): Promise<boolean> {
  const { jobId, repo, installationId, handle, minted, activeAttemptPersisted, reason } = opts;
  let registrationDeleted = false;
  try {
    if (minted) {
      registrationDeleted = await deleteRunnerRegistration(
        env,
        repo,
        minted.runnerId,
        installationId,
      );
    }
    // Deletion is the ownership fence: a false result includes a rejected or
    // unreachable GitHub request, so it cannot prove runner A will not claim
    // work later.  Keep its durable claim/attempt intact and refuse B even if
    // the local destroy below happens to report the box down.
    if (minted && !registrationDeleted) {
      await abandonContainer(env, handle, "runner", reason);
      logEvent("error", "container_start_registration_delete_unconfirmed", { jobId, repo, handle, reason });
      return false;
    }
    const tornDown = await abandonContainer(env, handle, "runner", reason);
    if (!activeAttemptPersisted) return true;
    if (!tornDown) {
      logEvent("error", "container_start_abandon_unconfirmed", { jobId, repo, handle, reason });
      return false;
    }
    // The start callback records this handle before issuing its provider RPC.
    // Retire only the matching attempt after liveness says it is down, retaining
    // the same slot/claim for attempt B. Any stale or replacement record blocks
    // the retry rather than clearing a newer runner's ownership proof.
    const active = await concurrencySlots(env).readActiveAttempt(jobId);
    if (!active || active.handle !== handle || active.runnerName !== minted?.runnerName
      || !(await concurrencySlots(env).retireActiveAttemptForRetry(jobId, active.generation, active.ownerToken, handle))) {
      logEvent("error", "container_start_attempt_retire_unconfirmed", { jobId, repo, handle, reason });
      return false;
    }
    logEvent("error", "container_start_abandoned", {
      jobId,
      repo,
      handle,
      runnerName: minted?.runnerName,
      registrationDeleted,
      reason,
    });
    return true;
  } catch (e) {
    logEvent("error", "abandon_failed", { jobId, handle, error: (e as Error).message });
    return false;
  }
}

// Spawn one runner container with the ALREADY-authorized cache-warm CLW_* overlay
// (`mint`, from buildContainerEnv, computed BEFORE any JIT is minted). On a warm
// mint we stash job_id→pat_id (revoke keys on pat_id) AND job_id→tenant
// (completion bills/releases the DERIVED tenant). The overlay's CLW_TENANT is the
// server-derived tenant, never wrangler's var.
//
// The GitHub JIT is minted HERE, once per container-start ATTEMPT, rather than once
// per job by the caller. A JIT config registers one single-use runner: two boxes
// booted with the same one cannot both register, so sharing it across retries meant
// a superseded attempt could win the registration race and leave the box we
// actually track unable to work. Each attempt now gets its own registration, and
// `cancelSpawnAttempt` deletes the registration of every attempt that loses.
//
// Returns the surviving attempt's handle AND its runner name — the caller cannot
// know either in advance, because both belong to the attempt that won.
async function spawnRunner(
  env: Env,
  jobId: string,
  mint: ContainerEnvResult,
  opts: ContainmentDriveOpts,
): Promise<{ handle: string; runnerName: string; attempt: number }> {
  const { repo, installationId, labels } = opts;
  const { handle, provisioned, attempt } = await startWithRetry<{ minted: MintedJit; attempt: number; activeAttemptPersisted?: boolean }>(
    async (attempt) => {
      const minted = await mintJit(env, repo, labels, installationId);
      // Persist the exact GitHub runner identity before its container can start.
      // Completion later presents this same runner_name; without this fence a
      // fast completion could race a post-start binding and leave capacity held.
      if (opts.bindProviderIdentity && !(await opts.bindProviderIdentity(minted.runnerName))) {
        throw new Error("spawn claim provider binding unavailable");
      }
      await bumpMetrics(env, "jit_minted");
      // Recorded so a box killed by the WRONG completion can be traced back to the
      // job it was minted for — the permutation is invisible without this line.
      logEvent("info", "runner_minted", {
        jobId,
        repo,
        runnerName: minted.runnerName,
        attempt,
      });
      return { minted, attempt };
    },
    async (h, provisioned) => {
      const minted = provisioned.minted;
      // This is deliberately before both the provider RPC and every KV
      // projection below.  A timeout/crash at any later point therefore leaves
      // a generation-bound exact-handle teardown obligation in the singleton
      // authority, not an expiring best-effort KV hint.
      if (!(await concurrencySlots(env).persistActiveAttempt(
        jobId, h, minted.runnerName, minted.runnerId, provisioned.attempt, repo, installationId, mint.preparationId,
      ))) throw new Error("active spawn attempt was not durably recorded");
      provisioned.activeAttemptPersisted = true;
      if (mint.computeReservationId) await containmentAuthority(env).claimComputeProvider(mint.computeReservationId, jobId);
      await getContainer(env.RUNNER_CONTAINER, h).startWithEnv({
        CORELINK_RUNNER_JITCONFIG: minted.jit,
        ...mint.containerEnv, // CLW_* overlay (empty on a cold spawn)
      });
    },
    (h, provisioned, reason) =>
      cancelSpawnAttempt(env, { jobId, repo, installationId, handle: h, minted: provisioned.minted, activeAttemptPersisted: provisioned.activeAttemptPersisted, reason }),
    // A fresh provider attempt needs its own independently funded reservation.
    mint.computeReservationId ? 1 : SPAWN_MAX_ATTEMPTS,
  );
  const runnerName = provisioned.minted.runnerName;
  if (isContainmentDrive(opts)) {
    // The retry helper returned only after this JIT candidate's start completed.
    // Timed-out/abandoned candidates never get a reusable DO witness.
    await writeContainmentEvidence(
      env,
      opts,
      "attempt",
      "github:generate-jitconfig",
      JSON.stringify({ runner_name: runnerName, runner_id: provisioned.minted.runnerId, attempt }),
      { attempt_count: attempt },
    );
  }
  // (The jobId->patId revoke-key is now written at MINT time in driveSpawn, BEFORE
  // the spawn — F2/W3 — so a start failure can revoke the PAT rather than orphan it.
  // Intentionally NOT re-written here.)
  // Normal arrivals use the compatibility projections directly. Contained work
  // keeps its immutable witness/read-back sequence below, which binds the same
  // facts to the containment permit before it may become terminal.
  if (!isContainmentDrive(opts)) {
    await persistSpawnPostStartProjections(env, opts, mint, handle, runnerName, provisioned.minted.runnerId);
    return { handle, runnerName, attempt };
  }
  if (mint.tenant && env.RUNNER_JOB_PATS) {
    // Stash the derived tenant for completion (concurrency-slot release + billing).
    try { await env.RUNNER_JOB_PATS.put(jobTenantKey(jobId), mint.tenant); }
    catch (e) { logEvent("error", "kv_put_job_tenant_failed", { jobId, error: (e as Error).message }); throw e; }
    // Cache the tenant's monthly compute allowance (server #975) so COMPLETION —
    // a separate Worker invocation that never talks to the mint — can tell how
    // close this tenant is to it. Keyed per TENANT, not per job: the allowance is
    // a property of the subscription, so one key refreshed on every spawn beats a
    // write per job. Absent ⇒ no metered ceiling ⇒ nothing to warn about.
    if (typeof mint.maxVcpuH === "number" && mint.maxVcpuH > 0) {
      try { await env.RUNNER_JOB_PATS.put(vcpuCeilingKey(mint.tenant), String(mint.maxVcpuH), {
        expirationTtl: VCPU_KEY_TTL_S,
      }); } catch (e) { logEvent("error", "kv_put_vcpu_ceiling_failed", { jobId, error: (e as Error).message }); throw e; }
    }
  }
  if (env.RUNNER_JOB_PATS) {
    // Stash the DO handle so `completed` can tear the container down immediately
    // (vs the 45m sleepAfter idle-out that starves new spawns). Best-effort: a
    // miss just falls back to sleepAfter (fail-safe, never blocks a legacy
    // spawn). Contained work is different: record the immutable lease witness
    // from the known handle FIRST, then require the mutable completion source to
    // be durably written/read back before it may reach terminal RESULT.
    if (isContainmentDrive(opts)) {
      await writeContainmentEvidence(env, opts, "lease", jobHandleKey(jobId), handle);
      await env.RUNNER_JOB_PATS.put(jobHandleKey(jobId), handle, {
        expirationTtl: JOB_PAT_TTL_S,
      });
      const storedHandle = await env.RUNNER_JOB_PATS.get(jobHandleKey(jobId));
      if (storedHandle !== handle) throw new Error("containment lease source was not durably written");
    } else {
      try { await env.RUNNER_JOB_PATS.put(jobHandleKey(jobId), handle, {
        expirationTtl: JOB_PAT_TTL_S,
      }); } catch (e) { logEvent("error", "kv_put_job_handle_failed", { jobId, error: (e as Error).message }); throw e; }
    }
    // …and stash it under the RUNNER NAME, which is what completion tears down by.
    // This is the binding that survives GitHub's job→runner permutation; the
    // jobId one above is kept only as a fallback for a payload with no
    // runner_name (a job that died before assignment).
    //
    // The value carries the GitHub runner id + repo + installation alongside the
    // handle, because the keep-alive sweep has to be able to ask GitHub about THIS
    // box's runner specifically. Written here, at spawn, rather than at
    // registration: `generate-jitconfig` creates the runner entity and returns its
    // id immediately, so waiting for a registration that may never happen would
    // leave the un-registered box — the exact one that leaks — unverifiable.
    try { await env.RUNNER_JOB_PATS.put(
      runnerHandleKey(runnerName),
      encodeRunnerBinding({
        h: handle,
        rid: provisioned.minted.runnerId,
        repo,
        inst: installationId,
        // The job this box was started FOR — a CANDIDATE for the stranded sweep,
        // never a conclusion (GitHub assigns by label match; see RunnerBinding).
        jid: jobId,
        t: Date.now(),
      }),
      {
        expirationTtl: JOB_PAT_TTL_S,
      },
    ); } catch (e) { logEvent("error", "kv_put_runner_handle_failed", { jobId, runnerName, error: (e as Error).message }); throw e; }
    // …and the reaper's durable twin, which deliberately OUTLIVES the binding
    // above. Same facts, longer TTL: this is what lets the sweep find a box that
    // has outlived its own keep-alive record instead of trusting `sleepAfter`.
    try { await env.RUNNER_JOB_PATS.put(
      spawnedBoxKey(runnerName),
      JSON.stringify({
        h: handle,
        rid: provisioned.minted.runnerId,
        repo,
        inst: installationId,
        t: Date.now(),
      }),
      { expirationTtl: SPAWNED_BOX_TTL_S },
    ); } catch (e) { logEvent("error", "kv_put_spawned_box_failed", { jobId, runnerName, error: (e as Error).message }); throw e; }
  }
  return { handle, runnerName, attempt };
}

/**
 * Write the best-effort KV compatibility projections after the provider has
 * started. The durable active-attempt record already exists at this point, so a
 * failed projection deliberately throws into exact-handle cleanup rather than
 * silently returning a false successful spawn.
 */
export async function persistSpawnPostStartProjections(
  env: Env,
  opts: Pick<ContainmentDriveOpts, "jobId" | "repo" | "installationId">,
  mint: Pick<ContainerEnvResult, "tenant" | "maxVcpuH">,
  handle: string,
  runnerName: string,
  runnerId: number | undefined,
): Promise<void> {
  const { jobId, repo, installationId } = opts;
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) return;
  if (mint.tenant) {
    try { await kv.put(jobTenantKey(jobId), mint.tenant); }
    catch (e) { logEvent("error", "kv_put_job_tenant_failed", { jobId, error: (e as Error).message }); throw e; }
    if (typeof mint.maxVcpuH === "number" && mint.maxVcpuH > 0) {
      try { await kv.put(vcpuCeilingKey(mint.tenant), String(mint.maxVcpuH), { expirationTtl: VCPU_KEY_TTL_S }); }
      catch (e) { logEvent("error", "kv_put_vcpu_ceiling_failed", { jobId, error: (e as Error).message }); throw e; }
    }
  }
  try { await kv.put(jobHandleKey(jobId), handle, { expirationTtl: JOB_PAT_TTL_S }); }
  catch (e) { logEvent("error", "kv_put_job_handle_failed", { jobId, error: (e as Error).message }); throw e; }
  try {
    await kv.put(runnerHandleKey(runnerName), encodeRunnerBinding({ h: handle, rid: runnerId, repo, inst: installationId, jid: jobId, t: Date.now() }), { expirationTtl: JOB_PAT_TTL_S });
  } catch (e) { logEvent("error", "kv_put_runner_handle_failed", { jobId, runnerName, error: (e as Error).message }); throw e; }
  try {
    await kv.put(spawnedBoxKey(runnerName), JSON.stringify({ h: handle, rid: runnerId, repo, inst: installationId, t: Date.now() }), { expirationTtl: SPAWNED_BOX_TTL_S });
  } catch (e) { logEvent("error", "kv_put_spawned_box_failed", { jobId, runnerName, error: (e as Error).message }); throw e; }
}

/**
 * Drive the durable teardown intent.  This intentionally never clears a tenant
 * vCPU key: it is tenant-wide subscription state, not an attempt projection.
 */
async function recoverActiveSpawnAttempt(env: Env, jobId: string): Promise<boolean> {
  let attempt: ActiveSpawnAttempt | null;
  try { attempt = await concurrencySlots(env).readActiveAttempt(jobId); }
  catch { return false; }
  if (!attempt) return false;
  if (!attempt.teardownConfirmedAtMs) {
    // Revoke exactly the JIT registration recorded for this handle before a late
    // provider boot can claim unrelated queued work.  This is idempotent (404 is
    // success) and uses the installation captured at mint time.
    // A failed DELETE means GitHub may still allow this exact runner to claim a
    // job. Do not certify the container, release capacity, or terminalize the
    // intent until the registration fence is confirmed (404 is confirmed).
    if (!(await deleteRunnerRegistration(env, attempt.repo, attempt.runnerId, attempt.installationId))) return false;
    const container = getContainer(env.RUNNER_CONTAINER, attempt.handle);
    // A failed teardown is not evidence that the exact handle is down. Retain the
    // intent for cron even if a follow-up liveness probe happens to say false.
    try { await container.teardown(); } catch { return false; }
    let alive: boolean;
    try { alive = await container.isAlive(); } catch { return false; }
    if (alive) return false;
    try {
      if (!(await concurrencySlots(env).markAttemptTeardownConfirmed(jobId, attempt.generation, attempt.ownerToken, attempt.handle))) return false;
    } catch { return false; }
  }
  // Retain the intent until EVERY local release has acknowledged.  A release
  // outage remains in the cron retry set instead of becoming an unowned leak.
  if (attempt.preparationId) {
    try { await concurrencySlots(env).releasePreparation(jobId, attempt.preparationId); } catch { return false; }
  }
  if (!await releaseConcurrencySlot(env, jobId)) return false;
  let confirmed: ActiveSpawnAttempt | null;
  try {
    confirmed = await concurrencySlots(env).confirmAttemptTeardown(jobId, attempt.generation, attempt.ownerToken, attempt.handle);
  } catch { return false; }
  if (!confirmed) return false;
  return true;
}

export async function retryActiveSpawnTeardowns(env: Env): Promise<number> {
  let attempts: ActiveSpawnAttempt[];
  try { attempts = await concurrencySlots(env).pendingActiveAttempts(25); } catch { return 0; }
  let settled = 0;
  for (const attempt of attempts) if (await recoverActiveSpawnAttempt(env, attempt.jobId)) settled++;
  return settled;
}

export async function revokeCompletedJob(env: Env, jobId: string, derivedTenant?: string): Promise<boolean> {
  return revokeCompletedJobOwned(env, containmentAuthority(env), jobId, derivedTenant);
}

export async function retryFailedRevocations(env: Env): Promise<number> {
  return retryFailedRevocationsOwned(env, containmentAuthority(env));
}

export async function dispatchTenantSuspensionRevocations(
  env: Env,
  event: { event_id: string; tenant_id: string },
): Promise<number> {
  return dispatchTenantSuspensionRevocationsOwned(env, containmentAuthority(env), event);
}

// Tear down a completed job's runner container by the DO handle stashed at spawn.
// A finished ephemeral runner's container otherwise idles until `sleepAfter` (45m),
// holding account container-instance capacity and starving new spawns. A slot is
// released only after the exact handle reports down; a failed/uncertain teardown
// keeps its durable handle for the next completion/retry tick.
async function teardownCompletedRunner(
  env: Env,
  jobId: string,
  runnerName?: string,
): Promise<boolean> {
  if (!env.RUNNER_JOB_PATS) return false;
  // Resolve by RUNNER NAME first — that is the box which actually ran this job.
  // `jhandle:<jobId>` records the box we STARTED for this job's webhook, which
  // under GitHub's label-match assignment is frequently a DIFFERENT box that is
  // still busy; destroying it is what killed live jobs on 2026-08-02. The jobId
  // lookup survives only for a completion with no runner_name (a job cancelled
  // before assignment). Once GitHub supplied a runner identity, falling back to
  // a job pointer can destroy a replacement box for the same job.
  let handle: string | null = null;
  let resolvedBy: "runner_name" | "job_id" = "runner_name";
  try {
    if (runnerName) {
      // The `rhandle:` value is a binding record (or, for anything written before
      // that change shipped, a bare handle string) — `parseRunnerBinding` reads both.
      handle = parseRunnerBinding(await env.RUNNER_JOB_PATS.get(runnerHandleKey(runnerName)))?.h ?? null;
    }
    if (!runnerName) {
      resolvedBy = "job_id";
      // `jhandle:` was never given a record shape; it still holds a bare handle.
      handle = await env.RUNNER_JOB_PATS.get(jobHandleKey(jobId));
    }
  } catch {
    return false; // KV read failed ⇒ sleepAfter is the backstop
  }
  if (!handle) return false; // cold/legacy job, or already torn down
  logEvent("info", "teardown_resolved", { jobId, runnerName, resolvedBy });
  const container = getContainer(env.RUNNER_CONTAINER, handle);
  try {
    await container.teardown();
  } catch (e) {
    logEvent("error", "teardown_failed", { jobId, error: (e as Error).message });
    // A failed destroy is not evidence of a released provider slot.
    try {
      if (await container.isAlive()) return false;
    } catch {
      return false;
    }
  }
  try {
    if (await container.isAlive()) {
      logEvent("error", "teardown_still_alive", { jobId, handle });
      return false;
    }
  } catch (e) {
    logEvent("error", "teardown_confirmation_failed", { jobId, error: (e as Error).message });
    return false;
  }
  // With a runner name, remove only that exact runner binding. A job pointer can
  // already belong to a replacement attempt and must remain untouched.
  if (runnerName) {
    await env.RUNNER_JOB_PATS.delete(runnerHandleKey(runnerName)).catch(() => {
      /* best-effort: the key TTL-expires */
    });
  } else {
    await env.RUNNER_JOB_PATS.delete(jobHandleKey(jobId)).catch(() => {
      /* best-effort: the key TTL-expires */
    });
  }
  return true;
}

async function teardownObligationPresent(env: Env, jobId: string, runnerName?: string): Promise<boolean | null> {
  if (!env.RUNNER_JOB_PATS) return false;
  try {
    if (runnerName) {
      // Absence is not proof that a completion may release this job's current
      // slot: the exact runner binding could have been lost while a replacement
      // still owns the job pointer. Retain capacity until the exact completion
      // path confirms teardown and claim identity.
      return true;
    }
    return Boolean(await env.RUNNER_JOB_PATS.get(jobHandleKey(jobId)));
  } catch {
    return null;
  }
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
  // BILLING_REGION is an explicit override; otherwise use the actual colo
  // supplied by Cloudflare. Missing provider metadata remains unbillable.
  const candidate = env.BILLING_REGION ?? colo ?? "";
  return /^[a-z]{3}$/i.test(candidate) ? candidate.toLowerCase() : "";
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

// ── Near-ceiling warning (2026-08-02) ────────────────────────────────────────
//
// Accumulate this job's vCPU-seconds into the tenant's running period total and,
// if that crossed a threshold nobody has announced yet, say so LOUDLY and once.
//
// Why it belongs at completion: this Worker is the only component that sees a job
// finish, and the allowance only reached it via the mint (server #975). Neither
// half can do this alone.
//
// Deliberately NOT a gate — crossing the ceiling never stops a job. The customer
// keeps building and pays the overage; stopping someone's CI mid-sprint is a
// worse outcome than charging them, which is why overage exists instead of a hard
// block. This function's only power is to make the bill unsurprising.
//
// Best-effort throughout: it runs after the response and every failure is
// swallowed. A missed warning costs a surprised customer; a thrown one would cost
// the completion webhook, which also does revoke + teardown + billing.
async function warnIfNearVcpuCeiling(
  env: Env,
  jobId: string,
  tenant: string | undefined,
  wj: CompletedJob | undefined,
): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv || !tenant) return;
  const startedMs = wj?.started_at ? Date.parse(wj.started_at) : NaN;
  const completedMs = wj?.completed_at ? Date.parse(wj.completed_at) : NaN;
  if (!Number.isFinite(startedMs) || !Number.isFinite(completedMs)) return;
  try {
    const ceilingRaw = await kv.get(vcpuCeilingKey(tenant));
    if (!ceilingRaw) return; // no metered allowance on file ⇒ nothing to be near
    const ceilingVcpuH = Number.parseFloat(ceilingRaw);
    if (!Number.isFinite(ceilingVcpuH) || ceilingVcpuH <= 0) return;

    // Same unit as the billable event: ALLOCATED wall-clock × the box's vCPU
    // count. If these two ever diverge the warning fires at the wrong moment,
    // so they are deliberately computed the same way.
    const jobVcpuSeconds =
      Math.max(0, Math.floor((completedMs - startedMs) / 1000)) * RUNNER_BOX_VCPU;
    const period = billingPeriod(completedMs);
    const usageKey = vcpuUsageKey(tenant, period);
    // The usage read and the per-threshold marker reads are independent of each
    // other (only the ceiling above gates the early-returns), so they are issued
    // together; the resolved values and the resulting `warned` set are identical
    // to reading them one at a time.
    const [usageRaw, warnedRaws] = await Promise.all([
      kv.get(usageKey),
      Promise.all(VCPU_WARN_THRESHOLDS.map((t) => kv.get(vcpuWarnedKey(tenant, period, t)))),
    ]);
    const prior = Number.parseFloat(usageRaw ?? "0");
    const total = (Number.isFinite(prior) ? prior : 0) + jobVcpuSeconds;
    // Read-modify-write on KV is racy under concurrent completions, and that is
    // ACCEPTED: this counter drives a human-facing warning, not an invoice. The
    // invoice comes from the usage ledger + billing ingest, which are per-job and
    // idempotent. A warning that fires a few jobs late is fine; blocking the
    // completion path on a strongly-consistent counter would not be.
    await kv.put(usageKey, String(total), { expirationTtl: VCPU_KEY_TTL_S });

    const warned = new Set<number>();
    for (let i = 0; i < VCPU_WARN_THRESHOLDS.length; i++) {
      if (warnedRaws[i]) warned.add(VCPU_WARN_THRESHOLDS[i]);
    }
    const step = vcpuWarningStep(total, ceilingVcpuH, warned);
    if (step.crossed === null) return;

    // Mark BEFORE announcing: a duplicate announcement is worse than a missed
    // one here — an alert that repeats on every job is an alert that gets muted.
    await kv.put(vcpuWarnedKey(tenant, period, step.crossed), "1", {
      expirationTtl: VCPU_KEY_TTL_S,
    });
    const exceeded = step.crossed >= 1;
    logEvent(exceeded ? "error" : "info", exceeded ? "vcpu_ceiling_exceeded" : "vcpu_ceiling_approaching", {
      jobId,
      tenant,
      period,
      consumedVcpuH: Math.round(step.consumedVcpuH * 100) / 100,
      ceilingVcpuH: step.ceilingVcpuH,
      pct: Math.round(step.fraction * 100),
      // Spelled out so the log line alone is actionable without the price list.
      note: exceeded
        ? "further usage this period bills as overage at $0.30/vCPU-h"
        : "approaching the included allowance; overage bills at $0.30/vCPU-h",
    });
    await bumpMetrics(env, exceeded ? "vcpu_ceiling_exceeded" : "vcpu_ceiling_approaching");
  } catch (e) {
    logEvent("error", "vcpu_ceiling_warn_failed", { jobId, error: (e as Error).message });
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

type SpawnClaimLease = Pick<SpawnClaimRecord, "generation" | "ownerToken">;

async function acquireSpawnClaimAtomic(env: Env, jobId: string): Promise<SpawnClaimLease | null> {
  let acquired: SpawnClaimLease | null = null;
  try {
    if (!env.CONCURRENCY_SLOTS) return null;
    const result = await concurrencySlots(env).acquireSpawnClaim(jobId, SPAWN_CLAIM_TTL_S * 1000);
    if (result.status !== "acquired") return null;
    const lease = { generation: result.generation, ownerToken: result.ownerToken };
    acquired = lease;
    // Containment evidence still reads the historical marker.  Publish only a
    // self-describing marker owned by this lease; an old literal/timestamp is
    // treated as occupied and is never adopted or deleted by this path.
    if (env.RUNNER_JOB_PATS) {
      const key = `spawn:${jobId}`;
      const existing = await env.RUNNER_JOB_PATS.get(key);
      if (existing) {
        await concurrencySlots(env).releaseSpawnClaim(jobId, lease.generation, lease.ownerToken);
        return null;
      }
      await env.RUNNER_JOB_PATS.put(key, JSON.stringify({
        schema_version: 2,
        generation: lease.generation,
        owner_token: lease.ownerToken,
        claimed_at_ms: Date.now(),
      }), { expirationTtl: SPAWN_CLAIM_TTL_S });
      const stored = await env.RUNNER_JOB_PATS.get(key);
      if (!stored || !stored.includes(lease.ownerToken)) throw new Error("spawn claim marker was not durably bound");
    }
    return lease;
  } catch (error) {
    if (acquired && env.CONCURRENCY_SLOTS) {
      await concurrencySlots(env).releaseSpawnClaim(jobId, acquired.generation, acquired.ownerToken).catch(() => undefined);
    }
    logEvent("error", "spawn_claim_authority_unavailable", { jobId, error: (error as Error).message });
    return null;
  }
}

async function releaseSpawnClaimAtomic(env: Env, jobId: string, lease: SpawnClaimLease | null): Promise<void> {
  if (!lease) return;
  try {
    const released = await concurrencySlots(env).releaseSpawnClaim(jobId, lease.generation, lease.ownerToken);
    if (released === "released" && env.RUNNER_JOB_PATS) {
      const key = `spawn:${jobId}`;
      const raw = await env.RUNNER_JOB_PATS.get(key);
      try {
        const marker = raw ? JSON.parse(raw) as { generation?: number; owner_token?: string } : null;
        if (marker?.generation === lease.generation && marker.owner_token === lease.ownerToken) {
          await env.RUNNER_JOB_PATS.delete(key);
        }
      } catch { /* legacy marker: conservative adoption leaves it untouched */ }
    }
  } catch (error) {
    logEvent("error", "spawn_claim_release_failed", { jobId, error: (error as Error).message });
  }
}

function spawnClaimCallbacks(env: Env, jobId: string): {
  claim: () => Promise<boolean>;
  release: () => Promise<void>;
  active: () => Promise<boolean>;
  bindProvider: (providerIdentity: string) => Promise<boolean>;
} {
  let lease: SpawnClaimLease | null = null;
  return {
    claim: async () => {
      lease = await acquireSpawnClaimAtomic(env, jobId);
      return lease !== null;
    },
    release: async () => {
      await releaseSpawnClaimAtomic(env, jobId, lease);
      lease = null;
    },
    active: async () => {
      if (!lease || !env.CONCURRENCY_SLOTS) return false;
      try { return await concurrencySlots(env).markSpawnClaimActive(jobId, lease.generation, lease.ownerToken); }
      catch { return false; }
    },
    bindProvider: async (providerIdentity: string) => {
      if (!lease || !env.CONCURRENCY_SLOTS) return false;
      try { return await concurrencySlots(env).bindSpawnClaimProvider(jobId, lease.generation, lease.ownerToken, providerIdentity); }
      catch { return false; }
    },
  };
}

function retryEpochAuthority(env: Env): RetryEpochAuthorityRpc | null {
  if (!env.CONCURRENCY_SLOTS) return null;
  return retryEpochClient(() => concurrencySlots(env));
}

async function recordRetryAttempt(
  env: Env,
  jobId: string,
  epochId: string,
  legacyFloor: number,
): Promise<{ attempts: number; recorded: boolean } | null> {
  const authority = retryEpochAuthority(env);
  if (!authority) return null;
  try {
    return await authority.record(jobId, epochId, legacyFloor);
  } catch (error) {
    logEvent("error", "retry_epoch_authority_failed", { jobId, error: (error as Error).message });
    return null;
  }
}

async function readRetryAttempts(env: Env, jobId: string): Promise<number | null> {
  const authority = retryEpochAuthority(env);
  if (!authority) return null;
  try {
    return await authority.read(jobId);
  } catch (error) {
    logEvent("error", "retry_epoch_authority_read_failed", { jobId, error: (error as Error).message });
    return null;
  }
}

async function retryOwnerEpochId(effectId: string, owner: string, epoch: number): Promise<string> {
  return sha256Hex(JSON.stringify({ effect_id: effectId, owner, epoch }));
}

// Best-effort release of a spawn's concurrency slot (by globally-unique jobId).
// Fully guarded: swallows BOTH a synchronous throw (an unbound binding in a
// partial/test env) AND an async DO error — a missed release self-heals at the
// slot TTL, so a release failure must NEVER break the webhook / spawn-fail path.
async function releaseConcurrencySlot(env: Env, jobId: string): Promise<boolean> {
  try {
    await concurrencySlots(env).release(jobId);
    return true;
  } catch (e) {
    logEvent("error", "concurrency_slot_release_failed", { jobId, error: (e as Error).message });
    return false;
  }
}

// This legacy KV-only view cannot prove provider ownership or serialize against
// a replacement claim. Keep it diagnostic until provider cancellation and a
// transactional generation authority exist (F007/T3-W16); it must never mutate
// a claim or release capacity based on absence from a separate KV key.
const STALE_CLAIM_DIAGNOSTIC_LIMIT = 32;

/** Observe old claims; authoritative cancellation remains deferred. */
export async function reapStaleSpawnClaims(env: Env, nowMs = Date.now()): Promise<number> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) return 0;
  let listed: { keys: { name: string }[] };
  try {
    listed = await kv.list({ prefix: "spawn:" });
  } catch (e) {
    logEvent("error", "stale_spawn_claim_list_failed", { error: (e as Error).message });
    return 0;
  }
  let diagnosed = 0;
  for (const { name } of listed.keys) {
    const jobId = name.slice("spawn:".length);
    if (!jobId) continue;
    let raw: string | null;
    try {
      raw = await kv.get(name);
    } catch {
      continue;
    }
    const age = spawnClaimAgeMs(raw, nowMs);
    if (age === null || age < SPAWN_CLAIM_TTL_S * 1000) continue;
    let handle: string | null;
    try {
      handle = await kv.get(jobHandleKey(jobId));
    } catch {
      continue;
    }
    if (diagnosed < STALE_CLAIM_DIAGNOSTIC_LIMIT) {
      logEvent("error", "stale_spawn_claim_candidate", {
        jobId,
        ageMs: age,
        durableHandlePresent: Boolean(handle),
        authority: "deferred_provider_cancellation",
      });
      diagnosed++;
    }
  }
  return 0;
}

// Atomically acquire a concurrency slot for THIS spawn — warm OR cold:
//   • warm (server-derived tenant + entitlement): key = the tenant, perKeyCap =
//     min(entitlement, FLEET) so a tenant never exceeds what it bought NOR the
//     physical fleet.
//   • cold (no derived tenant): key = `repo:<repo>`, perKeyCap = COLD_REPO_CAP —
//     the old KV path skipped cold spawns entirely (unlimited runners); they are
//     now capped per-repo AND under the same global FLEET cap.
// A clean cap refusal stops the attempt. A transient slot-authority failure
// uses the bounded ContainmentDO budget so a short hiccup does not block
// legitimate work, while a sustained outage admits at most five starts per
// rolling minute and an unavailable budget refuses closed.
export async function acquireConcurrencySlot(
  env: Env,
  jobId: string,
  mint: Pick<ContainerEnvResult, "tenant" | "maxConcurrency">,
  repo: string,
  preparationId?: string,
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
      preparationId,
    );
  } catch (e) {
    // The budget is authoritative: an absent, unreadable, or failed
    // ContainmentDO transaction refuses rather than bypassing the bound.
    let verdict: { admitted: boolean; reason?: string };
    try {
      verdict = env.CONTAINMENT
        ? await containmentAuthority(env).spendAdmissionBudget()
        : { admitted: false, reason: "slot_failopen_budget_unreadable" };
    } catch {
      verdict = { admitted: false, reason: "slot_failopen_budget_unreadable" };
    }
    logEvent("error", "concurrency_slot_acquire_error_failopen", {
      jobId,
      key,
      error: (e as Error).message,
      admitted: verdict.admitted,
      ...(verdict.reason ? { reason: verdict.reason } : {}),
    });
    return verdict;
  }
}

// Prepare every mutable prerequisite before a canonical owner record reaches
// DRIVING. A returned preparation retains its slot for the eventual provider
// attempt; known preparation failures clean up their own slot and credential.
async function prepareSpawn(
  env: Env,
  opts: ContainmentDriveOpts,
): Promise<ContainerEnvResult> {
  const { jobId, repo, installationId, labels } = opts;
  // A deleted App installation is a durable terminal fence.  Check at the last
  // point before authorization/mint too: an event may have entered an inbox
  // before its deletion delivery was processed.
  if (installationId && await installationIsTombstoned(env, installationId)) {
    throw new SpawnRefusedError("installation_deleted");
  }
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
  // A configured repository PAT supplements the server-owned installation
  // identity. Both reach authorization and mint, which must derive the same
  // tenant (ADR-0013). Missing configured credentials refuse preparation.
  // Reservation-contained re-drives explicitly use their supplied installation
  // only. They never let a first-party Option-C mapping convert an unmapped cold
  // candidate (or a stored external orphan) into a different credential source.
  const patSecretName = opts.credential_source === "installation-only"
    ? undefined
    : tenantPatSecretForRepo(env.REPO_TENANT_PAT_MAP, repo);
  const acquiringPat = patSecretName
    ? (env as unknown as Record<string, string | undefined>)[patSecretName]
    : undefined;
  if (patSecretName && (typeof acquiringPat !== "string" || !acquiringPat.length || acquiringPat.trim() !== acquiringPat)) {
    throw new RunnerAuthorizationError();
  }
  if (patSecretName && acquiringPat) {
    logEvent("info", "mint_option_c_pat_dispatch", { jobId, repo, patSecret: patSecretName });
  }
  const preparationId = crypto.randomUUID();
  const params = { jobId, repoFullName: repo, installationId, acquiringPat, computeReservationId: preparationId };
  const authorized = await authorizeRunner(env, params);
  let computeOwned = false;
  const releasePreparation = async () => {
    if (computeOwned) {
      try { await containmentAuthority(env).abandonUnusedCompute(preparationId); }
      catch { logEvent("error", "compute_cleanup_pending", { jobId }); }
    }
    try { await concurrencySlots(env).releasePreparation(jobId, preparationId); }
    catch { logEvent("error", "preparation_slot_release_pending", { jobId, preparationId }); }
  };
  const slot = await acquireConcurrencySlot(env, jobId, authorized, repo, preparationId);
  if (!slot.admitted) {
    await bumpMetrics(env, "spawn_at_ceiling");
    logEvent("info", "spawn_at_ceiling", { jobId, repo, tenant: authorized.tenant, reason: slot.reason });
    throw new SpawnRefusedError(slot.reason ?? "unknown");
  }
  let mint: ContainerEnvResult | undefined;
  try {
    if (authorized.computeGrant) {
      computeOwned = true;
      await containmentAuthority(env).prepareCompute({ token: authorized.computeGrant, reservationId: preparationId,
        tenantId: authorized.tenant, workloadKind: "spawn_worker_runner", workloadId: jobId, vcpuCount: 4, maximumWallMs: 28_800_000 });
    }
    mint = await buildContainerEnv(env, { ...params, credentialOperationId: preparationId }, env0);
    if (mint.patId && mint.tenant) {
      await containmentAuthority(env).registerCredential({ jobId, tenant: mint.tenant, patId: mint.patId, lifecycleGeneration: mint.lifecycleGeneration });
    }
  } catch (error) {
    // Until registration succeeds, the issuer's unadopted operation owns
    // revocation. Close local redemption independently of that remote recovery.
    if (mint?.patId && mint.tenant) {
      try {
        await env.CRED_STASH.get(env.CRED_STASH.idFromName(runnerCredentialLeaseId(jobId, mint.tenant, mint.patId))).wipe();
      } catch { logEvent("error", "preparation_stash_wipe_pending", { jobId }); }
    }
    await releasePreparation();
    throw error;
  }
  if (mint.authz !== "ok" || mint.tenant !== authorized.tenant
    || mint.maxConcurrency !== authorized.maxConcurrency || mint.maxVcpuH !== authorized.maxVcpuH) {
    if (mint.patId && mint.tenant) {
      await revokeIssuedCredential(env, containmentAuthority(env), { jobId, tenant: mint.tenant, patId: mint.patId, lifecycleGeneration: mint.lifecycleGeneration });
    }
    await releasePreparation();
    await bumpMetrics(env, "spawn_forbidden");
    logEvent("error", "mint_forbidden", { jobId, repo, reason: "authorization_unavailable_or_changed" });
    throw new RunnerAuthorizationError();
  }
  // F2 (W3): register the revoke-key jobId->patId at MINT time — BEFORE the spawn.
  // Previously it was written only AFTER a successful container start (spawnRunner),
  // so a start failure orphaned the minted cas:rw PAT to its 2h TTL (and the 60s
  // reconciler re-minted a fresh orphan each tick — 3-lens audit F2/Lens A). Writing
  // it here lets the spawn-failure catch below revoke it immediately.
  if (mint.patId) {
    if (!mint.tenant) throw new Error("credential authority requires server-derived tenant");
    if (env.RUNNER_JOB_PATS) await env.RUNNER_JOB_PATS.put(jobId, mint.patId, { expirationTtl: JOB_PAT_TTL_S }).catch((e) =>
      logEvent("error", "kv_put_job_pat_failed", { jobId, error: (e as Error).message }),
    );
  }
  // Required immutable attribution precedes JIT/container effects. Any
  // issued credential is already registered in the independent cleanup authority.
  if (mint.tenant) {
    try {
      const authorityStore = jobAttributionStore(env);
      if (!authorityStore) throw new Error("durable job attribution authority unavailable");
      await persistJobAttribution(authorityStore, { jobId, tenant: mint.tenant });
      if (!mint.patId) throw new RunnerAuthorizationError();
      // The issuer retires its timeout obligation only after durable local
      // credential ownership and attribution exist, before any provider effect.
      await adoptIssuedRunnerCredential(env, preparationId, mint.patId);
    } catch (e) {
      if (mint.patId) await revokeIssuedCredential(env, containmentAuthority(env), { jobId, tenant: mint.tenant, patId: mint.patId, lifecycleGeneration: mint.lifecycleGeneration }).catch(() => {});
      await releasePreparation();
      throw e;
    }
  }
  if (computeOwned) mint.computeReservationId = preparationId;
  mint.preparationId = preparationId;
  return mint;
}

/** An invocation that never reached provider dispatch owns only its preparation. */
async function abandonPreparedSpawn(
  env: Env, authority: ReturnType<typeof containmentAuthority>, jobId: string,
  prepared: ContainerEnvResult | undefined,
): Promise<void> {
  if (!prepared) return;
  if (prepared.computeReservationId) {
    try { await authority.abandonUnusedCompute(prepared.computeReservationId); }
    catch { logEvent("error", "compute_cleanup_pending", { jobId }); }
  }
  if (prepared.preparationId) {
    try { await concurrencySlots(env).releasePreparation(jobId, prepared.preparationId); }
    catch { logEvent("error", "preparation_slot_release_pending", { jobId, preparationId: prepared.preparationId }); }
  }
  if (prepared.patId && prepared.tenant) {
    await revokeIssuedCredential(env, authority, { jobId, tenant: prepared.tenant, patId: prepared.patId, lifecycleGeneration: prepared.lifecycleGeneration });
  }
}

// The spawn drive shared by the webhook path AND the re-drive reconciler:
// Server authorization → per-tenant concurrency → env-0 mint → GitHub JIT →
// spawn. Canonical callers pass their pre-claim preparation so DRIVING means a
// provider effect may actually follow; legacy callers prepare just in time.
async function driveSpawn(
  env: Env,
  opts: ContainmentDriveOpts,
  prepared?: ContainerEnvResult,
): Promise<ProviderDriveReceipt | void> {
  const { jobId } = opts;
  const containmentIdentity = isContainmentDrive(opts)
    ? normalizeRedriveIdentity(opts.repo, opts.jobId)
    : null;
  if (isContainmentDrive(opts)) {
    const observedResourceId = containmentIdentity
      ? `job:${containmentIdentity.repo}/${containmentIdentity.job_id}`
      : null;
    // The receipt identity is independently derived from the actual request
    // coordinates. It must agree with the durable canonical binding before a
    // provider effect is attempted; opts.repo itself remains raw for mint.
    if (!observedResourceId || observedResourceId !== opts.effect_binding.resource_id) {
      throw new Error("containment resource identity mismatch");
    }
  }
  const mint = prepared ?? await prepareSpawn(env, opts);
  // Authorized ⇒ spawn. The GitHub JIT is minted per container-start ATTEMPT
  // inside spawnRunner (see the ghost-container note on `startWithRetry`), not
  // once here — a single-use registration shared across retries is what let a
  // superseded attempt take the registration the surviving box needed.
  try {
    const spawned = await spawnRunner(env, jobId, mint, opts);
    await bumpMetrics(env, "runner_spawned");
    // The container started — but a started container is NOT a placed job. Record
    // the placement as PROVISIONAL so the reconciler can notice if this box never
    // comes online and claims the job; confirmation clears it. This is the ONLY
    // thing standing between "the box didn't show up" and a job that hangs `queued`
    // until GitHub cancels it 24h later, because `workflow_job.queued` is never
    // redelivered and nothing else reports the failure.
    await recordPlacement(env, opts);
    if (isContainmentDrive(opts)) {
      await bindContainmentPlacement(env, opts);
      await writeContainmentResultEvidence(env, opts, spawned.attempt);
    }
    return {
      resource_id: containmentIdentity
        ? `job:${containmentIdentity.repo}/${containmentIdentity.job_id}`
        : `job:${opts.repo}/${opts.jobId}`,
      receipt_id: spawned.handle,
      provider_signature: spawned.runnerName,
    };
  } catch (e) {
    if (mint.computeReservationId) {
      try { await containmentAuthority(env).abandonUnusedCompute(mint.computeReservationId); }
      catch { logEvent("error", "compute_cleanup_pending", { jobId }); }
    }
    // A failure after the provider start has an ActiveSpawnAttempt.  Its exact
    // handle must be confirmed down before this generation's preparation/claim/
    // slot can be released; a replay must therefore remain deduped even after
    // every KV projection expires.  Pre-start failures have no record and retain
    // the historical guarded release behaviour.
    const recovered = await recoverActiveSpawnAttempt(env, jobId);
    if (!recovered) {
      let outstanding = false;
      try { outstanding = Boolean(await concurrencySlots(env).readActiveAttempt(jobId)); } catch { outstanding = true; }
      if (!outstanding) await releaseConcurrencySlot(env, jobId);
    }
    // Revoke this exact issued credential after the attempt fails. The durable
    // authority retains a retry obligation if remote revoke or local wipe fails;
    // a later attempt's credential and the job's admission remain independent.
    if (mint.patId && mint.tenant) await revokeIssuedCredential(env, containmentAuthority(env), { jobId, tenant: mint.tenant, patId: mint.patId, lifecycleGeneration: mint.lifecycleGeneration });
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
  opts: {
    jobId: string;
    repo: string;
    installationId: string;
    labels: string[];
    effect_id?: string;
    failure_class?: "edge_proxy_403" | "authz_403";
  },
): Promise<void> {
  if (!env.RUNNER_JOB_PATS || !opts.installationId) return; // cold ⇒ not warm-recoverable
  try {
    const key = orphanKey(opts.jobId);
    if (await env.RUNNER_JOB_PATS.get(key)) return; // FIRST-failure record only (don't clobber/bump)
    // Commit the durable initial epoch before exposing a retryable orphan. A
    // missing or failed DO is a refusal: the failed spawn remains failed, with
    // no KV retry side effect to suggest that recovery is available.
    const initial = await recordRetryAttempt(env, opts.jobId, "initial", 0);
    if (!initial) return;
    const rec = {
      repo: opts.repo,
      installationId: opts.installationId,
      labels: opts.labels,
      attempts: initial.attempts,
      // Stamped once, at first record. Every later write-back preserves it so the
      // refusal wait is bounded by an ABSOLUTE window (orphanRefusalStep) rather
      // than by a TTL that would reset on each re-put.
      firstRecordedMs: Date.now(),
      ...(opts.effect_id ? { effect_id: opts.effect_id } : {}),
      ...(opts.failure_class ? { failure_class: opts.failure_class } : {}),
    } as OrphanRecord;
    await env.RUNNER_JOB_PATS.put(key, JSON.stringify(rec), { expirationTtl: ORPHAN_TTL_S });
    logEvent("info", "orphan_recorded", { jobId: opts.jobId, repo: opts.repo });
  } catch (e) {
    // Best-effort: never break the (already-failed) spawn path on a KV hiccup.
    logEvent("error", "orphan_record_failed", { jobId: opts.jobId, error: (e as Error).message });
  }
}

// Record a spawn that SUCCEEDED as provisionally placed: a container was started
// for this job, but nothing has yet confirmed that a runner came online and claimed
// it. See the placement-confirmation block in lib.ts for why a started container is
// not proof of placement (measured: 11 jobs lost in exactly this state).
//
// Writes the SAME `orphan:<jobId>` record the failure path uses, plus `placedMs`.
// Reusing one record is deliberate — it keeps ONE lifecycle and ONE set of bounds
// per job, so a job that is spawned, comes back unconfirmed, is re-driven and fails
// again cannot escape `MAX_ORPHAN_ATTEMPTS` by laundering itself through the
// success path.
//
// Preserves `attempts` and `firstRecordedMs` from any existing record: a job that
// has already burned two attempts does not get a fresh budget by being re-spawned,
// and the absolute refusal window keeps running from when the trouble STARTED.
//
// Best-effort: a KV failure here costs at most one unrecovered job — exactly the
// behaviour before this change, never worse — so it must not fail the spawn.
export async function recordPlacement(
  env: Env,
  opts: ContainmentDriveOpts,
): Promise<void> {
  // Legacy cold spawns retain their historical no-placement-record behavior:
  // re-drive cannot safely authorize them. A contained event is different: its
  // DO-owned proof needs the actual provisional-placement bytes even when the
  // webhook was unmapped, otherwise disarmed cold intake wedges at RESULT.
  if (!env.RUNNER_JOB_PATS) {
    if (isContainmentDrive(opts)) throw new Error("containment placement requires RUNNER_JOB_PATS");
    return;
  }
  if (!opts.installationId && !isContainmentDrive(opts)) return;
  const kv = env.RUNNER_JOB_PATS;
  try {
    const key = orphanKey(opts.jobId);
    const raw = await kv.get(key);
    const prior = raw ? (JSON.parse(raw) as OrphanRecord) : null;
    const now = Date.now();
    const rec = {
      repo: opts.repo,
      installationId: opts.installationId,
      labels: opts.labels,
      attempts: prior?.attempts ?? 0,
      firstRecordedMs: prior?.firstRecordedMs ?? now,
      placedMs: now,
      ...(opts.effect_id ? { effect_id: opts.effect_id } : {}),
    } as OrphanRecord;
    const encoded = JSON.stringify(rec);
    if (isContainmentDrive(opts)) {
      // The exact placement bytes exist before the mutable orphan write. Bind
      // them immutably first; if the legacy source write/read-back then fails,
      // RESULT is never written and the issued permit remains safely unresolved.
      await writeContainmentEvidence(env, opts, "placement", key, encoded);
      await kv.put(key, encoded, { expirationTtl: ORPHAN_TTL_S });
      if (await kv.get(key) !== encoded) throw new Error("containment placement source was not durably written");
      return;
    }
    await kv.put(key, encoded, { expirationTtl: ORPHAN_TTL_S });
  } catch (e) {
    if (isContainmentDrive(opts)) throw e;
    logEvent("error", "placement_record_failed", {
      jobId: opts.jobId,
      error: (e as Error).message,
    });
  }
}

// Drop the provisional placement record — the job is confirmed no longer waiting on
// us. Called from `workflow_job.completed` (free confirmation, no API call) and from
// the reconciler when GitHub reports the job as placed.
async function clearPlacementRecord(env: Env, jobId: string): Promise<void> {
  if (!env.RUNNER_JOB_PATS) return;
  await env.RUNNER_JOB_PATS.delete(orphanKey(jobId)).catch(() => {
    /* best-effort: the record TTL-expires on its own */
  });
}

// Ask GitHub whether ONE job is still waiting for a runner. Used only for a
// placement that is already past its grace window, so this is normally zero calls
// per tick — and at most one per genuinely-stuck job.
//
// Returns the raw job shape for `jobPlacementVerdict` to judge; `null` on ANY
// failure, which that function maps to "unknown" ⇒ leave the record alone. Never
// throws: an unreachable GitHub must not stop the rest of the reconciler tick.
//
// AUTH (2026-08-24): same discipline as `fetchJobObservation` below — when the
// caller holds an installation id we mint a PER-INSTALLATION token via
// `mintJitAuthToken`. The static GITHUB_MINT_TOKEN only has rights on HuGR-Labs
// repos, so authenticating with it for a CUSTOMER repo 403/404s every call ⇒
// verdict "unknown" ⇒ the record sits out ORPHAN_TTL_S with the job still queued:
// the warm dead-letter recovery was dead exactly for the customers it was built
// for. The first-party token stays as the FALLBACK for records with no
// installation id (cold spawns), which is the path that works today and needs no
// App credential.
async function fetchJobPlacement(
  env: Env,
  repo: string,
  jobId: string,
  installationId: string,
): Promise<{ status?: string; runner_id?: number | null } | null> {
  if (!installationId && !env.GITHUB_MINT_TOKEN) return null;
  let authToken = env.GITHUB_MINT_TOKEN ?? "";
  if (installationId) {
    // A mint failure here collapses to `null` by the catch below — "unknown",
    // never an error that stops the tick (same contract as fetchJobObservation).
    authToken = (await mintJitAuthToken(env, installationId)) || authToken;
  }
  if (!authToken) return null;
  try {
    const r = await fetch(`https://api.github.com/repos/${repo}/actions/jobs/${jobId}`, {
      headers: {
        authorization: `Bearer ${authToken}`,
        accept: "application/vnd.github+json",
        "user-agent": "corelink-spawn-worker",
      },
    });
    if (!r.ok) return null;
    return (await r.json()) as { status?: string; runner_id?: number | null };
  } catch {
    return null;
  }
}

// Ask GitHub about ONE runner: `GET /repos/{owner}/{repo}/actions/runners/{id}`.
//
// Documented response fields include `status` (required, string) and `busy`
// (required, boolean) — see docs.github.com/en/rest/actions/self-hosted-runners.
// `status` is `"online"` / `"offline"`; GitHub's runner UI names the three states
// Idle ("connected to GitHub and is ready to execute jobs"), Active ("currently
// executing a job") and Offline ("not connected to GitHub"). `busy` is what
// separates Idle from Active.
//
// AUTH: the SAME credential the JIT was minted with (`mintJitAuthToken` — the
// per-installation App token, else the first-party GITHUB_MINT_TOKEN). This read
// is strictly weaker than what we already do on this repo: we CREATE runner
// registrations (`generate-jitconfig`) and DELETE them with that credential, so a
// GET of one of them needs no permission we do not already hold. No credential at
// all ⇒ `null` ⇒ "unknown" ⇒ the box keeps being renewed.
//
// Never throws, and every non-200 that is not a 404 is reported as-is for
// `runnerActivityVerdict` to resolve to "unknown". A rate-limited or unreachable
// GitHub must make us MORE conservative, not less.
async function fetchRunnerActivity(
  env: Env,
  repo: string,
  runnerId: number,
  installationId: string,
): Promise<RunnerObservation | null> {
  try {
    const authToken = await mintJitAuthToken(env, installationId);
    if (!authToken) return null;
    const r = await fetch(`https://api.github.com/repos/${repo}/actions/runners/${runnerId}`, {
      headers: {
        authorization: `Bearer ${authToken}`,
        accept: "application/vnd.github+json",
        "user-agent": "corelink-spawn-worker",
      },
    });
    if (r.status !== 200) return { httpStatus: r.status, runner: null };
    return {
      httpStatus: 200,
      runner: (await r.json()) as { status?: string; busy?: boolean },
    };
  } catch {
    return null; // unreachable / unparseable ⇒ unknown ⇒ keep renewing
  }
}

// A hard ceiling on GitHub reads per tick. The fleet cap is `max_instances: 20`,
// so this never binds in normal operation — it bounds the PATHOLOGICAL case (a KV
// full of stale bindings) so the sweep can never become the thing that exhausts
// the installation's REST budget and breaks spawning. Bindings past the cap are
// treated as unverifiable, i.e. RENEWED: running out of API budget must not start
// reclaiming boxes we can no longer ask about.
const KEEPALIVE_MAX_VERIFY_PER_TICK = 40;

// The same hard ceiling, for the stranded-job sweep, and for the same reason: a
// KV full of stale bindings must never let a backstop exhaust the installation's
// REST budget and break SPAWNING, which is the thing customers actually pay for.
// Bindings past the cap are simply not examined this tick; the sweep runs every
// minute and the binding lives 2 h, so nothing is lost by deferring one.
const STRAND_MAX_VERIFY_PER_TICK = 40;

// The same hard ceiling, for `reapStaleBoxes`, and now it is LOAD-BEARING in a way
// it was not before. Until this change the reaper refused to verify any record with
// an empty `inst`, so the population that reached the verify call was only the WARM
// spawns. Dropping that requirement (which was wrong — see the note at the guard)
// widens the population to EVERY `sbox:` record under 24 h, and the 2026-08-31
// measurement of that population is 279 records. Uncapped, one tick of this sweep
// would fire 279 GETs at api.github.com in a burst.
//
// ⚠️ 40 IS NOT CONSERVATISM — IT IS A REGIME BOUNDARY. The naive arithmetic ("279
// per minute ≈ 15 000/h vs the 5 000/h budget") uses the right number against the
// wrong limit: 5 000/h is the CORE bucket, drained over an hour. What a burst meets
// FIRST is the SECONDARY (concurrency/abuse) limit, which trips on RATE, not on
// accumulated volume. Measured in this campaign, one call apart:
//
//   gh api rate_limit         → remaining=4935/5000
//   gh api repos/HuGR-Labs/…  → 403 "API rate limit exceeded"
//
// 65 calls inside one second were enough. Note also that `/rate_limit` does NOT
// report the secondary limit, so anyone diagnosing this sees plenty of headroom and
// goes looking for the cause somewhere else. Removing the ceiling therefore does not
// raise consumption by 3× — it SWITCHES REGIME, from a bounded drip to a burst that
// can 403 the whole installation and break SPAWNING, which is the thing customers
// pay for. Records past the cap are simply not examined this tick (no escalation,
// nothing destroyed); the sweep runs every minute and the record lives 24 h.
const REAP_MAX_VERIFY_PER_TICK = 40;

// ── Keep-alive sweep (2026-08-02; verified against GitHub 2026-08-03) ────────
//
// Why this exists: `RunnerContainer.sleepAfter` was acting as a hard 15-minute cap
// on job DURATION, not as an idle timeout — see the long note on the class. The
// SDK can only see activity that arrives through `containerFetch`, and nothing
// ever dials INTO a runner box, so it concluded every box was idle from the moment
// it booted. The cron supplies the activity signal the SDK cannot observe.
//
// WHAT THE FIRST VERSION GOT WRONG. It renewed every box that still had an
// `rhandle:` binding, on the stated theory that such a box "has a job on it". That
// binding is written at SPAWN and lives `JOB_PAT_TTL_S` (2 h). A box that boots and
// never registers with GitHub never produces a completion event naming it, so
// nothing ever drops its binding — and the sweep renewed it once a minute for two
// hours, holding a standard-4 out of a fleet of 20 and turning the 15-minute idle
// window into a no-op. Plausibly a larger slot sink than the ghost containers
// fixed in #446.
//
// WHAT REPLACES IT. A binding proves a box was STARTED for a job. Only GitHub can
// say whether that box is WORKING, so we ask it about that box's own runner id.
// Only `busy` renews.
//
// ⛔ WHY IT IS KEYED ON THE RUNNER, NOT THE JOB. "Job A is still queued" does NOT
// imply "the box we started for A is idle": `generate-jitconfig` binds a runner to
// a repo + label set and to nothing else, so GitHub assigns queued jobs to idle
// runners by LABEL MATCH and the job→box mapping is a permutation. Teardown keyed
// on the spawn's jobId is what SIGKILLed five live customer jobs on 2026-08-02 (see
// the note above `RUNNER_HANDLE_PREFIX`). This sweep never asks about a job. It
// asks GitHub about one specific runner id, and it acts only on that runner's own
// reported state.
//
// FAIL SAFE, NOT CLEAN — the deliberate asymmetry. An inconclusive check (no
// credential, an API error, a rate limit, a body we do not recognise, a legacy
// binding with no runner id, or a tick that hit the verification cap) KEEPS the box
// renewed. Leaking a container slot is recoverable — `sleepAfter` still reaps it
// eventually and only the fleet cap suffers. Killing a running customer job is not.
// `keepalive_renewed_unverifiable` is the meter on how much we are paying for that
// choice; if it dominates, the fix is better verification, never a cheaper default.
//
// AND STOPPING IS NOT KILLING. Renewing sets the deadline to now + 15m, so a box we
// stop renewing still has ~14 minutes, and the sweep re-checks it every minute of
// them: it must be continuously verified-not-busy for the whole window to actually
// sleep, and one busy observation anywhere in that span restores the full window.
// A 15:1 ratio between the idle window and the poll interval is what makes acting
// on a single observation safe.
//
// Best-effort throughout: this is a liveness backstop, never a gate. Every failure
// is swallowed so a KV hiccup or a dead handle can never break the cron (which also
// drives orphan recovery and billing).
//
// `verify` is injected so the decision is testable without reaching GitHub.
export async function keepAliveLiveRunners(
  env: Env,
  verify: (
    env: Env,
    repo: string,
    runnerId: number,
    installationId: string,
  ) => Promise<RunnerObservation | null> = fetchRunnerActivity,
): Promise<number> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) return 0;
  let listed: { keys: { name: string }[] };
  try {
    listed = await kv.list({ prefix: RUNNER_HANDLE_PREFIX });
  } catch (e) {
    logEvent("error", "keepalive_list_failed", { error: (e as Error).message });
    return 0;
  }
  let renewed = 0;
  let busyCount = 0;
  let idleCount = 0;
  let unverifiable = 0;
  let verifications = 0;
  for (const { name } of listed.keys) {
    const runnerName = name.slice(RUNNER_HANDLE_PREFIX.length);
    let binding: RunnerBinding | null = null;
    try {
      binding = parseRunnerBinding(await kv.get(name));
    } catch {
      continue; // transient KV read miss — next tick retries
    }
    if (!binding) continue; // torn down between list and get, or unreadable value

    // Can we ask? A legacy bare-handle binding (written before this change) has no
    // runner id, and a cold spawn has no installation — both are unverifiable and
    // therefore renewed. So is anything past the per-tick verification cap.
    const verifiable =
      typeof binding.rid === "number" && !!binding.repo && !!binding.inst &&
      verifications < KEEPALIVE_MAX_VERIFY_PER_TICK;

    let activity: "busy" | "idle" | "unknown" = "unknown";
    if (verifiable) {
      verifications++;
      // `fetchRunnerActivity` swallows its own errors, but this must hold for ANY
      // verifier: a throw here would otherwise abandon the sweep mid-list and stop
      // renewing every box after this one — a fail-unsafe hidden inside a loop.
      // Caught per box, resolved to "unknown", which renews.
      try {
        activity = runnerActivityVerdict(
          await verify(env, binding.repo!, binding.rid!, binding.inst!),
        );
      } catch (e) {
        logEvent("error", "keepalive_verify_threw", { runnerName, error: (e as Error).message });
      }
    }

    if (activity === "idle") {
      // GitHub says this runner is not executing anything (or has forgotten it).
      // Stop renewing and let `sleepAfter` do its job. Nothing is destroyed here.
      idleCount++;
      logEvent("info", "keepalive_unrenewed_idle", { runnerName, runnerId: binding.rid });
      continue;
    }
    if (activity === "unknown") unverifiable++;
    else busyCount++;

    if (binding.jid) {
      try {
        if (!(await concurrencySlots(env).renew(binding.jid, SLOT_TTL_S * 1000))) {
          logEvent("error", "keepalive_slot_missing", { jobId: binding.jid, runnerName });
        }
      } catch {
        logEvent("error", "keepalive_slot_renewal_failed", { jobId: binding.jid, runnerName });
      }
    }

    try {
      await getContainer(env.RUNNER_CONTAINER, binding.h).keepAlive();
      renewed++;
    } catch (e) {
      // A dead/destroyed handle throws here. Not worth alarming on — the binding
      // will TTL out — but worth seeing if it becomes common.
      logEvent("info", "keepalive_skipped", {
        runnerName,
        error: (e as Error).message,
      });
    }
  }
  if (renewed > 0) {
    logEvent("info", "keepalive_renewed", {
      count: renewed,
      busy: busyCount,
      unverifiable,
    });
  }
  await bumpMetrics(
    env,
    ...Array(busyCount).fill("keepalive_renewed_busy"),
    ...Array(idleCount).fill("keepalive_unrenewed_idle"),
    ...Array(unverifiable).fill("keepalive_renewed_unverifiable"),
  );
  return renewed;
}

// The `rhandle:` key list is followed across at most this many `list` pages
// (1000 keys per page). The fleet cap is 250 boxes, so one page always suffices
// in reality; the loop exists so a pathological KV can never TRUNCATE the fleet
// view into a falsely-idle verdict.
const FLEET_BUSY_MAX_LIST_PAGES = 10;

/**
 * The GET /internal/v1/fleet/busy body. `busy` is the number of runners GitHub
 * reports `busy: true`; `runners` names exactly those (name + `owner/repo`, no
 * other field); `checked` is how many `rhandle:` bindings were examined; and
 * `unverifiable` is how many produced no authoritative answer.
 *
 * ⛔ `unverifiable > 0` does NOT mean idle. See `fleetBusySnapshot`.
 */
export interface FleetBusySnapshot {
  busy: number;
  runners: { name: string; repo: string }[];
  checked: number;
  unverifiable: number;
}

// ── Fleet busy-read (2026-08-24) — the pre-roll deploy gate's authority ──────
//
// WHY THIS EXISTS. `deploy-spawn-worker.yml` refuses to roll the fleet while
// boxes are executing customer work (#496). It asked GitHub directly, which needs
// `administration: read` across every RECONCILER_REPOS repo — a permission the
// Actions `GITHUB_TOKEN` does not have and cannot be granted cross-repo, and
// GitHub exposes no API to mint a PAT. So the gate could never authenticate and
// every deploy refused.
//
// This Worker already holds the answer. It owns the GitHub App credential and
// already asks GitHub, per runner, whether that runner is `busy` — that is what
// `keepAliveLiveRunners` does every minute. Exposing the same question as a read
// costs the gate no GitHub permission at all.
//
// SAME AUTHORITY, SAME ENUMERATION, SAME CAP. This reuses the KV `rhandle:`
// binding list, `fetchRunnerActivity` (GET /repos/{owner}/{repo}/actions/runners/
// {id} under the per-installation App token) and `runnerActivityVerdict`. It
// deliberately does NOT introduce a second per-tick ceiling: bindings past
// KEEPALIVE_MAX_VERIFY_PER_TICK (40) are not asked about, exactly as in the sweep.
// `checked` reports how many bindings were examined, so a caller can see when the
// cap bound (checked > 40 with a matching floor of `unverifiable`).
//
// ⛔ THE FAIL-SAFE DIRECTION IS INVERTED RELATIVE TO THE SWEEP, ON PURPOSE.
// `keepAliveLiveRunners` resolves ignorance to "keep renewing" — leaking a slot is
// cheaper than killing a job. HERE ignorance must block a ROLL, which would kill
// exactly those jobs. So every runner whose state cannot be established — no
// runner id (legacy bare binding), no installation (cold spawn), an API error, a
// rate limit, an undocumented body, a KV read that failed, a truncated key list,
// or an unbound KV namespace — increments `unverifiable` and is NEVER counted as
// idle.
//
// THE CALLER'S CONTRACT, which is half of this function: the fleet is provably
// idle ONLY when `busy === 0 && unverifiable === 0`. `unverifiable > 0` means
// "cannot prove idle" and MUST be treated exactly like `busy > 0` — do not roll.
// scripts/ci/wait-for-idle-fleet.sh implements that, and `force=true` is the one
// deliberate override.
//
// WHAT IT MAY DISCLOSE. Runner names and `owner/repo` only. No tokens, no JIT
// config, no job payloads, no tenant identifiers, no DO handles — the gate needs
// a specific failure message, nothing more.
//
// `verify` is injected so the decision is testable without reaching GitHub.
export async function fleetBusySnapshot(
  env: Env,
  verify: (
    env: Env,
    repo: string,
    runnerId: number,
    installationId: string,
  ) => Promise<RunnerObservation | null> = fetchRunnerActivity,
): Promise<FleetBusySnapshot> {
  const runners: { name: string; repo: string }[] = [];
  let checked = 0;
  let unverifiable = 0;
  let verifications = 0;

  const kv = env.RUNNER_JOB_PATS;
  // No binding store ⇒ no way to enumerate the fleet ⇒ we cannot prove anything.
  // One unverifiable is enough to make the caller refuse; claiming idle here would
  // roll the fleet on the strength of a missing binding.
  if (!kv) return { busy: 0, runners, checked: 0, unverifiable: 1 };

  // The sweep reads a single `list` page. A truncated page would UNDER-count here,
  // which is the fail-unsafe direction, so this follows the cursor. The loop is
  // bounded; an unfinished list resolves to unverifiable rather than to idle.
  const keys: string[] = [];
  let cursor: string | undefined;
  let complete = false;
  for (let page = 0; page < FLEET_BUSY_MAX_LIST_PAGES; page++) {
    let listed: { keys: { name: string }[]; list_complete?: boolean; cursor?: string };
    try {
      listed = (await kv.list({ prefix: RUNNER_HANDLE_PREFIX, cursor })) as typeof listed;
    } catch (e) {
      logEvent("error", "fleet_busy_list_failed", { error: (e as Error).message });
      return { busy: 0, runners, checked: 0, unverifiable: 1 };
    }
    for (const k of listed.keys) keys.push(k.name);
    if (listed.list_complete !== false) {
      complete = true;
      break;
    }
    cursor = listed.cursor;
    // Truncated AND no cursor to continue from: we cannot finish the list, so we
    // leave `complete` false. Setting it true here would silently under-count the
    // fleet into a falsely-idle verdict — the fail-unsafe direction.
    if (!cursor) break;
  }
  if (!complete) unverifiable++; // the list did not finish ⇒ cannot prove idle

  for (const name of keys) {
    checked++;
    const runnerName = name.slice(RUNNER_HANDLE_PREFIX.length);
    let binding: RunnerBinding | null = null;
    let readFailed = false;
    try {
      binding = parseRunnerBinding(await kv.get(name));
    } catch {
      readFailed = true;
    }
    // A KV read that failed, or a value we cannot parse, is ignorance — and here
    // ignorance blocks. (The sweep may skip these; it is deciding whether to STOP
    // renewing, we are deciding whether to KILL.)
    if (readFailed || !binding) {
      unverifiable++;
      continue;
    }

    const verifiable =
      typeof binding.rid === "number" &&
      !!binding.repo &&
      !!binding.inst &&
      verifications < KEEPALIVE_MAX_VERIFY_PER_TICK;
    if (!verifiable) {
      unverifiable++;
      continue;
    }

    verifications++;
    let activity: RunnerActivity = "unknown";
    try {
      activity = runnerActivityVerdict(await verify(env, binding.repo!, binding.rid!, binding.inst!));
    } catch (e) {
      // A verifier that throws must not abandon the enumeration mid-list and
      // silently shrink the busy count — caught per box, resolved to unknown.
      logEvent("error", "fleet_busy_verify_threw", { runnerName, error: (e as Error).message });
    }
    if (activity === "busy") runners.push({ name: runnerName, repo: binding.repo! });
    else if (activity === "unknown") unverifiable++;
  }

  return { busy: runners.length, runners, checked, unverifiable };
}

// Ask GitHub about ONE job: `GET /repos/{owner}/{repo}/actions/jobs/{job_id}`.
//
// Distinct from `fetchJobPlacement` on purpose. That one serves the first-party
// placement reconciler, authenticates with the first-party `GITHUB_MINT_TOKEN`,
// and collapses every failure to `null`. The stranded sweep runs over CUSTOMER
// bindings, so it must use the SAME per-installation credential the JIT was minted
// with, and it must be able to tell a 404/403/429/5xx apart from a transport
// failure — that distinction is the entire authority argument (see
// `runnerGoneVerdict`). So it returns the literal status, and `null` ONLY when no
// answer was produced at all.
//
// AUTH: `mintJitAuthToken(env, installationId)` — strictly weaker than what we
// already hold on that repo (we CREATE and DELETE runner registrations on it).
// Never throws: an unreachable GitHub must not stop the tick.
async function fetchJobObservation(
  env: Env,
  repo: string,
  jobId: string,
  installationId: string,
): Promise<JobObservation | null> {
  try {
    const authToken = await mintJitAuthToken(env, installationId);
    if (!authToken) return null;
    const r = await fetch(`https://api.github.com/repos/${repo}/actions/jobs/${jobId}`, {
      headers: {
        authorization: `Bearer ${authToken}`,
        accept: "application/vnd.github+json",
        "user-agent": "corelink-spawn-worker",
      },
    });
    if (r.status !== 200) return { httpStatus: r.status, job: null };
    return {
      httpStatus: 200,
      job: (await r.json()) as JobObservation["job"],
    };
  } catch {
    return null; // unreachable / unparseable ⇒ unknown ⇒ conclude nothing
  }
}

// Write the TERMINAL dead-letter for a stranded job. Same `orphan:<jobId>` record
// and same `OrphanRecord` shape `recordOrphan` uses — extended, not duplicated,
// with `stranded`/`strandedRunner`. There is exactly ONE dead-letter format.
//
// It deliberately CLOBBERS a prior record for this job (unlike `recordOrphan`,
// which records only a first failure): a provisional `placedMs` record for a job
// we now know died in flight must not survive and drive a re-spawn. `attempts` and
// `firstRecordedMs` are preserved so the record keeps one lifecycle per job.
//
// `stranded` makes it terminal — `retryOrphanedSpawns` skips it. Re-driving
// in-flight work is a separate decision that has not been made.
async function recordStrandedJob(
  env: Env,
  opts: {
    jobId: string;
    repo: string;
    installationId: string;
    runnerName: string;
    nowMs: number;
  },
): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) return;
  try {
    const key = orphanKey(opts.jobId);
    const raw = await kv.get(key);
    const prior = raw ? (JSON.parse(raw) as OrphanRecord) : null;
    const rec: OrphanRecord = {
      repo: opts.repo,
      installationId: opts.installationId,
      labels: prior?.labels ?? [],
      attempts: prior?.attempts ?? 0,
      firstRecordedMs: prior?.firstRecordedMs ?? opts.nowMs,
      stranded: opts.nowMs,
      strandedRunner: opts.runnerName,
      // Explicitly NOT carried over: a job that died in flight is not a job
      // awaiting placement.
      placedMs: undefined,
    };
    await kv.put(key, JSON.stringify(rec), { expirationTtl: ORPHAN_TTL_S });
  } catch (e) {
    logEvent("error", "stranded_record_failed", {
      jobId: opts.jobId,
      error: (e as Error).message,
    });
  }
}

// ── Stranded in-flight jobs — the fifth sweep (2026-08-23) ───────────────────
//
// THE DEFECT. A runner box that dies MID-JOB is invisible to this Worker. The
// placement reconciler reads anything past `queued` as "placed" and drops the
// record; `listOrphanRunnerJobs` selects only `queued`; `recordOrphan` is written
// only from a SPAWN-time failure. So a box killed after a successful spawn enters
// no dead letter at all, and the first anyone hears of it is GitHub's own ~600 s
// timeout telling the CUSTOMER that "the self-hosted runner lost communication
// with the server". Meanwhile our accounting leaks for hours (the concurrency slot
// to SLOT_TTL_S, the per-job `cas:rw` PAT to its own TTL, the KV bindings to
// JOB_PAT_TTL_S) because every release is hung off the `completed` webhook that
// will never arrive.
//
// ⛔ WHAT THIS SWEEP IS NOT ALLOWED TO DO. It never stops, destroys, signals or
// tears down ANYTHING. There is no `destroy()`, no `stop()`, no teardown call in
// this function and there must never be one: on 2026-08-02 a teardown keyed on our
// own bookkeeping SIGKILLed five live customer boxes (see the note above
// `RUNNER_HANDLE_PREFIX`). Container termination stays where it already is — the
// idle window, `reapStaleBoxes`, and the DO alarm.
//
// WHAT IT MAY CONCLUDE FROM, AND ONLY FROM. GitHub's own answers, twice over:
//   1. a definitive 404 on the runner (`runnerGoneVerdict`) — the registration we
//      created no longer exists. A transport failure is `null`, a rate limit is
//      403/429, an outage is 5xx; none of them are 404 (see `runnerGoneVerdict`).
//   2. that job reported `in_progress` AND carrying OUR runner id
//      (`strandedJobVerdict`). GitHub confirming the job→box link is what makes
//      this different from the 2026-08-02 correlation: our binding names only the
//      job the box was STARTED for, and GitHub assigns by label match.
// The absence or staleness of one of our own KV records concludes NOTHING.
// Anything ambiguous ⇒ do nothing, try again next tick.
//
// WHAT IT MUTATES, on a confirmed strand and nothing else:
//   • a loud `console.error` (`job_stranded`) naming job, repo, runner and age;
//   • the `orphan:<jobId>` dead-letter, marked terminal (`stranded`) so nothing
//     re-drives it;
//   • the concurrency slot, released by jobId (idempotent, and it self-heals at
//     SLOT_TTL_S anyway — this just returns it ~40 min sooner);
//   • the per-job `cas:rw` PAT, revoked through the SAME path `completed` uses.
// All four are OUR bookkeeping. None of them touch the box.
//
// Both verifiers are injected so every branch is testable without a network.
export async function detectStrandedInFlightJobs(
  env: Env,
  nowMs: number,
  verifyRunner: (
    env: Env,
    repo: string,
    runnerId: number,
    installationId: string,
  ) => Promise<RunnerObservation | null> = fetchRunnerActivity,
  verifyJob: (
    env: Env,
    repo: string,
    jobId: string,
    installationId: string,
  ) => Promise<JobObservation | null> = fetchJobObservation,
): Promise<number> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) {
    // SAY SO. An unbound credential must not read as a quiet, healthy tick —
    // that ambiguity is exactly how a dead backstop stays dead for weeks.
    logEvent("info", "strand_sweep_skipped_unbound", { reason: "RUNNER_JOB_PATS unbound" });
    return 0;
  }
  let listed: { keys: { name: string }[] };
  try {
    listed = await kv.list({ prefix: RUNNER_HANDLE_PREFIX });
  } catch (e) {
    logEvent("error", "strand_list_failed", { error: (e as Error).message });
    return 0;
  }
  let stranded = 0;
  let verifications = 0;
  for (const { name } of listed.keys) {
    if (verifications >= STRAND_MAX_VERIFY_PER_TICK) break;
    const runnerName = name.slice(RUNNER_HANDLE_PREFIX.length);
    let binding: RunnerBinding | null = null;
    try {
      binding = parseRunnerBinding(await kv.get(name));
    } catch {
      continue; // transient KV read miss — next tick retries
    }
    // Unaskable: a legacy bare-handle binding, a cold spawn, or a binding written
    // before `jid` existed. Nothing to ask GitHub, so nothing to conclude.
    if (!binding || typeof binding.rid !== "number" || !binding.repo || !binding.inst || !binding.jid) {
      continue;
    }
    const { rid, repo, inst, jid } = binding;

    // Already classified on an earlier tick? Then the side effects below already
    // ran. Re-running them would be harmless (all idempotent) but would re-log the
    // alarm every minute for two hours, which trains people to ignore it.
    try {
      const priorRaw = await kv.get(orphanKey(jid));
      if (priorRaw && (JSON.parse(priorRaw) as OrphanRecord).stranded != null) continue;
    } catch {
      /* unreadable prior record ⇒ fall through and classify normally */
    }

    verifications++;
    // Every verifier call is caught per binding: a throw must not abandon the
    // sweep mid-list and silently skip every later box.
    let gone: "gone" | "present" | "unknown" = "unknown";
    try {
      gone = runnerGoneVerdict(await verifyRunner(env, repo, rid, inst));
    } catch (e) {
      logEvent("error", "strand_runner_verify_threw", { runnerName, error: (e as Error).message });
      continue;
    }
    // "present" ⇒ the registration is alive, the box is fine. "unknown" ⇒ GitHub
    // was unreachable / rate-limited / ambiguous ⇒ conclude NOTHING.
    if (gone !== "gone") continue;

    let verdict: "stranded" | "not_stranded" | "unknown" = "unknown";
    try {
      verdict = strandedJobVerdict(await verifyJob(env, repo, jid, inst), rid);
    } catch (e) {
      logEvent("error", "strand_job_verify_threw", { runnerName, jobId: jid, error: (e as Error).message });
      continue;
    }
    // `not_stranded` is the OVERWHELMINGLY common path: an ephemeral runner is
    // de-registered by GitHub the instant it finishes its one job, so a completed
    // job's runner is a 404 too. Ordinary completion — the webhook has it. Let the
    // binding expire normally; touch nothing.
    if (verdict !== "stranded") continue;

    // ── CONFIRMED STRANDED ────────────────────────────────────────────────────
    stranded++;
    const boundAgoMs = typeof binding.t === "number" ? Math.max(0, nowMs - binding.t) : null;
    logEvent("error", "job_stranded", {
      jobId: jid,
      repo,
      runnerName,
      runnerId: rid,
      boundAgoMs,
      note: "GitHub reports this job in_progress on a runner GitHub no longer knows about — the box died mid-job. Nothing was torn down; only our own accounting is released.",
    });
    await recordStrandedJob(env, {
      jobId: jid,
      repo,
      installationId: inst,
      runnerName,
      nowMs,
    });
    // Return the concurrency slot (idempotent; self-heals at SLOT_TTL_S anyway).
    await releaseConcurrencySlot(env, jid);
    // Revoke the per-job `cas:rw` PAT through the SAME path `workflow_job.completed`
    // uses — shrinking a live credential's window from its full TTL to now.
    let derivedTenant: string | undefined;
    try {
      derivedTenant = (await kv.get(jobTenantKey(jid))) ?? undefined;
    } catch {
      /* The durable credential authority supplies the exact tenant identity. */
    }
    await revokeCompletedJob(env, jid, derivedTenant);
  }
  if (stranded > 0) await bumpMetrics(env, ...Array(stranded).fill("job_stranded"));
  return stranded;
}

/**
 * Destroy boxes that have outlived any job they could plausibly be running.
 *
 * The keep-alive sweep can only RENEW; nothing in the fleet actively STOPS a
 * container, so termination rests entirely on the DO's `sleepAfter` alarm. When
 * that alarm does not fire — observed in prod 2026-08-23, three boxes `running`
 * for 10.2 h against a 15-minute window — the box burns until someone notices.
 * This is the second layer.
 *
 * ⚠️ The fail-safe here is the OPPOSITE of `keepAliveLiveRunners`, on purpose.
 * That sweep renews when it cannot verify, because renewing on ignorance only
 * wastes money. This one DESTROYS, so it must never act on ignorance: killing a
 * box that is in fact running a customer's job costs them the job. A box is
 * reaped ONLY on a definite "GitHub says this runner is not busy". Unverifiable
 * (missing runner id, missing repo, a GitHub error, a throw) ⇒ left alone and
 * retried next tick. A MISSING INSTALLATION is explicitly NOT in that list —
 * see the long note at the guard below.
 *
 * Returns the number of boxes destroyed.
 */
export async function reapStaleBoxes(
  env: Env,
  nowMs: number,
  verify: (
    env: Env,
    repo: string,
    runnerId: number,
    installationId: string,
  ) => Promise<RunnerObservation | null> = fetchRunnerActivity,
): Promise<number> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) return 0;
  let listed: { keys: { name: string }[] };
  try {
    listed = await kv.list({ prefix: SPAWNED_BOX_PREFIX });
  } catch (e) {
    logEvent("error", "reap_list_failed", { error: (e as Error).message });
    return 0;
  }
  let reaped = 0;
  let verifications = 0;
  let deferred = 0;
  for (const { name } of listed.keys) {
    const runnerName = name.slice(SPAWNED_BOX_PREFIX.length);
    let rec: { h?: string; rid?: number; repo?: string; inst?: string; t?: number } | null = null;
    try {
      const raw = await kv.get(name);
      rec = raw ? JSON.parse(raw) : null;
    } catch {
      continue; // transient read miss or unparseable — next tick retries
    }
    if (!rec || typeof rec.h !== "string" || typeof rec.t !== "number") continue;

    // Too young to judge: it may well be mid-job.
    if (nowMs - rec.t < STALE_BOX_AGE_MS) continue;

    // Can we get a DEFINITE answer? `rid` and `repo` build the GitHub URL, so
    // without them there is nothing to ask and the box is left alone.
    //
    // An EMPTY `inst` is NOT ignorance and must not be treated as such.
    //
    // ⚠️ WHICH POPULATION THIS IS. It is NOT Option-C. Option-C per-tenant-PAT
    // dispatch omits the installation id only in the body of the request to OUR
    // mint (src/lib.ts:296) — `installationId` still flows to the GitHub JIT/box
    // registration, as the dispatch site itself says (src/index.ts, the Option-C
    // note: "The installationId still flows for the GitHub JIT/box registration
    // below — only the CAS-tenant changes"). The records with `inst: ""` are COLD
    // SPAWNS: a REPO webhook carries no `installation.id`, and REPO_INSTALLATION_MAP
    // injects one only for mapped repos — of which there is exactly one. So a spawn
    // for any unmapped repo is COLD and writes `inst: ""`. (The record that anchored
    // the 2026-08-31 measurement, `repo = 'HuGR-Labs/corelink-server'`, is precisely
    // that case: wrangler.jsonc keeps it OUT of the map deliberately.)
    //
    // The thesis is unchanged and so is the fix: a DELIBERATE EMPTY IS NOT
    // IGNORANCE. `mintJitAuthToken` already handles exactly that — no App creds or
    // no installation ⇒ it returns the static `GITHUB_MINT_TOKEN`, the same
    // credential the spawn itself used. So the verification below works fine without
    // an installation — and if no credential resolves at all, `fetchRunnerActivity`
    // returns null and the record is unverifiable through the normal path, fail-safe
    // intact.
    //
    // Requiring `inst` here made EVERY cold-spawn box structurally unreapable: the
    // record could never reach the verify call, so it was skipped, at `info`, with
    // no age ceiling and no escalation — every minute until its 24 h TTL expired.
    // Measured live 2026-08-31: 279 distinct `sbox:` RECORDS in that state, aged
    // 7.4 h to 22.3 h, none of their runners registered with GitHub at all.
    //
    // The skip names WHICH field is missing. Without that it reported 279 records a
    // minute for hours while saying nothing about the cause, and a hand-read sample
    // of those records appeared to contradict the log — a skip that does not name
    // its reason cannot be acted on, only guessed at.
    if (typeof rec.rid !== "number" || !rec.repo) {
      logEvent("info", "reap_skipped_unverifiable", {
        runnerName,
        ageMs: nowMs - rec.t,
        reason: typeof rec.rid !== "number" ? `rid:${typeof rec.rid}` : "repo:empty",
      });
      continue;
    }

    // Past the per-tick ceiling ⇒ not examined this tick. See REAP_MAX_VERIFY_PER_TICK:
    // this guard is what keeps the widened population a drip instead of a burst.
    if (verifications >= REAP_MAX_VERIFY_PER_TICK) {
      deferred++;
      continue;
    }
    verifications++;

    let activity: "busy" | "idle" | "unknown" = "unknown";
    try {
      // `inst ?? ""` is the cold-spawn shape: an absent installation selects the
      // static mint token inside `mintJitAuthToken`, never an App token for "".
      activity = runnerActivityVerdict(await verify(env, rec.repo, rec.rid, rec.inst ?? ""));
    } catch (e) {
      logEvent("error", "reap_verify_threw", { runnerName, error: (e as Error).message });
      continue; // ignorance ⇒ never destroy
    }
    if (activity !== "idle") continue; // busy or unknown ⇒ leave it running

    try {
      await getContainer(env.RUNNER_CONTAINER, rec.h).destroy();
      reaped++;
      logEvent("error", "stale_box_reaped", {
        runnerName,
        ageMs: nowMs - rec.t,
        note: "outlived JOB_PAT_TTL_S and GitHub reports its runner idle",
      });
      await kv.delete(name).catch(() => {});
    } catch (e) {
      // A throw from destroy() is NOT proof the box is down — it may be a
      // transient DO error, and this sweep only runs past the `rhandle:` TTL, so
      // the `sbox:` record is the last cron-visible handle. Deleting it on a
      // throw would strand a RUNNING box forever, contradicting the contract
      // above ("a throw ⇒ left alone and retried next tick"). Same shape as
      // sweepGhostContainers: probe liveness and only reap on a definite "down".
      logEvent("info", "reap_destroy_skipped", { runnerName, error: (e as Error).message });
      let alive: boolean;
      try {
        alive = await getContainer(env.RUNNER_CONTAINER, rec.h).isAlive();
      } catch {
        alive = false; // unreachable DO ⇒ nothing is running
      }
      if (alive) {
        // Still up after a failed destroy: keep the record and retry next tick.
        logEvent("error", "stale_box_still_alive", { runnerName });
        continue;
      }
      await kv.delete(name).catch(() => {});
    }
  }
  if (deferred > 0) {
    // Not an error: the sweep runs every minute and the record lives 24 h. It IS
    // worth seeing, because a persistently non-zero `deferred` means the `sbox:`
    // population is outrunning the sweep and the real defect is upstream.
    logEvent("info", "reap_deferred_over_cap", { deferred, cap: REAP_MAX_VERIFY_PER_TICK });
  }
  if (reaped > 0) await bumpMetrics(env, ...Array(reaped).fill("stale_box_reaped"));
  return reaped;
}

// ─────────────────────────────────────────────────────────────────────────────
// ORPHAN-BOX RECONCILIATION (B-002) — reap-keyed on PLATFORM TRUTH, not on us.
// ─────────────────────────────────────────────────────────────────────────────
//
// `reapStaleBoxes` above starts from `kv.list({prefix:"sbox:"})` — OUR
// bookkeeping. That is the right belt for a box whose keep-alive binding expired,
// but it is STRUCTURALLY BLIND to the failure it was built for: the three boxes
// that ran 10.2 h in the 2026-08-23 incident had NO `sbox:` record at all, so a
// sweep that enumerates `sbox:` can never see them. A detector for bookkeeping
// LOSS cannot start from the bookkeeping.
//
// `reconcileOrphanBoxes` is the mirror image: it starts from the Cloudflare
// Containers API — whatever is running IS running, recorded or not — and asks the
// opposite question. It is the in-Worker, cron-driven analogue of the external
// `scripts/orphan-box-check.sh` CI probe (2026-08-24), so an orphan is caught on
// the 1-minute tick rather than only when the scheduled workflow runs.
//
// ⛔ OBSERVE-ONLY AT THIS STAGE. There is NO `.destroy()`/`.stop()`/`teardown()`
// call anywhere in this function, and there must not be one: it only LOGS orphan
// candidates and bumps an observe counter. That is not a stylistic choice — there
// is (today) no path from a CF Containers instance name back to its Durable
// Object that would let a teardown land (POST /v1/teardown with an instance name
// resolves `idFromName()` to a fresh unrelated DO and no-ops with a 204). Actual
// teardown is a SEPARATE, owner-gated flag (`RECONCILE_ORPHAN_TEARDOWN`) and a
// future landing, exactly like the B-038 audit lease.
//
// ⚠️ UNVERIFIED JOIN KEY, surfaced not trusted. The join is `instance.name` ===
// the DO handle stored as `sbox:` `h` (both bare `crypto.randomUUID()` UUIDs —
// `getContainer(ns, h)` addresses the container by `idFromName(h)`). That equality
// is asserted by the task/CHANGELOG but NOT yet proven against a live instance
// list in this repo. So the dry-run logs BOTH sides every tick: if the key is
// wrong it flags EVERY running box as an orphan — a loud 100 % signal — and the
// teardown flag stays off until an operator confirms the join from a real
// `scripts/container-instances.sh --json` dump.

// Age floor for an orphan candidate: 2×`STALE_BOX_AGE_MS` (= 2×JOB_PAT_TTL_S = 4 h).
// `sbox:` is written at spawn COMPLETION, so the only window a live box lacks its
// record is sub-second (container start → the KV PUT landing) or a logged
// `kv_put_spawned_box_failed`. No legitimate mid-spawn box is 4 h old, so
// "no sbox: AND age > 4 h" is genuinely un-accounted, never a record that simply
// has not landed yet. Deliberately DOUBLE `reapStaleBoxes`' own floor.
const ORPHAN_MIN_AGE_MS = 2 * STALE_BOX_AGE_MS;

// Single-page size for the platform enumeration. Must stay comfortably above the
// account's TOTAL instance record count (running + retained tombstones) so the
// completeness proof below (cursor absent) holds in ONE request. Matches
// scripts/container-instances.sh's default; overridable there via env, a constant
// here (raise it if tombstones ever outgrow it — a truncated page fails closed).
const ORPHAN_SCAN_PER_PAGE = 2000;

// The ONE container application whose instances this sweep may consider. The CF
// account also hosts `corelink-prod-*` (customer-serving CoreLink servers),
// unrelated application instances, the `corelink-fabricd-*` app, and this very
// worker's OWN `corelink-spawn-worker-checkhostcontainer` app — NONE of which
// write `sbox:` records. Without this filter every long-running instance of every
// one of them satisfies the "no sbox record + age>4h" orphan predicate and floods
// `orphan_box_detected` (and, once teardown is armed, would be a destroy target).
//
// Match the STABLE app-id; the derived name is asserted only as a secondary
// signal. This is deliberately NOT a prefix match: `corelink-spawn-worker-*` also
// matches the checkhost app (a DIFFERENT DO namespace this sweep must not reap via
// the runner binding), and a bare `corelink` prefix would match the prod servers.
// The app-id is the guarantee that keeps the sweep scoped to runner instances.
const RUNNER_APP_ID = "a03d11a2-7e03-48a4-96bb-4d2c43892cd4";
const RUNNER_APP_NAME = "corelink-spawn-worker-runnercontainer";

// One RUNNING platform instance, as enumerated from the CF Containers API. `name`
// is the join key (the DO-handle UUID). `started_at` is the platform's own clock
// — age is computed from IT, never from any KV record.
interface RunningInstance {
  app: string;
  id: string;
  name: string;
  started_at: string | null;
  image: string | null;
}

// A flag is "enabled" only on an explicit truthy value. Unset/blank/"0"/"false"
// ⇒ OFF (the default-safe direction for every gate here).
function flagEnabled(v: string | undefined): boolean {
  if (!v) return false;
  const s = v.trim().toLowerCase();
  return s === "1" || s === "true" || s === "yes" || s === "on";
}

// Resolve the containers-read token (preferred name first), mirroring the script.
function containersApiToken(env: Env): string | undefined {
  return env.CLOUDFLARE_CONTAINERS_API_TOKEN || env.CLOUDFLARE_API_TOKEN || undefined;
}

// A GET against the CF API that FAILS LOUD on anything that is not a clean 200 +
// success:true — never hands a partial/failed body back to a caller that would
// then count records out of it. Throws; the sweep's outer catch turns the throw
// into "return 0, touch nothing this tick".
async function cfContainersGet(
  env: Env,
  accountId: string,
  token: string,
  path: string,
): Promise<{ result?: unknown; result_info?: { next_page_token?: string } }> {
  const resp = await fetch(`https://api.cloudflare.com/client/v4${path}`, {
    headers: {
      authorization: `Bearer ${token}`,
      accept: "application/json",
      "user-agent": "corelink-spawn-worker",
    },
  });
  if (!resp.ok) {
    // A rejected token must NEVER read as "nothing to report" (a bad token answers
    // 400 code:9106 on this endpoint, not 401/403). Any non-200 ⇒ throw ⇒ fail-closed.
    throw new Error(`GET ${path} → HTTP ${resp.status}`);
  }
  const body = (await resp.json()) as {
    success?: boolean;
    errors?: unknown;
    result?: unknown;
    result_info?: { next_page_token?: string };
  };
  if (body.success !== true) {
    throw new Error(`GET ${path} → success=false: ${JSON.stringify(body.errors ?? null)}`);
  }
  return body;
}

// Enumerate the ACTUAL running container instances, per-application, transcribing
// the proven algorithm in scripts/container-instances.sh (a Worker cannot shell
// out to it). The three traps that script documents, all handled here:
//
//   1. applications[].instances is the health-block SUM (active+healthy+stopped+…),
//      NOT a running count — it read 22 while 3 ran. We read ONLY `.result[].id`
//      (+ `.name`) from the applications list, never `.instances`.
//   2. TOMBSTONES: terminated instances are retained forever with
//      status.state === "inactive" (one app held 3 running vs 350+ tombstones).
//      We filter on status.state === "running" explicitly.
//   3. PAGINATION FLIPS SHAPE + the paginated walk's running count is
//      NON-DETERMINISTIC under churn (measured 6 then 20 as the cursor window
//      slid — that is why the script's machine-readable `--json` mode does NOT
//      walk pages). We take ONE page larger than the whole record set and REQUIRE
//      the cursor to be ABSENT: `next_page_token` missing is the API stating there
//      is nothing after this page — completeness proven BY the payload, at one
//      instant. If the cursor is PRESENT the page was capped and we have NOT seen
//      the whole fleet ⇒ THROW (never a partial fleet, never a false "clean").
//      This is the pagination self-check, in the form that suits a live detector.
//
// Deduped by instance id. Throws on any API error / truncation ⇒ the caller
// returns 0 and touches nothing (fail-closed on ignorance).
export async function listRunningInstances(env: Env): Promise<RunningInstance[]> {
  const accountId = env.CLOUDFLARE_ACCOUNT_ID;
  const token = containersApiToken(env);
  if (!accountId || !token) return []; // gate already checked; belt-and-braces
  const appsBody = await cfContainersGet(
    env,
    accountId,
    token,
    `/accounts/${accountId}/containers/applications`,
  );
  const apps = Array.isArray(appsBody.result)
    ? (appsBody.result as Array<{ id?: string; name?: string }>)
    : [];
  // SCOPE TO THE RUNNER APP ONLY (see RUNNER_APP_ID). Every other app in the
  // account lacks `sbox:` records and would otherwise flood false orphans.
  const runnerApps = apps.filter((a) => a.id === RUNNER_APP_ID);
  if (runnerApps.length === 0) {
    // The runner app was not found (a rename/redeploy changed its id, or the token
    // cannot see it). Fail-QUIET on detection — never fabricate orphans from an
    // empty match — but log LOUD so this silent-death is visible, not mistaken for
    // a genuinely clean fleet.
    logEvent("error", "orphan_scan_runner_app_missing", {
      expectedId: RUNNER_APP_ID,
      expectedName: RUNNER_APP_NAME,
      appsSeen: apps.map((a) => a.name ?? a.id ?? "?"),
    });
    return [];
  }
  // Secondary assertion: the pinned id should carry the name we expect. A mismatch
  // does NOT change behavior (the id is authoritative) but is surfaced.
  for (const a of runnerApps) {
    if (a.name && a.name !== RUNNER_APP_NAME) {
      logEvent("info", "orphan_scan_runner_app_name_drift", {
        id: a.id,
        sawName: a.name,
        expectedName: RUNNER_APP_NAME,
      });
    }
  }
  const out: RunningInstance[] = [];
  const seen = new Set<string>();
  for (const app of runnerApps) {
    if (!app.id) continue;
    const appName = app.name ?? app.id;
    const body = await cfContainersGet(
      env,
      accountId,
      token,
      `/accounts/${accountId}/containers/applications/${app.id}/instances?per_page=${ORPHAN_SCAN_PER_PAGE}`,
    );
    if (body.result_info?.next_page_token) {
      // Capped page ⇒ the whole fleet was NOT seen; an orphan past the cap would
      // read as clean. Fail closed rather than emit a partial fleet.
      throw new Error(
        `TRUNCATED PAGE for app ${app.id}: per_page=${ORPHAN_SCAN_PER_PAGE} still returned a next_page_token`,
      );
    }
    const instances = Array.isArray((body.result as { instances?: unknown })?.instances)
      ? ((body.result as { instances: Array<Record<string, unknown>> }).instances)
      : [];
    for (const inst of instances) {
      const state = (inst.status as { state?: string } | undefined)?.state;
      if (state !== "running") continue; // drop inactive tombstones + anything not live
      const id = typeof inst.id === "string" ? inst.id : undefined;
      const name = typeof inst.name === "string" ? inst.name : undefined;
      if (!id || !name) continue;
      if (seen.has(id)) continue; // dedupe by instance id
      seen.add(id);
      out.push({
        app: appName,
        id,
        name,
        started_at: typeof inst.started_at === "string" ? inst.started_at : null,
        image:
          typeof inst.image === "string"
            ? inst.image
            : typeof (inst.configuration as { image?: string } | undefined)?.image === "string"
              ? (inst.configuration as { image: string }).image
              : null,
      });
    }
  }
  return out;
}

// Read the durable `sbox:` set and return the Set of KNOWN DO handles (`h`). This
// is the SAME reference set `reapStaleBoxes` uses (24 h TTL — it outlives the
// box's own max lifetime, so a >4 h orphan that HAS an sbox record is correctly
// excluded). Throws on a KV list/read failure so the sweep fails closed: we must
// never assert "no sbox record" from an unread KV (that would flag live boxes).
//
// PAGINATED, fail-closed. A single `kv.list` caps at 1000 keys. `sbox:` has a 24 h
// TTL and one key is written per spawn, so a CI storm (>1000 spawns/24 h — well
// within reach now that corelink-server's Rust gate runs on `runs-on: corelink`)
// would truncate this set. A truncated known-handle set turns accounted, LIVE
// boxes into false orphans — the fail-UNSAFE direction, the exact reason
// `detectStrandedInFlightJobs` follows its own cursor. So we walk the cursor to
// completion; if the walk cannot complete we THROW, and the caller returns 0 and
// touches nothing this tick. (`reapStaleBoxes` shares the single-call form but is
// safe there — its GitHub-idle guard means truncation only MISSES reaps.)
export async function listSpawnedBoxHandles(env: Env): Promise<Set<string>> {
  const kv = env.RUNNER_JOB_PATS;
  const known = new Set<string>();
  if (!kv) return known;
  let cursor: string | undefined;
  for (;;) {
    const listed = await kv.list({ prefix: SPAWNED_BOX_PREFIX, cursor });
    for (const { name } of listed.keys) {
      const raw = await kv.get(name);
      if (!raw) continue;
      try {
        const rec = JSON.parse(raw) as { h?: unknown };
        if (typeof rec.h === "string") known.add(rec.h);
      } catch {
        /* an unparseable record contributes no handle — next tick retries */
      }
    }
    if (listed.list_complete) break;
    cursor = listed.cursor;
    if (!cursor) {
      // Not complete yet no cursor to continue — completeness is unprovable. Fail
      // closed rather than return a truncated (fail-unsafe) handle set.
      throw new Error("sbox: KV list incomplete but returned no cursor");
    }
  }
  return known;
}

/**
 * Observe-only reconciliation of running platform instances against `sbox:`.
 *
 * The mirror image of `reapStaleBoxes`: that one starts from OUR bookkeeping,
 * this one starts from PLATFORM TRUTH. A running instance whose `name` is in NO
 * `sbox:` record AND whose platform age exceeds 2×JOB_PAT_TTL_S (4 h) is an
 * ORPHAN candidate — a box the fabric cannot account for.
 *
 * DRY-RUN by construction: it LOGS each candidate + a per-tick join audit (both
 * sides of the key), bumps `orphan_box_detected`, and returns the count. It
 * NEVER tears anything down at this stage.
 *
 * FAIL-SAFE / FAIL-CLOSED:
 *   • not enabled (`RECONCILE_ORPHAN_BOXES` falsy) ⇒ return 0, no enumeration;
 *   • CF creds absent ⇒ return 0 (this is what keeps it inert until owner-wired);
 *   • any CF API error / truncated page / KV read failure ⇒ log + return 0,
 *     touch nothing this tick (unknown ⇒ leave everything running).
 *
 * `listInstances` and `listSbox` are injected so every branch is testable without
 * a network or a live KV.
 */
export async function reconcileOrphanBoxes(
  env: Env,
  nowMs: number,
  listInstances: (env: Env) => Promise<RunningInstance[]> = listRunningInstances,
  listSbox: (env: Env) => Promise<Set<string>> = listSpawnedBoxHandles,
): Promise<number> {
  // 1) GATE — fail-safe OFF, two independent conditions.
  if (!flagEnabled(env.RECONCILE_ORPHAN_BOXES)) {
    return 0; // inert: not even the platform is enumerated
  }
  if (!env.CLOUDFLARE_ACCOUNT_ID || !containersApiToken(env)) {
    logEvent("info", "orphan_reconcile_skipped_no_creds", {
      reason: "CLOUDFLARE_ACCOUNT_ID and/or a containers-read token unbound",
    });
    return 0;
  }

  // 2) PLATFORM TRUTH — whatever is running IS running. Fail closed on any error.
  let instances: RunningInstance[];
  try {
    instances = await listInstances(env);
  } catch (e) {
    logEvent("error", "orphan_reconcile_platform_read_failed", { error: (e as Error).message });
    return 0;
  }

  // 3) BOOKKEEPING — the KNOWN handle set. Fail closed: an unread sbox set must
  // NOT be treated as "nothing is accounted for" (that would flag live boxes).
  let known: Set<string>;
  try {
    known = await listSbox(env);
  } catch (e) {
    logEvent("error", "orphan_reconcile_sbox_read_failed", { error: (e as Error).message });
    return 0;
  }

  // Per-tick JOIN AUDIT — log BOTH sides so an operator can eyeball-confirm the
  // (unverified) join key BEFORE any teardown flag is ever flipped. If the key is
  // wrong this shows every box as an orphan; that mismatch is the whole point of
  // logging it.
  logEvent("info", "orphan_reconcile_scan", {
    scanned: instances.length,
    sboxKnown: known.size,
    instanceNames: instances.map((i) => i.name),
    knownHandles: [...known],
    teardownArmed: flagEnabled(env.RECONCILE_ORPHAN_TEARDOWN), // false; teardown NOT wired here
  });

  // 4) ORPHAN PREDICATE + 5) DRY-RUN ACTION.
  let count = 0;
  for (const inst of instances) {
    // (a) DOUBLE-CHECK the sbox absence: name must be in NO sbox record.
    if (known.has(inst.name)) continue;
    // (b) age (from the PLATFORM clock) must exceed the 4 h floor. No/unparseable
    // started_at ⇒ cannot prove it is old ⇒ leave it alone (fail-safe).
    if (!inst.started_at) continue;
    const startedMs = Date.parse(inst.started_at);
    if (Number.isNaN(startedMs)) continue;
    const ageMs = nowMs - startedMs;
    if (ageMs <= ORPHAN_MIN_AGE_MS) continue;

    count++;
    logEvent("error", "orphan_box_detected", {
      id: inst.id,
      name: inst.name,
      app: inst.app,
      ageMs,
      started_at: inst.started_at,
      image: inst.image,
      note: "running platform instance with NO sbox: record, older than 2×JOB_PAT_TTL_S — un-accounted capacity. DRY-RUN: nothing torn down.",
    });
  }

  logEvent(count > 0 ? "error" : "info", "orphan_boxes_detected", {
    count,
    scanned: instances.length,
    sboxKnown: known.size,
  });
  if (count > 0) await bumpMetrics(env, ...Array(count).fill("orphan_box_detected"));
  return count;
}

// driveSpawn wrapped so ANY failure RELEASES the spawn claim — a GitHub redelivery
// or a later reconciler tick can then re-drive the job (never a silent orphan).
async function driveSpawnGuarded(
  env: Env,
  opts: ContainmentDriveOpts,
): Promise<void> {
  const spawnClaim = spawnClaimCallbacks(env, opts.jobId);
  if (!(await spawnClaim.claim())) return;
  try {
    if (!(await spawnClaim.active())) throw new Error("spawn claim authority unavailable");
    const receipt = await driveSpawn(env, { ...opts, bindProviderIdentity: spawnClaim.bindProvider });
    if (receipt && !(await spawnClaim.bindProvider(receipt.provider_signature))) {
      throw new Error("spawn claim provider binding unavailable");
    }
  } catch (e) {
    await spawnClaim.release();
    // A ceiling REFUSAL still needs the dead-letter (that is the whole point —
    // otherwise the job is lost forever), but it is not an error and must not be
    // counted or logged as one: `spawn_at_ceiling` was already emitted at the
    // refusal site with the tenant/cap/reason detail, and marking normal
    // backpressure as `spawn_failed` would make a healthy busy fleet look broken
    // on the dashboard — and hide real failures in the noise.
    if (!(e instanceof SpawnRefusedError)) {
      await bumpMetrics(env, "spawn_failed");
      logEvent("error", "spawn_drive_failed", { jobId: opts.jobId, error: (e as Error).message });
    }
    // W7/F8: record the WARM-recoverable failure as a dead-letter so the scheduled
    // reconciler retries it WARM (for ANY repo). driveSpawnGuarded is the "first
    // attempt" context (webhook + first-party GitHub scan) — the retry path calls
    // the THROWING driveSpawn directly, so it never re-enters this recording catch.
    await recordOrphan(env, opts);
  }
}

type ContainmentDriveOpts = {
  jobId: string;
  repo: string;
  installationId: string;
  labels: string[];
  // Omitted is the historical webhook/legacy behavior. Reservation-contained
  // re-drives set this to freeze their credential source to installation-only.
  credential_source?: "installation-only";
  effect_id?: string;
  containment_event_id?: string;
  // The immutable one-shot permit is evidence identity, not a lease capability:
  // an old continuation may finish durable evidence after its lease expires but
  // cannot mark/ack. A reclaimer never receives this field for a new effect.
  effect_permit_id?: string;
  // Supplied by runCanonicalEffect only after it has durably bound the provider
  // identity. This is local route evidence, never a caller-provided protocol field.
  effect_binding?: ContainmentEffectBinding;
  /** Internal attempt callback; persisted before the runner container starts. */
  bindProviderIdentity?: (providerIdentity: string) => Promise<boolean>;
};
function isContainmentDrive(opts: ContainmentDriveOpts): opts is ContainmentDriveOpts & { effect_id: string; containment_event_id: string; effect_permit_id: string; effect_binding: ContainmentEffectBinding } {
  return typeof opts.effect_id === "string"
    && typeof opts.containment_event_id === "string"
    && typeof opts.effect_permit_id === "string"
    && typeof opts.effect_binding?.resource_id === "string";
}
async function writeContainmentEvidence(
  env: Env,
  opts: ContainmentDriveOpts & { effect_id: string; containment_event_id: string; effect_permit_id: string },
  kind: ContainmentEffectWitnessKind,
  sourceKey: string,
  sourceValue: string,
  extras: Pick<ContainmentEffectEvidence, "terminal" | "attempt_count"> = {},
): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) throw new Error("containment evidence requires RUNNER_JOB_PATS");
  const evidence: ContainmentEffectEvidence = {
    schema_version: 1,
    kind,
    effect_id: opts.effect_id,
    event_id: opts.containment_event_id,
    job_id: opts.jobId,
    permit_id: opts.effect_permit_id,
    source_key: sourceKey,
    source_value: sourceValue,
    source_sha256: await sha256Hex(sourceValue),
    ...extras,
  };
  const key = containmentEffectEvidenceKey(opts.effect_id, kind);
  const encoded = canonicalContainmentEvidence(evidence);
  const prior = await kv.get(key);
  if (prior) {
    if (prior !== encoded) throw new Error(`containment ${kind} evidence conflicts`);
    return;
  }
  // No TTL: a head may remain safely blocked for longer than a job's normal
  // lifetime. Expiry would turn that block into a re-drive bypass. Final ACK
  // cleans these immutable records after the DO has advanced the cursor.
  await kv.put(key, encoded);
  if (await kv.get(key) !== encoded) throw new Error(`containment ${kind} evidence was not durably bound`);
}

async function bindContainmentSpawnClaim(
  env: Env,
  opts: ContainmentDriveOpts & { effect_id: string; containment_event_id: string; effect_permit_id: string },
): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) throw new Error("containment effect witnesses require RUNNER_JOB_PATS");
  const key = `spawn:${opts.jobId}`;
  const raw = await kv.get(key);
  if (!raw) throw new Error("containment spawn claim missing after acquisition");
  const jobBinding = JSON.stringify({ schema_version: 1, effect_id: opts.effect_id, event_id: opts.containment_event_id, job_id: opts.jobId, permit_id: opts.effect_permit_id });
  const jobKey = containmentEffectJobKey(opts.jobId);
  const priorBinding = await kv.get(jobKey);
  if (priorBinding && priorBinding !== jobBinding) throw new Error("containment job binding conflicts");
  if (!priorBinding) await kv.put(jobKey, jobBinding);
  if (await kv.get(jobKey) !== jobBinding) throw new Error("containment job binding was not durably bound");
  await writeContainmentEvidence(env, opts, "spawn_claim", key, raw);
}

async function bindContainmentPlacement(
  env: Env,
  opts: ContainmentDriveOpts & { effect_id: string; containment_event_id: string; effect_permit_id: string },
): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) throw new Error("containment effect witnesses require RUNNER_JOB_PATS");
  // `recordPlacement` writes this immutable evidence before touching the
  // mutable orphan key. Completion may clear that key after its write/read-back,
  // so terminalization must verify the durable witness rather than re-read it.
  const evidenceRaw = await kv.get(containmentEffectEvidenceKey(opts.effect_id, "placement"));
  let evidence: ContainmentEffectEvidence | null = null;
  try {
    evidence = evidenceRaw ? (JSON.parse(evidenceRaw) as ContainmentEffectEvidence) : null;
  } catch {
    evidence = null;
  }
  let placement: (OrphanRecord & { effect_id?: string }) | null = null;
  try { placement = evidence ? (JSON.parse(evidence.source_value) as OrphanRecord & { effect_id?: string }) : null; } catch { placement = null; }
  if (!evidenceRaw || !evidence || canonicalContainmentEvidence(evidence) !== evidenceRaw || evidence.kind !== "placement" || evidence.effect_id !== opts.effect_id || evidence.event_id !== opts.containment_event_id || evidence.job_id !== opts.jobId || evidence.permit_id !== opts.effect_permit_id || evidence.source_key !== orphanKey(opts.jobId) || evidence.source_sha256 !== await sha256Hex(evidence.source_value) || !placement || placement.effect_id !== opts.effect_id || !Number.isFinite(placement.placedMs) || JSON.stringify(placement) !== evidence.source_value) {
    throw new Error("containment placement cannot bind this effect");
  }
}

async function writeContainmentResultEvidence(
  env: Env,
  opts: ContainmentDriveOpts & { effect_id: string; containment_event_id: string; effect_permit_id: string },
  attemptCount: number,
): Promise<void> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv || !Number.isSafeInteger(attemptCount) || attemptCount < 1) throw new Error("containment terminal evidence lacks an attempt");
  const prior = await Promise.all(CONTAINMENT_EFFECT_WITNESS_KINDS.slice(0, 4).map((kind) => kv.get(containmentEffectEvidenceKey(opts.effect_id, kind))));
  const evidence = prior.map((raw) => {
    if (!raw) return null;
    try {
      const parsed = JSON.parse(raw) as ContainmentEffectEvidence;
      return canonicalContainmentEvidence(parsed) === raw ? parsed : null;
    } catch {
      return null;
    }
  });
  if (evidence.some((rec) => !rec) || evidence[1]!.attempt_count !== attemptCount) throw new Error("containment terminal evidence lacks bound predecessors");
  const source = JSON.stringify({
    terminal: "DELIVERED",
    attempt_count: attemptCount,
    spawn_claim_sha256: evidence[0]!.source_sha256,
    attempt_sha256: evidence[1]!.source_sha256,
    placement_sha256: evidence[2]!.source_sha256,
    lease_sha256: evidence[3]!.source_sha256,
  });
  await writeContainmentEvidence(env, opts, "result", "result", source, { terminal: "DELIVERED", attempt_count: attemptCount });
}

async function containmentEvidenceDigest(env: Env, event: ContainmentEvent): Promise<string | null> {
  const kv = env.RUNNER_JOB_PATS;
  if (!kv) return null;
  try {
    const records = await Promise.all(CONTAINMENT_EFFECT_WITNESS_KINDS.map((kind) => kv.get(containmentEffectEvidenceKey(event.effect_id, kind))));
    if (records.some((raw) => !raw)) return null;
    const canonical = records.map((raw) => {
      const parsed = JSON.parse(raw as string) as ContainmentEffectEvidence;
      return canonicalContainmentEvidence(parsed) === raw ? raw : null;
    });
    if (canonical.some((raw) => !raw)) return null;
    return sha256Hex(JSON.stringify(canonical));
  } catch {
    return null;
  }
}

type ContainmentDrainDependencies = {
  claimSpawn?: typeof claimSpawn;
  bindContainmentSpawnClaim?: typeof bindContainmentSpawnClaim;
  driveSpawn?: typeof driveSpawn;
  containmentEvidenceDigest?: typeof containmentEvidenceDigest;
};

export async function runContainmentDrain(env: Env, dependencies: ContainmentDrainDependencies = {}): Promise<void> {
  // Containment drain creates NEW runner admissions. Completion/teardown is
  // handled by the webhook completed leg and scheduled teardown retry below;
  // pausing this drain leaves those lifecycle paths available.
  if (admissionPaused(env.FABRIC_ADMISSION_PAUSED)) return;
  const claim = dependencies.claimSpawn ?? claimSpawn;
  const bindClaim = dependencies.bindContainmentSpawnClaim ?? bindContainmentSpawnClaim;
  const drive = dependencies.driveSpawn ?? driveSpawn;
  const evidenceDigest = dependencies.containmentEvidenceDigest ?? containmentEvidenceDigest;
  let authority: DurableObjectStub<ContainmentDO>;
  try { authority = containmentAuthority(env); } catch { return; }
  const owner = crypto.randomUUID();
  let lease: { owner: string; epoch: number; expires_ms: number } | null;
  try { lease = await authority.acquireLease(owner); } catch { return; }
  if (!lease) return;
  try {
    for (;;) {
      if (!(await authority.renewLease(owner, lease.epoch))) break;
      const head = await authority.claimNext(owner, lease.epoch);
      if (!head) break;
      const event = head.event;
      if (head.committed) {
        if (!(await authority.acknowledge(event.event_id, owner, lease.epoch))) break;
        continue;
      }
      const tombstoned = await authority.installationTombstoned(event.installation_id);
      // A permit may have crossed into an external effect before deletion. Keep
      // that head on the proof-bound recovery path below; a permit-free head is
      // terminalized entirely inside the authority and can never reach a side
      // effect. A crash after terminalization is recovered by the committed leg.
      if (tombstoned && !event.effect_permit) {
        if (!(await authority.terminalizeTombstonedEvent(event.event_id, owner, lease.epoch))) break;
        if (!(await authority.acknowledge(event.event_id, owner, lease.epoch))) break;
        continue;
      }
      if (event.effect_permit) {
        const proof = await evidenceDigest(env, event);
        if (!proof || !(await authority.recoverEffectCommitted(event.effect_id, owner, lease.epoch, proof))) break;
        if (!(await authority.acknowledge(event.event_id, owner, lease.epoch))) break;
        continue;
      }
      if (!env.RUNNER_JOB_PATS) break;
      if (tombstoned) break;
      if (installationAllowlistArmed(env.INSTALLATION_ALLOWLIST)
        && !isInstallationAllowlisted(env.INSTALLATION_ALLOWLIST, event.installation_id)) break;
      if (env.WEBHOOK_LIMITER && !(await env.WEBHOOK_LIMITER.limit({ key: `spawn:${event.repo}` })).success) break;
      const tuple = await drainOwnerTuple(event.repo, event.job_id, event.effect_id, event.event_id, owner, lease.epoch);
      const spawnOpts: ContainmentDriveOpts = { jobId: event.job_id, repo: event.repo, installationId: event.installation_id, labels: event.labels };
      let prepared: ContainerEnvResult | undefined;
      const spawnClaim = spawnClaimCallbacks(env, event.job_id);
      const routeResult = await runCanonicalEffect({
        ledger: authority,
        tuple,
        opts: spawnOpts,
        provider: "cloudflare-container",
        resource_id: `job:${event.repo}/${event.job_id}`,
        idempotency_key: event.effect_id,
        admit: () => authority.admitDrainOwner(event.event_id, tuple),
        beforeClaim: async () => {
          if (drive === driveSpawn) prepared = await prepareSpawn(env, spawnOpts);
        },
        abandonPreparation: () => abandonPreparedSpawn(env, authority, event.job_id, prepared),
        claim: dependencies.claimSpawn ? () => claim(env.RUNNER_JOB_PATS!, event.job_id) : spawnClaim.claim,
        release: dependencies.claimSpawn ? () => releaseSpawnClaim(env.RUNNER_JOB_PATS!, event.job_id) : spawnClaim.release,
        beforeDrive: async () => dependencies.claimSpawn ? true : spawnClaim.active(),
        drive: async driveOpts => {
          const typed = driveOpts as ContainmentDriveOpts & { effect_id: string; containment_event_id: string; effect_permit_id: string };
          await bindClaim(env, typed);
          const receipt = await drive(env, { ...typed, bindProviderIdentity: spawnClaim.bindProvider }, prepared);
          if (!dependencies.claimSpawn && receipt && !(await spawnClaim.bindProvider(receipt.provider_signature))) throw new Error("spawn claim provider binding unavailable");
          return receipt;
        },
        beforeConfirm: permitId => authority.beginEffect(event.event_id, owner, lease!.epoch, Date.now(), permitId),
        beforeBegin: async permit => !!(await authority.beginEffect(event.event_id, owner, lease!.epoch, Date.now(), permit.permit_id)),
        finalize: async () => (await authority.markEffectCommitted(event.event_id, owner, lease!.epoch))
          && (await authority.acknowledge(event.event_id, owner, lease!.epoch)),
      });
      if (routeResult.status !== "committed" || !routeResult.finalized) break;
    }
  } finally {
    await authority.releaseLease(owner, lease.epoch);
  }
}

/** Recover normal arrivals without moving them into the containment backlog. */
export async function runNormalIntakeDrain(env: Env, alreadyRateAdmittedEventId?: string): Promise<void> {
  // Normal-intake drain creates NEW runner admissions. A completed webhook is
  // intentionally independent and continues to revoke, tear down, and release
  // capacity while this gate is active.
  if (admissionPaused(env.FABRIC_ADMISSION_PAUSED)) return;
  const authority = containmentAuthority(env);
  for (const event of await authority.normalIntakePending(25)) {
    if (parseContainmentSwitch(env.AUTOSCALER_INTAKE_PAUSED) !== "normal") return;
    if ((await authority.snapshot()).backlog_count !== 0) return;
    if (await authority.installationTombstoned(event.installation_id)) {
      await authority.normalIntakeSettle(event.event_id, event.body_sha256, "complete");
      continue;
    }
    if (installationAllowlistArmed(env.INSTALLATION_ALLOWLIST)
      && !isInstallationAllowlisted(env.INSTALLATION_ALLOWLIST, event.installation_id)) {
      await authority.normalIntakeSettle(event.event_id, event.body_sha256, "complete");
      continue;
    }
    if (event.event_id !== alreadyRateAdmittedEventId && env.WEBHOOK_LIMITER
      && !(await env.WEBHOOK_LIMITER.limit({ key: `spawn:${event.repo}` })).success) {
      await authority.normalIntakeSettle(event.event_id, event.body_sha256, "retry");
      continue;
    }
    const spawnOpts: ContainmentDriveOpts = { jobId: event.job_id, repo: event.repo,
      installationId: event.installation_id, labels: event.labels };
    let prepared: ContainerEnvResult | undefined;
    const spawnClaim = spawnClaimCallbacks(env, event.job_id);
    const result = await runCanonicalEffect({
      ledger: authority,
      tuple: await intakeOwnerTuple(event.repo, event.job_id, `containment:v1:${event.event_id}`, event.event_id),
      opts: spawnOpts, provider: "cloudflare-container",
      resource_id: `job:${event.repo}/${event.job_id}`, idempotency_key: `containment:v1:${event.event_id}`,
      admit: async () => parseContainmentSwitch(env.AUTOSCALER_INTAKE_PAUSED) === "normal"
        && (await authority.snapshot()).backlog_count === 0,
      beforeClaim: async () => { prepared = await prepareSpawn(env, spawnOpts); },
      abandonPreparation: () => abandonPreparedSpawn(env, authority, event.job_id, prepared),
      claim: spawnClaim.claim,
      release: spawnClaim.release,
      beforeDrive: spawnClaim.active,
      drive: async opts => {
        await bindContainmentSpawnClaim(env, opts);
        const receipt = await driveSpawn(env, { ...opts, bindProviderIdentity: spawnClaim.bindProvider }, prepared);
        if (receipt && !(await spawnClaim.bindProvider(receipt.provider_signature))) throw new Error("spawn claim provider binding unavailable");
        return receipt;
      },
    });
    await authority.normalIntakeSettle(event.event_id, event.body_sha256,
      result.status === "committed" ? "complete"
        : result.status === "unknown_terminal" || result.status === "mirror_tampered" ? "uncertain" : "retry");
    if (result.status === "committed") await bumpMetrics(env, "webhook_spawn_claimed");
    else if (result.status === "claim_refused") await bumpMetrics(env, "webhook_spawn_deduped");
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
    // Reuse the existing cron: post-start projection failures retain an exact
    // teardown intent in ConcurrencySlotsDO until the provider confirms down.
    ctx.waitUntil(retryActiveSpawnTeardowns(env));
    if (env.FABRIC_COMPUTE_URL && env.CONTAINMENT) {
      ctx.waitUntil(containmentAuthority(env).drainUnusedCompute().catch(() => logEvent("error", "compute_cleanup_pending", {})));
    }
    // Family-aware (mirrors the webhook gate): the reconcilers scan for the
    // `corelink` label family, not a fixed default, so an orphaned/unbilled
    // `runs-on: corelink` customer job is recovered too. `AUTOSCALER_LABEL`, if
    // set, pins to the exact label. Passed as the `configured` arg.
    const configured = env.AUTOSCALER_LABEL;
    const now = Date.now();
    {
      try {
        await deliverInvalidConfig(env);
        const intake = parseContainmentSwitch(env.AUTOSCALER_INTAKE_PAUSED);
        if (intake === "invalid") await observeInvalidConfig(env, "AUTOSCALER_INTAKE_PAUSED", env.AUTOSCALER_INTAKE_PAUSED as string);
        if (intake === "normal") {
          await runContainmentDrain(env);
          await runNormalIntakeDrain(env);
        }
      } catch (e) {
        logEvent("error", "containment_tick_failed", { error: (e as Error).message });
      }
    }
    try {
      // FIRST: a live box being SIGTERMed costs a whole customer job, which
      // outranks orphan recovery and billing backfill. Guarded so it can never
      // throw out of scheduled() and take the rest of the tick with it.
      await keepAliveLiveRunners(env);
    } catch (e) {
      logEvent("error", "keepalive_failed", { error: (e as Error).message });
    }
    try {
      // SECOND: reclaim the containers left by abandoned start attempts. It runs
      // before the re-drive reconcilers on purpose — those place NEW boxes, and
      // they should be placing them into a fleet whose ghosts have already been
      // returned to it. Guarded: a backstop must never take the tick down.
      await sweepGhostContainers(env);
    } catch (e) {
      logEvent("error", "ghost_sweep_failed", { error: (e as Error).message });
    }
    try {
      // THIRD: destroy boxes that outlived any job they could be running. The
      // keep-alive sweep above can only RENEW — without this, termination rests
      // solely on the DO `sleepAfter` alarm, and a box whose alarm does not fire
      // burns forever. Runs after the ghost sweep and before the re-drives, for
      // the same reason: hand the placers a fleet whose waste is already back.
      const reaped = await reapStaleBoxes(env, now);
      if (reaped > 0) logEvent("error", "stale_boxes_reaped", { count: reaped });
    } catch (e) {
      logEvent("error", "stale_box_reap_failed", { error: (e as Error).message });
    }
    try {
      // THIRD-b (immediately after the reap): the platform-truth MIRROR of it.
      // `reapStaleBoxes`
      // starts from OUR `sbox:` bookkeeping and is blind to a box that has no
      // record — the exact 10.2 h leak. This one starts from the Cloudflare
      // Containers API and LOGS any running instance the fabric cannot account
      // for. OBSERVE-ONLY + default-off + inert without CF creds; it tears
      // NOTHING down. Guarded so it can never throw out of the tick.
      const orphans = await reconcileOrphanBoxes(env, now);
      if (orphans > 0) logEvent("error", "orphan_boxes_reconciled", { count: orphans });
    } catch (e) {
      logEvent("error", "orphan_reconcile_failed", { error: (e as Error).message });
    }
    try {
      // Reclaim claims that never acquired a durable handle, and prune expired
      // slot leases even on a quiet fleet. Live handles remain fenced.
      await reapStaleSpawnClaims(env, now);
      if (env.CONCURRENCY_SLOTS) {
        const slots = env.CONCURRENCY_SLOTS.get(env.CONCURRENCY_SLOTS.idFromName("global"));
        if (typeof slots.pruneExpired === "function") await slots.pruneExpired();
      }
    } catch (e) {
      logEvent("error", "lifecycle_reap_failed", { error: (e as Error).message });
    }
    // FOURTH: detect jobs whose box died MID-JOB — the one loss nothing watched.
    // Its own `waitUntil` + its own `.catch()`, so it can neither delay nor take
    // down the placement work below. OBSERVE-ONLY: it tears nothing down (see the
    // long note on the function), it releases only OUR accounting.
    ctx.waitUntil(
      detectStrandedInFlightJobs(env, now)
        .then((n) => {
          if (n > 0) logEvent("error", "stranded_jobs_detected", { count: n });
        })
        .catch((e) => logEvent("error", "strand_sweep_failed", { error: (e as Error).message })),
    );
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
      await retryFailedRevocations(env);
    } catch (e) {
      logEvent("error", "revoke_retry_failed", { error: (e as Error).message });
    }
    try {
      const pushed = await reconcileCompletedJobBilling(
        env,
        configured,
        now,
        env.CONTAINMENT ? (jobId) => readBillingJobAttribution(env, jobId) : undefined,
      );
      if (pushed > 0) {
        logEvent("info", "billing_reconcile_pushed", { count: pushed });
      }
    } catch (e) {
      // Never let the billing reconciler throw out of scheduled() — it is a
      // backstop, not a gate; a failure here just means next tick retries.
      logEvent("error", "billing_reconcile_failed", { error: (e as Error).message });
    }
    try {
      // Flush the durable per-job source with KV pagination and bounded ingest
      // chunks. A malformed record is quarantined in isolation; an HTTP failure
      // leaves its source record pending for the next tick.
      const flushed = await flushBillingUsageBacklog(env);
      if (flushed.pushed > 0 || flushed.quarantined > 0) {
        logEvent("info", "billing_backlog_flushed", { ...flushed });
      }
    } catch (e) {
      logEvent("error", "billing_backlog_flush_failed", { error: (e as Error).message });
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

    // Authenticated fabric suspension signal. The producer is fabricd's
    // durable suspension outbox and uses the scoped lifecycle bearer;
    // this route is never public or tenant-authenticated.
    if (request.method === "POST" && pathname === "/internal/v1/tenant-suspension") {
      if (!controlAuthed(request, env)) return unauthorized();
      let body: { event_id?: string; tenant_id?: string; lifecycle_generation?: string; action?: string };
      try { body = (await request.json()) as typeof body; } catch { return json({ error: "invalid JSON body" }, 400); }
      if (body.action !== "suspended" || !body.event_id || !body.tenant_id || !body.lifecycle_generation || Object.keys(body).sort().join(",") !== "action,event_id,lifecycle_generation,tenant_id" || body.lifecycle_generation.length > 19 || !/^(0|[1-9][0-9]*)$/.test(body.lifecycle_generation) || (() => { try { return BigInt(body.lifecycle_generation!) > 9_223_372_036_854_775_807n; } catch { return true; } })()) return json({ error: "invalid suspension event" }, 400);
      try {
        const input: TenantSuspensionInput = { event_id: body.event_id, tenant_id: body.tenant_id, lifecycle_generation: body.lifecycle_generation };
        const authority = containmentAuthority(env) as unknown as TenantSuspensionConsumerDependencies["authority"];
        const result = await consumeTenantSuspensionCredentials(env, input, {
          authority,
          revokeCredential: async identity => {
            if (!(await revokeIssuedCredential(env, authority, identity))) throw new Error("credential revoke pending");
          },
        });
        const complete = result.complete;
        return new Response(JSON.stringify({ ...input, complete }), {
          status: complete ? 200 : 202,
          headers: { "content-type": "application/json", "x-corelink-legacy-coverage": complete ? "verified" : "unknown" },
        });
      } catch (e) {
        logEvent("error", "tenant_suspension_dispatch_failed", { eventId: body.event_id, error: (e as Error).message });
        return json({ error: "suspension dispatch unavailable" }, 503);
      }
    }

    // ── POST /internal/v1/containment/drain — admin resume request ──────────
    if (request.method === "POST" && pathname === "/internal/v1/containment/drain") {
      const key = env.CONTAINMENT_ADMIN_KEY ?? "";
      if (!key) return json({ error: "not found" }, 404);
      if (!safeEqual(request.headers.get("x-corelink-internal-auth") ?? "", key)) return unauthorized();
      if ((await request.text()).length !== 0) return json({ error: "body must be empty" }, 400);
      try {
        const authority = containmentAuthority(env);
        const meta = await authority.requestDrain();
        const state = parseContainmentSwitch(env.AUTOSCALER_INTAKE_PAUSED);
        if (state === "invalid") await observeInvalidConfig(env, "AUTOSCALER_INTAKE_PAUSED", env.AUTOSCALER_INTAKE_PAUSED as string);
        if (state === "normal") ctx.waitUntil(runContainmentDrain(env));
        return json({ schema_version: 1, drain_requested: meta.drain_requested, intake_paused: state !== "normal", backlog_count: meta.backlog_count, drain_cursor: meta.drain_cursor }, 200);
      } catch {
        return json({ error: "containment authority unavailable" }, 503);
      }
    }

    // ── GET /internal/v1/fleet/busy — "is any box executing customer work" ───
    // The pre-roll gate in deploy-spawn-worker.yml asks THIS, not GitHub: the
    // question needs `administration: read` across every RECONCILER_REPOS repo,
    // which the Actions GITHUB_TOKEN does not have and GitHub offers no API to
    // mint. This Worker already holds the App credential and already asks GitHub
    // per runner (see `fleetBusySnapshot` / `keepAliveLiveRunners`).
    //
    // Gated by its OWN key (X-Corelink-Internal-Auth) — NOT the spawn-CONTROL
    // token and NOT the metrics key. Default-off, fail-closed: key unset → 404
    // (the route is invisible); header mismatch → 401; match → 200.
    //
    // ⛔ CALLER CONTRACT: idle is `busy === 0 && unverifiable === 0`. A runner
    // whose state cannot be established is reported as `unverifiable` and MUST be
    // treated exactly like a busy one — "cannot prove idle", so do not roll.
    // Reading `unverifiable` as idle would roll the fleet on ignorance, which is
    // the one outcome this endpoint exists to prevent.
    if (request.method === "GET" && pathname === "/internal/v1/fleet/busy") {
      const key = env.FLEET_BUSY_READ_KEY ?? "";
      if (key.length === 0) return json({ error: "not found" }, 404);
      const presented = request.headers.get("x-corelink-internal-auth") ?? "";
      if (!safeEqual(presented, key)) return unauthorized();
      return json(await fleetBusySnapshot(env), 200);
    }

    // ── POST /webhook (GitHub autoscaler) — HMAC-authed, NOT bearer ──────────
    // A queued workflow_job with our label ⇒ mint a JIT + spawn a runner. This
    // is the all-Cloudflare autoscaler: no external fabric.  Installation
    // deletion is an App lifecycle event and deliberately does not require the
    // runner-mint token, so configuration is split below by event family.
    if (request.method === "POST" && pathname === "/webhook") {
      const appSecret = env.GITHUB_WEBHOOK_SECRET;
      const repoSecret = env.GITHUB_WEBHOOK_REPO_SECRET;
      if (!appSecret && !repoSecret) {
        return json({ error: "autoscaler not configured" }, 503);
      }
      const rawBytes = await request.arrayBuffer();
      const sig = request.headers.get("x-hub-signature-256") ?? "";
      const [appValid, repoValid] = await Promise.all([
        appSecret ? verifyGithubHmacBytes(appSecret, sig, rawBytes) : Promise.resolve(false),
        repoSecret ? verifyGithubHmacBytes(repoSecret, sig, rawBytes) : Promise.resolve(false),
      ]);
      if (!appValid && !repoValid) {
        // Metrics are an intentional side effect only of an actually configured
        // bad HMAC (not a malformed/missing signature header). Missing
        // configuration and valid ignored events stay quiet.
        if (/^sha256=[0-9a-f]{64}$/i.test(sig.trim())) ctx.waitUntil(bumpMetrics(env, "webhook_auth_failed"));
        return unauthorized();
      }
      let raw: string;
      try {
        raw = new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(rawBytes);
      } catch {
        return json({ error: "invalid UTF-8 body" }, 400);
      }
      const githubEvent = request.headers.get("x-github-event");
      // Installation deletion is handled before workflow parsing.  A repository
      // hook secret cannot authorize this branch: only the GitHub App secret has
      // authority to retire an App installation.
      if (githubEvent === "installation") {
        if (!appValid) return unauthorized();
        let installation: { action?: unknown; installation?: { id?: unknown } };
        try { installation = JSON.parse(raw) as typeof installation; }
        catch { return json({ error: "invalid installation payload" }, 400); }
        const installationId = canonicalInstallationId(installation.installation?.id);
        if (installation.action !== "deleted" || !installationId) {
          return json({ error: "invalid installation deletion" }, 400);
        }
        try {
          const bodySha = await sha256Hex(raw);
          const delivery = trimAsciiWhitespace(request.headers.get("x-github-delivery") ?? "")
            || await sha256Hex(`installation.deleted:v1\n${installationId}\n${bodySha}`);
          const result = await containmentAuthority(env).tombstoneInstallation(installationId, delivery, bodySha);
          if (result === "conflict") return json({ error: "delivery id conflicts with different body" }, 409);
          return json({ ok: true, installation_deleted: true, duplicate: result === "duplicate", installation_id: installationId }, 202);
        } catch {
          return json({ error: "installation tombstone unavailable", retryable: true }, 503);
        }
      }
      // Only workflow_job needs runner-mint configuration.  Other valid GitHub
      // events remain harmless acknowledgements even in lifecycle-only deploys.
      if (githubEvent !== "workflow_job") {
        return json({ ok: true, ignored: "not workflow_job" }, 200);
      }
      if (!env.GITHUB_MINT_TOKEN) return json({ error: "autoscaler not configured" }, 503);
      let evt: {
        action?: string;
        workflow_job?: {
          labels?: string[];
          id?: number;
          started_at?: string;
          completed_at?: string;
          // Echoed back by GitHub on in_progress/completed: the runner that
          // ACTUALLY ran this job. The teardown correlation key.
          runner_name?: string | null;
        };
        repository?: { full_name?: string };
        // GitHub-App delivery: the installation whose id the server maps to a
        // tenant. Present on App-authed webhooks (required for the runner mint).
        installation?: { id?: number | string };
      };
      let lexicalJobId: string | null = null;
      try {
        lexicalJobId = canonicalWorkflowJobIdFromRaw(raw);
        if (lexicalJobId === null) return json({ error: "no workflow_job.id in payload" }, 400);
        evt = JSON.parse(raw) as typeof evt;
      } catch {
        return json({ error: "invalid JSON body" }, 400);
      }
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
      // The shared freeze applies only to the NEW queued admission leg. Parse
      // and route the event first so workflow_job.completed still reaches its
      // revoke/teardown/slot-release cleanup path during containment.
      if (evt.action === "queued" && admissionPaused(env.FABRIC_ADMISSION_PAUSED)) {
        return admissionPausedResponse();
      }
      // The stable correlation id across queued→completed for THIS job. The PAT
      // is minted under it (job_id) so completion can revoke the SAME PAT.
      const jobId = lexicalJobId;
      const decodedJobId: unknown = evt.workflow_job?.id;
      if (jobId === null || !((typeof decodedJobId === "number" && Number.isSafeInteger(decodedJobId) && String(decodedJobId) === jobId)
        || (typeof decodedJobId === "string" && decodedJobId === jobId))) return json({ error: "no workflow_job.id in payload" }, 400);

      // T3-W17 queued-intake gate. Completed events deliberately skip this block
      // and continue through the existing cleanup path below.
      if (evt.action === "queued") {
        const rawRepo = evt.repository?.full_name ?? "";
        const identity = normalizeRedriveIdentity(rawRepo, jobId);
        if (!identity) return json({ error: "no repository in payload" }, 400);
        const repo = identity.repo;
        const resolvedInstallation = resolveWebhookInstallationId(evt.installation?.id, repo, env.REPO_INSTALLATION_MAP);
        if (resolvedInstallation.invalid) return json({ error: "invalid installation id" }, 400);
        const installationId = resolvedInstallation.installationId;
        if (installationAllowlistArmed(env.INSTALLATION_ALLOWLIST)
          && !isInstallationAllowlisted(env.INSTALLATION_ALLOWLIST, installationId)) {
          logEvent("info", "webhook_installation_not_allowlisted", { jobId, repo, installationId });
          ctx.waitUntil(bumpMetrics(env, "webhook_installation_not_allowlisted"));
          return json({ ok: true, ignored: "installation not allowlisted", job_id: jobId }, 202);
        }
        let intake: ContainmentSwitch;
        try {
          intake = parseContainmentSwitch(env.AUTOSCALER_INTAKE_PAUSED);
          if (intake === "invalid") await observeInvalidConfig(env, "AUTOSCALER_INTAKE_PAUSED", env.AUTOSCALER_INTAKE_PAUSED as string);
          const authority = containmentAuthority(env);
          if (installationId && await authority.installationTombstoned(installationId)) {
            return json({ ok: true, ignored: "installation deleted", job_id: jobId }, 202);
          }
          const bodySha = await sha256Hex(rawBytes);
          const delivery = trimAsciiWhitespace(request.headers.get("x-github-delivery") ?? "");
          const eventId = delivery || await sha256Hex(`containment:v1\n${jobId}\n${evt.action}\n${bodySha}`);
          const bootstrapped = await authority.bootstrapContainedEventIndex(repo, jobId);
          if (bootstrapped.status === "blocked" || bootstrapped.status === "invalid") return json({ error: "containment pair authority unavailable" }, 503);
          // One DO transaction observes backlog/switch state and either admits
          // continuation or appends. A fresh arrival cannot race a draining head.
          const result = await authority.admitQueued({ schema_version: 1, event_id: eventId, received_at_ms: Date.now(), body_sha256: bodySha, raw_payload: raw, action: "queued", job_id: jobId, repo, installation_id: installationId, labels: mintLabels, effect_id: `containment:v1:${eventId}` }, intake);
          if (result.status === "conflict") return json({ error: "delivery id conflicts with different body" }, 409);
          if (result.status === "authority-busy") return json({ error: "containment redrive authority busy" }, 503);
          if (result.status === "redrive_owned") return json({ ok: true, redrive_owned: true, job_id: jobId }, 202);
          if (result.status !== "continued") {
            ctx.waitUntil(deliverInvalidConfig(env));
            if (intake === "normal") ctx.waitUntil(runContainmentDrain(env));
            return json({ ok: true, contained: true, deduped: result.status === "duplicate", job_id: jobId }, 202);
          }
        } catch {
          return json({ error: "containment authority unavailable" }, 503);
        }
      }

      // This visibility-only signal is deliberately after containment. A paused
      // event must produce zero success/continuation side effects of any kind.
      const unserved = unservedCapabilityClaims(mintLabels);
      if (unserved.length > 0) {
        console.warn(JSON.stringify({ event: "capability_claim_unserved", labels: unserved, served_instance_type: SERVED_INSTANCE_TYPE, repo: evt.repository?.full_name ?? "", job_id: jobId, note: "served the standard box; the requested shape does not exist in the fleet" }));
        await bumpMetrics(env, "capability_claim_unserved");
      }

      // ── workflow_job:completed ⇒ revoke the per-job CAS PAT (hardening) ──────
      // Shrinks the post-job window the PAT is valid (TTL is the backstop).
      // Best-effort + fail-open: a revoke failure never breaks the webhook.
      if (evt.action === "completed") {
        // Look up the DERIVED tenant stashed at spawn (fallback: wrangler's
        // CLW_TENANT for legacy/cold jobs). Used for revoke, the concurrency-slot
        // release, AND billing — so every completion acts on the RIGHT tenant.
        let derivedTenant: string | undefined;
        try {
          const authorityStore = jobAttributionStore(env);
          derivedTenant = (await readJobAttribution(authorityStore, jobId))?.tenant;
          // Migration bridge for jobs spawned before the immutable record was
          // introduced: `jtenant:` was also written from the server mint and is
          // still a durable, tenant-specific identity. Never fall back to the
          // deploy's CLW_TENANT or any in-memory/default value.
          if (!derivedTenant && env.RUNNER_JOB_PATS) {
            derivedTenant = (await env.RUNNER_JOB_PATS.get(jobTenantKey(jobId))) ?? undefined;
          }
        } catch (e) {
          // An invalid or ambiguous identity is never replaced with a deploy
          // default. Under-billing is safer than billing the wrong tenant.
          logEvent("error", "job_attribution_unusable", { jobId, error: (e as Error).message });
        }
        // The job is over ⇒ it is definitively not waiting on us. Drop any
        // provisional placement record so the reconciler never re-drives a job that
        // already ran (and so the common case costs ZERO GitHub API calls — this
        // clears the record long before the confirmation window would ask).
        await clearPlacementRecord(env, jobId);
        // Reservation cleanup is deliberately separate from legacy completion
        // cleanup. A verified `completed` may race the redrive owner's final
        // continuation; the DO either clears an already-COMPLETED tombstone or
        // latches observation on EFFECT_ELIGIBLE for that owner to resolve.
        const completedIdentity = normalizeRedriveIdentity(evt.repository?.full_name ?? "", jobId);
        if (completedIdentity && env.CONTAINMENT) {
          try {
            await containmentAuthority(env).clearCompletedRedrive(
              completedIdentity.repo,
              completedIdentity.job_id,
              redriveEffectId(completedIdentity.repo, completedIdentity.job_id),
            );
          } catch (e) {
            // A failed cleanup leaves the durable tombstone/latch for a later
            // verified completion delivery; it never weakens completion itself.
            logEvent("error", "containment_redrive_completion_cleanup_failed", { jobId, error: (e as Error).message });
          }
        }
        let revoked = false;
        try {
          revoked = await revokeCompletedJob(env, jobId, derivedTenant);
        } catch (e) {
          // Missing server-derived identity is a loud refusal, but must not
          // turn a GitHub completion delivery into a redelivery storm. The PAT
          // mapping remains durable for operator-visible repair.
          logEvent("error", "completion_revoke_refused", { jobId, error: (e as Error).message });
        }
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
        // Tell the customer they are nearing the included allowance BEFORE the
        // invoice does. Runs after the ledger write so the durable record exists
        // even if this best-effort warning path throws (it swallows its own
        // errors either way — a missed warning must never cost a completion).
        await warnIfNearVcpuCeiling(env, jobId, derivedTenant, evt.workflow_job);
        // Drop the derived-tenant stash (still needed for revoke + billing above;
        // the usage ledger above already captured the tenant durably for backfill).
        if (derivedTenant && env.RUNNER_JOB_PATS) {
          await env.RUNNER_JOB_PATS.delete(jobTenantKey(jobId)).catch(() => {
            /* best-effort: TTL is the backstop */
          });
        }
        // Keep the immutable attribution record after completion. Billing
        // settlement/outbox retry and any pending revoke/teardown obligation
        // may arrive in a later invocation; T4-W2 owns the eventual retention
        // policy once those obligations are durably settled.
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
        const tornDown = await teardownCompletedRunner(
          env,
          jobId,
          evt.workflow_job?.runner_name ?? undefined,
        );
        // Release capacity only after exact-handle teardown is confirmed. A
        // missing legacy handle has no provider obligation; an unreadable or
        // still-live handle retains the slot until a later completion/retry.
        const teardownPending = await teardownObligationPresent(
          env,
          jobId,
          evt.workflow_job?.runner_name ?? undefined,
        );
        let exactClaimReleased = !evt.workflow_job?.runner_name;
        if (tornDown || teardownPending === false) {
          try {
            const providerIdentity = evt.workflow_job?.runner_name;
            if (providerIdentity && env.CONCURRENCY_SLOTS) {
              const released = await concurrencySlots(env).releaseSpawnClaimForCompletion(jobId, providerIdentity);
              // `spawn:` remains a migration/evidence projection only. Its value
              // is never read to decide completion ownership.
              exactClaimReleased = released === "released";
              if (exactClaimReleased && env.RUNNER_JOB_PATS) await env.RUNNER_JOB_PATS.delete(`spawn:${jobId}`);
            }
          } catch (e) {
            logEvent("error", "spawn_claim_release_deferred", { jobId, error: (e as Error).message });
          }
        }
        if ((tornDown || teardownPending === false) && exactClaimReleased) await releaseConcurrencySlot(env, jobId);
        else if (teardownPending === true || teardownPending === null || !exactClaimReleased) {
          logEvent("error", "concurrency_slot_release_deferred", { jobId });
        }
        // Exact PAT leases are closed by the durable revocation authority above.
        // Also clean the historical job-scoped stash during migration.
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
      // NOTE: the rate limiter used to run HERE, before the repo and installation
      // id were resolved. It now runs AFTER them (below), because a refusal has
      // to dead-letter the job and the dead-letter record needs both.
      // The repo is the webhook's repository (full_name).
      const rawRepo = evt.repository?.full_name ?? "";
      const repo = normalizeRedriveIdentity(rawRepo, jobId)?.repo ?? rawRepo;
      // NOTE: the `!repo` 400 is deliberately NOT here. It moved BELOW the rate
      // limiter, because a queued+labeled job must consult the limiter even when
      // the payload names no repository (invariant I1: never fail-open to
      // unbounded). Rejecting first would have let a repo-less flood bypass the
      // limiter entirely — caught by the I1 regression test when this block was
      // first reordered.
      // Resolve the server authorization identity before durable admission.
      const resolvedInstallation = resolveWebhookInstallationId(evt.installation?.id, repo, env.REPO_INSTALLATION_MAP);
      if (resolvedInstallation.invalid) return json({ error: "invalid installation id" }, 400);
      const installationId = resolvedInstallation.installationId;
      if (env.CORELINK_RUNNER_MINT_AUTH_KEY && !installationId) {
        logEvent("info", "installation_id_missing", { jobId, repo });
      }
      if (!repo) return json({ error: "no repository in payload" }, 400);
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
      // A 202 acknowledges a recoverable command, including limiter refusals.
      // This inbox is separate from T3-W17's ordered containment backlog.
      try {
        const authority = containmentAuthority(env);
        const bodySha = await sha256Hex(raw);
        const eventId = trimAsciiWhitespace(request.headers.get("x-github-delivery") ?? "")
          || await sha256Hex(`containment:v1\n${jobId}\nqueued\n${bodySha}`);
        const admitted = !env.WEBHOOK_LIMITER || (await env.WEBHOOK_LIMITER.limit({ key: `spawn:${repo}` })).success;
        const result = await authority.normalIntakeEnqueue({
          schema_version: 1, event_id: eventId, body_sha256: bodySha, job_id: jobId,
          repo, installation_id: installationId, labels: mintLabels, received_at_ms: Date.now(),
        }, admitted ? 0 : 60_000);
        if (result.status === "conflict") return json({ error: "delivery id conflicts with different body" }, 409);
        if (result.status === "tombstoned") return json({ ok: true, ignored: "installation deleted", job_id: jobId }, 202);
        if (result.status === "full") return json({ error: "intake capacity unavailable", retryable: true }, 503);
        if (admitted) ctx.waitUntil(runNormalIntakeDrain(env, eventId));
        else ctx.waitUntil(bumpMetrics(env, "webhook_rate_limited"));
        return json({ ok: true, queued: true, rate_limited: !admitted, job_id: jobId }, 202);
      } catch {
        return json({ error: "durable intake unavailable", retryable: true }, 503);
      }
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
        // Failed ticket redemption is intentionally a single public shape:
        // forged, closed, expired, and unknown leases reveal no state oracle.
        // A valid ticket remains multi-use until its lease deadline.
        return json({ error: "no such lease" }, 404);
      }
    }

    // ── POST /v1/leases/{lease_id}/runner-diag — box→Worker diagnostic sink ────
    // The runner container has only CF-internal egress + no `wrangler containers
    // logs`, so a `./run.sh --jitconfig` registration FAILURE inside the box is
    // otherwise invisible. entrypoint.sh POSTs the tail of run.sh's output here on a
    // non-zero exit; we `logEvent` it so it surfaces in `wrangler tail`. Ticket-less
    // (the box holds no bearer), capped, and it carries NO secret (run.sh never
    // echoes the jitconfig). Keyed by leaseId==jobId for correlation.
    //
    // ⚠️ NOT anonymous (2026-08-24): this route used to answer BEFORE the bearer
    // gate below, so anyone could write 3000 attacker-chosen characters into the
    // logs as an error-level `runner_diag` under ANY jobId. A credential is not an
    // option here — a COLD box holds no CLW_CRED_TICKET at all, and requiring one
    // would kill diagnostics for precisely the boxes that fail most. The gate
    // instead requires the named job to be ACTUALLY CLAIMED (a live spawn-claim in
    // KV): an unknown jobId gets a small uniform response that reveals nothing
    // about which ids exist.
    //
    // ACCEPTED COST, instrumented deliberately: a job running longer than
    // SPAWN_CLAIM_TTL_S (7200 s) has no claim left, so its diag POST is refused.
    // That refusal bumps `runner_diag_no_claim` + logs at info level so we can SEE
    // if it ever bites instead of discovering it as silence.
    {
      const diag = pathname.match(/^\/v1\/leases\/([^/]+)\/runner-diag$/);
      if (request.method === "POST" && diag) {
        const jobId = decodeURIComponent(diag[1]);
        let claimed = false;
        try {
          claimed = !!(await env.RUNNER_JOB_PATS?.get(`spawn:${jobId}`));
        } catch {
          /* KV hiccup ⇒ treated as unclaimed below; never 5xx a diagnostic sink */
        }
        // A claim proves the job EXISTS; it does not prove the caller is its box.
        // Job ids are public on a public repo, so a targeted flood against a real
        // in-flight job stays possible — bound it with the limiter this Worker
        // already declares (wrangler.jsonc: 30 req / 60 s), keyed per job so one
        // abused id cannot drown the diagnostics of every other box.
        if (env.WEBHOOK_LIMITER) {
          const { success } = await env.WEBHOOK_LIMITER.limit({ key: `diag:${jobId}` });
          if (!success) {
            ctx?.waitUntil?.(bumpMetrics(env, "runner_diag_rate_limited"));
            return json({ ok: true }, 200);
          }
        }
        if (!claimed) {
          ctx?.waitUntil?.(bumpMetrics(env, "runner_diag_no_claim"));
          logEvent("info", "runner_diag_refused_unknown_job", { jobId });
          return json({ ok: true }, 200);
        }
        const raw = await request.text().catch(() => "");
        logEvent("error", "runner_diag", { jobId, output: raw.slice(0, 3000) });
        return json({ ok: true }, 200);
      }
    }

    // Admission, command execution and lifecycle operations have separate authority.
    if (!controlAuthed(request, env)) return unauthorized();

    const jobStatus = pathname.match(/^\/v1\/jobs\/([^/]+)\/status$/);
    if (request.method === "GET" && jobStatus) {
      const jobId = decodeURIComponent(jobStatus[1]);
      if (!jobId || jobId.includes("/")) return json({ error: "invalid job id" }, 400);
      try {
        const refusal = await concurrencySlots(env).getRefusal(jobId);
        return refusal ? json(refusal, 200) : json({ error: "job status unavailable" }, 404);
      } catch {
        return json({ error: "concurrency authority unavailable", retryable: true }, 503);
      }
    }

    // POST /v1/spawn
    if (request.method === "POST" && pathname === "/v1/spawn") {
      // This is a NEW provider admission. Existing status, exec, teardown, and
      // egress-cutoff routes remain available so already-issued handles drain.
      if (admissionPaused(env.FABRIC_ADMISSION_PAUSED)) {
        return admissionPausedResponse();
      }
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
          // refuse to spawn one — mirroring controlAuthed()'s "no secret ⇒ deny" gate
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
          // Retry the DO start on a transient CF reset (fresh handle each try) —
          // and DESTROY each superseded attempt, which otherwise holds a container
          // instance until `sleepAfter` with nothing referencing it.
          const { handle } = await startWithRetry<undefined>(
            async () => undefined,
            (h) =>
              getContainer(env.CHECK_HOST_CONTAINER, h).start({
                envVars: {
                  ...body.env,
                  TOOLCHAIN_DIGEST: body.toolchain_digest!,
                  // Track-C C2b (now REQUIRED, guaranteed present by the check above):
                  // provider ingress only. The entrypoint converts it to the
                  // mode-0400 file consumed by the durable exec-server.
                  EXEC_SERVER_AUTH_TOKEN: execAuthToken,
                  EXEC_SERVER_AUTH_TOKEN_FILE,
                },
                enableInternet: true,
              }),
            (h, _p, reason) => abandonContainer(env, h, "check", reason),
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
        // retry the DO start on a transient CF reset (fresh handle each attempt),
        // and DESTROY each superseded attempt rather than leaving it to hold a
        // fleet instance until `sleepAfter`.
        //
        // ⚠️ On THIS path the JIT arrives in `body.env` from the fabric, so the
        // Worker cannot mint a fresh one per attempt the way the autoscaler does:
        // every attempt necessarily boots with the SAME single-use registration.
        // Destroying the superseded attempt is therefore not just capacity hygiene
        // here, it is what stops a late box from consuming the registration the
        // surviving box needs.
        const { handle } = await startWithRetry<undefined>(
          async () => undefined,
          (h) => getContainer(env.RUNNER_CONTAINER, h).startWithEnv(body.env),
          (h, _p, reason) => abandonContainer(env, h, "runner", reason),
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
      // Only a confirmed provider teardown permits the fabric to free capacity.
      // An exception is retryable uncertainty, including an unavailable binding.
      try {
        const container =
          body.mode === "check"
            ? getContainer(env.CHECK_HOST_CONTAINER, handle)
            : getContainer(env.RUNNER_CONTAINER, handle);
        await container.teardown();
      } catch {
        logEvent("error", "teardown_route_failed", {
          handle,
          mode: body.mode ?? "runner",
        });
        return json({ error: "provider teardown unconfirmed" }, 503);
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
type RedriveOrphanedJobsDependencies = {
  listOrphanRunnerJobs?: typeof listOrphanRunnerJobs;
  releaseSpawnClaim?: typeof releaseSpawnClaim;
  claimSpawn?: typeof claimSpawn;
  driveSpawn?: typeof driveSpawn;
  recordOrphan?: typeof recordOrphan;
};

export async function redriveOrphanedJobs(
  env: Env,
  ctx: ExecutionContext,
  configured: string | undefined,
  now: number,
  dependencies: RedriveOrphanedJobsDependencies = {},
): Promise<void> {
  // Re-drive is a NEW spawn admission; leave lifecycle cleanup to its own
  // scheduled paths while the shared freeze is active.
  if (admissionPaused(env.FABRIC_ADMISSION_PAUSED)) return;
  // Optional only for deterministic callers: production continues to invoke the
  // same functions at the same seams when no dependency object is supplied.
  const list = dependencies.listOrphanRunnerJobs ?? listOrphanRunnerJobs;
  const release = dependencies.releaseSpawnClaim ?? releaseSpawnClaim;
  const claim = dependencies.claimSpawn ?? claimSpawn;
  const drive = dependencies.driveSpawn ?? driveSpawn;
  const orphan = dependencies.recordOrphan ?? recordOrphan;
  let redriveState: ContainmentSwitch;
  try {
    redriveState = parseContainmentSwitch(env.AUTOSCALER_REDRIVE_PAUSED);
    if (redriveState === "invalid") await observeInvalidConfig(env, "AUTOSCALER_REDRIVE_PAUSED", env.AUTOSCALER_REDRIVE_PAUSED as string);
  } catch { return; }
  if (redriveState !== "normal") return;
  if (!(await containmentRedriveAuthorityReadable(env))) return;
  const reservationAuthority = containmentAuthority(env);
  const staticRepos = parseReconcilerRepos(env.RECONCILER_REPOS);
  let registryRepos: ReconcilerRepository[] | null = null;
  if (env.RECONCILER_REGISTRY_URL?.trim()) {
    // A configured registry provides candidates, never eligibility. Every
    // candidate must also appear in that installation's GitHub repository
    // inventory before it may reach the existing scan and reservation path.
    // Registry or membership uncertainty is not permission to use a stale
    // static list, so this tick remains read-only.
    try {
      const candidates = await discoverAuthorizationCandidates(env);
      if (candidates === null) return;
      const liveCandidates: ReconcilerRepository[] = [];
      for (const candidate of candidates) {
        if (!(await installationIsTombstoned(env, candidate.installationId))) liveCandidates.push(candidate);
      }
      registryRepos = await confirmInstallationRepositories(env, liveCandidates, now);
    } catch (e) {
      logEvent("error", "reconciler_registry_membership_failed", { error: (e as Error).message });
      return;
    }
    if (registryRepos === null) return;
  }
  const candidates: Array<{ repo: string; installationId?: string }> = registryRepos
    ? registryRepos.map(entry => ({ repo: entry.repo, installationId: entry.installationId }))
    : staticRepos.map(repo => ({ repo }));
  if (candidates.length === 0) return; // opt-in: no allowlist/registry ⇒ reconciler off
  if (!env.GITHUB_WEBHOOK_SECRET || !env.GITHUB_MINT_TOKEN) return; // autoscaler not configured
  for (const candidate of candidates) {
    const repo = candidate.repo;
    const redriveInstallationId = candidate.installationId
      ?? installationIdForRepo(env.REPO_INSTALLATION_MAP, repo);
    // Do not even mint an installation token or list GitHub after deletion.
    // A tombstone wins before every redrive reservation, handoff and KV write.
    if (redriveInstallationId && await installationIsTombstoned(env, redriveInstallationId)) continue;
    let scanEnv: Env = env;
    if (candidate.installationId) {
      // Registry entries carry the only installation identity accepted for this
      // scan. The token is scoped to that installation before any GitHub list.
      let token: string;
      try {
        token = await mintJitAuthToken(env, candidate.installationId);
      } catch (e) {
        logEvent("error", "reconciler_installation_token_failed", {
          repo,
          installationId: candidate.installationId,
          error: (e as Error).message,
        });
        continue;
      }
      if (!token) continue;
      scanEnv = { ...env, GITHUB_RECONCILER_TOKEN: token };
    }
    const orphans = await list(scanEnv, repo, configured, RECONCILE_MIN_AGE_MS, now);
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
      let redriveRepo = repo;
      let redriveJobId = jobId;
      let reservation: ContainmentRedriveReservation | null = null;
      if (reservationAuthority) {
        const identity = normalizeRedriveIdentity(repo, jobId);
        if (!identity) continue;
        // The containment ledger owns its canonical key. Keep the authorized
        // registry spelling in redriveRepo for prepareSpawn and its downstream
        // authorize/mint request; normalizing it here would change that external
        // identity before the issuer sees it.
        let admitted: Awaited<ReturnType<ContainmentDO["reserveRedriveCandidate"]>>;
        try {
          const bootstrapped = await reservationAuthority.bootstrapContainedEventIndex(identity.repo, identity.job_id);
          if (bootstrapped.status === "blocked" || bootstrapped.status === "invalid") continue;
          admitted = await reservationAuthority.reserveRedriveCandidate(identity.repo, identity.job_id, now);
        } catch {
          continue; // authority uncertainty is fail-closed before any KV seam
        }
        if (admitted.status !== "reserved" || !admitted.reservation) continue;
        reservation = admitted.reservation;
      }
      const reInstallationId = redriveInstallationId;
      // ── Age-gate the force-release (2026-08-24) ──────────────────────────────
      // "queued ≥ 90 s with no runner" is ALSO what a healthy-but-slow spawn looks
      // like: the placement machinery itself waits PLACEMENT_CONFIRM_GRACE_MS
      // (180 s) before even asking GitHub, calling that the slowest healthy boot.
      // Force-releasing a claim that young yanks it from a spawn still in flight —
      // second mint, second JIT registration, second container. So the release now
      // keys on the CLAIM's age, never on the job's age: only a claim old enough
      // that no healthy boot could still be driving it may be cleared. A younger
      // claim means this job is skipped THIS tick and left exactly as found.
      //
      // ⚠️ LEGACY VALUES: a claim written before claims were timestamped reads "1"
      // and carries NO timestamp. That case is deliberately treated as OLD (allow
      // the force-release), not as young: SPAWN_CLAIM_TTL_S is 7200 s, so treating
      // an un-aged claim as young would block orphan RECOVERY for up to two hours
      // during the rollout window — and stranding a real orphan is worse than the
      // duplicate-spawn race this gate closes. The window is bounded and
      // self-clearing as pre-change claims TTL out.
      const rawClaim = await env.RUNNER_JOB_PATS?.get(`spawn:${redriveJobId}`);
      const claimAgeMs = spawnClaimAgeMs(rawClaim, now);
      if (claimAgeMs !== null && claimAgeMs < PLACEMENT_CONFIRM_GRACE_MS) {
        // A live spawn is probably still in flight — leave its claim alone.
        continue;
      }
      if (reservation && reservationAuthority) {
        const ownedReservation = reservation;
        const ownedAuthority = reservationAuthority;
        const retryEpoch = await retryOwnerEpochId(ownedReservation.effect_id, ownedReservation.owner, ownedReservation.epoch);
        if (!await recordRetryAttempt(env, redriveJobId, retryEpoch, 0)) continue;
        ctx.waitUntil((async () => {
          const effect = ownedReservation.effect_id;
          const spawnOpts: ContainmentDriveOpts = { jobId: redriveJobId, repo: redriveRepo, installationId: reInstallationId, labels, credential_source: "installation-only" };
          let prepared: ContainerEnvResult | undefined;
          const spawnClaim = spawnClaimCallbacks(env, redriveJobId);
          const useInjectedClaim = !!dependencies.claimSpawn;
          const result = await runCanonicalEffect({
            ledger: ownedAuthority,
            tuple: await redriveOwnerTuple(ownedReservation.repo, ownedReservation.job_id, effect, ownedReservation.owner, ownedReservation.token, ownedReservation.epoch),
            opts: spawnOpts,
            provider: "cloudflare-container",
            resource_id: `job:${ownedReservation.repo}/${ownedReservation.job_id}`,
            idempotency_key: effect,
            admit: async () => (await ownedAuthority.beginReservedEffect(ownedReservation.repo, ownedReservation.job_id, ownedReservation.owner, ownedReservation.token, ownedReservation.epoch, ownedReservation.path, effect)).status === "eligible",
            beforeClaim: async () => {
              if (useInjectedClaim) await release(env.RUNNER_JOB_PATS!, redriveJobId);
              if (drive === driveSpawn) prepared = await prepareSpawn(env, spawnOpts);
            },
            abandonPreparation: () => abandonPreparedSpawn(env, ownedAuthority, redriveJobId, prepared),
            claim: useInjectedClaim ? () => claim(env.RUNNER_JOB_PATS!, redriveJobId) : spawnClaim.claim,
            release: useInjectedClaim ? () => release(env.RUNNER_JOB_PATS!, redriveJobId) : spawnClaim.release,
            beforeDrive: async () => useInjectedClaim ? true : spawnClaim.active(),
            drive: async driveOpts => {
              await bindContainmentSpawnClaim(env, driveOpts);
              const receipt = await drive(env, { ...driveOpts, bindProviderIdentity: spawnClaim.bindProvider }, prepared);
              if (!useInjectedClaim && receipt && !(await spawnClaim.bindProvider(receipt.provider_signature))) throw new Error("spawn claim provider binding unavailable");
              return receipt;
            },
            finalize: async () => {
              const terminal = await ownedAuthority.completeRedrive(ownedReservation.repo, ownedReservation.job_id, ownedReservation.owner, ownedReservation.token, ownedReservation.epoch, effect);
              return terminal.status === "completed" || terminal.status === "cleared_after_completion";
            },
          });
          if (result.status !== "committed" || !result.finalized) logEvent("error", "contained_redrive_blocked", { jobId: redriveJobId, repo: redriveRepo, status: result.status, ...(result.status !== "committed" ? { reason: result.reason } : {}) });
        })());
        continue;
      }
      const handoff = await claimReconcileHandoff(env.RUNNER_JOB_PATS, {
        schema_version: 1,
        repo: redriveRepo,
        job_id: redriveJobId,
        installation_id: reInstallationId,
        labels,
        enqueued_at_ms: now,
      }, now);
      if (!handoff) continue;
      if (!await recordRetryAttempt(env, redriveJobId, `legacy-redrive:${redriveRepo}:${redriveJobId}`, 0)) {
        await releaseReconcileHandoff(env.RUNNER_JOB_PATS, redriveRepo, redriveJobId);
        continue;
      }
      const spawnClaim = spawnClaimCallbacks(env, redriveJobId);
      const useInjectedClaim = !!dependencies.claimSpawn;
      if (useInjectedClaim ? await claim(env.RUNNER_JOB_PATS!, redriveJobId) : await spawnClaim.claim()) {
        logEvent("info", "reconciler_redrive", {
          jobId: redriveJobId,
          repo: redriveRepo,
          warm: !!reInstallationId,
        });
        ctx.waitUntil((async () => {
          try {
            if (!useInjectedClaim && !(await spawnClaim.active())) throw new Error("spawn claim authority unavailable");
            await drive(env, { jobId: redriveJobId, repo: redriveRepo, installationId: reInstallationId, labels, bindProviderIdentity: spawnClaim.bindProvider });
          } catch (e) {
            if (useInjectedClaim) await release(env.RUNNER_JOB_PATS!, redriveJobId);
            else await spawnClaim.release();
            if (!(e instanceof SpawnRefusedError)) {
              await bumpMetrics(env, "spawn_failed");
              logEvent("error", "spawn_drive_failed", { jobId: redriveJobId, error: (e as Error).message });
            }
            await orphan(env, { jobId: redriveJobId, repo: redriveRepo, installationId: reInstallationId, labels });
          } finally {
            // A successful spawn leaves the ordinary spawn claim as the
            // lifetime idempotency record; failures release it and this marker
            // so a later authoritative poll can hand the job off again.
            await releaseReconcileHandoff(env.RUNNER_JOB_PATS, redriveRepo, redriveJobId);
          }
          })());
      } else {
        await releaseReconcileHandoff(env.RUNNER_JOB_PATS, redriveRepo, redriveJobId);
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
  now: number,
  drive: (
    env: Env,
    opts: ContainmentDriveOpts,
    prepared?: ContainerEnvResult,
  ) => Promise<ProviderDriveReceipt | void> = driveSpawn,
  // Injected for the same reason as `drive` — so the placement-confirmation
  // branches are testable without reaching the real GitHub API. Takes the
  // installation id from the record (same seam as `fetchJobObservation`).
  verify: (
    env: Env,
    repo: string,
    jobId: string,
    installationId: string,
  ) => Promise<{ status?: string; runner_id?: number | null } | null> = fetchJobPlacement,
): Promise<void> {
  // Dead-letter retry is also a NEW spawn admission. Existing teardown/status
  // retries continue independently from the scheduled tick.
  if (admissionPaused(env.FABRIC_ADMISSION_PAUSED)) return;
  let redriveState: ContainmentSwitch;
  try {
    redriveState = parseContainmentSwitch(env.AUTOSCALER_REDRIVE_PAUSED);
    if (redriveState === "invalid") await observeInvalidConfig(env, "AUTOSCALER_REDRIVE_PAUSED", env.AUTOSCALER_REDRIVE_PAUSED as string);
  } catch { return; }
  if (redriveState !== "normal") return;
  if (!(await containmentRedriveAuthorityReadable(env))) return;
  const reservationAuthority = containmentAuthority(env);
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
    let jobId = name.slice(ORPHAN_KEY_PREFIX.length);
    let deferredPlacementUnconfirmed: { repo: string; waitedMs: number; attempts: number } | null = null;
    // Parse the record (a malformed/absent value ⇒ null ⇒ the "missing" branch).
    let rec: OrphanRecord | null = null;
    try {
      const raw = await kv.get(name);
      rec = raw ? (JSON.parse(raw) as OrphanRecord) : null;
    } catch {
      rec = null;
    }
    // A contained re-drive must use the exact validated orphan identity. Do not
    // repair malformed values or substitute a first-party installation: that
    // would turn a stale dead-letter into an authorization boundary bypass.
    if (reservationAuthority) {
      if (!rec) continue;
      const orphan = rec;
      const identity = normalizeRedriveIdentity(orphan.repo, jobId);
      const labelsAreStrings = Array.isArray(orphan.labels)
        && orphan.labels.every((label) => typeof label === "string");
      const managedLabels = labelsAreStrings
        ? matchManagedLabels(orphan.labels, env.AUTOSCALER_LABEL)
        : null;
      const labelsAreUnique = labelsAreStrings
        && new Set(orphan.labels).size === orphan.labels.length;
      const labelsMatchExactly = !!managedLabels
        && managedLabels.length === orphan.labels.length
        && managedLabels.every((label, index) => label === orphan.labels[index]);
      // The first-party map can assert an expected installation for its own
      // repos; it is validation-only here. An unmapped external orphan keeps its
      // stored installation — there is no independent expected-id authority, so
      // canonical decimal format + repo/job + exact managed labels are frozen.
      const expectedInstallationId = identity
        ? installationIdForRepo(env.REPO_INSTALLATION_MAP, identity.repo)
        : "";
      if (!identity || !labelsAreStrings || !labelsAreUnique || !labelsMatchExactly || typeof orphan.installationId !== "string" || canonicalInstallationId(orphan.installationId) !== orphan.installationId || (expectedInstallationId !== "" && expectedInstallationId !== orphan.installationId)) continue;
      // From this point onward, every effectful seam uses the one normalized
      // identity. Keep the orphan's installation id byte-for-byte; no map or
      // first-party fallback is ever substituted into this retry.
      jobId = identity.job_id;
      rec = { ...orphan, repo: identity.repo };
    }
    // ⛔ TERMINAL: this job died IN FLIGHT (the stranded sweep classified it from
    // GitHub's own answers). Its accounting was already released; the record is
    // kept for visibility until it TTLs. Re-driving in-flight work is a separate
    // decision that has not been made, so it is never retried here.
    if (rec?.stranded != null) continue;
    // KV is only a projection. Read the durable count before placement checks
    // or the cap decision, so stale KV cannot reset the bound.
    if (!rec) continue;
    // This must precede placement verification, retry epoch writes, reservation
    // acquisition and every provider/GitHub retry seam.
    if (await installationIsTombstoned(env, rec.installationId)) continue;
    const durableAttempts = await readRetryAttempts(env, jobId);
    if (durableAttempts === null) continue;
    rec = { ...rec, attempts: Math.max(rec.attempts, durableAttempts) };
    // ── Placement confirmation (2026-08-03) ──────────────────────────────────
    // A record carrying `placedMs` is a spawn we believe SUCCEEDED. Most of these
    // are healthy in-flight jobs, so the default action is to do nothing at all.
    // Only once the grace window has elapsed with no confirmation do we ask GitHub
    // about that one job — and only an authoritative "still queued, still no
    // runner" re-drives it. Every other answer (placed, or an unreachable API)
    // leaves the record untouched for a later tick, so this can never duplicate a
    // running job.
    if (rec) {
      const placement = placementConfirmStep(rec, now, PLACEMENT_CONFIRM_GRACE_MS);
      if (placement.action === "within_grace") continue; // booting — leave it alone
      if (placement.action === "verify") {
        // The record carries the installation id the spawn was WARM-minted with —
        // pass it so the verification authenticates per-installation (FIX 1).
        const verdict = jobPlacementVerdict(
          await verify(env, rec.repo, jobId, rec.installationId),
        );
        if (verdict === "placed") {
          // The box did come online (or the job is already over) — drop the record.
          await kv.delete(name).catch(() => {
            /* best-effort: the key TTL-expires */
          });
          continue;
        }
        if (verdict === "unknown") {
          // We could not tell. Do NOT re-drive on ignorance — leave the record and
          // ask again next tick, bounded by ORPHAN_TTL_S like everything else.
          logEvent("info", "placement_verify_unknown", { jobId, repo: rec.repo });
          continue;
        }
        // "lost": the container we started never claimed the job. Fall through to
        // the normal retry path — which BUMPS `attempts`, so a job whose box can
        // never come online (a bad image, a broken registration) still dead-letters
        // in MAX_ORPHAN_ATTEMPTS ticks instead of respawning forever on our COGS.
        // Clearing `placedMs` returns the record to the plain dead-letter lifecycle.
        //
        // ⛔ THE RE-DRIVE DELIBERATELY DOES NOT DESTROY THE BOX IT REPLACES.
        // The only identifier this record carries is the JOB id, and "job A is still
        // queued" does not mean "the box started for A is idle" — GitHub assigns by
        // label match, so that box is frequently running somebody ELSE's job. Killing
        // it on a job-keyed lookup is precisely the correlation that SIGKILLed five
        // live jobs on 2026-08-02; doing it here would re-open that incident to buy
        // back a slot.
        //
        // It does not need to be bought back that way any more. Whatever that box is,
        // the keep-alive sweep now decides its fate on ITS OWN runner's reported
        // state: if it is running the other job it stays renewed (correct — it is
        // working), and if it is genuinely idle or never registered the sweep stops
        // renewing it and `sleepAfter` reclaims it within one idle window. That
        // collapses the leak this branch used to leave from ~2 h (the binding TTL) to
        // ~15 min, with no new kill path and no new correlation to get wrong.
        // Explicit teardown here would need a runner-keyed lookup from a job-keyed
        // record; the ~12 minutes it would save do not justify inventing one.
        if (reservationAuthority) {
          // Do not emit a mutation-capable metric before this candidate holds
          // its tuple/eligibility fence. Legacy fixture behavior remains eager.
          deferredPlacementUnconfirmed = { repo: rec.repo, waitedMs: placement.waitedMs, attempts: rec.attempts };
        } else {
          logEvent("error", "placement_unconfirmed", {
            jobId,
            repo: rec.repo,
            waitedMs: placement.waitedMs,
            attempts: rec.attempts,
          });
          await bumpMetrics(env, "placement_unconfirmed");
        }
        rec = { ...rec, placedMs: undefined };
      }
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
    let reservation: ContainmentRedriveReservation | null = null;
    if (reservationAuthority) {
      const identity = normalizeRedriveIdentity(rec!.repo, jobId);
      if (!identity) continue;
      let admitted: Awaited<ReturnType<ContainmentDO["reserveRedriveCandidate"]>>;
      try {
        const bootstrapped = await reservationAuthority.bootstrapContainedEventIndex(identity.repo, identity.job_id);
        if (bootstrapped.status === "blocked" || bootstrapped.status === "invalid") continue;
        admitted = await reservationAuthority.reserveRedriveCandidate(identity.repo, identity.job_id, now);
      } catch {
        continue; // authority uncertainty precedes every retry mutation
      }
      if (admitted.status !== "reserved" || !admitted.reservation) continue;
      reservation = admitted.reservation;
    }
    // Commit the retry epoch before any external claim. The authority's result
    // is the count we project into KV, so stale KV cannot reset a higher durable
    // count and an authority failure cannot produce a retry side effect.
    const retryEpoch = reservation && reservationAuthority
      ? await retryOwnerEpochId(reservation.effect_id, reservation.owner, reservation.epoch)
      : `legacy-retry:${step.nextAttempts}`;
    const retryCommit = await recordRetryAttempt(env, jobId, retryEpoch, rec!.attempts);
    if (!retryCommit) continue;
    const bumped: OrphanRecord = { ...(rec as OrphanRecord), attempts: retryCommit.attempts };
    if (reservation && reservationAuthority) {
      const ownedReservation = reservation;
      const ownedAuthority = reservationAuthority;
      const effect = ownedReservation.effect_id;
      const spawnOpts: ContainmentDriveOpts = { jobId: ownedReservation.job_id, repo: bumped.repo, installationId: bumped.installationId, labels: bumped.labels, credential_source: "installation-only" };
      let prepared: ContainerEnvResult | undefined;
      const spawnClaim = spawnClaimCallbacks(env, ownedReservation.job_id);
      const result = await runCanonicalEffect({
        ledger: ownedAuthority,
        tuple: await redriveOwnerTuple(ownedReservation.repo, ownedReservation.job_id, effect, ownedReservation.owner, ownedReservation.token, ownedReservation.epoch),
        opts: spawnOpts,
        provider: "cloudflare-container",
        resource_id: `job:${ownedReservation.repo}/${ownedReservation.job_id}`,
        idempotency_key: effect,
        admit: async () => (await ownedAuthority.beginReservedEffect(ownedReservation.repo, ownedReservation.job_id, ownedReservation.owner, ownedReservation.token, ownedReservation.epoch, ownedReservation.path, effect)).status === "eligible",
        beforeClaim: async () => {
          if (deferredPlacementUnconfirmed) {
            logEvent("error", "placement_unconfirmed", { jobId, ...deferredPlacementUnconfirmed });
            await bumpMetrics(env, "placement_unconfirmed");
          }
          await kv.put(name, JSON.stringify(bumped), { expirationTtl: ORPHAN_TTL_S });
          if (drive === driveSpawn) prepared = await prepareSpawn(env, spawnOpts);
        },
        abandonPreparation: () => abandonPreparedSpawn(env, ownedAuthority, ownedReservation.job_id, prepared),
        claim: spawnClaim.claim,
        release: spawnClaim.release,
        beforeDrive: spawnClaim.active,
        drive: async driveOpts => {
          await bindContainmentSpawnClaim(env, driveOpts);
          const receipt = await drive(env, { ...driveOpts, bindProviderIdentity: spawnClaim.bindProvider }, prepared);
          if (receipt && !(await spawnClaim.bindProvider(receipt.provider_signature))) throw new Error("spawn claim provider binding unavailable");
          return receipt;
        },
        finalize: async () => {
          const terminal = await ownedAuthority.completeRedrive(ownedReservation.repo, ownedReservation.job_id, ownedReservation.owner, ownedReservation.token, ownedReservation.epoch, effect);
          return terminal.status === "completed" || terminal.status === "cleared_after_completion";
        },
      });
      if (result.status !== "committed" || !result.finalized) logEvent("error", "contained_orphan_retry_blocked", { jobId, repo: bumped.repo, status: result.status, ...(result.status !== "committed" ? { reason: result.reason } : {}) });
      continue;
    }
    await kv
      .put(name, JSON.stringify(bumped), { expirationTtl: ORPHAN_TTL_S })
      .catch(() => {
        /* best-effort: a failed bump just means next tick re-reads the old count */
      });
    // Idempotent: if the job is already claimed (a live path / another tick won
    // it), skip this tick and LEAVE the record for later.
    const spawnClaim = spawnClaimCallbacks(env, jobId);
    if (!(await spawnClaim.claim())) continue;
    try {
      if (!(await spawnClaim.active())) throw new Error("spawn claim authority unavailable");
      const receipt = await drive(env, {
        jobId,
        repo: bumped.repo,
        installationId: bumped.installationId,
        labels: bumped.labels,
        ...(reservation ? { credential_source: "installation-only" as const } : {}),
        bindProviderIdentity: spawnClaim.bindProvider,
      });
      if (receipt && !(await spawnClaim.bindProvider(receipt.provider_signature))) throw new Error("spawn claim provider binding unavailable");
      // Re-driven ⇒ do NOT delete the record here.
      //
      // This used to delete it, which was correct only while "drive returned" meant
      // "the job is placed". It does not: `driveSpawn` returns as soon as the
      // CONTAINER started, and the whole point of the placement record is that a
      // started container is not a placed job. Deleting here would throw away the
      // provisional record `driveSpawn` just wrote — and it would do so precisely
      // for the jobs already known to be in trouble, which are the likeliest to fail
      // to come online a second time.
      //
      // The record's lifecycle now belongs to `recordPlacement` (written on a
      // successful spawn) and to the confirmation above / `workflow_job.completed`
      // (which clear it). The spawn claim is still left to TTL-expire, blocking
      // redeliveries for the job's lifetime, same as the live path.
      logEvent("info", "orphan_retry_recovered", {
        jobId,
        repo: bumped.repo,
        attempts: bumped.attempts,
      });
    } catch (e) {
      // Release the claim either way so a later tick (or the live path) can re-drive.
      await spawnClaim.release();
      if (e instanceof SpawnRefusedError) {
        // BACKPRESSURE, not failure — the fleet was full again this tick. Do NOT
        // consume the 3-strike budget: that budget bounds genuine errors, and
        // spending it on refusals would mean any burst lasting more than
        // MAX_ORPHAN_ATTEMPTS ticks still loses every job behind the ceiling —
        // exactly the bug this whole change exists to fix. So write the record
        // back with the ORIGINAL attempt count (un-bump), bounded instead by the
        // absolute ORPHAN_TTL_S window from firstRecordedMs.
        const step = orphanRefusalStep(rec as OrphanRecord, now);
        if (step.action === "giveup") {
          await kv.delete(name).catch(() => {
            /* best-effort: the key TTL-expires */
          });
          // LOUD: a job we waited the full window for and never placed is a real
          // capacity fault, not routine backpressure. It is the signal that the
          // fleet cap is undersized for this tenant's load.
          logEvent("error", "orphan_refusal_giveup", {
            jobId,
            repo: (rec as OrphanRecord).repo,
            waitedS: step.waitedS,
            reason: e.reason,
          });
          await bumpMetrics(env, "spawn_at_ceiling");
          continue;
        }
        await kv
          .put(name, JSON.stringify(rec), { expirationTtl: step.ttlS })
          .catch(() => {
            /* best-effort: next tick re-reads whatever survived */
          });
        logEvent("info", "orphan_refusal_waiting", {
          jobId,
          repo: (rec as OrphanRecord).repo,
          waitedS: step.waitedS,
          reason: e.reason,
        });
        continue;
      }
      // Genuine failure ⇒ LEAVE the (bumped) record for the next tick.
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
