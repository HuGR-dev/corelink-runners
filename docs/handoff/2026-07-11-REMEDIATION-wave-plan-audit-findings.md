# Remediation WAVE PLAN — 6-lens audit findings (techlead orchestration)

**Baseline:** `main @ ee2f9e0` · **2026-07-11** · Source of findings:
`2026-07-11-AUDIT-findings-external-customer-path-gap.md`. This is the FROZEN decomposition +
per-WP contract each dispatched agent is held to. The lead (orchestrator) cold-verifies every
returned WP against its DoD before merge; nothing merges on an agent's self-report.

---

## Section 0 — Ownership triage (what a fan-out CAN and CANNOT close)

| Class | Findings | Fannable? |
|---|---|---|
| **BUILDABLE (this repo)** | App-token minting CODE, multi-tenant hardening (rate-limit/billing/dedup), fabricd counter completeness, relay/runbook docs | ✅ fan-out |
| **OWNER-GATED (config/secret/$)** | bind App private key + app_id secrets; raise `max_instances`; App `Administration:write` perm | ❌ owner action (WP-4 documents the exact steps) |
| **CROSS-REPO** | route the App `workflow_job` webhook → spawn-worker (App config = owner; or signup-worker fan-out = server-TL); corelink-server billing aggregator | ❌ relay (WP-4) |

**Consequence:** the fan-out closes the **CODE** half of the CRITICAL external gap (App-token
minting) + all in-repo hardening. It does NOT by itself make an external customer live — that also
needs the owner/cross-repo webhook routing + secrets (WP-4 makes those a crisp, verifiable list).

## Section 1 — go/no-go (disjointness)

Shared file = `deploy/cloudflare/src/index.ts` (mintJit + webhook handler). To avoid AP-1
(merge-hell), slices are made disjoint by **module + crate + worktree isolation**:
- WP-1 lives in a **NEW file** (`github_app.ts`) + a SINGLE small `mintJit` seam.
- WP-2 owns `index.ts`/`lib.ts` hardening seams that do NOT overlap WP-1's `mintJit` auth line.
- WP-3 is a **different crate** (Rust fabricd) — fully disjoint.
- WP-4 is **docs** — fully disjoint.
- All code WPs run in **`isolation: worktree`**; the lead merges in DAG order WP-3/WP-4 (independent)
  → WP-1 → WP-2 (WP-2 rebases on WP-1 since both touch `index.ts`).

---

## Section 2 — The WPs (each: SCOPE · INVARIANTS · COMPLETENESS · QUALITY · DoD)

### WP-1 — App-installation-token minting (closes the CRITICAL external mint break, CODE half)
**Owner-files:** `deploy/cloudflare/src/github_app.ts` (NEW) · `deploy/cloudflare/src/index.ts`
(only the `mintJit` auth seam) · `deploy/cloudflare/test/github-app.test.ts` (NEW) ·
`deploy/cloudflare/src/index.ts` Env additions (`GITHUB_APP_ID?`, `GITHUB_APP_PRIVATE_KEY?`).
**Scope:** For a customer repo, `generate-jitconfig` needs an **installation access token** for the
App on THAT repo, not the static first-party `GITHUB_MINT_TOKEN`. Build, in `github_app.ts`, pure +
testable:
1. `appJwt(appId, privateKeyPem, nowMs)` → a short-lived RS256 App JWT (`iss=app_id`, `iat`, `exp≤10m`)
   via WebCrypto (`crypto.subtle`, `RSASSA-PKCS1-v1_5`/SHA-256; import the PKCS#8 PEM).
2. `installationToken(env, installationId, nowMs)` → `POST /app/installations/{id}/access_tokens`
   with `Bearer <appJwt>` → returns `{ token, expires_at }`; **cache** in KV (`RUNNER_JOB_PATS`,
   `ghtok:<installationId>` key, TTL to a safety margin before `expires_at`).
3. Wire `mintJit`: when `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY` are set AND an `installationId`
   is available → use the installation token; ELSE fall back to `GITHUB_MINT_TOKEN` (today's
   first-party dogfood path — byte-identical when App creds absent).
**INVARIANTS (must always hold):**
- I1: App creds ABSENT ⇒ behaviour byte-identical to today (static token). Default-safe.
- I2: The App JWT `exp` ≤ 10 min; the private key NEVER logged, NEVER leaves the worker, NEVER in
  an error body. (Grep the diff: 0 key material in logs.)
- I3: A JIT mint for `installationId=X` uses ONLY the token for installation X (never a cross-install
  token) — the cache key is the installationId.
- I4: An installation-token fetch failure FAILS the mint (throw → `spawn_failed` + claim release),
  NEVER silently falls back to the first-party token for a foreign repo (which would 404 anyway, but
  must not mask the real cause).
**COMPLETENESS CRITERIA:** every code path that reaches `generate-jitconfig` is covered by the
auth-selection logic; the cache read/write/expiry is implemented (not TODO); Env type declares the
two new optional secrets.
**QUALITY:** `tsc --noEmit` clean; `vitest` for `appJwt` (deterministic given a fixed key+now:
assert header/claims/signature-verifiable) + `installationToken` (mock fetch: 201→token+cache;
cache-hit skips fetch; expiry re-fetches; non-2xx→throw) + the mintJit auth-selection (App-present
vs absent). No `any` on the new surface; constant structure mirrors `lib.ts` house style.
**DoD:** ≥6 new tests green; full `vitest` suite green; `tsc` clean; the diff shows App-absent path
unchanged; a one-paragraph return card: {files, test count, the exact mintJit auth-selection
condition, confirmation I1–I4 hold with file:line}.

### WP-2 — multi-tenant hardening bundle (rate-limit key · billing tenant-safety · completed dedup)
**Owner-files:** `deploy/cloudflare/src/index.ts` (webhook handler seams: the `WEBHOOK_LIMITER.limit`
call + the `completed` action block) · `deploy/cloudflare/src/lib.ts` (`reconcileCompletedJobBilling`
tenant guard) · `deploy/cloudflare/test/index.test.ts` (additions).
**Scope (3 contained fixes):**
- **2a rate-limit per-tenant:** `WEBHOOK_LIMITER.limit({ key: "spawn" })` → key by a tenant/repo
  discriminator (`spawn:<repoFullName>` — the repo is known at that point; a busy repo can no longer
  starve every other tenant's spawns). Keep the global limiter as a coarse backstop only if trivial.
- **2b billing tenant-safety:** `reconcileCompletedJobBilling` currently bills `env.CLW_TENANT` for
  EVERY completed family job it finds — mis-attributing a customer's job to the dogfood tenant.
  Fix: the reconciler MUST NOT push a usage event for a job whose true tenant it cannot derive
  (no KV-stashed `jtenant:` mapping). Skip-and-log instead of mis-billing. (The live webhook path
  already has the derived tenant; only the reconciler backstop is blind.)
- **2c completed-leg dedup:** the `evt.action === "completed"` block has no idempotency guard, so a
  GitHub redelivery re-bumps `webhook_job_completed`. Add a completion claim (KV, `done:<jobId>`,
  short TTL) so a redelivered completion is a no-op for the counter (mirror the queued `claimSpawn`).
**INVARIANTS:** I1 (2a): a repo with no discriminator still gets rate-limited (never fail-open to
unbounded spawns). I2 (2b): the reconciler NEVER emits a usage event attributed to a tenant that is
not the job's true tenant — when unknown, it emits NOTHING (correctness over completeness). I3 (2c):
a first completion still counts exactly once; a redelivery counts zero; the security actions
(revoke/teardown) remain idempotent and are NOT gated by the dedup (they self-heal).
**COMPLETENESS:** all three seams changed; no other `key: "spawn"` / un-guarded `CLW_TENANT` bill
site remains (grep proves it).
**QUALITY:** `tsc` clean; `vitest` for each: 2a (two repos get distinct keys), 2b (reconciler with
no derivable tenant emits 0 pushes), 2c (redelivered completion → counter unchanged). House style.
**DoD:** ≥3 new tests green; full suite green; grep-proof no residual global-key/blind-bill site;
return card with file:line for each of I1–I3.

### WP-3 — fabricd counter completeness (Rust; disjoint crate)
**Owner-files:** `crates/corelink-fabric-server/src/admission.rs` (queue-mode rejection counters) ·
`crates/corelink-fabric-server/src/observability.rs` (a dedicated `acquire_rejected_no_plan` counter
if reclassifying the smear) · `crates/corelink-fabric-server/src/handlers/leases.rs` (the no-plan
site) · relevant tests.
**Scope:** (3a) wire the queue-mode admission rejection counters — under
`FABRIC_ADMISSION_MODE=queue`, the rejection seams in `admission.rs` (queue-full, queued-timeout,
over-compute-waiter, capacity-503, fail-closed) currently increment NOTHING; wire them to the
existing/added counters so the rejection denominators are not dark in queue mode. (3b) de-smear
`acquire_rejected_over_cap` at the no-plan site (`leases.rs:446`) — either a dedicated
`acquire_rejected_no_plan` counter or reclassify to `bad_request`; pick the clearer signal and wire
consistently in `observability.rs` (Counters + snapshot + CounterSnapshot — keep the three
field-for-field). (3c) leave `revoke_attempts` (honestly named "attempts") as-is; document the
stale-PAT-cleanup inflation in a comment.
**INVARIANTS:** I1: the `reject` (default) admission mode is byte-identical — new increments fire
ONLY on the queue-mode paths. I2: `Counters`/`snapshot()`/`CounterSnapshot` stay field-for-field
identical (add a field in all three or none). I3: no double-count — a queue-mode rejection
increments exactly one rejection counter.
**COMPLETENESS:** every rejection seam in `admission.rs` has a counter; the no-plan site no longer
smears over_cap.
**QUALITY:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` for the crate green; a
test drives a queue-mode rejection (or the admission unit) and asserts the counter moved.
**DoD:** gate green; the queue-mode + no-plan counters proven by a test; return card with the seam
file:lines + the counter each maps to.

### WP-4 — the owner/cross-repo action set + relays (docs, disjoint)
**Owner-files:** `docs/handoff/2026-07-11-OWNER-external-customer-activation-runbook.md` (NEW) ·
`docs/handoff/2026-07-11-RELAY-to-server-tl-external-webhook-routing.md` (NEW).
**Scope:** produce two crisp, VERIFIABLE docs:
- **Owner runbook:** the exact activation steps the CODE (WP-1) can't self-serve — (1) bind
  `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY` on the spawn-worker (the private key already exists on
  the signup-worker; `wrangler secret put`), (2) grant the App `Administration:write` (self-hosted
  runners) on the target scope + the re-approval implication, (3) raise `RunnerContainer
  max_instances` to cover entitlements (a cost decision — state the tradeoff), (4) the size taxonomy
  input (multi-size). Each step: what, why, exact command/click, and the OBSERVABLE that confirms it.
- **Server-TL relay:** the webhook-routing architecture question — the App has ONE webhook URL
  (bound to the signup-worker install callback); an external repo's `workflow_job:queued` must reach
  the spawn-worker `/webhook` (HMAC = `GITHUB_APP_WEBHOOK_SECRET`). Ask: route the App webhook to a
  spawn-worker route, or fan out `workflow_job` events from the signup-worker? Include the exact
  contract (event subscription + HMAC secret parity + the `installation.id` the mint needs).
**INVARIANTS:** every claim cites code/config file:line (source of truth = code, no theory); no
step is "should work" — each has a verification.
**COMPLETENESS:** covers ALL owner/cross-repo residual (secrets, perms, capacity, webhook routing,
size taxonomy, split-brain reconcile).
**QUALITY:** house style; owner steps are non-technical-legible with `!`-runnable commands where the
owner acts.
**DoD:** both docs written; the owner runbook is a complete "external customer live" checklist that,
combined with WP-1, has NO unstated gap; return card listing each residual finding → which doc/step
addresses it.

---

## Section 3 — lead-held (NOT fanned out)
- **B deploy:** rebuild fabricd with WP-3 merged → new image → pin → rollout → verify the counters
  live (incl. the #376 `acquire_rejected_lease_invalid`). Lead does this at INTEGRATE.
- **TS↔Rust split-brain (D):** a cross-reference comment + the decision to converge at multi-size
  activation — folded into WP-4's relay (not a code change now; dormant).
- **Multi-size activation, external orphan recovery:** cross-repo-sequenced; documented in WP-4,
  not built now (would be built wrong before the external path shapes them).

## Section 4 — merge DAG + verify gates
`WP-3 (Rust) ∥ WP-4 (docs)` → independent, merge first. `WP-1` → merge. `WP-2` → rebase on WP-1,
merge. Lead cold-checks each: (a) count the tests actually run, (b) grep the invariants hold, (c)
`tsc`/`cargo` gate locally, (d) SHA-match + CI green before merge. Then B deploy + a live
`corelink-smoke` regression + (if App creds present) a real external-repo probe.
