# Union catalog ledger — 2026-08-25 audit ∪ 2026-08-30 ultra audit

> **HISTORICAL CATALOG — reviewed at `474c456`.** This ledger is retained as provenance for
> that planning snapshot; it is not a current-HEAD review or a freeze/dispatch record.

**Historical baseline:** `8631abb` (snapshot `474c456` adds only `docs/plan/*` on top of it —
the old `git diff --stat 8631abb 474c456` no-code claim applies only to that historical snapshot).
It is **not** a claim about the current review input `af4ed85` or current HEAD; current code and
production-state claims require a fresh SHA-labelled review.

**Sources**
- (a) 2026-08-30 ultra audit — ids: `docs/plan/audit-2026-08-30-finding-ids.txt` (247);
  titles: scratchpad `audit_index.txt` (`[SEVERITY|CLASS|VERIFIED] id: title`).
- (b) in-repo 2026-08-25 comprehensive audit — `docs/audits/2026-08-25-comprehensive-audit.md`
  (written at `e228249`, in Portuguese).

**Method / evidence rule.** Every code claim in this ledger cites a `file:line` I opened at
the baseline myself. Where (b)'s citation had drifted I re-located it and give the HEAD line;
where a claim's substance had changed I say so. (b)'s own numbers are treated as claims to
check, not as facts. Note: **essentially every `deploy/cloudflare/src/index.ts` citation in
(b) is stale** — the file has grown ~+300–500 lines since `e228249`, so (b)'s line numbers
land in unrelated code. That is drift, not refutation; each row carries the HEAD line.

**Enumeration decisions (so "every finding appears exactly once" is checkable)**
- **RH1 is split** into `RH1a` / `RH1b`: its section states two distinct defects — a disarmed
  `INSTALLATION_ALLOWLIST` (config posture) and a code-level `buildContainerEnv` fail-open
  (legacy authz short-circuit). They have different dispositions, so they are separate rows.
- (b)'s **PARTE IV (Low)** is unnumbered prose. It is enumerated here as `L1…L11` in source
  order, minus two items: the lease-existence oracle (it is explicitly "já listado (M19)" and
  is carried on M19's row), and `NF run_to_completion 1s×600 ok`, which is a *clean-check*
  declaration, not a finding — it is therefore not a row.
- (b)'s **PARTE V (perf)** and **PARTE VI (economia)** are analysis sections outside the
  "~45 achados" count, but they contain five claim-shaped, actionable items. They are carried
  as `P1`, `E1`–`E4` rather than dropped (unfiltered-list rule).

---

## Summary counts

| Disposition | N |
|---|---|
| MAPPED | 15 |
| PARTIAL | 5 |
| NEW | 30 |
| CLOSED | 1 |
| **Total rows** | **51** |

New ids assigned: `union-01` … `union-30`.

---

## Per-finding ledger

### Criticals

| id | claim (b) | stated fix (b) | disposition | evidence / 2026-08-30 id |
|---|---|---|---|---|
| RC1 | Jobs >2h bill ZERO: the `jtenant:` tenant stash is written with `expirationTtl = JOB_PAT_TTL_S` (2h) while boxes outlive it, so `completed` finds no tenant and no usage row is ever written | `jtenant:` TTL 60d, or stamp the tenant on the ledger row at START | **MAPPED** | `billing-money-path-03`: "Jobs longer than 2h bill zero and are unrecoverable (jtenant KV TTL) — still present at HEAD"; also `hist-06`. Constant re-located and confirmed at HEAD: `deploy/cloudflare/src/index.ts:850` `const JOB_PAT_TTL_S = 7200`, applied at `:1265`, `:1285`, `:1311`, `:1712`; the self-confession quoted by (b) is now `index.ts:890` |
| RC2 | Orphan boxes are detected but not killable: `POST /v1/teardown` by instance-name resolves `idFromName()` to a fresh unrelated DO, destroys nothing, returns 204 | land ADR-0010 (enumerable DO names) + class-registration gate in the detector | **MAPPED** | `hist-09`: "RC2 / B-044 / B-002 / ADR-0010: orphan boxes are detectable but NOT killable — teardown not built, not armed"; also `gap-09`, `spawn-cf-10`. Re-verified: `scripts/orphan-box-check.sh:20-31` states the no-path-back constraint and the silent-204 measurement verbatim |
| RC3 | A paying customer's job can be lost permanently in the `waitUntil` window (isolate killed after the claim, before any dead-letter; GitHub delivers `queued` once) | write-ahead dead-letter before spawn, or drive from a DO alarm / Queue consumer | **MAPPED** | `hist-08`: "RC3 open at HEAD: a paying customer's job can be lost permanently in the waitUntil window (external repos have no recovery scan)"; also `adopt-16` |

### Highs

| id | claim (b) | stated fix (b) | disposition | evidence / 2026-08-30 id |
|---|---|---|---|---|
| RH1a | External-GA installation allowlist deliberately DISARMED in prod config | arm the allowlist (value already documented) | **MAPPED** | `gap-01`: "External-GA installation allowlist DISARMED on the live spawn-worker"; also `adopt-15`, `hist-07`, `spawn-cf-08`. Re-verified: `deploy/cloudflare/wrangler.jsonc:77` "Leaving INSTALLATION_ALLOWLIST out keeps the gate DISARMED"; gate site re-located to `index.ts:3502` (was cited `:3036-3045`) |
| RH1b | `buildContainerEnv` short-circuits to `authz:"ok"` COLD when the mint key is absent/misconfigured ("legacy fail-open"), and no deploy-time assertion refuses to serve `/webhook` with the gates unarmed | fail-closed config assertion at deploy time | **NEW → `union-01`** | Re-located: `deploy/cloudflare/src/lib.ts:495-505` — comment "No mint key … ⇒ COLD (legacy fail-open)" and `return { authz: "ok", containerEnv: {} }`. (b) cited `lib.ts:482-487`; the code moved, the defect did not. No 2026-08-30 id covers it (`runner-core-06` is image-script fail-opens; `fabric-core-08` is durable-suspension read; `billing-money-path-10` is the ceiling sentinel) |
| RH2 | One static `CLOUDFLARE_SPAWN_AUTH_TOKEN` authorizes `/v1/spawn`, arbitrary-argv `/v1/exec`, teardown, status and egress-cutoff; no rotation, no scoping | per-domain tokens or mTLS / CF Access service-auth; set `PINNED_IMAGE_DIGEST` | **NEW → `union-02`** | Re-located and confirmed: `index.ts:709-714` (`authed()` reads the single token, fail-closed on empty) is the *only* gate, applied once at `index.ts:3628`, and covers `/v1/spawn` `:3631`, `/v1/exec` `:3735`, `/v1/status/` `:3789`, `/v1/teardown` `:3809`, `/v1/egress-cutoff` `:3849`. (b) cited `:675-680`. Established by the lead as uncovered by (a); re-confirmed by search of the 247 titles |
| RH3 | DO acquire fail-OPEN bypasses the whole cap system on an infra error: `catch → { admitted: true }` | split by warmth — warm/paid fail-CLOSED, cold fail-open; metric + alert | **NEW → `union-03`** | **RH3 citation re-located (see §RH3 verdict below).** (b) cited `index.ts:1608-1617`, which at HEAD is unrelated. The defect is real and present at **`deploy/cloudflare/src/index.ts:1650-1658`**: `} catch (e) { … logEvent("error","concurrency_slot_acquire_error_failopen", …); return { admitted: true }; }` inside `acquireConcurrencySlot` (declared `:1631`), with the verbatim comment "Infra hiccup ⇒ ADMIT (never block a legitimate job on a DO error)" |
| RH4 | `runner ALL=(ALL) NOPASSWD:ALL` — untrusted job is root in the box; root can ptrace the Listener, read `/proc/*/environ`, tamper `clw` and poison the tenant's own AC | remove sudo / scoped sudoers; stop claiming non-root as hardening | **MAPPED** | `sec-06`: "Untrusted runner user has passwordless sudo + rootful containerd/buildkitd; in-container privilege containment is nil (isolation is the microVM only)". Re-verified at the cited line: `deploy/runner/Dockerfile:515` |
| RH5 | Unrestricted egress from a shared IP (`enableInternet=true`, `deniedHosts` removed, exact-host-only metadata denylist, raw sockets bypass the SDK) + the IMDS "probe owed" doc contradiction | platform-network-layer egress allowlist + live IMDS test | **PARTIAL** → `gap-16` | `gap-16` covers the egress half exactly: "G2 IMDS/link-local egress blocking is exact-host only; CIDR entries INERT, raw sockets bypass — platform-layer filtering missing". Re-verified: `index.ts:589` `enableInternet = true`, `:592-595` deniedHosts removed. **Uncovered: the in-file doc contradiction** — `index.ts:45-46` still says "A live-account smoke is still owed" while `index.ts:595` says "G2 is settled by the metadata probe" (both at HEAD; (b) cited `:561`, now `:595`) |
| RH6 | `flush_now` POSTs the ENTIRE buffer; server caps a batch at 1024 → a ≥1024 backlog is a permanent poison pill. Paired doc/impl divergence on partial-accept | chunk flush ≤512, client-side UUID validation, fix the doc | **MAPPED** | `billing-money-path-06` (whole-buffer POST / poison pill) + `billing-money-path-07` (module doc promises per-record rejection; ingest is all-or-nothing). Re-verified at the cited lines: `crates/corelink-fabric-server/src/corelink_billing.rs:279-299` — one `post_batch` of the full snapshot, non-2xx ⇒ `bail!` "retaining batch for retry" |
| RH7 | Garbage / negative / overflow / **fractional** `max_vcpu_h` is treated as ABSENT → sentinel 0 → ceiling silently disabled (unlimited) | 3-state parse, loud malformed, clamp overflow, fractional support, garbage conformance vector | **MAPPED** | `billing-money-path-10`: "Garbage/fractional max_vcpu_h silently disables the ceiling (fail-open unlimited) — sentinel 0 conflates 'unmetered' with 'unreadable'". Re-verified: `crates/corelink-fabric-server/src/corelink_plans.rs:129-135` (`parse_max_vcpu_h_ceiling_ms`, missing ⇒ `return 0`, "Anything else … is treated as absent") |
| RH8 | Mint-key rotation is operator-prayer: the container reads env only at boot, and the boot self-check covers the introspect key only — a wrong mint key boots healthy and fail-opens cold spawns silently. Secret inventory also drifted | mint-key boot self-check mirroring `classify_introspect_bootcheck`; backfill the inventory | **NEW → `union-04`** | Verified: `crates/corelink-fabric-server/src/server.rs:1078` `classify_introspect_bootcheck` is the only boot credential probe (grep for a mint equivalent returns nothing but introspect); the boot-read-only mechanism is documented at `docs/runbook/secret-inventory.md:77-81`. No 2026-08-30 id covers a mint-key boot probe (`fabricd-04`/`fabricd-deploy-06` are `FABRIC_TEST_MINT_KEY` arming). The inventory-drift half is carried separately as M20 |
| RH9 | The limiter answers GitHub with **429** and an HMAC failure answers **401 with no counter**; no streak monitoring and no paging anywhere in the Worker | 202 on limiter-refuse, `webhook_auth_failed` metric, external paging on streaks | **PARTIAL** → `hist-12` (+`deploy-13`) | `hist-12` ("Canary alerting Worker still NOT DEPLOYED at HEAD — no automated paging") and `deploy-13` ("alert DELIVERY is unarmed") cover the *paging* half. **Uncovered: the two response/counter defects**, both live at HEAD: `index.ts:3487` `return json({ error: "rate limited" }, 429)` (a dead-letter *is* written at `:3485`, so (b)'s severity is softened but the 429 stands) and `index.ts:3265` `return unauthorized()` on HMAC failure with **no** metric bump — grep finds no `webhook_auth_failed` counter anywhere in the Worker |
| RH10 | `claimSpawn` is a non-atomic KV get→put; geodistributed colos + eventual consistency ⇒ two deliveries both win the claim (double mint, two boxes) | move the claim into `ConcurrencySlotsDO.acquire` (atomic set-add); KV claim becomes a hint | **NEW → `union-05`** | Re-verified at the cited lines (unmoved): `deploy/cloudflare/src/lib.ts:113-124` — `const existing = await kv.get(key); if (existing) return false; … await kv.put(...)`, no CAS. No 2026-08-30 id covers the claim race (`spawn-cf-12` is only the `spawnClaimAgeMs` docstring) |

### Mediums

| id | claim (b) | stated fix (b) | disposition | evidence / 2026-08-30 id |
|---|---|---|---|---|
| M1 | Offboarding TOCTOU: revocation hangs only off the `completed` webhook or the PAT TTL, so a tenant suspended mid-job keeps a live `cas:rw` PAT for up to 2h | revoke on suspension | **NEW → `union-06`** | Verified: the only revoke driver is `revokeCompletedJob` (`index.ts:1347`), called from the completion/teardown paths (`:1736`, `:1770`, `:2585`, `:3336`); no suspension-triggered call site exists. (b)'s `:1625-1731` is stale. `fabric-core-08` is fabricd durable suspension, a different path |
| M2 | `/v1/leases/{id}/runner-diag` has no auth, no rate limit and no lease validation → unbounded error-level log injection | authenticate / gate the sink | **CLOSED** | Closed **in code at HEAD**, not by a CHANGELOG line: `index.ts:3595-3622` now requires a live spawn-claim (`RUNNER_JOB_PATS.get('spawn:'+jobId)`, `:3600`), applies `WEBHOOK_LIMITER` keyed `diag:<jobId>` (`:3610`) and returns a uniform `{ok:true}` for unknown jobs (`:3617`); the header comment at `:3581-3587` records the fix. Residual deploy-state risk (was verified OPEN in prod 2026-08-25, no redeploy artifact) is (a)'s `hist-05`/`fabricd-09`, not this row |
| M3 | Cred ticket is MULTI-USE for up to 7200s, and the completion revoke is FAIL-OPEN (error swallowed) → a plaintext `cas:rw` PAT survives ~2h past the job | fail-loud revoke + short TTL | **PARTIAL** → `sec-03` | `sec-03` covers the multi-use half exactly ("Live env-0 cred redemption (CredStashDO) is MULTI-USE while its own comment + the fabricd handler declare/implement SINGLE-USE"); re-verified at `lib.ts:455-465` (`decideRedeem` docstring: "MULTI-USE within the lease (2026-07-06) … NOT single-use"). **Uncovered: the fail-open revoke** — `index.ts:1347-1369`, comment "Fail-OPEN: any error is swallowed (the PAT TTL-expires)", `catch` logs and returns `false` with no retry or dead-letter |
| M4 | `SLOT_TTL_S = 2700` is shorter than real job durations and the slot is never renewed (keepAlive renews only the container) → fleet count under-reports → admission into a physically full fleet | slot heartbeat renewal | **NEW → `union-07`** | Verified: `deploy/cloudflare/src/lib.ts:611` `export const SLOT_TTL_S = 2700`, consumed once at spawn (`index.ts:1648`); no renewal call site exists — the only renewal path is the container keepalive. (b) cited `lib.ts:594,647`; re-located. No 2026-08-30 id covers slot-TTL/renewal |
| M5 | No queue semantics at all: at-cap beyond `ORPHAN_TTL_S` (30 min) the retry gives up, deletes the record and the job is permanently dropped; the correct pattern (`pg_queue`) exists but is unwired | port/wire the durable fair queue | **PARTIAL** → `fabric-core-14` (+`gap-13`) | `fabric-core-14` covers the inert-`pg_queue` half ("pg_queue (durable cross-instance fair queue)" among dormant anchors — re-verified: the only reference is `crates/corelink-fabric-server/src/admission.rs:301 with_durable_queue`, no production caller) and `gap-13` the unratified queued-admission mode. **Uncovered: the spawn-worker giveup drop** — `index.ts:4073-4083`: `if (step.action === "giveup") { await kv.delete(name)…; logEvent("error","orphan_retry_giveup",…); continue; }` — no terminal state, no customer signal, no queue position |
| M6 | The mint happens BEFORE the slot check, so every dead-letter retry burns a mint+revoke pair for a job that is then refused | check the slot first | **NEW → `union-08`** | Verified ordering at HEAD: `const mint = await buildContainerEnv(...)` `index.ts:1695` precedes `const slot = await acquireConcurrencySlot(...)` `index.ts:1720`; the comment at `index.ts:3476-3478` states the amplification explicitly. (b) cited `:1653→:1678` |
| M7 | The JIT config is passed on ARGV, so it is readable from `/proc/<pid>/cmdline` for the lease's lifetime | pass it off-argv | **NEW → `union-09`** | Re-verified at the cited line (unmoved): `deploy/runner/entrypoint.sh:199` `./run.sh --jitconfig "$CORELINK_RUNNER_JITCONFIG" …`. `sc-07` is a different site (Rust `HttpRequest` Debug printing `json_body`) |
| M8 | The stale-Pending reaper deletes the record BEFORE teardown, so a teardown flake leaks the box, logged once, never retried (the Held reaper does teardown-first) | teardown-first / retryable tombstone | **NEW → `union-10`** | Re-verified at the cited lines: `crates/corelink-fabric-server/src/reaper.rs:~940-955` — "We won the delete … A failure is logged but does NOT un-reclaim the cap slot … the box may leak but the slot is already freed". `fabric-core-11` is the *lifecycle.rs vs reaper* ordering drift, an adjacent but different site |
| M9 | Spawn bookkeeping is write-once-swallow: the `rhandle:`/`sbox:` KV puts are `.catch(logEvent)` AFTER the container has started — a KV failure leaves a running box with no record (the exact class of the 3 measured 10.2h boxes) | compensate or abort on bookkeeping failure | **NEW → `union-11`** | Re-located and verified: `index.ts:1313-1319` — `.catch((e) => logEvent("error","kv_put_runner_handle_failed",…))` after the start, with the durable twin written at `:1323+`. (b) cited `:1274-1296` |
| M10 | No `installation.deleted` handler — an uninstall purges nothing and the env maps are never pruned | add the handler | **NEW → `union-12`** | Verified stronger than claimed: the webhook accepts only one event type — `index.ts:3268` `if (request.headers.get("x-github-event") !== "workflow_job") return … ignored`; grep finds no `installation.` event handling anywhere in the Worker |
| M11 | Dual tenant resolution (installation_id vs PAT-introspect) with no owner-of-record and no deployed-state validation; a declarative deploy already replaced `REPO_TENANT_PAT_MAP` with `{}` in prod and mis-attributed CAS + billing silently | declare an owner-of-record + deployed-map self-check | **NEW → `union-13`** | Re-verified at the corrected lines: `deploy/cloudflare/wrangler.jsonc:65-76` records the incident verbatim ("This key held `\"{}\"` while prod ran a non-empty map … attribute its CAS + billing to the dogfood tenant d863fafb instead of 3c7d77b1, with NO error anywhere"). `spawn-cf-14` is an adjacent but different gap (corelink-server unmapped ⇒ COLD) |
| M12 | A cold at-ceiling refusal leaves a stranded job with no terminal state (early-return without `installationId`) and holds a phantom slot for 45 min | record a terminal state + release the slot | **NEW → `union-14`** | Verified the stranded class exists as a log/counter only: `index.ts:2560` `logEvent("error","job_stranded",…)` and `:2587` bump; the refusal path itself writes no terminal record. (b)'s `:1671-1706,1780` is stale. `spawn-cf-01` is the counter-registration bug, not the stranded-state gap |
| M13 | Suspend/rollback of a Crashed lease emits slot events without the durable acquire stamp → `terminal_without_acquire` skipped → slot-seconds vanish from reconciliation | stamp the acquire | **MAPPED** | `billing-money-path-11`: "Secondary fabricd terminal paths emit slot events WITHOUT the durable acquire stamp — billed slot-seconds vanish for those leases after a restart" |
| M14 | The spawn→pickup gap is non-billable COGS (the box is up while GitHub still reports `queued`) | — (noted) | **MAPPED** | `billing-money-path-15`: "Structural COGS gaps outside the billable window: spawn/boot time and leaked idle boxes are cost with no billing counterpart" |
| M15 | The prod vCPU ceiling is ADVISORY (warn, never stops) and the Stripe materializer is dormant without `STRIPE_PRICE_ID_RUNNER_*` — no cap anywhere | flip the ceiling + verify the first overage invoice | **MAPPED** | `billing-money-path-09`: "vCPU-h ceiling ENFORCE exists only on the fabricd lease path (not the prod GH-job path); prod is WARN-only by design, and the ceiling VALUE is server-side/not-yet-live" (the Stripe-materializer half is cross-repo, corelink-server) |
| M16 | The canary observes counters, not behavior — no synthetic acquire→spawn→release transaction, so a systematic leak is invisible until a paying tenant starves | add a synthetic transaction | **NEW → `union-15`** | Verified: `deploy/cloudflare-canary/src/index.ts:144-147` fetches three read-only metric/health surfaces and nothing else; no spawn/acquire call exists in the Worker. `deploy-13`/`gap-11` are about alert *delivery* and arming, not about what is observed |
| M17 | `cas_cred` handlers emit ad-hoc `{"error": msg}` on a PUBLIC route, outside the frozen `ErrorBody{code,message}` vocabulary | conform to `ErrorBody` | **NEW → `union-16`** | Re-verified at the cited line: `crates/corelink-fabric-server/src/handlers/cas_cred.rs:43` `fn err(status, msg) -> (status, Json(json!({ "error": msg })))`. The Worker twin does the same (`index.ts:3566-3569`) |
| M18 | The GitHub Action interpolates `${{ inputs.* }}` directly into a bash script (injection via reusable-workflow callers); the Buildkite hook does it correctly with env indirection | env-indirect the inputs | **NEW → `union-17`** | Re-verified at the cited lines: `integrations/github-actions/action.yml:133-142` — `ARGS+=(--url "${{ inputs.url }}")`, `if [[ -n "${{ inputs.image }}" ]]`, etc., inside `shell: bash` steps |
| M19 | Lease-existence oracle in the cred redeem (401 vs 404 vs 410 on enumerable ids) | uniform response | **PARTIAL** → `fabricd-06` | `fabricd-06` names the same defect on the Rust handler ("cas_cred::redeem's no-oracle claim is contradicted by its own ordering"). **Uncovered: the spawn-worker's own redeem route**, which makes the identical distinction at `index.ts:3566-3569` (`401 invalid ticket` / `410 ticket already redeemed` / `404 no such lease`). (b) cited `:3078-3104`; re-located |
| M20 | `secret-inventory.md` claims completeness while ≥4 secrets are missing from its table | backfill | **NEW → `union-18`** | Verified: `docs/runbook/secret-inventory.md` (183 lines) contains **zero** occurrences of `COLD_ORGANIC_TENANT_PAT`, `CORELINK_CF_ACCESS_CLIENT_ID`, `FABRIC_TEST_MINT_KEY`, `PINNED_IMAGE_DIGEST` — all four are live/declared names elsewhere in the repo (e.g. `deploy/cloudflare/wrangler.jsonc:74`). No 2026-08-30 id covers inventory drift |
| M21 | Fabricd secret pickup requires image churn (rebuild + repin + deploy per rotation); rollback digests live in commented prose | make rotation not require a new digest, or make the rollout a one-command recipe | **NEW → `union-19`** | Verified both halves: `docs/runbook/secret-inventory.md:77-81` "reads them **only at boot** … a container rollout (new image digest + `wrangler deploy`) is what makes fabricd pick it up"; the digest lineage/rollback pointer is narrative prose at `deploy/cloudflare-fabricd/wrangler.jsonc:190-205` ("SUPERSEDES 5eed854a") |
| M22 | Internal pricing drift inside one file: the ladder table says $16/$40 while the enum docs say $8/$20 | pick one | **NEW → `union-20`** | Re-verified: `crates/corelink-fabric/src/plans.rs:13-14` (`Starter $16`, `Pro $40`) vs `:77`/`:79` ("solo dev / entry tier ($8/mo)", "growing dev + agents ($20/mo)") |

### Lows (enumerated from PARTE IV prose)

| id | claim (b) | disposition | evidence / 2026-08-30 id |
|---|---|---|---|
| L1 | Replay window >2h re-spawns one wasted box (requires a stolen secret) | **NEW → `union-21`** | The claim window is `SPAWN_CLAIM_TTL_S`-bounded; the claim itself is the only replay guard (`lib.ts:113-124`), so a delivery replayed past the claim TTL re-enters `driveSpawn`. No 2026-08-30 id covers replay |
| L2 | check-exec-server bearer lives in the container env (real only when untrusted code shares the container) | **NEW → `union-22`** | Verified: `crates/corelink-check-exec-server/src/lib.rs:53` `pub const AUTH_TOKEN_ENV: &str = "EXEC_SERVER_AUTH_TOKEN"` (injected at spawn, read from env). `sc-11` covers the surface/doc drift, not the env-borne token |
| L3 | pids `ulimit` is fail-open | **MAPPED** | `runner-core-06`: "Deliberate fail-open paths in the runner image scripts — all documented, all cache/limit optimizations, none a security boundary". Re-verified: `deploy/runner/entrypoint.sh:143-147` — "Fail-OPEN, matching the moat north-star: a ulimit that the platform refuses to …" |
| L4 | The memoize key has no tenant component — isolation rests entirely on CAS-side scoping (dependency note, not a bug) | **NEW → `union-23`** | Verified: `actions/corelink-memoize/action.yml` contains no tenant input at all (grep for `tenant` returns nothing); the key is built from run/inputs/env/tool (`:39-42`) |
| L5 | CLI keyset rotation picks an arbitrary first key | **MAPPED** | `sc-04`: "CLI ignores the attestation key-set SELECTION layer (fabric_key_id + expires_ms) — verify breaks the moment key rotation publishes a second key" |
| L6 | The action uses `shell: python3`, which breaks bash-only runners | **NEW → `union-24`** | Re-verified: `integrations/github-actions/action.yml:174` `shell: python3 {0}` (the other three steps are `shell: bash`) |
| L7 | `CLW_TENANT` fallback inside `revokeCompletedJob` — kill the pattern before it is copied into billing | **NEW → `union-25`** | Re-located: `index.ts:1364` `await revokeCasPatById(env, patId, derivedTenant ?? env.CLW_TENANT)`, with the call-site comment at `:2583` ("best-effort: revokeCompletedJob falls back to CLW_TENANT"). (b) cited `:1329`. `billing-money-path-08` names a `CLW_TENANT` fallback but on the DevEnv metering path |
| L8 | `recordOrphan` is a benign get-then-put | **NEW → `union-26`** | Verified: `index.ts:1798-1810` — `if (await …get(key)) return;` then `put(...)`, no CAS; the same non-atomic bump recurs in the retry loop at `index.ts:4085-4091`, whose own comment concedes "a failed bump just means next tick re-reads the old count". Benign as (b) says (worst case: a lost attempt-count bump) |
| L10 | README says "eight crates" and lists seven | **MAPPED** | `docs-truth-18`: "README architecture section: 'Eight crates' but the table lists seven (corelink-check-exec-server missing)" |
| L11 | README's "Firecracker fleet" claim is untrue (the substrate is CF Containers) | **MAPPED** | `docs-truth-19`: "'Firecracker fleet/microVM' claims about Cloudflare Containers are asserted as fact in README, ci.yml and the go-live runbook with no in-repo evidence" |

*(The Low prose additionally repeats the lease-existence oracle — carried on M19 — and records one clean-check, `NF run_to_completion 1s×600 ok`, which is not a finding and gets no row.)*

### Perf / economy items (PARTE V–VI)

| id | claim (b) | disposition | evidence / 2026-08-30 id |
|---|---|---|---|
| P1 | Spawn tax of 15–30s queued→RUNNING undermines the pitch on small/fast jobs; the math assumes warm nodes | **NEW → `union-27`** | (b)'s own waterfall; no 2026-08-30 id makes a latency/pitch claim. The one code anchor I checked: the start timeout/retry structure referenced by (b) is the `container.start()` path invoked from `driveSpawn` (`index.ts:1720+`) |
| E1 | The real Cloudflare Containers rate appears NOWHERE in the repo — every margin inherits the $0.10/vCPU-h Northflank proxy | **NEW → `union-28`** | Verified: `docs/product/pricing.md:10,16,22,27,37` all derive from the "$0.10/vCPU-h basis"; no CF rate is cited anywhere in the doc |
| E2 | The 85–95% memoization hit-rate carrying the pricing claim is UNMEASURED (pricing.md admits it) | **NEW → `union-29`** | Same file; no in-repo measurement artifact exists (the moat proofs in memory are single-run `[clw] cache hit` confirmations, not a rate) |
| E3 | Fleet cap 250 with headroom NOT ready (compiled const + wrangler edit + deploy + a CF limit raise) → a spike past the cap is a clean refusal and a job queued forever | **NEW → `union-30`** | `docs-truth-08` records the *constant drift* (20→250) but not the headroom/tunability gap; `FLEET_MAX_CONCURRENCY` is a compiled constant consumed at `index.ts:1648` |
| E4 | `pricing.md` still asserts a hard-stop ("no more compute runs. No overage that leaks") contradicting the advisory+overage model in code | **MAPPED** | `docs-truth-01`: "Canonical pricing doc says the vCPU-h ceiling HARD-STOPS compute; README + owner-ratified CHANGELOG say it bills overage and 'is deliberately not a gate'" |

---

## RH3 verdict (required re-verification)

**RE-LOCATED, still present.** (b) cited `deploy/cloudflare/src/index.ts:1608-1617`; at baseline
`8631abb` that range is inside the slot-release path, so the citation is stale. The fail-open is
at **`deploy/cloudflare/src/index.ts:1650-1658`**, in `acquireConcurrencySlot` (declared `:1631`):

```
  } catch (e) {
    // Infra hiccup ⇒ ADMIT (never block a legitimate job on a DO error). A clean
    // at-capacity decision above is NOT an error and is honored as a real refusal.
    logEvent("error", "concurrency_slot_acquire_error_failopen", { jobId, key, error: … });
    return { admitted: true };
  }
```

Every spawn funnels through this one call, so (b)'s claim — a DO throw bypasses both the fleet cap
and paid entitlements — holds verbatim at HEAD. Disposition: **NEW `union-03`**.

## Other citations that no longer hold as written

None were found **unreproducible in substance** — every (b) finding I re-checked is still real at
`8631abb`. What did move (each re-located above, HEAD line given in the row): `RH1a`
(`:3036-3045`→`:3502`), `RH1b` (`lib.ts:482-487`→`:495-505`), `RH2` (`:675-680`→`:709-714`),
`RH3` (`:1608-1617`→`:1650-1658`), `RH5` (`:561`→`:595`), `RH9` (`:3014-3023`→`:3480-3488`;
`:2797-2800`→`:3265`), `M1` (`:1625-1731`→`:1347` + call sites), `M2` (`:3107-3122`→`:3595-3622`,
and the defect is CLOSED), `M3` (`lib.ts:448-458`→`:455-465`; `index.ts:1313-1329`→`:1347-1369`),
`M4` (`lib.ts:594,647`→`:611`), `M5` (`lib.ts:1102`/`:3541-3551`→`index.ts:4073-4083`),
`M6` (`:1653→1678`→`:1695`/`:1720`), `M9` (`:1274-1296`→`:1313-1319`), `M12` (`:1671-1706,1780`→
no terminal-record site; the stranded log lives at `:2560`), `M19` (`:3078-3104`→`:3566-3569`),
`L7` (`:1329`→`:1364`). Two citations (b) had already self-corrected in its PARTE IX — the RH6
crate path (`corelink-fabric-server/`) and the M11 wrangler lines (`:67-74`, now `:65-76`) — check
out at HEAD as corrected.

---

## NEW AND PARTIAL — proposed acceptance items

Capability legend: C1 control plane up · C2 shippability · C3 job lifecycle · C4 money ·
C5 external stranger onboarding · C6 alerting · C7 doc truth.

| id | from | sev | class | cap | proposed acceptance item |
|---|---|---|---|---|---|
| union-01 | RH1b | HIGH | C1-broken | C5 | **test:** `buildContainerEnv` with `CORELINK_RUNNER_MINT_AUTH_KEY` unset returns a refusal (not `authz:"ok"`), **and probe:** `POST /webhook` answers 503 on a deploy where the mint key or the installation allowlist is unarmed |
| union-02 | RH2 | HIGH | C3-missing | C1 | **test:** a bearer accepted on `/v1/status` is rejected on `/v1/exec` and `/v1/spawn` (per-domain tokens or CF Access service-auth), **and probe:** rotating the spawn credential requires no `index.ts` change |
| union-03 | RH3 | HIGH | C1-broken | C4 | **test:** `acquireConcurrencySlot` against a throwing `ConcurrencySlotsDO` returns `admitted:false` + a dead-letter for a WARM/paid spawn, and `admitted:true` only on the COLD path; a `do_acquire_error` counter is bumped either way |
| union-04 | RH8 | HIGH | C3-missing | C6 | **probe:** boot fabricd with a wrong `CORELINK_RUNNER_MINT_AUTH_KEY` and assert the boot self-check FATALs (or logs `mint_bootcheck_rejected`) instead of booting healthy and fail-opening cold |
| union-05 | RH10 | HIGH | C1-broken | C4 | **test:** two concurrent `claimSpawn` calls for one `jobId` against an eventually-consistent KV stub yield exactly one `true` (claim held by an atomic DO set-add, KV demoted to a hint) |
| union-06 | M1 | MEDIUM | C3-missing | C4 | **test:** a tenant suspended mid-job has its per-job `cas:rw` PAT revoked within one reaper tick, not at the 2h PAT TTL |
| union-07 | M4 | MEDIUM | C1-broken | C3 | **test:** a job still alive past `SLOT_TTL_S` still holds its concurrency slot (keepalive renews the slot, not just the container), and the fleet count matches the number of live boxes |
| union-08 | M6 | MEDIUM | C1-broken | C4 | **test:** `driveSpawn` acquires the concurrency slot BEFORE minting — a spawn refused at capacity performs zero mint/revoke pairs |
| union-09 | M7 | MEDIUM | C1-broken | C3 | **probe:** with a job running, `/proc/<run.sh pid>/cmdline` inside the box contains no jitconfig token (the config is passed via stdin or a 0600 file) |
| union-10 | M8 | MEDIUM | C1-broken | C3 | **test:** the stale-Pending sweep tears down BEFORE deleting the record, and a teardown failure leaves a retryable tombstone that a later tick re-attempts |
| union-11 | M9 | MEDIUM | C1-broken | C4 | **test:** a failing `RUNNER_JOB_PATS.put` on the spawn path compensates (tears the box down) or writes a durable retry record — never leaves a started box with no `sbox:` record |
| union-12 | M10 | MEDIUM | C3-missing | C5 | **test:** an `installation.deleted` delivery purges that installation's map/allowlist entries and a subsequent spawn for it is refused before mint |
| union-13 | M11 | MEDIUM | C1-broken | C4 | **test:** a repo carrying both an `installation_id` and a `REPO_TENANT_PAT_MAP` entry resolves exactly one declared owner-of-record, **and probe:** a deploy that would empty `REPO_TENANT_PAT_MAP` fails a config self-check instead of silently re-attributing billing |
| union-14 | M12 | MEDIUM | C1-broken | C3 | **test:** a cold at-ceiling refusal writes a terminal, queryable job record (carrying `installationId`) and releases its slot immediately rather than at the 45-minute TTL |
| union-15 | M16 | MEDIUM | C3-missing | C6 | **probe:** the canary runs a synthetic acquire→spawn→release transaction each tick and alerts when the slot count fails to return to 0 |
| union-16 | M17 | LOW | C1-broken | C7 | **test:** every error response from `cas_cred` (Rust) and the Worker's `/cas-cred` deserializes as the frozen `ErrorBody{code,message}` |
| union-17 | M18 | MEDIUM | C1-broken | C5 | **test:** a caller passing `$(id)` as an action input has it forwarded via env indirection and never evaluated by the action's bash |
| union-18 | M20 | LOW | C5-docs-drift | C7 | **test:** a CI check diffs the secret names referenced in `wrangler.jsonc` + workflows against `docs/runbook/secret-inventory.md` and fails on drift |
| union-19 | M21 | MEDIUM | C3-missing | C1 | **probe:** rotating a fabricd container secret takes effect without minting a new image digest — or the runbook carries a one-command rollout recipe that a fresh operator executes successfully |
| union-20 | M22 | LOW | C5-docs-drift | C7 | **test:** `plans.rs`'s enum docstrings and its module ladder table quote the same prices (a unit test asserting the strings agree) |
| union-21 | L1 | LOW | C1-broken | C3 | **test:** a `workflow_job.queued` delivery replayed after the spawn-claim TTL does not spawn a second box for the same `jobId` |
| union-22 | L2 | LOW | C3-missing | C1 | **test:** the check-exec-server reads its auth token from a file/secret mount rather than a process env var visible to co-resident code |
| union-23 | L4 | LOW | C4-unverified-claim | C7 | **test:** two tenants computing an identical memoize key cannot read each other's entry (asserted at the CAS scoping layer), with the dependency documented in the action README |
| union-24 | L6 | LOW | C3-missing | C5 | **test:** the action completes on a runner image with no `python3` (bash-only) |
| union-25 | L7 | LOW | C1-broken | C4 | **test:** `revokeCompletedJob` with no derived tenant refuses loudly instead of falling back to the wrangler `CLW_TENANT` var |
| union-26 | L8 | LOW | C1-broken | C3 | **test:** two concurrent `recordOrphan` bumps for one `jobId` do not lose an attempt count |
| union-27 | P1 | MEDIUM | C4-unverified-claim | C7 | **probe:** publish a measured queued→RUNNING distribution (p50/p95) from ≥50 real jobs and state the job-size break-even in the pitch docs |
| union-28 | E1 | MEDIUM | C4-unverified-claim | C4 | **probe:** record the real Cloudflare Containers vCPU-h/GiB-h rate in `pricing.md` with a cited invoice line and re-derive the margin table from it |
| union-29 | E2 | MEDIUM | C4-unverified-claim | C7 | **probe:** instrument `clw` hit/miss over a real week and report the measured rate; `pricing.md`'s 85–95% claim cites it or is withdrawn |
| union-30 | E3 | MEDIUM | C3-missing | C3 | **test:** `FLEET_MAX_CONCURRENCY` is env-tunable without a recompile, and an over-cap spawn returns a queued position/backpressure signal instead of a silent permanent drop |
| RH5 (partial) | uncovered half | LOW | C5-docs-drift | C7 | **test:** no two statements in `deploy/cloudflare/src/index.ts` assert opposite metadata-probe status (`:45-46` vs `:595`); the surviving statement cites the closed probe run |
| RH9 (partial) | uncovered half | MEDIUM | C1-broken | C6 | **test:** a limiter-refused webhook returns 202 with its dead-letter written, and a bad-HMAC `POST /webhook` bumps a registered `webhook_auth_failed` counter |
| M3 (partial) | uncovered half | MEDIUM | C1-broken | C3 | **test:** a revoke failure on the completed path retries or dead-letters and bumps `revoke_failed`; **probe:** no `cas:rw` PAT outlives its job by more than the agreed grace |
| M5 (partial) | uncovered half | MEDIUM | C3-missing | C3 | **test:** an orphan record exhausting `MAX_ORPHAN_ATTEMPTS` transitions to a terminal, queryable state surfaced to the customer instead of being deleted |
| M19 (partial) | uncovered half | LOW | C1-broken | C3 | **test:** the Worker's `/v1/leases/{id}/cas-cred` returns one uniform response for a bad ticket and an unknown lease (no existence oracle) |
