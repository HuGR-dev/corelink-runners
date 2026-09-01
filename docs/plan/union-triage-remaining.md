> **LEAD LANDING NOTE (2026-08-31).** This triage was produced against the plan as of `8631abb`,
> before rev-5 added its own items — so 14 of its proposed ids COLLIDED with ids the plan had already
> taken. Merging it as returned would have silently overwritten live suite items. Resolved
> structurally rather than by shuffling numbers: every item proposed here now lives in its own
> **`AU` namespace** (`AU1.x` … `AU7.x`, capability-grouped), so provenance is visible in the id and
> a collision with the `A` suite is impossible by construction.
>
> **Five structural consequences the agent correctly escalated instead of silently deciding — lead
> rulings:**
> 1. **The worker-monolith serial chain grows from 8 → 11 links** when T3-W14, T3-W9 and T8-W5 are
>    added. The canonical Wave-2 table also contains the separate T9-W1 devenv quarantine row, so
>    its total row count would be 9 → 12 if these AU rows were ever promoted. Money-path correctness
>    still outranks wall-clock; `index.ts` modularization has no AU WP and remains post-GA.
> 2. **`T5-W2` → `T5-W3` is serial.** The canonical T5-W2 owns `integrations/**`, while proposed
>    T5-W3 owns `integrations/github-actions/action.yml`. T5-W3 therefore cannot dispatch or seal
>    until T5-W2 has sealed; the shared file is never assigned to parallel agents.
> 3. **AU7.10 extends canonical `T5-W1`.** Its only in-repo target is
>    `actions/corelink-memoize/README.md`, already inside T5-W1's X. Proposed T7-W4 retains AU7.8
>    and AU7.9 only; this avoids a cross-WP ownership fiction.
> 4. **`T3-W10` is serialized behind `T3-W4` → `T4-W4`.** Three WPs would write
>    `crates/corelink-fabric-server/**`, and that is exactly the AP-1 trap the plan refuses.
> 5. **Two new gating ids are staged:** owner arming **`O-CFRATE`** (one Cloudflare Containers
>    invoice line) and relay **`R6`** (two-tenant same-memoize-key CAS read-refusal, which only
>    corelink-server can assert — `INV-6` forbids asserting it here).
>
> **`union-14` was re-scoped, not parked:** the agent checked it at HEAD and found 2 of its 3 claims
> dead (refusal now throws `SpawnRefusedError`, `index.ts:1744`; no phantom slot). The surviving half
> — `recordOrphan` returning before writing when `installationId` is absent (`index.ts:1796`), so
> every COLD at-ceiling refusal is dropped — is what carries forward. That is the ledger being
> verified rather than trusted, which is the point.
>
> **ROUND-3 CORRECTION (2026-09-01).** Round 3 found that the `AU` namespace is intentionally
> invisible to the principal `wp-check.py`, three proposed WP ids collide with rev-5 WPs, and several items still carry
> an implementation fork or an unfixed proof threshold. The corrections below rename the colliding
> WPs, split AU3.23's test from its live proof, and pre-fix the formerly open criteria. Full
> disposition is in `docs/plan/2026-09-01-round3-remediation-delta.md`.
>
> **NOT FROZEN; NOT MERGED INTO THE SUITE.** `AU` remains a staging namespace and must not be
> promoted merely because this triage is corrected. Doctrine now requires a new cold pass over the
> combined proposal and then **two consecutive quiet cold-review rounds** before freeze, baseline,
> or dispatch.

# Union catalog — triage of the remaining 25 NEW (MEDIUM/LOW) + 5 PARTIAL findings

**WP:** T0-W2 · **Authored:** 2026-08-31

**Baseline.** Worktree HEAD `eba6e8a`. The union ledger was written against `8631abb`;
`git diff --stat 8631abb eba6e8a` touches `deploy/cloudflare-fabricd/**` (the fabricd proxy
Worker + its wrangler config + two of its tests), `.claude/skills/**` and `docs/plan/**` —
**no finding triaged here cites any of those paths**, so every citation below is equally a
citation at the ledger's declared baseline. All `file:line` references were opened by me at
`eba6e8a`.

**Sources**
- `docs/plan/union-catalog-ledger.md` — the 51-finding reconciliation and its proposed items.
- `docs/plan/2026-08-30-golive-remediation-plan.md` rev-4 — §2.2 bucket vocabulary, §3 suite,
  §4 decisions D1–D10, §5 waves + WP tables, §6 armings, §7 relays, §13 invariants.
- `docs/plan/plan-check.py` — the authoritative bucket assignment for the 247.

**Scope.** The 25 NEW MEDIUM/LOW (`union-06` … `union-30`) plus the 5 PARTIAL rows
(`RH5`, `RH9`, `M3`, `M5`, `M19` uncovered halves). Out of scope and untouched:
`union-01`…`union-05` (already A3.15–A3.18), `union-31`/`32`/`33` (recorded in
`docs/plan/2026-08-30-O1-fabricd-outage-diagnosis.md`), `union-34` (WITHDRAWN — not resurrected).

**Method.** Every finding was re-opened in the code before placement. One (`union-14`) is
**partially stale** and is re-scoped to the half that survives; the rest reproduce exactly.
Nothing is placed in CLEAN-no-action or DEFER-needs-waiver — see §4.

---

## 1. Summary counts

| bucket | n | findings |
|---|---|---|
| W0-unblock | 0 | — |
| W1-parallel | 7 | union-09 · union-10 · union-16 · union-17 · union-18 · union-22 · union-24 |
| W2-serial-worker | 14 | union-06 · union-07 · union-08 · union-11 · union-12 · union-13 · union-14 · union-21 · union-25 · union-26 · RH5p · RH9p · M3p · M19p |
| W3-live-proof | 5 | union-15 · union-19 · union-27 · union-28 · union-29 |
| W4-post-decision | 2 | union-30 · M5p |
| DECISION | 0 | — |
| ARMING | 0 | (one new owner ask, **O-CFRATE**, gates `union-28`; the finding itself stays W3) |
| RELAY | 1 | union-23 (new relay **R6**; its in-repo doc half extends T5-W1) |
| DOCS-sweep | 1 | union-20 |
| CLEAN-no-action | 0 | — |
| DEFER-needs-waiver | 0 | — |
| **total** | **30** | |

**Acceptance items proposed: 31 for 30 source findings** (`AU1.8`–`AU1.9`, `AU3.19`–`AU3.22`, `AU3.23a`, `AU3.23b`,
`AU3.24`–`AU3.28`, `AU4.14`–`AU4.19`, `AU5.10`–`AU5.12`, `AU6.16`–`AU6.17`,
`AU7.6`–`AU7.12`). Every one is RED at `8bf1de7` — the
evidence column is the proof of redness, since each item asserts the negation of a behaviour
I read in the code.

**New WPs named: 10** — T3-W14, T3-W9, T3-W10, T8-W5, T8-W6, T8-W4, T5-W3, T7-W4,
T7-W5, T2-W6. The round-3 renames avoid rev-5's T3-W8, T8-W3 and T2-W5.
**Existing WPs extended: 5** — T4-W1 (+1), T8-W1 (+1), T6-W6 (+1), T3-W5 (+2), T5-W1 (+1).
The proposed ownership is mechanically checked at ≤4 items per WP by the staging gate
`python3 docs/plan/au-check.py`: it validates 30 source findings, 31 AU ids, exact-once ownership,
the coordinated WP renames, and principal/AU id separation. The principal `wp-check.py` intentionally
parses only `A`; `au-check.py` does not promote AU, alter the principal catalog, or adjudicate the
explicit serial edges and scope ownership above. A future suite-delta change must carry the same
edges and ownership into the principal catalog after review convergence.

---

## 2. Per-finding placement

Item ids continue the plan's §3 namespace, in the capability the ledger assigned each row.

| id | origin | sev | bucket | wave / WP | INV | dependency | proposed acceptance item | evidence I verified at `eba6e8a` |
|---|---|---|---|---|---|---|---|---|
| union-06 | M1 | MEDIUM | W2-serial-worker + W3-live-proof | W2 · **T8-W5** *(new)* | INV-3, INV-8 | `fabric-core-08` is a hard predecessor and must expose the durable suspension edge to the worker | **AU4.16 — test+probe:** the unit test drives the real suspension handler through the worker and asserts revoke dispatch; the live proof mints a real per-job `cas:rw` PAT, suspends the tenant mid-job, and demonstrates that the same credential is refused by CAS within **75 s**, in **3/3** independent runs, rather than merely observing a fake `revokeCasPatById` call | `deploy/cloudflare/src/index.ts:1347` `revokeCompletedJob` is the only revoke driver; its five call sites are `:1736`, `:1770`, `:2585`, `:3336` (completion / teardown / stranded-reaper) — `grep -n "suspend" index.ts` returns **zero** hits, so no suspension edge exists |
| union-07 | M4 | MEDIUM | W2-serial-worker | W2 · **T3-W14** *(new)* | INV-3 | none | **AU3.19 — test:** a job still alive past `SLOT_TTL_S` still holds its concurrency slot (the keepalive renews the slot, not only the container), and the DO's fleet count equals the number of live boxes at T+`SLOT_TTL_S`+1 s | `deploy/cloudflare/src/lib.ts:611` `export const SLOT_TTL_S = 2700`; consumed exactly once, at acquire, `index.ts:1648`. Every other mention (`index.ts:2440,2467,2575`, `lib.ts:610,1411`) is a comment or a release — **no renewal call site exists** |
| union-08 | M6 | MEDIUM | W2-serial-worker | W2 · **T3-W14** *(new)* | INV-3, INV-8 | none | **AU4.14 — test:** `driveSpawn` acquires the concurrency slot BEFORE minting — a spawn refused at capacity performs zero mint/revoke pairs (assert the fake mint is never called on the refusal path) | `index.ts:1695` `const mint = await buildContainerEnv(...)` precedes `index.ts:1720` `const slot = await acquireConcurrencySlot(env, jobId, mint, repo)`; the refusal branch at `:1736` then revokes the PAT it just minted, in-comment: *"the CAS PAT was already minted … but we're REFUSING the spawn"* |
| union-09 | M7 | MEDIUM | W1-parallel | W1 · **T8-W4** *(new)* | INV-8 | none | **AU3.26 — probe:** across **10/10 independently started jobs**, `/proc/<run.sh pid>/cmdline` inside the box contains no jitconfig token; the config is supplied through one 0600 file that is unlinked immediately after `run.sh` opens it, and the artifact records the image digest | `deploy/runner/entrypoint.sh:199` `./run.sh --jitconfig "$CORELINK_RUNNER_JITCONFIG" > "$RUNSH_FIFO" 2>&1 &` — the credential is on argv for the lease's lifetime |
| union-10 | M8 | MEDIUM | W1-parallel | W1 · **T3-W10** *(new, serial after T4-W4 — same crate)* | INV-3 | T3-W4 → T4-W4 (all three write `crates/corelink-fabric-server/**`) | **AU3.25 — test:** the stale-Pending sweep tears down BEFORE deleting the record; a throwing teardown leaves a retryable tombstone that a later tick re-attempts, and the cap slot is not reclaimed until teardown succeeds | `crates/corelink-fabric-server/src/reaper.rs:941-944` — *"3. TEARDOWN (best-effort) — no lock held. We won the delete"* … *"logged but does NOT un-reclaim the cap slot"*: delete precedes teardown, failure is terminal |
| union-11 | M9 | MEDIUM | W2-serial-worker | W2 · **T3-W9** *(new)* | INV-3 | none | **AU4.15 — test:** a throwing `RUNNER_JOB_PATS.put` after `container.start()` synchronously compensates by destroying that container and releasing its slot; the red→green test makes every post-start binding write throw and asserts exactly one destroy, one release, zero running boxes and zero swallowed success | `index.ts:1313-1319` — the `rhandle:` put is `.catch((e) => logEvent("error","kv_put_runner_handle_failed", …))` **after** `container.start()`; the durable twin at `:1323+` is a second unguarded write |
| union-12 | M10 | MEDIUM | W2-serial-worker | W2 · **T3-W9** *(new)* | INV-3 | **O-APP** (the App's webhook subscription must include `installation` events) | **AU5.10 — test:** an `installation.deleted` delivery purges that installation's tenant-map / allowlist entries, and a subsequent `workflow_job.queued` for one of its repos is refused **before** mint | `index.ts:3268` `if (request.headers.get("x-github-event") !== "workflow_job") return json({ ok: true, ignored: "not workflow_job" }, 200)` — the worker accepts exactly one event type; no `installation.*` handling exists anywhere in the file |
| union-13 | M11 | MEDIUM | W2-serial-worker | W2 · **T4-W1** *(existing, 1 → 2 items)* | INV-4, INV-5 | W0 deploy unblock (the self-check runs at deploy time); adjacent to **O-ALLOWLIST** | **AU4.18 — test:** a repo carrying both an `installation_id` and a `REPO_TENANT_PAT_MAP` entry resolves exactly one **declared** owner-of-record; **and probe:** a deploy that would empty `REPO_TENANT_PAT_MAP` fails a config self-check instead of silently re-attributing CAS + billing | `deploy/cloudflare/wrangler.jsonc:68-75` records the incident verbatim — *"This key held `\"{}\"` while prod ran a non-empty map … attribute its CAS + billing to the dogfood tenant d863fafb instead of 3c7d77b1, with NO error anywhere"*; the live map is `:75` |
| union-14 | M12 | MEDIUM | W2-serial-worker | W2 · **T3-W14** *(new)* | INV-3 | none | **AU3.20 — test:** a **COLD** at-ceiling refusal (no `installationId`) writes a terminal, queryable job record and is surfaced — today `recordOrphan` returns before writing anything | **PARTIALLY STALE — re-scoped.** Two of (b)'s three claims no longer hold: the refusal `throw`s a typed `SpawnRefusedError` (`index.ts:1744`) rather than early-returning, and it holds no phantom slot (`slot.admitted === false` means nothing was acquired, `index.ts:1721`). **The surviving half is real and is the item above:** `index.ts:1796` `if (!env.RUNNER_JOB_PATS || !opts.installationId) return; // cold ⇒ not warm-recoverable` — the catch in `driveSpawnGuarded` (`:3094`) calls `recordOrphan`, which drops every cold refusal on the floor |
| union-15 | M16 | MEDIUM | W3-live-proof | W3 · **T6-W6** *(existing, 3 → 4 items)* | — (C6 capability; no §13 invariant) | **O-CANARY** + the canary deploy (`hist-12`) | **AU6.17 — probe:** the canary executes **20/20** synthetic acquire→spawn→release transactions on consecutive ticks; every tick returns the slot count to 0 within **75 s**, and any non-zero result delivers an alert within **120 s**; the artifact records the deployed canary version id | `deploy/cloudflare-canary/src/index.ts:144-147` — the tick fetches exactly three read-only surfaces (`fetchSurface` ×2, `fetchHealth` ×1) and performs no acquire or spawn |
| union-16 | M17 | LOW | W1-parallel | W1 · **T3-W10** *(new)* | INV-1 | T3-W4 → T4-W4 (same crate) | **AU7.7 — test:** every error response from `cas_cred` (Rust) deserializes as the frozen `ErrorBody{code,message}`; the Worker twin is asserted in the same suite | `crates/corelink-fabric-server/src/handlers/cas_cred.rs:43` `fn err(status, msg) -> (status, Json(json!({ "error": msg })))` — an ad-hoc shape on a public route; the Worker twin repeats it at `index.ts:3566-3569` |
| union-17 | M18 | MEDIUM | W1-parallel | W1 · **T5-W3** *(new — serial after T5-W2)* | INV-3, INV-8 | **T5-W2 is a hard predecessor** (shared `integrations/github-actions/action.yml`) | **AU5.11 — test:** a caller passing `$(id)` / `"; touch pwned; #` as an action input has it forwarded via env indirection and never evaluated by the action's bash (assert on the composite-action source, not on a run) | `integrations/github-actions/action.yml:133-135` `ARGS+=(--url "${{ inputs.url }}")`, `:138` `if [[ -n "${{ inputs.image }}" ]]`, `:143` `if [[ "${{ inputs.verify }}" == "false" ]]` — all inside `shell: bash` steps |
| union-18 | M20 | LOW | W1-parallel | W1 · **T7-W4** *(new)* | INV-5 | none | **AU7.8 — test:** a CI check extracts credential/binding names from this exhaustive tracked-file universe: every `deploy/**/wrangler*.jsonc`, `.github/workflows/**/*.{yml,yaml}`, `deploy/**/src/**/*.{ts,js}`, `crates/**/src/**/*.rs`, `deploy/**/*.{sh,Dockerfile}` and `scripts/**/*.sh`; the only exclusions are generated/vendor trees declared in the checker. It diffs that set against `docs/runbook/secret-inventory.md`, fails on any missing/stale name, and carries one planted fixture for each source class | `docs/runbook/secret-inventory.md` (183 lines) contains **zero** occurrences of `COLD_ORGANIC_TENANT_PAT`, `CORELINK_CF_ACCESS_CLIENT_ID`, `FABRIC_TEST_MINT_KEY`, `PINNED_IMAGE_DIGEST` (`grep -c` = 0 for each); all four are live names elsewhere (e.g. `deploy/cloudflare/wrangler.jsonc:65`, `:76`) |
| union-19 | M21 | MEDIUM | W3-live-proof | W3 · **T2-W6** *(new)* | INV-2 | **O1** · **T2-W2b** (hard predecessors: the deploy path must work before rotation proof) | **AU1.8 — probe:** the runbook contains one prescribed secret-rotation rollout command sequence; a fresh operator executes it end to end in **≤10 min**, the new container rejects the old secret and accepts the new secret, the before/after Worker and container version ids are recorded, and the image digest is unchanged | `docs/runbook/secret-inventory.md:77-81` — *"reads them **only at boot** … a container rollout (new image digest + `wrangler deploy`) is what makes fabricd pick it up"*; the rollback lineage is narrative prose in `deploy/cloudflare-fabricd/wrangler.jsonc` comments, not a recipe |
| union-20 | M22 | LOW | DOCS-sweep | W1 · **T7-W4** *(new)* | INV-4, INV-5 | none | **AU7.9 — test:** a unit test asserts `plans.rs`'s per-variant docstring prices and its module ladder table quote the same numbers; it fails on today's tree | `crates/corelink-fabric/src/plans.rs:13-14` module table says `Starter $16` / `Pro $40`; `:77` and `:79` docstrings say *"entry tier ($8/mo)"* and *"growing dev + agents ($20/mo)"* — and `:81` adds a third figure, *"small team / fleet ($50/mo)"* vs the table's `Team $100` |
| union-21 | L1 | LOW | W2-serial-worker | W2 · **T3-W9** *(new)* | INV-3 | union-05 / A3.18 should land first (the atomic claim is the mechanism this item then bounds) | **AU3.22 — test:** a `workflow_job.queued` delivery replayed after `SPAWN_CLAIM_TTL_S` does not spawn a second box for the same `jobId` (a durable per-job terminal marker outlives the claim) | `deploy/cloudflare/src/lib.ts:113-124` — `claimSpawn` is the only replay guard, and its key is written with `{ expirationTtl: SPAWN_CLAIM_TTL_S }` (`:122`), `SPAWN_CLAIM_TTL_S = 7200` (`:95`). Past 2 h the claim is gone and the delivery re-enters `driveSpawn` |
| union-22 | L2 | LOW | W1-parallel | W1 · **T8-W4** *(new)* | INV-8 | none | **AU1.9 — test:** the check-exec-server reads its auth token only from a 0400 secret-mounted file and refuses to start when only `EXEC_SERVER_AUTH_TOKEN` is present, so a co-resident process cannot read it from `/proc/self/environ` | `crates/corelink-check-exec-server/src/lib.rs:53` `pub const AUTH_TOKEN_ENV: &str = "EXEC_SERVER_AUTH_TOKEN"`, documented at `:49-52` as *"injected into the container env at spawn"* |
| union-23 | L4 | LOW | **RELAY (new R6)** + DOCS-sweep half | W1 · **T5-W1** *(existing extension)*; R6 for the assertion | INV-3, INV-5 | **R6 is a HARD predecessor:** AU7.10 stays red and T5-W1 cannot seal until the committed relay artifact records **20/20 refusals in each direction** for two tenants using the same memoize key; the isolation assertion lives in `corelink-server` and the session fence forbids editing it here | **AU7.10 — test:** only after R6 is committed, `actions/corelink-memoize/README.md` cites its artifact id and states that memoize-key isolation rests entirely on CAS-side tenant scoping because the key carries no tenant component; a doc test fails when the R6 id is absent or unresolved | `actions/corelink-memoize/action.yml` — `grep -i tenant` returns **nothing**; the key inputs are `CL_RUN` / `CL_INPUTS` / `CL_ENVNAMES` / `CL_TOOLS` (`:38-42`), no tenant among them |
| union-24 | L6 | LOW | W1-parallel | W1 · **T5-W3** *(new — serial after T5-W2)* | — | **T5-W2 is a hard predecessor** (shared `integrations/github-actions/action.yml`) | **AU5.12 — test:** CI executes the complete action in a pinned bash-only container containing no `python3`, and the run exits 0 while producing the expected outputs | `integrations/github-actions/action.yml:174` `shell: python3 {0}` — the only non-bash step of the four |
| union-25 | L7 | LOW | W2-serial-worker | W2 · **T8-W5** *(new)* | INV-4 | none | **AU4.17 — test:** `revokeCompletedJob` with no derived tenant refuses loudly (logs + bumps a registered counter, returns an error) instead of falling back to the wrangler `CLW_TENANT` var | `index.ts:1356` `await revokeCasPatById(env, patId, derivedTenant ?? env.CLW_TENANT)`; the call-site comment at `:2583` concedes *"best-effort: revokeCompletedJob falls back to CLW_TENANT"*. `index.ts:1574` already records that this exact pattern **mis-attributed a customer's `runner_slot_seconds`** on the metering path |
| union-26 | L8 | LOW | W2-serial-worker | W2 · **T3-W14** *(new)* | — (benign; correctness of an attempt counter) | none | **AU3.21 — test:** the concurrency Durable Object owns the orphan-attempt count, and 100 concurrent `recordOrphan` / retry-bump calls for one `jobId` produce an exact count of 100 with no lost update | `index.ts:1796-1810` — `if (await …get(key)) return;` then `put(...)`, no CAS; the retry loop repeats the pattern near `:4085` |
| union-27 | P1 | MEDIUM | W3-live-proof | W3 · **T7-W5** *(new)* | INV-5 | **O1** (needs a live, warm fleet before the distribution means anything) | **AU7.11 — probe:** publish a measured queued→RUNNING distribution (p50/p95) from ≥50 real jobs, recorded under `docs/plan/evidence/` with the worker version id, and state the job-size break-even in the pitch docs | No measured distribution exists in-repo: `docs/plan/evidence/` does not exist, and no doc outside the two 2026-08-25 audit files carries a spawn-latency measurement. The start/retry path the claim is about is `container.start()` driven from `driveSpawn` (`index.ts:1752`) |
| union-28 | E1 | MEDIUM | W3-live-proof | W3 · **T7-W5** *(new)* | INV-4, INV-5 | **O-CFRATE** *(new owner ask: produce one Cloudflare Containers invoice line — vCPU-h and GiB-h — for a named billing period)* | **AU4.19 — probe:** `docs/product/pricing.md` records the real Cloudflare Containers vCPU-h/GiB-h rate with a cited invoice line, and the margin table is re-derived from it | `grep -c Cloudflare docs/product/pricing.md` = **0**. Every margin derives from the Northflank proxy basis: `:10`, `:22`, `:37`, `:42`, `:153`, `:191` (*"COGS basis: Northflank per-CI-minute, $0.10/vCPU-hour"*), `:254` |
| union-29 | E2 | MEDIUM | W3-live-proof | W3 · **T7-W5** *(new)* | INV-5 | **O1** · **T3-W7** (A3.9 is a hard predecessor) | **AU7.12 — probe:** instrument `clw` hit/miss for **7 consecutive 24 h windows and ≥500 real executions**, report the measured rate with a 95 % Wilson interval, and replace `pricing.md`'s 85–95 % margin range with the margin interval derived from that artifact | `docs/product/pricing.md:219-222` — *"The typical margin (85–95%) depends on the **memoization hit-rate**, which is high for repetitive CI/agent workloads in theory but **unmeasured**"*; the same 85–95 % carries the tier table at `:157-161` |
| union-30 | E3 | MEDIUM | W4-post-decision | W4 · **T3-W5** *(existing, named in §4 as D4's WP)* | INV-3 | **D4 is a hard predecessor** and its committed ADR artifact must contain the exact over-cap response schema | **AU3.27 — test:** `FLEET_MAX_CONCURRENCY` is env-tunable without a recompile, and 100 over-cap requests all return the one response schema fixed by D4; zero requests disappear without a terminal customer-visible response | `deploy/cloudflare/src/lib.ts:620` `export const FLEET_MAX_CONCURRENCY = 250` — a compiled constant, consumed at `index.ts:1640,1647`; `deploy/cloudflare/wrangler.jsonc:239` says it *"MUST stay in sync with FLEET_MAX_CONCURRENCY in src/lib.ts"*, i.e. the coupling is manual |
| **RH5** *(partial)* | RH5 uncovered half | LOW | W2-serial-worker | W2 · **T8-W1** *(existing, 3 → 4 items)* | INV-5 | none | **AU7.6 — test:** no two statements in `deploy/cloudflare/src/index.ts` assert opposite metadata-probe status; the surviving statement cites the closed probe run by artifact id | **What `gap-16` does NOT cover:** `gap-16` is the egress mechanism (exact-host-only denylist, inert CIDRs, raw-socket bypass). The uncovered half is the **in-file doc contradiction**, both live: `index.ts:46` *"A live-account smoke is still owed even for the exact-host entries above"* vs `index.ts:595` *"G2 is settled by the metadata probe"* |
| **RH9** *(partial)* | RH9 uncovered half | MEDIUM | W2-serial-worker | W2 · **T3-W9** *(new)* | INV-3 | A3.3 (`COUNTER_NAMES` registration, T3-W2) must land first or the new counter is unregistered by construction | **AU6.16 — test:** a limiter-refused webhook returns 202 with its dead-letter written, and a bad-HMAC `POST /webhook` bumps a **registered** `webhook_auth_failed` counter | **What `hist-12`/`deploy-13` do NOT cover:** they are alert *delivery* (canary undeployed, delivery unarmed). The uncovered half is the two response/counter defects: `index.ts:3487` `return json({ error: "rate limited" }, 429)` (the dead-letter at `:3485` softens it, the 429 to GitHub stands), and `index.ts:3264-3265` `if (!(await verifyGithubHmac(...))) return unauthorized();` with **no** metric bump — `grep -c webhook_auth_failed index.ts metrics.ts` = 0, 0 |
| **M3** *(partial)* | M3 uncovered half | MEDIUM | W2-serial-worker + W3-live-proof | W2 · **T8-W5** + W3 · **T8-W6** *(new)* | INV-3, INV-8 | AU3.23a is a hard predecessor of AU3.23b | **AU3.23a — test:** a throwing revoke on the completed path writes a durable retry, and a later tick succeeds; every failure bumps the registered `revoke_failed` counter. **AU3.23b — probe:** in **10/10** real completed jobs, the minted `cas:rw` PAT is refused by CAS within **75 s** of completion; the artifact records completion, retry and refusal timestamps | **What `sec-03` does NOT cover:** `sec-03` is the multi-use redemption window (`lib.ts:455-465`). The uncovered half is the **fail-open revoke**: `index.ts:1345-1346` *"Fail-OPEN: any error is swallowed (the PAT TTL-expires) — never breaks the webhook"*, implemented at `:1359-1362` — `catch` logs `revoke_failed` and returns `false`, with no retry, no dead-letter and no counter |
| **M5** *(partial)* | M5 uncovered half | MEDIUM | W4-post-decision | W4 · **T3-W5** *(existing)* | INV-3 | **D4** (what a customer is owed at cap is exactly the ADR-0005 question) | **AU3.28 — test:** an orphan record exhausting `MAX_ORPHAN_ATTEMPTS` / `ORPHAN_TTL_S` transitions to a terminal, queryable state surfaced to the customer instead of being deleted | **What `fabric-core-14`/`gap-13` do NOT cover:** they are the inert Rust `pg_queue` and the unratified queued-admission mode. The uncovered half is the **spawn-worker giveup drop**: `index.ts:4078` `logEvent("error","orphan_retry_giveup", …)` inside the `giveup` branch that `kv.delete(name)`s the record — no terminal state, no customer signal. Bounds: `lib.ts:1119` `ORPHAN_TTL_S = 1800`, `:1122` `MAX_ORPHAN_ATTEMPTS = 3` |
| **M19** *(partial)* | M19 uncovered half | LOW | W2-serial-worker | W2 · **T8-W5** *(new)* | INV-3 | none | **AU3.24 — test:** the Worker's `/v1/leases/{id}/cas-cred` returns one uniform response for a bad ticket, an already-redeemed ticket and an unknown lease (no existence oracle over enumerable lease ids) | **What `fabricd-06` does NOT cover:** it names the defect on the **Rust** handler only. The uncovered half is the Worker's own twin, which makes the identical three-way distinction: `index.ts:3566-3568` — `401 invalid ticket` / `410 ticket already redeemed` / `404 no such lease` |

---

## 3. Structural consequences

**New WPs and their item counts** (all ≤ the `wp-check.py` 4-item ceiling):

| WP | wave | owns | exclusive files (the X) |
|---|---|---|---|
| **T3-W14** *(new)* — slot & admission accounting | W2 (serial) | AU3.19 AU4.14 AU3.20 AU3.21 | `deploy/cloudflare/src/index.ts` + `lib.ts` |
| **T3-W9** *(new)* — spawn/webhook path durability | W2 (serial) | AU4.15 AU3.22 AU5.10 AU6.16 | `deploy/cloudflare/src/index.ts` + `lib.ts` |
| **T8-W5** *(new)* — credential-revocation lifecycle | W2 (serial; AU4.16 also requires live proof) | AU4.16 AU3.23a AU4.17 AU3.24 | `deploy/cloudflare/src/index.ts` |
| **T8-W6** *(new)* — credential-revocation live proof | W3 | AU3.23b | `docs/plan/evidence/**` |
| **T3-W10** *(new)* — fabric-server reaper + error vocabulary | W1 (serial after T4-W4) | AU3.25 AU7.7 | `crates/corelink-fabric-server/**` |
| **T8-W4** *(new)* — in-box secret hygiene | W1 | AU3.26 AU1.9 | `deploy/runner/entrypoint.sh`, `crates/corelink-check-exec-server/**` |
| **T5-W3** *(new)* — GitHub Action shell safety | W1 (serial after T5-W2) | AU5.11 AU5.12 | `integrations/github-actions/action.yml` |
| **T7-W4** *(new)* — inventory + price-string truth | W1 | AU7.8 AU7.9 | `docs/runbook/secret-inventory.md`, `crates/corelink-fabric/src/plans.rs`, new `scripts/ci/secret-inventory-drift.sh` |
| **T7-W5** *(new)* — measured-claim probes | W3 | AU7.11 AU4.19 AU7.12 | `docs/product/pricing.md`, `docs/plan/evidence/**` |
| **T2-W6** *(new)* — fabricd secret-rotation rollout | W3 | AU1.8 | `docs/runbook/**` (rotation playbook) |

**Extensions to existing WPs:** T4-W1 `+AU4.18` (1→2) · T8-W1 `+AU7.6` (3→4) ·
T6-W6 `+AU6.17` (3→4) · T5-W1 `+AU7.10` (1→2) · T3-W5 `+AU3.27 +AU3.28`
(0→2 — this is the first content T3-W5 owns, so D4 landing now has a falsifiable WP behind it).

**Three things the lead must decide or fix before dispatch:**

1. **The worker-monolith serial chain grows from 8 links to 11** (T3-W14, T3-W9, T8-W5 inserted).
   T9-W1 is the separate devenv row in the canonical Wave-2 table; including it would make the
   full table 9 → 12. `index.ts` modularization remains post-GA and is not assigned to this triage.
2. **T3-W10 must be serialized behind T3-W4 → T4-W4** — three WPs now write
   `crates/corelink-fabric-server/**`. The same exemption the Wave-2 chain carries applies.
3. **`O-CFRATE` is a new owner arming** not in §6: produce one Cloudflare Containers invoice
   line (vCPU-h + GiB-h, named billing period). Without it AU4.19 cannot go green, and every
   margin number in `pricing.md` keeps resting on a Northflank proxy rate.
   **`R6` is a new relay** not in §7: the two-tenant same-memoize-key CAS read-refusal proof,
   which only `corelink-server` can assert.

---

## 4. Findings

### Findings I did NOT place in CLEAN-no-action or DEFER — and why

The brief forbids parking a MEDIUM-or-higher without justification. **I parked none.** The
one candidate was `union-14` (MEDIUM), whose ledger claim is two-thirds stale at HEAD:

- *"early-return without `installationId`"* — **stale.** The refusal `throw`s
  `SpawnRefusedError` (`index.ts:1744`), and the in-code comment at `:1737-1743` documents
  that the bare `return` was the previous bug and was removed.
- *"holds a phantom slot for 45 min"* — **stale.** The refusal branch is entered on
  `!slot.admitted`, i.e. no slot was ever acquired (`index.ts:1721`).
- *"a cold at-ceiling refusal leaves a stranded job with no terminal state"* — **real, and
  kept.** `recordOrphan` returns before writing when `installationId` is absent
  (`index.ts:1796`), which is exactly the cold case. AU3.20 is scoped to this half only.

I therefore report `union-14` as **partially stale, re-scoped, still placed** rather than as
CLEAN. Every other finding reproduced exactly as the ledger describes it.

## 5. Unplaceable

**None.** All 30 findings have a bucket, a wave, a WP and one or more acceptance items (31 proposed
AU ids; M3 intentionally owns the split AU3.23a/AU3.23b pair).

Two placements have hard predecessors, not implementation choices:

- **`union-30`** is placed in W4 behind **D4** because its second half (a customer-visible
  over-cap signal) *is* the ADR-0005 question. The finding stays whole in T3-W5; D4's committed
  artifact fixes the exact response schema before dispatch.
- **`union-23`** is the only row whose assertion cannot be written in this repo at all: CAS
  tenant scoping lives in `corelink-server` and the session fence (INV-6) forbids reaching it.
  Relay **R6** is a hard predecessor of the in-repo documentation test AU7.10 and T7-W4 seal.
