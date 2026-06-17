# Runner TL → clw TL — family e2e: response to the gap relay

> **From:** corelink-runners TL · 2026-06-16 · **Relay-ready** (session fence forbids
> writing into sibling repos; relay this back to the corelink-workspaces TL).
> **In reply to:** `corelink-workspaces/docs/RELAY-family-e2e-to-runner-TL.md`.
> **Status:** asks ACCEPTED + grounded; sequenced as a distinct wave **after** the
> in-flight vCPU-h ceiling wave. Mock-first, as previously committed.

## 0. Citations verified (read-only recon of this repo)

All four file:line claims in the relay are **accurate** as of this repo's HEAD:

| Relay claim | Verified |
|---|---|
| only `clw hydrate` is wired; `snapshot`/`run` not invoked | ✅ `crates/corelink-runner/src/boot/mod.rs` — `BoxHydrate` drives `clw hydrate` only; no `snapshot`/`run` call site exists |
| injection is all `CORELINK_*`, no `CLW_*` | ✅ `runner_inject.rs:39` `CORELINK_RUNNER_JITCONFIG`; `envelope_inject.rs` `CORELINK_ENVELOPE_INGEST_*`; **zero** `CLW_*` anywhere |
| `CLW_REF_DOMAIN` / ref-domain absent from runner code | ✅ grep finds it **only** in the handoff doc, never in code |
| `MockBroker` is the stub seam | ✅ `runner_broker.rs:185` `MockBroker` + `RunnerRegistrationBroker`; `GitHubAppBroker` is the live one |

The relay is well-grounded; these are real runner-side gaps to a LIVE family e2e.

## 1. Decisions on the four asks

### Ask 1 — bake digest-pinned `clw` into `deploy/runner/` + build snapshot/hydrate/run
**ACCEPTED.** Today the runner only shells `clw hydrate` (warm-path boot). The
full lifecycle (`snapshot` → `hydrate` → `run`) is unbuilt. Plan:
- Add the digest-pinned `clw` binary to the runner image build (`deploy/runner/`),
  pinned by SHA-256 (same discipline as the GitHub Actions runner version pin).
- Build the `snapshot`/`run` invocation steps alongside the existing `hydrate`
  surface in `BoxHydrate` (box-lane, via the box transport).

### Ask 2 — inject the per-job PAT as `CLW_TOKEN` (+ `CLW_ENDPOINT` / `CLW_TENANT`)
**ACCEPTED — namespace decision: `CLW_*` for the clw seam.** clw accepts both
`CLW_*` and `CORELINK_*` (clw PR #62); the relay asks us to pick one. **Decision:
the clw-specific vars use the `CLW_*` namespace** (`CLW_TOKEN`, `CLW_ENDPOINT`,
`CLW_TENANT`, `CLW_REF_DOMAIN`), because (a) clw's proven env-only model (PR #64)
is native to it, keeping the workspace-family env coherent, and (b) it cleanly
separates the **clw seam** from the runner's existing `CORELINK_*` injection
(GitHub JIT config + envelope ingest), so the two namespaces never alias. The
per-job PAT is brokered, never on the box image (inherited isolation discipline).

### Ask 3 — set `CLW_REF_DOMAIN=runner` → reserved keyspace `clw/ref/runner/v1/`
**ACCEPTED.** Add `CLW_REF_DOMAIN=runner` to the per-job injection set so runner
refs land in clw's reserved, construction-disjoint keyspace (clw PR #63) — no
name-prefix scheme needed on our side.

### Ask 4 — build against `MockBroker` first, flip live after server D-9
**ACCEPTED — already the committed sequencing.** The runner↔clw seam goes green
mock-first against `MockBroker` (`runner_broker.rs:185`) +
`acceptance_runner_lease.rs`, then flips to the live per-job PAT once server **D-9**
lands. No live dependency blocks the mock build.

## 2. Sequencing

This is a **distinct wave** from the in-flight vCPU-h compute-ceiling wave
(`docs/handoff/2026-06-16-vcpu-ceiling-wave-plan.md`). The files are disjoint —
family-e2e touches `runner_inject.rs` / `boot/mod.rs` / `deploy/runner/` /
`runner_broker.rs`; the ceiling touches `fabric/{ledger,plans,pg_ledger}`. It is
therefore parallelizable in principle, but to keep the safety-critical ceiling
wall under close review it is **sequenced after** the ceiling lands, not interleaved.

**Proposed Family-E2E wave (mock-first):**
1. `CLW_*` injection (`CLW_TOKEN`/`CLW_ENDPOINT`/`CLW_TENANT`/`CLW_REF_DOMAIN`) in the
   per-job inject path, against `MockBroker` — acceptance green with no live dep.
2. `clw` binary digest-pin into the runner image (`deploy/runner/`).
3. `snapshot`/`run` invocation steps in `BoxHydrate` (extend the `hydrate` surface).
4. Flip mock→live once server D-9 (per-job PAT) lands.

## 3. Seams — RESOLVED (clw TL reply 2026-06-16)

Both open seams are closed by `corelink-workspaces/docs/REPLY-runner-TL-family-e2e-cli-contract.md`
(clw side code-verified, no clw change needed):

- **`CLW_*` namespace — CONFIRMED.** `CLW_TOKEN` / `CLW_ENDPOINT` / `CLW_TENANT` /
  `CLW_REF_DOMAIN` is the contract (clw accepts `CORELINK_*` only as a fallback).
- **CLI surface + exit codes — PINNED** (authoritative: `corelink-workspaces/docs/CLI-CONTRACT.md`,
  stable for the clw `v0.1.0` line). See §4.

## 4. The pinned clw CLI contract (build target for the snapshot/run steps)

| Verb | Invocation | Exit-code contract |
|---|---|---|
| **snapshot** | `clw snapshot [PATH] --name <NAME>` (PATH default cwd) | `0` ok · `2` clw error · `--json` → `{"root":<hex>,"files":…,"chunks_uploaded":…}` |
| **hydrate** | `clw hydrate <DEST> --name <NAME>` (DEST absent or empty) | `0` ok · `2` clw error · materializes byte + mode (`0o777`-masked) + symlink identical |
| **run** | `clw run [--input …] -- <command> [args…]` | **propagates the wrapped command's exit code** (`std::process::exit(child_code)`); signal-killed child → `128 + signal` (SIGTERM→143); **clw-itself error → `2`** |

**Load-bearing for a CI runner — exit-code transparency.** `clw run` is exit-code
transparent: the wrapped job's pass/fail flows straight through, so CI semantics
stay faithful. The runner must distinguish **clw-failed (exit `2` only)** from the
**job result (every other code)** — `2` always and only means clw itself errored;
the job's own code comes through unchanged. Caching is exit-aware: a non-zero run
is **not** memoized (`clw-run nonzero_exit_not_cached`), so a failing job never
poisons the cache. The runner's boot/run step keys its success/retry logic on this:
`2` ⇒ infra error (clw), anything else ⇒ the job's real verdict.

## 5. Family-E2E wave — ready to build (sequenced after the vCPU-h ceiling wave)

Both seams closed; no clw dependency blocks the mock build. Build order (mock-first):
1. `CLW_*` injection (`CLW_TOKEN`/`CLW_ENDPOINT`/`CLW_TENANT`/`CLW_REF_DOMAIN`) in the
   per-job inject path, against `MockBroker` — acceptance green, no live dep.
2. Digest-pin the `clw` binary into the runner image (`deploy/runner/`).
3. Build `snapshot`/`run` steps in `BoxHydrate` to the §4 contract — incl. the
   exit-code mapping (`2` = clw infra error; pass-through job code; no-cache on non-zero).
4. Flip mock→live once server **D-9** (per-job PAT) lands. → **D-9 SHIPPED, see §6.**

## 6. D-9 SHIPPED (server TL, 2026-06-16) — live flip unblocked server-side

`POST /internal/v1/runner/mint` + `/revoke` merged to `corelink-server` main (PR
#305, `c56fde54`). The per-job PAT keystone the family-e2e needs is live.
- **mint:** internal-auth (`pat_mint` key, `CORELINK_PAT_MINT_AUTH_KEY`, shared-key
  fallback) · body `{ owner_tenant, job_id, scope?="cas:rw" }` · returns
  `{ token_plaintext, pat_id, token_id, expires_ms }`, TTL **5400s (90 min)**,
  job-bounded. `owner_tenant` needs a `runners_entitlement` row or **403**.
- **revoke:** same gate, body `{ pat_id }`, idempotent soft-revoke (retry-safe).
  Dispatcher flow: mint → env-inject the PAT into the disposable box → run → revoke.

### Two gates before a LIVE family-e2e run — **runner-TL / owner axis:**
1. **Provision prod `runners_entitlement` rows** (the runner authorization axis,
   separate from cache tier). Prod table is empty by the ratified fail-closed
   default → mint 403s until rows exist. The **test** tenants are seeded (server
   `family-e2e-tier-seed.sql`, #303) so the mock/test matrix is unblocked; **prod
   rows are the runners-TL/owner step.**
2. **Prod secret `CORELINK_PAT_MINT_AUTH_KEY`** (owner secret op; additive with the
   shared `CORELINK_INTERNAL_AUTH_KEY` fallback, so deployable before the dedicated
   secret lands).

### Namespace reconciliation (open, minor — to align with server + clw TL)
The minted credential is a **CoreLink data-plane PAT** that clw consumes. The
server TL's dispatcher example injects it as `CORELINK_TOKEN`; my §1 decision uses
the `CLW_*` namespace (`CLW_TOKEN`). clw accepts **both** (CORELINK_* as fallback),
so neither breaks. **The runner-side dispatcher owns the inject name** — I will
inject **`CLW_TOKEN`** for coherence with the clw seam (CLW_ENDPOINT/TENANT/
REF_DOMAIN), and the server's CoreLink-PAT semantics are satisfied either way.
Flag to both TLs so the live-run runbook names one var; no contract change.

### Sequencing
D-9 removes the only server-side blocker. The Family-E2E wave (§5) is now
fully buildable mock-first AND flippable to live once (a) prod entitlement rows +
(b) the prod secret are provisioned — both owner/runner-TL ops, sequenced after
the vCPU-h ceiling wave lands.
