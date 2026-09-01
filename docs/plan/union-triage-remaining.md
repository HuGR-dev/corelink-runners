> **LEAD LANDING NOTE (2026-08-31).** This triage was produced against the plan as of `8631abb`,
> before rev-5 added its own items — so 14 of its proposed ids COLLIDED with ids the plan had already
> taken. Merging it as returned would have silently overwritten live suite items. Resolved
> structurally rather than by shuffling numbers: every item proposed here now lives in its own
> **`AU` namespace** (`AU1.x` … `AU7.x`, capability-grouped), so provenance is visible in the id and
> a collision with the `A` suite is impossible by construction.
>
> **Five structural consequences the agent correctly escalated instead of silently deciding — lead
> rulings:**
> 1. **The worker-monolith additions are serialized.** The exact spine and ready sets live only in
>    `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; `index.ts` modularization has no AU WP and
>    remains post-GA.
> 2. **The release path is ordered in the canonical DAG.** It records T5-W3 after T5-W2 and makes
>    T5-W3 a hard predecessor of T5-W6, so the publish proof cannot precede the repaired action.
> 3. **AU7.10 extends canonical `T5-W1`.** Its only in-repo target is
>    `actions/corelink-memoize/README.md`, already inside T5-W1's X. Proposed T7-W4 retains AU7.8
>    and AU7.9 only; this avoids a cross-WP ownership fiction.
> 4. **`T3-W10` is serialized with the earlier fabric-server packets in the canonical DAG.** Three
>    WPs would otherwise write `crates/corelink-fabric-server/**` concurrently, the AP-1 trap.
> 5. **Two new gating ids are staged:** owner arming **`O-CFRATE`** (one Cloudflare Containers
>    invoice line) and relay **`R6`** (two-tenant same-memoize-key CAS read-refusal, which only
>    corelink-server can assert — `INV-6` forbids asserting it here).
>
> **`union-14` was re-scoped, not parked:** the agent reopened it in the reconciled tree and found 2 of its 3 claims
> dead (refusal now throws `SpawnRefusedError`, `index.ts:1802`; no phantom slot). The surviving half
> — `recordOrphan` returning before writing when `installationId` is absent (`index.ts:1854`), so
> every COLD at-ceiling refusal is dropped — is what carries forward. That is the ledger being
> verified rather than trusted, which is the point.
>
> **ROUND-3 CORRECTION (2026-09-01).** Round 3 found that the `AU` namespace is intentionally
> invisible to the principal `wp-check.py`, three proposed WP ids collide with rev-5 WPs, and several items still carry
> an implementation fork or an unfixed proof threshold. The corrections below rename the colliding
> WPs, split AU3.23's test from its live proof, and pre-fix the formerly open criteria. Full
> disposition is in `docs/plan/2026-09-01-round3-remediation-delta.md`.
>
> **ROUND-5 CORRECTION (2026-09-01).** All 30 source findings were re-opened against the
> reconciled runtime tree at `b70deae` and the planning snapshot at `3fe8d06`; runtime paths cited
> below are byte-identical between those commits. The mixed historical baselines and stale line
> anchors were removed. AU4.16 and AU3.26 now split repo tests from live proofs, AU6.17 moves behind
> its alert-rule predecessors, and the remaining acceptance contracts and evidence ownership are
> fixed below. Schedule truth lives only in
> `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; this document stages AU ownership and criteria
> and does not create a second schedule.
>
> **NOT FROZEN; NOT MERGED INTO THE SUITE.** `AU` remains a staging namespace and must not be
> promoted merely because this triage is corrected. Doctrine now requires a new cold pass over the
> combined proposal and then **two consecutive quiet cold-review rounds** before freeze, baseline,
> or dispatch.

# Union catalog — triage of the remaining 25 NEW (MEDIUM/LOW) + 5 PARTIAL findings

**WP:** T0-W2 · **Authored:** 2026-08-31

**Validation snapshot.** The union ledger was historically written against `8631abb` and the
first triage pass used `eba6e8a`; neither historical SHA supplies current credit. For this correction,
all 30 source findings were re-opened in the reconciled runtime source at `b70deae` and checked again
in the planning snapshot `3fe8d06`. `git diff b70deae..3fe8d06` changes planning/docs/CI only, so the
runtime anchors below are identical at both commits. The only partially stale source claim remains
`union-14`, re-scoped explicitly below. This revalidation is staging evidence, not an acceptance
baseline and not green credit.

**Sources**
- `docs/plan/union-catalog-ledger.md` — the 51-finding reconciliation and its proposed items.
- `docs/plan/2026-08-30-golive-remediation-plan.md` — bucket vocabulary, acceptance suite,
  decisions, WP inventory, armings, relays and invariants at the reconciled planning snapshot.
- `docs/plan/2026-09-01-reconciled-dispatch-dag.md` — the sole canonical combined dependency graph,
  phase assignment and topological ready sets; no schedule is independently inferred here.
- `docs/plan/plan-check.py` — the authoritative bucket assignment for the 247.

**Scope.** The 25 NEW MEDIUM/LOW (`union-06` … `union-30`) plus the 5 PARTIAL rows
(`RH5`, `RH9`, `M3`, `M5`, `M19` uncovered halves). Out of scope and untouched:
`union-01`…`union-05` (already A3.15–A3.18), `union-31`/`32`/`33` (recorded in
`docs/plan/2026-08-30-O1-fabricd-outage-diagnosis.md`), `union-34` (WITHDRAWN — not resurrected).

**Method.** Every finding was re-opened in the reconciled source before placement. One (`union-14`)
is **partially stale** and is re-scoped to the half that survives; the other 29 reproduce. Historical
incident statements are labelled as such and are not represented as current live observations.
Nothing is placed in CLEAN-no-action or DEFER-needs-waiver — see §4.

---

## 1. Summary counts

The bucket table counts each source finding exactly once by primary placement. Split live-proof
acceptance items for union-06 and union-09 do not double-count their source rows.

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

**Acceptance items proposed: 33 for 30 source findings** (`AU1.8`–`AU1.9`, `AU3.19`–`AU3.22`,
`AU3.23a`, `AU3.23b`, `AU3.24`–`AU3.25`, `AU3.26a`, `AU3.26b`, `AU3.27`–`AU3.28`,
`AU4.14`–`AU4.15`, `AU4.16a`, `AU4.16b`, `AU4.17`–`AU4.19`, `AU5.10`–`AU5.12`,
`AU6.16`–`AU6.17`, `AU7.6`–`AU7.12`). Each is RED at the reconciled `b70deae` runtime
tree: the source column records current negative evidence. This does not mix those observations
with the historical `8bf1de7` containment snapshot and does not constitute freeze evidence.

**New WPs named: 11** — T3-W14, T3-W9, T3-W10, T8-W5, T8-W6, T8-W7, T8-W4,
T5-W3, T7-W4, T7-W5, T2-W6. The round-3 renames avoid rev-5's T3-W8, T8-W3 and
T2-W5. **Existing WPs extended: 5** — T4-W1 (+1), T8-W1 (+1), T6-W10 (+1),
T3-W5 (+2), T5-W1 (+1).
The proposed ownership is mechanically checked at ≤4 items per WP by the staging gate
`python3 docs/plan/au-check.py`: it validates 30 source findings, 33 AU ids, exact-once ownership,
the coordinated WP renames, and principal/AU id separation. The principal `wp-check.py` intentionally
parses only `A`; `au-check.py` does not promote AU, alter the principal catalog, or adjudicate the
explicit serial edges and scope ownership above. A future suite-delta change must carry the same
edges and ownership into the principal catalog after review convergence.

---

## 2. Per-finding placement

Item ids continue the plan's §3 namespace, in the capability the ledger assigned each row.

| id | origin | sev | bucket | phase / WP | INV | dependency | proposed acceptance item | source evidence revalidated at `b70deae` / `3fe8d06` |
|---|---|---|---|---|---|---|---|---|
| union-06 | M1 | MEDIUM | W2-serial-worker + W3-live-proof | repo · **T8-W5**; live · **T8-W6** | INV-3, INV-8 | AU4.16a consumes `fabric-core-08`'s durable suspension signal; AU4.16a → AU4.16b; WP routing is canonical-DAG-owned | **AU4.16a — test:** drive the real suspension event/handler through the Worker and assert exactly one revoke is durably dispatched for the active job, with no fake-call-only credit. **AU4.16b — probe:** mint a real per-job `cas:rw` PAT, suspend the tenant mid-job, and prove that credential is refused by CAS within **75 s** in **3/3** independent runs; write only `docs/plan/evidence/au4.16b-suspension-pat-revocation.json` with version, suspend and refusal timestamps | `deploy/cloudflare/src/index.ts:1350` is the only revoke driver; its current call sites are `:1794`, `:1828`, `:2669`, `:3478` (at-ceiling/spawn failure, teardown/stranded, completion). No suspension event or suspension-triggered call site exists |
| union-07 | M4 | MEDIUM | W2-serial-worker | repo · **T3-W14** | INV-3 | none | **AU3.19 — test:** a job still alive past `SLOT_TTL_S` still holds its concurrency slot (the keepalive renews the slot, not only the container), and the DO's fleet count equals the number of live boxes at T+`SLOT_TTL_S`+1 s | `deploy/cloudflare/src/lib.ts:656` fixes `SLOT_TTL_S = 2700`; its only operational consumption is acquisition at `index.ts:1651`. The other current mentions are comments or release/backstop descriptions; no slot-renewal call site exists |
| union-08 | M6 | MEDIUM | W2-serial-worker | repo · **T3-W14** | INV-3, INV-8 | none | **AU4.14 — test:** `driveSpawn` acquires the concurrency slot BEFORE minting — a spawn refused at capacity performs zero mint/revoke pairs (assert the fake mint is never called on the refusal path) | `index.ts:1739` builds/mints the container environment before `:1778` acquires a slot; the refusal branch at `:1794` revokes the already-minted PAT |
| union-09 | M7 | MEDIUM | W1-parallel + W3-live-proof | repo · **T8-W4**; live · **T8-W7** | INV-8 | AU3.26a → runner image build/deploy nodes in the canonical DAG → AU3.26b | **AU3.26a — test:** the entrypoint supplies JIT config through one mode-0600 file, never argv or inherited environment, and unlinks it immediately after `run.sh` opens it; source/process-fixture tests assert no secret-bearing `--jitconfig` argument. **AU3.26b — probe:** across **10/10** independently started jobs, `/proc/<run.sh pid>/cmdline` and `/proc/<run.sh pid>/environ` contain no JIT config token; write only `docs/plan/evidence/au3.26b-jitconfig-process-surface.json` with the deployed image digest and version | `deploy/runner/entrypoint.sh:199` still launches `./run.sh --jitconfig "$CORELINK_RUNNER_JITCONFIG"`, leaving the credential on argv for the lease lifetime |
| union-10 | M8 | MEDIUM | W1-parallel | repo · **T3-W10** | INV-3 | fabric-server predecessors are canonical-DAG-owned | **AU3.25 — test:** the stale-Pending sweep tears down BEFORE deleting the record; a throwing teardown leaves a retryable tombstone that a later tick re-attempts, and the cap slot is not reclaimed until teardown succeeds | `crates/corelink-fabric-server/src/reaper.rs:941-950` deletes/reclaims before best-effort teardown and states that failure leaves a leaked box with the slot already freed and no retry |
| union-11 | M9 | MEDIUM | W2-serial-worker | repo · **T3-W9** | INV-3 | none | **AU4.15 — test:** before post-start binding writes, commit a teardown intent in ConcurrencySlotsDO. Inject failure independently at each `RUNNER_JOB_PATS.put` (`jtenant:`, `vcpu-ceiling:`, `jhandle:`, `rhandle:`, `sbox:`), then inject all five jointly. Every cell returns failure, destroys exactly the started container, releases the slot exactly once and leaves zero running boxes. If destroy or release compensation fails independently or jointly, the intent remains a durable retry; a later tick completes destroy+release idempotently, and no cell returns success | `index.ts:1254` starts the container; all post-start binding writes at `:1267-1342` catch/log or otherwise lack a transactional compensation boundary. In particular `rhandle:` (`:1301-1322`) and its `sbox:` durable twin (`:1323-1342`) can both fail after start while `spawnRunner` still returns success |
| union-12 | M10 | MEDIUM | W2-serial-worker | repo · **T3-W9** | INV-3 | **O-APP** (the App subscription includes `installation` events) | **AU5.10 — test:** an `installation.deleted` delivery purges that installation's tenant-map / allowlist entries, and a subsequent `workflow_job.queued` for one of its repos is refused **before** mint | `index.ts:3410` accepts only `workflow_job`; no `installation.deleted`/`installation.*` handler exists in the Worker |
| union-13 | M11 | MEDIUM | W2-serial-worker | repo · **T4-W1** *(existing extension)* | INV-4, INV-5 | **D13 is a hard predecessor** and fixes the exact precedence/conflict-error contract; deploy self-check follows the canonical DAG and is adjacent to O-ALLOWLIST | **AU4.18 — test:** implement D13 verbatim: fixtures where `installation_id` and `REPO_TENANT_PAT_MAP` agree resolve the D13 owner; divergent values return D13's exact conflict error before mint, attribution, claim or spawn. A deploy whose candidate config would empty a previously non-empty map fails the config self-check rather than silently re-attributing CAS or billing | `deploy/cloudflare/wrangler.jsonc:65-75` preserves the historical incident narrative and the current non-empty map. Current Worker resolution still has two inputs (`index.ts:1732-1742`), but no committed owner-of-record/conflict contract; D13, not the implementer, must choose it |
| union-14 | M12 | MEDIUM | W2-serial-worker | repo · **T3-W14** | INV-3 | none | **AU3.20 — test:** on a COLD at-ceiling refusal, the ConcurrencySlotsDO commits `{job_id,state:"refused_at_ceiling",reason,recorded_at_ms}` under the job id before the spawn claim is released; authenticated `GET /v1/jobs/{job_id}/status` returns that exact terminal object within **5 s** and continues to return it for **24 h**. A store failure returns the fixed retryable 503 and creates zero mint/JIT/lease/box side effects | **PARTIALLY STALE, precisely scoped.** The historical early-return and phantom-slot claims are closed: current refusal throws `SpawnRefusedError` at `index.ts:1802`, and `!slot.admitted` at `:1779` means no slot was acquired. The surviving defect remains: `recordOrphan` returns for missing `installationId` at `:1854`; `driveSpawnGuarded` calls it at `:3236`, so a COLD refusal has no terminal query surface |
| union-15 | M16 | MEDIUM | W3-live-proof | live · **T6-W10** *(existing extension)* | — (C6 capability; no §13 invariant) | alert-rule, delivery, arming and deploy predecessors are canonical-DAG-owned | **AU6.17 — probe:** the canary executes **20/20** synthetic acquire→spawn→release transactions on consecutive ticks; every tick returns slot count to 0 within **75 s**, and any non-zero result delivers an alert within **120 s**; write only `docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json` with deployed canary/Worker versions and transaction/alert timestamps | Current `deploy/cloudflare-canary/src/index.ts:140-163` constructs only status, health and metrics fetches (fabric probes may be disabled); it has no acquire, spawn or release transaction |
| union-16 | M17 | LOW | W1-parallel | repo · **T3-W10** | INV-1 | fabric-server predecessors are canonical-DAG-owned | **AU7.7 — test:** every error response from `cas_cred` (Rust) deserializes as the frozen `ErrorBody{code,message}`; a read-only assertion over the Worker twin checks the same vocabulary without assigning Worker writes to this WP | `crates/corelink-fabric-server/src/handlers/cas_cred.rs:43-44` still emits ad-hoc `{"error":msg}`; the Worker twin distinguishes ad-hoc errors at `index.ts:3709-3711` |
| union-17 | M18 | MEDIUM | W1-parallel | repo · **T5-W3** | INV-3, INV-8 | release predecessors are canonical-DAG-owned; T5-W6 includes T5-W3 | **AU5.11 — test:** a caller passing `$(id)` / `"; touch pwned; #` as each action input has it forwarded via env indirection and never evaluated by bash; the composite-action source assertion covers every input interpolation | `integrations/github-actions/action.yml:133-143` still interpolates `inputs.url`, `inputs.check`, `inputs.check-id`, `inputs.image` and `inputs.verify` directly inside a `shell: bash` script |
| union-18 | M20 | LOW | W1-parallel | repo · **T7-W4** | INV-5 | none | **AU7.8 — test:** a CI check extracts credential/binding names from this exhaustive tracked-file universe: every `deploy/**/wrangler*.jsonc`, `.github/workflows/**/*.{yml,yaml}`, `deploy/**/src/**/*.{ts,js}`, `crates/**/src/**/*.rs`, `deploy/**/*.{sh,Dockerfile}` and `scripts/**/*.sh`; the only exclusions are generated/vendor trees declared in the checker. It diffs that set against `docs/runbook/secret-inventory.md`, fails on missing/stale names, and carries one planted fixture for each source class | The current 183-line inventory still contains zero occurrences of `COLD_ORGANIC_TENANT_PAT`, `CORELINK_CF_ACCESS_CLIENT_ID`, `FABRIC_TEST_MINT_KEY` and `PINNED_IMAGE_DIGEST`; current definitions/usages include `deploy/cloudflare/wrangler.jsonc:64-75`, `deploy/cloudflare-fabricd/src/index.ts:91,118` and `deploy/cloudflare/src/index.ts:151,159` |
| union-19 | M21 | MEDIUM | W3-live-proof | live · **T2-W6** | INV-2 | O1 · T2-W2b as routed by the canonical DAG | **AU1.8 — probe:** the runbook contains one prescribed secret-rotation rollout command sequence; a fresh operator executes it end to end in **≤10 min**, the new container rejects the old secret and accepts the new secret, the before/after Worker and container version ids are recorded, and the image digest is unchanged; write only `docs/plan/evidence/au1.8-fabricd-secret-rotation.json` | `docs/runbook/secret-inventory.md:77-81` still says fabricd reads secrets only at boot and requires a container rollout; rollback digest lineage remains narrative `SUPERSEDES` comments in `deploy/cloudflare-fabricd/wrangler.jsonc:245,278,298`, not an executable recipe |
| union-20 | M22 | LOW | DOCS-sweep | W1 · **T7-W4** *(new)* | INV-4, INV-5 | none | **AU7.9 — test:** a unit test asserts `plans.rs`'s per-variant docstring prices and its module ladder table quote the same numbers; it fails on today's tree | `crates/corelink-fabric/src/plans.rs:13-14` module table says `Starter $16` / `Pro $40`; `:77` and `:79` docstrings say *"entry tier ($8/mo)"* and *"growing dev + agents ($20/mo)"* — and `:81` adds a third figure, *"small team / fleet ($50/mo)"* vs the table's `Team $100` |
| union-21 | L1 | LOW | W2-serial-worker | repo · **T3-W9** | INV-3 | A3.18 is a hard predecessor | **AU3.22 — test:** keep attempt 1 non-terminal with its box active, advance to `SPAWN_CLAIM_TTL_S + 1 s`, then replay the same `workflow_job.queued` delivery 100 times concurrently. A durable active-attempt marker that outlives the claim TTL yields zero additional mints, JIT configs, slots or boxes; only an authoritative terminal/absent transition may clear it | `deploy/cloudflare/src/lib.ts:100,118-128` shows the only replay guard is `spawn:<jobId>` with the 7200-second TTL. When it expires, no separate durable active-attempt marker prevents re-entry |
| union-22 | L2 | LOW | W1-parallel | W1 · **T8-W4** *(new)* | INV-8 | none | **AU1.9 — test:** the check-exec-server reads its auth token only from a 0400 secret-mounted file and refuses to start when only `EXEC_SERVER_AUTH_TOKEN` is present, so a co-resident process cannot read it from `/proc/self/environ` | `crates/corelink-check-exec-server/src/lib.rs:53` `pub const AUTH_TOKEN_ENV: &str = "EXEC_SERVER_AUTH_TOKEN"`, documented at `:49-52` as *"injected into the container env at spawn"* |
| union-23 | L4 | LOW | **RELAY (new R6)** + DOCS-sweep half | repo · **T5-W1** *(existing extension)*; R6 owns the assertion | INV-3, INV-5 | **R6 is a HARD predecessor:** AU7.10 stays red and T5-W1 cannot seal until the committed relay artifact records **20/20 refusals in each direction** for two tenants using the same memoize key | **AU7.10 — test:** only after R6 is committed, `actions/corelink-memoize/README.md` cites its artifact id and states that memoize-key isolation rests entirely on CAS-side tenant scoping because the key carries no tenant component; a doc test fails when the R6 id is absent or unresolved | Current `actions/corelink-memoize/action.yml:38-42` builds the key from run, inputs, env names and tools; the file has no tenant input/component |
| union-24 | L6 | LOW | W1-parallel | repo · **T5-W3** | — | release predecessors are canonical-DAG-owned; T5-W6 includes T5-W3 | **AU5.12 — test:** in a pinned container with `bash` and no `python3`, a stub `corelink` returning `{"lease_id":"lease-au5-12","exit":0,"verified":true}` executes the complete action successfully and produces exactly the public outputs `exit=0`, `verified=true`, `lease-id=lease-au5-12` (backed by step outputs `exit`, `verified`, `lease_id`) | `integrations/github-actions/action.yml:74-85` declares those three public outputs, while the parse step at `:172-207` still requires `shell: python3 {0}` |
| union-25 | L7 | LOW | W2-serial-worker | repo · **T8-W5** | INV-4 | none | **AU4.17 — test:** `revokeCompletedJob` with no derived tenant refuses loudly (logs + bumps a registered counter, returns an error) instead of falling back to the wrangler `CLW_TENANT` var | `index.ts:1359` still calls `revokeCasPatById(..., derivedTenant ?? env.CLW_TENANT)`; `:2667` documents the fallback. The billing path's historical mis-attribution is recorded separately at `:1577-1580`; that comment is incident history, not proof of a current live occurrence |
| union-26 | L8 | LOW | W2-serial-worker | repo · **T3-W14** | — (benign; retry accounting) | none | **AU3.21 — test:** the concurrency Durable Object owns retry epochs and the orphan-attempt count. For one job, 100 concurrent duplicates carrying the same epoch id increment the count exactly once (`+1`); two later distinct epoch ids each increment once (final count `3`), and 100 replays of any consumed epoch add zero. The test counts retry epochs, not API calls | `index.ts:1857-1868` uses get-then-put for the first record, while the retry bump at `:4227-4233` is another non-atomic put whose own comment accepts a lost update. Neither stores an idempotency/epoch key |
| union-27 | P1 | MEDIUM | W3-live-proof | live · **T7-W5** | INV-5 | O1 as routed by the canonical DAG | **AU7.11 — probe:** publish queued→RUNNING p50/p95 from ≥50 real jobs in exactly `docs/plan/evidence/au7.11-queued-running-latency.json`, with Worker version id and timestamps, and state the measured job-size break-even in the pitch docs | `docs/plan/evidence/` now exists, but its current files are PG containment/rate samples, not a qualifying queued→RUNNING distribution. The relevant start remains `spawnRunner`/`container.start` (`index.ts:1232-1259`, invoked by `driveSpawn` at `:1810`) |
| union-28 | E1 | MEDIUM | W3-live-proof | live · **T7-W5** | INV-4, INV-5 | O-CFRATE as routed by the canonical DAG | **AU4.19 — probe:** record the invoice input in exactly `docs/plan/evidence/au4.19-cloudflare-container-rate.json`; `docs/product/pricing.md` cites it, records the real Cloudflare Containers vCPU-h/GiB-h rate for the named billing period and re-derives the margin table | Current `docs/product/pricing.md:8-16,153,191-197,252-256` derives its live ladder from the Northflank $0.10/vCPU-h proxy; it contains no Cloudflare Containers invoice rate |
| union-29 | E2 | MEDIUM | W3-live-proof | live · **T7-W5** | INV-5 | O1 · T3-W7 as routed by the canonical DAG | **AU7.12 — probe:** instrument `clw` hit/miss for **7 consecutive 24 h windows and ≥500 real executions**, report the measured rate and 95% Wilson interval in exactly `docs/plan/evidence/au7.12-memoization-hit-rate.json`, and replace `pricing.md`'s 85–95% margin range with the margin interval derived from that artifact | `docs/product/pricing.md:220-225` still calls the memoization hit rate theoretical and unmeasured; the 85–95% range remains in `:95` and the tier table at `:157-161` |
| union-30 | E3 | MEDIUM | W4-post-decision | post-decision · **T3-W5** *(existing extension)* | INV-3 | D4 is a hard predecessor; its committed artifact supplies the one response schema | **AU3.27 — test:** `FLEET_MAX_CONCURRENCY` is env-tunable without a recompile, and 100 over-cap requests all return the response schema fixed by D4; zero requests disappear without a terminal customer-visible response | `deploy/cloudflare/src/lib.ts:665` still compiles the value `250`; Worker admission consumes it at `index.ts:1643,1650`, and `deploy/cloudflare/wrangler.jsonc:239` requires manual synchronization |
| RH5 (partial) | RH5 uncovered half | LOW | W2-serial-worker | repo · **T8-W1** *(existing extension)* | INV-5 | none | **AU7.6 — test:** no two statements in `deploy/cloudflare/src/index.ts` assert opposite metadata-probe status; the surviving statement cites the closed probe artifact id | **What `gap-16` does not cover:** the uncovered in-file contradiction remains current: `index.ts:45-46` says a live-account smoke is owed, while `:598` says G2 is settled. `gap-16` concerns the separate egress mechanism |
| RH9 (partial) | RH9 uncovered half | MEDIUM | W2-serial-worker | repo · **T3-W9** | INV-3 | A3.3/T3-W2 counter registration is a hard predecessor | **AU6.16 — test:** limiter refusal has two fixed cells: dead-letter commit succeeds → 202; dead-letter write is unavailable/fails → 503. Both cells produce zero claims, mints, JIT configs, leases, starts or boxes. Separately, bad-HMAC `POST /webhook` bumps the registered `webhook_auth_failed` counter and performs the same zero spawn-path side effects | **What `hist-12`/`deploy-13` do not cover:** current limiter code schedules a best-effort dead-letter then returns 429 (`index.ts:3627-3629`), and bad HMAC returns 401 at `:3406-3408` with no `webhook_auth_failed` counter in the Worker |
| M3 (partial) | M3 uncovered half | MEDIUM | W2-serial-worker + W3-live-proof | repo · **T8-W5**; live · **T8-W6** | INV-3, INV-8 | AU3.23a → AU3.23b | **AU3.23a — test:** a throwing revoke on the completed path writes a durable retry, a later tick succeeds and every failure bumps the registered `revoke_failed` counter. **AU3.23b — probe:** in **10/10** real completed jobs, the minted `cas:rw` PAT is refused by CAS within **75 s**; write only `docs/plan/evidence/au3.23b-completion-pat-revocation.json` with completion, retry and refusal timestamps | **What `sec-03` does not cover:** the multi-use redemption behavior remains at `lib.ts:469-488`. The uncovered fail-open revoke remains explicit at `index.ts:1347-1365`: its catch logs and returns false, with no retry/dead-letter/counter |
| M5 (partial) | M5 uncovered half | MEDIUM | W4-post-decision | post-decision · **T3-W5** *(existing extension)* | INV-3 | D4 | **AU3.28 — test:** an orphan record exhausting `MAX_ORPHAN_ATTEMPTS` / `ORPHAN_TTL_S` transitions to a terminal, queryable state surfaced to the customer instead of being deleted | **What `fabric-core-14`/`gap-13` do not cover:** the current Worker give-up branch at `index.ts:4215-4224` deletes the record and logs `orphan_retry_giveup`, with no terminal customer surface. Current bounds are `lib.ts:1218` (1800 s) and `:1221` (3) |
| M19 (partial) | M19 uncovered half | LOW | W2-serial-worker | repo · **T8-W5** | INV-3 | none | **AU3.24 — test:** the Worker's `/v1/leases/{id}/cas-cred` returns one uniform response for a bad ticket, an already-redeemed ticket and an unknown lease | **What `fabricd-06` does not cover:** it names the Rust handler only. The current Worker twin still exposes `401 invalid ticket`, `410 ticket already redeemed` and `404 no such lease` at `index.ts:3709-3711` |

---

## 3. Staged ownership consequences

This table records only proposed AU ownership and exact write scopes. The canonical phase,
predecessor and ready-set schedule is `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; any
conflict is resolved in favor of that document. All packets remain staged and non-dispatchable.

**New WPs and their item counts** (all ≤ the four-item ceiling):

| WP | owns | exact exclusive write scope (the X) |
|---|---|---|
| **T3-W14** *(new)* — slot & admission accounting | AU3.19 AU4.14 AU3.20 AU3.21 | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts` |
| **T3-W9** *(new)* — spawn/webhook path durability | AU4.15 AU3.22 AU5.10 AU6.16 | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts` |
| **T8-W5** *(new)* — credential-revocation lifecycle | AU4.16a AU3.23a AU4.17 AU3.24 | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts` |
| **T8-W6** *(new)* — credential-revocation live proof | AU4.16b AU3.23b | `docs/plan/evidence/au4.16b-suspension-pat-revocation.json`, `docs/plan/evidence/au3.23b-completion-pat-revocation.json` |
| **T8-W7** *(new)* — JIT process-surface live proof | AU3.26b | `docs/plan/evidence/au3.26b-jitconfig-process-surface.json` |
| **T3-W10** *(new)* — fabric-server reaper + error vocabulary | AU3.25 AU7.7 | `crates/corelink-fabric-server/src/reaper.rs`, `crates/corelink-fabric-server/src/handlers/cas_cred.rs` |
| **T8-W4** *(new)* — in-box secret hygiene | AU3.26a AU1.9 | `deploy/runner/entrypoint.sh`, `crates/corelink-check-exec-server/src/**` |
| **T5-W3** *(new)* — GitHub Action shell safety | AU5.11 AU5.12 | `integrations/github-actions/action.yml` |
| **T7-W4** *(new)* — inventory + price-string truth | AU7.8 AU7.9 | `docs/runbook/secret-inventory.md`, `crates/corelink-fabric/src/plans.rs`, `scripts/ci/secret-inventory-drift.sh` |
| **T7-W5** *(new)* — measured-claim probes | AU7.11 AU4.19 AU7.12 | `docs/product/pricing.md`, `docs/plan/evidence/au7.11-queued-running-latency.json`, `docs/plan/evidence/au4.19-cloudflare-container-rate.json`, `docs/plan/evidence/au7.12-memoization-hit-rate.json` |
| **T2-W6** *(new)* — fabricd secret-rotation rollout | AU1.8 | `docs/runbook/secret-rotation.md`, `docs/plan/evidence/au1.8-fabricd-secret-rotation.json` |

**Extensions to existing WPs:** T4-W1 `+AU4.18` (1→2) · T8-W1 `+AU7.6` (3→4) ·
T6-W10 `+AU6.17` (3→4; exact artifact
`docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json`) · T5-W1 `+AU7.10` (1→2) ·
T3-W5 `+AU3.27 +AU3.28`
(0→2 — this is the first content T3-W5 owns, so D4 landing now has a falsifiable WP behind it).

The worker serialization, release ordering (including T5-W3 as a T5-W6 predecessor), D13,
O-CFRATE and R6 routing are all encoded in the canonical DAG. This triage does not duplicate its
edges or ready sets. `index.ts` modularization remains post-GA and has no AU packet.

---

## 4. Findings

### Findings I did NOT place in CLEAN-no-action or DEFER — and why

The brief forbids parking a MEDIUM-or-higher without justification. **I parked none.** The
one candidate was `union-14` (MEDIUM), whose ledger claim is two-thirds stale in the reconciled tree:

- *"early-return without `installationId`"* — **stale.** The refusal `throw`s
  `SpawnRefusedError` (`index.ts:1802`), and the in-code comment at `:1795-1801` documents
  that the bare `return` was the previous bug and was removed.
- *"holds a phantom slot for 45 min"* — **stale.** The refusal branch is entered on
  `!slot.admitted`, i.e. no slot was ever acquired (`index.ts:1779`).
- *"a cold at-ceiling refusal leaves a stranded job with no terminal state"* — **real, and
  kept.** `recordOrphan` returns before writing when `installationId` is absent
  (`index.ts:1854`), which is exactly the cold case. AU3.20 is scoped to this half only.

I therefore report `union-14` as **partially stale, re-scoped, still placed** rather than as
CLEAN. Every other finding reproduced exactly as the ledger describes it.

## 5. Unplaceable

**None.** All 30 findings have a bucket and one or more staged owners/acceptance items (33 proposed
AU ids; union-06, union-09 and M3 intentionally own repo-test/live-proof pairs). The canonical DAG,
not this section, supplies phase and dispatch order.

Two placements have hard predecessors, not implementation choices:

- **`union-30`** is placed in W4 behind **D4** because its second half (a customer-visible
  over-cap signal) *is* the ADR-0005 question. The finding stays whole in T3-W5; D4's committed
  artifact fixes the exact response schema before dispatch.
- **`union-23`** is the only row whose assertion cannot be written in this repo at all: CAS
  tenant scoping lives in `corelink-server` and the session fence (INV-6) forbids reaching it.
  Relay **R6** is a hard predecessor of the in-repo documentation test AU7.10 and T5-W1 seal.
