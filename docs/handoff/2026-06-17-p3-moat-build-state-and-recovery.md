# Handoff — P3 moat build: state, the off-baseline lesson, and the correct continuation

> **Written 2026-06-17** mid-build, as a durable checkpoint (the orchestrator context grew large after
> a long session + an off-baseline fan-out setback). Self-contained: resume the moat build from here.
> **Read §0 RESUME first.**

## 0. RESUME (do this first)

1. **Invoke `/techlead`** (the discipline owns this build).
2. The decomposition is DONE + cold-reviewed: `docs/handoff/2026-06-17-p3-moat-wave-plan.md`
   (acceptance suite A1–A13, WP slices, frozen §4 contract).
3. The build branch is **`build/p3-moat` @ `6a61a40`** — it has the **contract stubs** (`e57c46b`:
   cas_http, runner_cas_mint, inject_clw_env, + the suite agent added ac_pre_lease + clw_drive stubs)
   **+ the RED acceptance suite** (`6a61a40`: 26 tests). It COMPILES (the #14 cold-fail was a stale
   parallel-worktree cache artifact — `Blake3Key` IS exported from cas_http.rs:118 + imported fine).
4. **Build SEQUENTIALLY on `build/p3-moat`, NOT with worktree isolation** (see §2 — the lesson).
5. **First harden the oracle** (§3), THEN implement the WPs (§4). Each green + change-scoped gate +
   cold-review before merge. Final: full gate → PR to `main` for owner merge.

## 1. WHAT IS SOLID
- **P2.1 hardening MERGED** to main (#87, `a913351`): S1 pin-hygiene, S2 admit box-backend guard, S3
  runner-disk floor. Plus drift-fix + relays (all on main, pushed).
- **All cross-TL seams CLOSED:** Cache TL decided CT-Q1=Option B + native CAS/AC HTTP (BLAKE3) + D-9
  mint (shipped, pending prod-Worker deploy) + AC pre-lease (see `2026-06-17-reply-from-cache-tl-...`).
  clw confirmed (CLW_* names, drive, exit-transparent). hugit ratified ADR-0004 D3 + v2 Path 1.
- **v2 attestation PRODUCER is conformance-green** (#14, 2026-06-17): `result_binding_sig_v2` byte-exact
  vs `conformance/result_binding_v2.json`, emitted on Exec/CloseResponse. hugit's verifier is built;
  the 3 enforcement unblocks are answered in `2026-06-17-reply-to-hugit-tl-attestation-transport-key.md`
  (transport = the result DTO; key = `GET /v1/attestation/key` + a `fabric_key_id` to wire; §13 live =
  infra-gated).
- **Moat foundation on `build/p3-moat`:** stubs + RED suite, compiles, 26 RED tests.

## 2. THE OFF-BASELINE LESSON (critical — do not repeat)
**`Agent(isolation:'worktree')` forks from `main`, NOT the current branch.** All 4 parallel moat-impl
WPs forked main (no stubs, no suite) → they re-created the foundation from scratch → **off-baseline
garbage**. Caught by the merge-base check (`git merge-base --is-ancestor 6a61a40 <wp-branch>` = 1) BEFORE
merging — stopped + cleaned. (Off-baseline impl SHAs, §4-faithful, usable as REFERENCE only: WP-2
cas_http `426fd11`; WP-6 clw_drive `d97dab9`; WP-7 ac_pre_lease `461ca4d` — recover a file via
`git show <sha>:<path>`.)
**Reliable build mechanism:** SEQUENTIAL agents on `build/p3-moat` WITHOUT isolation (they edit the
main tree on-baseline; one at a time — no concurrent tree edits). Parallel-on-a-feature-branch needs a
verified base mechanism (e.g. instruct each worktree agent to `git checkout 6a61a40 -- <foundation>`
first, or try the Workflow tool's worktree manager — UNVERIFIED; default to sequential for correctness).

## 3. ORACLE-HARDENING (do FIRST — the cold-review + security review found real holes)
The RED suite is ~half-solid. STRONG: a1, a2, a5×4, a9, a9b, a6, a6b, a7b-ttl. **Fix before building:**
- **a12 — RECONCILE (lead decision made):** the invariant is **fail-closed ABORT** on mid-hydrate
  substrate-down (`is_err()`/`SubstrateDown`, NOT a `Hydrated` outcome). Partial VALID layer writes are
  acceptable (content-addressed; a failed PUT doesn't commit). Drop the `write_count==0`-vs-`3` confusion;
  assert the abort + no-warm-proceed. Update the wave-plan A12 wording.
- **a10 — WEAK/vacuous:** byte-identity must be asserted at the `CasHttpClient.put_cas/put_ac` BODY
  level (two runs → identical PUT bytes), not the `cold_hydrate` pass-through.
- **a11 — WEAK:** add a `headers` field to `CasRequest` + assert `x-corelink-tenant-id` is NEVER emitted
  (currently unprovable — no headers field).
- **a13 — VACUOUS:** replace `if let Some(..)` with `.expect(..)` so the routing assertion is load-bearing.
- **a5b — WEAK:** add a `ForcedCold` discriminant (AC-unreachable ⇒ run cold RECORDED as forced-cold,
  not a hit — honest accounting); CAS-unreachable mid-hydrate ⇒ hard fail-closed.
- **Integration WEAKs (a3b/a4/a7×/a8×):** these are panic-gate placeholders — the impl WPs must FINALIZE
  them with REAL assertions (a3b: wire MockAcHook into AppState + assert the ledger shows 0 slots; a4:
  assert store-after-miss PUT; a7: assert acquire actually mints + fail-closed-no-box; a8: real
  BoxExec-backed ClwDrive, not mock-vs-mock).
- **Security review (af37cfa, 2026-06-17) design findings:** [P1] intra-tenant poisoning — the per-job
  PAT is tenant-wide `read-write` with no per-job key restriction (accepted-by-design per the contract,
  but ADD a test asserting the accepted posture OR a guard). [P2] **AC PUT must happen ONLY after a
  successful run, NEVER during hydrate** (else a substrate-down mid-hydrate writes a spurious AC = false
  hit) — assert the AC is never written during hydrate. [P2] mint-auth startup check: fail-closed if
  `CORELINK_PAT_MINT_AUTH_KEY` is empty/dev-default. [P2] a13-adversarial: a runner-supplied `content_key`
  with `_public:` prefix must be REJECTED (only the fabric supplies `_public` keys).

## 4. SEQUENTIAL BUILD PLAN (after the oracle is hardened)
On `build/p3-moat`, no-worktree, one at a time; reference the off-baseline §4-faithful impls (§2):
1. **WP-2 cas_http** (ref `426fd11`): CasHttpClient + HttpBootCas + status-class guard + BLAKE3 (add
   blake3 dep) + a11 headers + a10 put-body byte-identity + a5b ForcedCold. → the 14 runner tests green.
2. **WP-34 mint+inject+revoke** (touch leases.rs finalize/teardown only): D-9 mint client + inject CLW_*
   + revoke-on-every-terminal-path + TTL≤lease + fail-closed-rollback. → a6/a7/a7b green.
3. **WP-6 clw_drive** (ref `d97dab9`): drive snapshot→hydrate→run, exit-transparent, non-zero-not-cached,
   exit-2-internal; AC PUT only after success (security P2). → a8 green.
4. **WP-7 ac_pre_lease** (ref `461ca4d`, touch leases.rs acquire only): AC lookup BEFORE the slot reserve
   (`try_admit_with_compute` ~leases.rs:420); hit ⇒ skip box, 0 slots (a3b ledger oracle); miss ⇒ run +
   store. → a3b/a4 green.
5. **WP-8 flip-live** (LAST, infra-gated): wire the real UreqTransport (via a blanket impl at the
   fabric-server composition root) + the real CasPatMint + the AC hook into AppState + a feature flag;
   `GET /v1/attestation/key` + `fabric_key_id` (hugit ASK 2). Prove the guard prod-reachable. Gated on:
   Northflank allowance + D-9 prod-Worker deploy + clw binary digest (Workspaces TL).
Integrate each green WP → full workspace gate (fmt/clippy/test/deny; audit unaffected, zero dep changes
beyond blake3) → PR to main for owner merge.

## 5. PARALLEL / OTHER TASKS
- **#9 (S3 boot-validate)** — in flight on its own MAIN-based worktree (`worktree-agent-a7b3604c…`);
  it's correctly main-based (a P2.1 follow-up). When it lands: review + merge to main as its own small PR.
- **#10 queue-degrade, #11 capacity-reconciler, #12 githugr-benchmark, #13 §13-envelope-Phase-2,
  #14 v2-coordinate** — see the task list; #11/#12 gated on the PAYG allowance; #13 ratified (build it);
  #14 producer green (coordinate hugit's P2 live-wire).

## 6. OWNER PLATE (unblocks the live path; none block the BUILD)
- Buy the $50 Northflank credit → raise the resource allowance (PAYG; cost-neutral — see the capacity
  memory). Unblocks the moat flip-live (WP-8) + #82 cloud-fleet CI + §13 live + githugr benchmark.
- Forward the pending relays/replies (server-TL `max_vcpu_h`; githugr fleet-status; hugit ADR-0004 +
  hugit attestation-transport).
