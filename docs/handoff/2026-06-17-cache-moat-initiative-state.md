# Handoff — CoreLink Cache-Moat Initiative (STATE for resume after /clear)

> **Written 2026-06-17** as a durable save before a `/clear` (the prior session grew too
> large / over-compacted). This doc is **self-contained**: a fresh session can resume the
> whole initiative from here. Read it top-to-bottom, then follow §RESUME.

---

## 0. RESUME — do this first (post-/clear)

1. **Invoke `/techlead`** (the Skill) — this is a multi-phase, multi-agent, cross-TL
   initiative; the tech-lead discipline owns it. (In the prior turn I applied the doctrine
   in prose but did not call the Skill tool — the owner flagged it. Pull it for real.)
2. Read **this doc** (§1 north star, §2 plan, §3 the drift assessment) — the Phase-1
   deliverable is already DONE and captured verbatim in §3.
3. Resume at **Phase 2** (§2). The recommended first move is **§3.8 P2.0** — get the
   CoreLink TL to answer the two `⟨FILL⟩` decisions; in parallel take the non-cache
   cold-start hardening (S1/S2/S3) that needs no cross-TL answer.
4. Operational context (Northflank, the `!` mechanism, waivers, the fence) is in §4.
5. Prior shipped work + deferred items are in §5.

**Do NOT** re-run the Phase-1 drift-assessment workflow — it completed (run
`wf_dba3d1da-12f`, 7 agents, 501k subagent-tokens); its full output is in §3.

---

## 0.5 SESSION UPDATE — 2026-06-17 (Phase 2 in progress; `/techlead` pulled)

- **P2.0 (long pole) drafted — OWNER TO FORWARD:**
  `docs/handoff/2026-06-17-relay-to-corelink-cache-tl-warmboot-seam.md` (CT-Q1 + CT-Q2 — gates all
  of P3) and `…-relay-to-clw-tl-confirms.md` (4 clw confirms; clw side already pinned).
- **P2.1 landed on branch `harden/p2.1-cold-start-sclass`** (off `main@9635e31`), **gate GREEN**
  (fmt · clippy -D warnings · tests for corelink-cloud-engine + corelink-fabric-server, 0
  failures). **PR/merge PENDING OWNER GO** (committed, not pushed): `aa62e30` (docs) + `c683b2d`
  (code).
  - **S1 RECALIBRATED:** the §3.4/§3.5 "CRITICAL `<PIN-AT-BUILD>` placeholder → first box can't
    spawn" is **STALE** — the ubuntu:24.04 base digest was already pinned by **PR #75**
    (`sha256:786a8b55…`). Residue was stale comments + a `build-and-push.sh` guard that
    false-positived on its own comments. **Fixed; not a spawn blocker.**
  - **S2 (`cloud_exec.rs` + `leases.rs`):** added `BoxProvisioner::binds_boxes()` (default true;
    `NoBoxProvisioner`→false) + a guard at `leases.rs` (symmetric with the broker guard) that
    rejects a runner acquire `400` at admit when no box backend is wired — covers BOTH the `/v1`
    acquire and the webhook autoscaler path (same `leases::acquire`).
  - **S3 (`northflank.rs`):** `RUNNER_EPHEMERAL_STORAGE_FLOOR_MB = 4096`; `spawn` fails closed for
    a runner box below it (instead of silently inheriting the 1 GiB check default → ENOSPC).
    **Does NOT unblock #82** (the Northflank account allowance is an owner billing decision).
- **ENV FLAG (builder Mac):** the rustup **proxy** is broken — `~/.cargo/bin/*` are dangling
  symlinks to a missing `rustup`, so `cargo`/`rustc` fail on PATH. Real toolchains are intact
  under `~/.rustup/toolchains/`; the gate ran via
  `~/.rustup/toolchains/1.96.0-x86_64-apple-darwin/bin` directly. Worth repairing.
- **NEXT:** owner forwards the two relays. On **CT-Q1**, P3 (live `BootCas`) unblocks. **P2.2**
  (Family-E2E spec, test-first) can begin mock-first now (clw side pinned).

---

## 1. THE NORTH STAR (owner directive, 2026-06-17)

Owner's words (PT): *"vamos com calma. Um passo atrás, primeiro verificar o quão longo foi
esse drift, e o tamanho do estrago. Depois fazer o estudo e pesquisa pra wirar tudo 100%,
descobrir o que já tá pronto, perguntar pro TL do corelink, pro TL do corelink workspaces,
tudo que precisar alinhar, wirar tudo sota, tudo certinho, tudo pensado. … O user não pode
ter engasgos do tipo não conseguir rodar um job … se for o primeiro run, não tiver warm,
etc, tem que funcionar também … isso é seu novo norte, puxa sua skill de techlead, configure
a goal, e marcha. Você tem um time de 50 agents à sua disposição, e os outros Techleads estão
aguardando suas solicitações pra trabalharem junto."*

**Decoded:**
- **GOAL:** wire the CoreLink storage/cache moat into the runner — make the runner the
  *differentiated* product (cache-warm by construction + CAS-backed storage offload),
  not "rented metal."
- **HARD CONSTRAINT (the norte):** the end user must **NEVER** have a hiccup like "can't run
  a job." Even a **COLD first run (no warm cache, empty CAS) MUST work.** Cache/CAS is an
  optimization *on top of* a correct cold path; it never gates "can I run at all." Cache
  absent ⇒ **slow, never broken.**
- **METHOD:** `/techlead` discipline · a 50-agent team · the other TechLeads (CoreLink TL,
  CoreLink Workspaces/`clw` TL) are awaiting my requests to work in parallel.
- **SEQUENCE:** (1) assess the drift/damage ✅ DONE (§3) → (2) research + cross-TL alignment
  + discover what's ready → (3) wire it all, SOTA, cold-path-first.

---

## 2. THE PHASED PLAN

| Phase | What | Status |
|---|---|---|
| **P1 — Drift assessment** | Map vision × wired reality; size the gap; what's built/missing; cross-TL deps; cold-start risks | ✅ **DONE** — see §3 |
| **P2 — Research + cross-TL alignment** | Close the two `⟨FILL⟩` decisions w/ CoreLink TL; harden non-cache cold-start (S1/S2/S3); spec the Family-E2E wave | ⬜ **NEXT** |
| **P3 — Wire SOTA (cold-path-first)** | Live `BootCas`, `clw` in image, `CLW_*` inject, snapshot/run, D-9 mint client, fail-closed guard in live path | ⬜ blocked on P2.0 |

---

## 3. PHASE-1 DELIVERABLE — STORAGE/CACHE DRIFT ASSESSMENT

*Tech-lead synthesis · 6 recon dimensions · evidence-cited, tense-disciplined. Produced by
workflow `wf_dba3d1da-12f` (6 parallel recon agents + synthesis). Raw findings JSON saved at
`/tmp/drift_findings.json`; full synthesis at the task output file.*

### 3.1 THE DRIFT IN ONE PARAGRAPH

The drift is **total on the cache axis, and it is the load-bearing axis**. The product's one
differentiating thesis — *"cache-warm by construction; a runner that isn't warm off the CAS is
just rented metal"* (whitepaper §10, `corelink-runners-v1.md:256`) — describes **exactly what
the live runner is today: rented metal**. The wired path provisions a cold Northflank microVM
(or a stock ephemeral GitHub Actions runner on the ADR-0007 direct on-ramp), hands it a
**blank, deliberately oversized ephemeral scratch disk** (`northflank.rs:500-511`), pulls
source via GitHub's own `actions/checkout`, and does a **full cold `cargo build` into a local
`target/`** every job. There is **no CoreLink CAS or Action Cache anywhere on the runtime
path** — not a fetch, not a lookup, not a memoization short-circuit, not a `clw` invocation.
The entire cache-warm architecture (`boot::hydrate`/`cold_hydrate`/`BootCas`/`BoxHydrate`/
`materialize_sparse`) is **real, tested, fail-closed library code with ZERO production
callers** — proven only against in-memory fakes in `acceptance_c3`/`acceptance_c9`. The
"estrago" is therefore *not* broken code to repair — it is **an entire unbuilt data plane
behind a correctly-shaped seam**. The good news, equally honest: the control plane (introspect
auth → tenant + concurrency cap), the isolation/exec backend (Northflank Engine), the X4
digest-pin floor, the per-job env-injection seam, and the lease lifecycle are all **wired and
tested**. We have a correct skeleton with a correct *hole where the moat goes*. The thesis is
**aspirational-in-code today; do not report "runners boot warm off the CoreLink cache" as
real.**

### 3.2 WHAT IS ACTUALLY WIRED TODAY

The live first-run path, end to end:
- **Acquire → admit:** `leases.rs:164` acquire → CapGate + atomic `try_admit` reserve →
  `finalize_admitted_lease` (`leases.rs:476`).
- **JIT mint + inject:** runner mints a **GitHub Actions JIT config** (synchronous 3-leg
  GitHub exchange, `leases.rs:512-529`) and injects it as the **single box env var
  `CORELINK_RUNNER_JITCONFIG`** (`runner_inject.rs:39-52`). No `CLW_*`, no CAS URL, no
  `RUSTC_WRAPPER`/`sccache`.
- **Provision:** `provision_lease` (`app.rs:859`) → `NorthflankBoxProvisioner::provision`
  (`cloud_exec.rs:346-351`) → `NorthflankEngine::spawn` (`northflank.rs:681`). Spawn enforces
  the **X4 digest-pin floor before provider contact** (`northflank.rs:690-697`) + triggers
  `run_on_create`. **No hydration step anywhere in this chain.**
- **Storage:** create-job body provisions **only** `…ephemeralStorage.storageSize`
  (`northflank.rs:500-511`) — a blank per-job disk. A RUNNER box (`spec.allow_egress==true`)
  gets `runner_ephemeral_storage_mb` if the operator set
  `NORTHFLANK_RUNNER_EPHEMERAL_STORAGE_MB`, **else falls back to the CHECK default
  `ephemeral_storage_mb = 1024` MiB** (`northflank.rs:500-506`). Docstring says the 1 GiB
  default *"cannot hold a Rust workspace build"* (`:86-90`); a test sets 32 GiB (`:919-936`).
  **The literal inverse of the small/lazy/deduped CAS-view disk the vision calls for.**
- **Execution:** entrypoint execs `run.sh --jitconfig` (`entrypoint.sh:36`), self-registers a
  one-shot ephemeral GitHub runner, runs the customer's **unmodified GitHub Actions
  workflow** → full local `cargo build`.
- **The only "warmth":** (a) the **toolchain baked into the runner image** (rustup 1.96.0 +
  cargo-deny + cargo-audit + best-effort `cargo search` index pre-warm, `Dockerfile:113-130`);
  (b) **`Swatinem/rust-cache`** on the repo's *own* self-hosted CI (`ci.yml:28`,
  `release.yml:58`) — GitHub-Actions-cache style. **Neither is the CoreLink CAS** — (b) is the
  exact "cold, self-hosted" mechanism the whitepaper positions *against* (`whitepaper:223`).
- **Fabric exec content-addressing:** `exec.rs:13-20` computes a sha256 for stdout/stderr/
  memo-key but **explicitly does NOT write a CAS or query an AC** — *"the CAS write … lands
  with FC3"*; toolchain memo axis is a *"stand-in until a resolver maps refs to digests."*

**Net:** cold, fat-local-disk, GitHub-checkout, full-cold-compile box. `clw` is not in the
image; `BoxHydrate` is the **sole** `clw` call site and builds only `["clw","hydrate",…]`
(`boot/mod.rs:323`) — never `snapshot`, never `run`, never invoked by a live path.

### 3.3 WHAT THE VISION REQUIRES (each unmet in code)

1. **Cache-warm boot off CAS/AC — "before the first instruction."** Working set (toolchain,
   deps, **source-by-content**) local from the CAS before the first instruction
   (`whitepaper:65-71`; `interop.md:26`). Warm ≤10s; cold force-fetches every layer (≥60s) +
   **writes them back so the NEXT job is warm** (`boot/mod.rs:218-276`). Layers are
   content-addressed, **one physical copy per hash** (`boot/mod.rs:5-14`). **CAS/AC
   unreachable ⇒ explicit fail-closed error — never a silent cold result dressed as warm**
   (`interop.md:26`, `boot/mod.rs:40-46`).
2. **Memoized execution — AC hit ⇒ no runner spent.** Resolve action key → ask the AC first;
   on hit return the stored result, **job never runs** (`whitepaper:154-158`). Structural
   basis of "never charge for the customer's own compute twice."
3. **Sparse, content-addressed, lazily-hydrated disk (workspace-as-object).** Box disk = a
   **thin CAS view** hydrated by path-set (`materialize_sparse` writes only in-fence entries,
   `materialize/mod.rs:3-9,135-190`), driven by **`clw` as THE snapshot/hydrate/run client**
   (`boot/mod.rs:34-38`), digest-pinned into `deploy/runner/`. Workspace SKUs ride the same
   fabric via `clw snapshot/hydrate` (`interop.md:50-54`). **Small by construction.**

Seam consuming all three: **`BootCas` over R2 with Bearer-PAT auth, intra-tenant dedup at GA**
(`corelink-fabric-stub.md §A3,§C2,§H`). Runners *"a layer on the cache, not a parallel
system"* (`whitepaper:146`) — consumes, never reforks.

### 3.4 THE GAP, ITEMIZED

| Capability | Vision | Today | Built? | Missing |
|---|---|---|---|---|
| Warm boot off CAS | Working set local before first instr | Cold box; warmth = baked image layers | **Seam only** (test-fake) | Live `BootCas` impl; any live `hydrate` caller |
| Cold hydrate (≥60s, write-back) | Force-fetch every layer + write back | Full cold `cargo build`, no write-back | **Dead code** | Production call-site in provision/boot |
| Memoized exec (AC hit ⇒ 0 runner) | AC lookup before lease | No AC lookup on exec path | **No** | AC pre-flight + short-circuit; CAS write (FC3) |
| Box disk = thin CAS view | Sparse/deduped/lazy | Fat blank ephemeral disk for full `target/` | **Seam only** (test) | Wire `materialize_sparse`/`select_in_fence` into provision |
| Source-by-content | From CAS | GitHub `actions/checkout` | **No** | CAS-backed source materialization |
| `clw` on the box | Digest-pinned in image | Not in image (GH runner 2.335.1 only) | **No** | `clw` binary baked + pinned |
| `clw` verbs | snapshot→hydrate→run | Only `hydrate` argv built, never called live | **Partial seam** | `snapshot`, `run` (exit-code-transparent) |
| Per-job CAS PAT | Mint scoped PAT, inject `CLW_TOKEN` (D-9) | Zero `CLW_*` env | **No (runner-side)** | Runner mint client for `/internal/v1/runner/mint`; `CLW_*` inject |
| Fail-closed on substrate down | CAS unreachable ⇒ explicit error | No substrate consulted ⇒ guard unreachable in prod | **Seam only** | Guard on a live substrate the box contacts |
| `target/` offload to CoreLink storage | Shared CAS objects, re-run ~0 | Local provider ephemeral disk only | **No** | Any offload mechanism |
| Auth → tenant + cap | Introspect | **Wired + tested**, conformance-pinned | ✅ **YES** | — (M1: seed entitlement rows) |
| Isolation/exec backend | Managed microVM behind Engine seam | **Wired** | ✅ **YES** | — |
| X4 digest-pin floor | Image sha256-pinned pre-spawn | **Wired** | ✅ **YES** | Real base-image digest (placeholder — S1) |

### 3.5 COLD-START RISK REGISTER (north star: cache-absent = SLOW, never BROKEN)

Today the invariant **holds vacuously** — there is no cache to be absent. The real fragilities
are conventional plumbing:

- **S1 — CRITICAL · Base-image digest is a literal PLACEHOLDER.** `deploy/runner/Dockerfile:
  6-13,34-35,57-58` ship a `<PIN-AT-BUILD>` sha256. A prod build against the placeholder
  cannot resolve the image, and the X4 pin floor rejects it → **the very first real box fails
  to spawn.** (Build-time blocker.)
- **S2 — CRITICAL · Provisioner defaults to `NoBoxProvisioner` (default-off).**
  `cloud_exec.rs:298-313`: with no `NORTHFLANK_*` env, the fabric admits the lease, returns
  `Held`, binds **no box** → cold first run **silently has nowhere to execute**
  (`cloud_exec.rs:181`). Failure is *late* (at exec), not at admit.
- **S3 — HIGH · Default runner disk (1 GiB) → ENOSPC.** Operator who didn't set
  `NORTHFLANK_RUNNER_EPHEMERAL_STORAGE_MB` → runner box **silently inherits 1 GiB**
  (`northflank.rs:500-506`), which the docstring says cannot hold a Rust build → cold
  `cargo build` **dies disk-full.** **The ONE place a cold first run can genuinely BREAK
  today.** Mitigation is operational + NOT enforced in code. (This is the root of the
  Northflank disk-allowance saga — see §5.)
- **S4 — HIGH · Synchronous GitHub JIT mint in the hot path.** `leases.rs:512-529` — a GitHub
  App/network hiccup on a cold first acquire fails the lease closed (503).
- **S5 — MEDIUM · M1 entitlement boundary.** `corelink_plans.rs:33` — introspect-resolved
  tenant with no `max_concurrency` row → 0-slot reject → cold prod run needs
  `runners_entitlement` rows seeded (+ `CORELINK_PAT_MINT_AUTH_KEY` for the CAS-PAT path).
- **S6 — MEDIUM · Economic cold-start.** No CAS/AC ⇒ no memoization-elision, no cross-job
  layer reuse → every first run re-buys compute. Not a *break*, but **the pricing premise
  ("re-runs cost ~0") is unearned by the runner.**
- **S7 — LOW · Image pre-warm best-effort.** `Dockerfile:158` `cargo search` non-fatal.

> **Honest framing:** the invariant is satisfied *by absence of the feature*, not *by design*.
> The instant we wire a live `BootCas`, the fail-closed-on-substrate-down guard
> (`boot/mod.rs:40-46`) becomes **prod-reachable** and we must verify cold-degrades
> *slow-from-CAS*, not *broken*. Today the contract clause *"cache unreachable ⇒ explicit
> error, never a silent cold result dressed as warm"* (`interop.md:26`) is **unenforceable in
> the live path — every live run is silently cold, the exact failure the principle forbids.**

### 3.6 WHAT IS ALREADY BUILT (reusable foundation — build ON this)

- **Control plane (wired+tested):** `CoreLinkTokenStore` (PAT→tenant, fail-closed,
  `corelink_auth.rs:198-251`); `CoreLinkPlanStore` (introspect cap, `corelink_plans.rs:
  96-165`); both behind `FABRIC_AUTH_BACKEND=corelink` (default-off), conformance-pinned
  (`server.rs:50-51,73,433`; `corelink-introspect.json` sha `bfb38e28`).
- **Lease lifecycle (wired+tested):** `acquire`+CapGate+atomic `try_admit` *before any box*
  (`leases.rs:164-462`); `finalize_admitted_lease` mint+inject+provision + rollback-on-failure
  (`leases.rs:476-673`).
- **Exec backend (wired+tested):** Northflank Engine (`northflank.rs`, `cloud_exec.rs`);
  `NorthflankBoxProvisioner::provision/teardown/probe` — **the spawn lifecycle the cache-warm
  step hooks into** (`cloud_exec.rs:346-380`); **X4 floor** (`northflank.rs:690-697`);
  top-level `runtimeEnvironment` injection + explicit `run_on_create` (`:539-547,681-724`).
- **The proven injection seam to mirror:** `inject_runner_jitconfig` injects
  `CORELINK_RUNNER_JITCONFIG` per-job (`runner_inject.rs:39-52`) — **exactly the seam
  `CLW_TOKEN`/`CLW_ENDPOINT`/`CLW_TENANT`/`CLW_REF_DOMAIN` injection will copy.**
- **Cache architecture as correct fail-closed tested library code (needs a live backend, not a
  redesign):** `BootCas` trait (`boot/mod.rs:155-179`); `hydrate`/`cold_hydrate` (`:218-276`);
  `BoxHydrate`→`clw hydrate` (`:290-349`); `materialize_sparse`/`select_in_fence`
  (`materialize/mod.rs:135-190`); C3/C9 acceptance prove the seam against fakes.
- **Direct on-ramp (ADR-0007, wired):** GH Actions JIT broker + inject
  (`runner_broker.rs:503-521`) — works today, just CAS-cold.

### 3.7 CROSS-TL SEAMS + QUESTIONS

**To the CoreLink Cache TL (CAS/AC/R2)** — `fabric-stub §A3` and `§H1` are still `⟨FILL⟩`
(`corelink-fabric-stub.md:26-31,84`); must close before any live `BootCas`:
1. **Warm-boot mechanism — overlay or per-job fetch?** Pre-seeded overlay/snapshot-restore at
   provision, or per-job CAS fetch over the wire during boot? `boot/mod.rs:28` assumes layers
   are in the CAS *"before a container is started"* — **by whom, via what?** *(highest-leverage
   unblock; forks everything downstream.)*
2. **Remote-cache protocol** — REAPI v2 reuse or custom CoreLink? Endpoint shape?
3. **Per-job CAS PAT (D-9)** — exact shape of `POST /internal/v1/runner/mint` so the broker
   mints it like the GitHub JIT config; scope (`cas:rw` vs `cas+ac`); TTL (5400s) ≤ lease
   deadline.
4. **AC lookup contract** — runner-side pre-lease or fabric-side? (`whitepaper:154` unwired.)
5. **Cold-run blockers (owner-gated ops):** prod `runners_entitlement` rows empty by ratified
   fail-closed default (mint 403s, acquire 0-slots); prod `CORELINK_PAT_MINT_AUTH_KEY` unset.
6. **Slot/billing SKU** — can `corelink-billing` carry a flat licensed-slot SKU
   (quantity=slots)? plan tier push or pull? (`auth-billing-integration-request §3` open.)
7. **Tense discipline** — cross-tenant dedup staged (`CAP-DEDUP-CROSS-TENANT`), not live; only
   ever claim intra-tenant dedup warm.

**To the Workspaces/`clw` TL** — CLI surface + `CLW_*` namespace are **PINNED clw-side**
(`CLI-CONTRACT.md`: snapshot/hydrate/run, exit-2=clw-error, exit-code-transparent run,
non-zero not cached; `CLW_REF_DOMAIN=runner` → keyspace `clw/ref/runner/v1/`). **No remaining
clw-side dependency blocks a mock build — the debt is entirely runner-side.** Ask:
1. Confirm the published `clw` binary **digest** to pin into `deploy/runner/` (X4 needs it).
2. Confirm `clw↔CAS` auth posture: `Authorization: Bearer <per-job PAT>` + tenant-in-URL-path
   (runner never sets `x-corelink-tenant-id`)?
3. Namespace reconciliation (doc-only): server dispatcher example uses `CORELINK_TOKEN` vs
   runner-TL decision `CLW_TOKEN` — confirm runner-side inject name is `CLW_TOKEN`.
4. Confirm runner *drives* (not reimplements) snapshot→hydrate→run; `run` exit-code-transparent
   per `clw-run-response §4`.

### 3.8 RECOMMENDED NEXT PHASE (P2 = research + cross-TL alignment, NOT a build sprint)

- **P2.0 — Close the two `⟨FILL⟩` decisions (BLOCKING, week 1).** Get CoreLink TL to answer
  **CT-Q1 (overlay vs per-job fetch)** + **CT-Q2 (protocol/endpoint)**. Whether we write a
  network `BootCas` or a mount-and-restore path forks on these. **Do not write a line of
  `BootCas` impl before it lands.**
- **P2.1 — De-risk non-cache cold-start S-class (parallel, week 1, no cross-TL needed):**
  - **S1:** resolve `<PIN-AT-BUILD>` base digest → real sha256.
  - **S3:** **enforce a minimum runner disk in code** — reject/clamp a RUNNER box that would
    inherit the 1 GiB CHECK default; turn the operational mitigation into a guard. *The only
    place a cold first run can genuinely break today.*
  - **S2:** make `NoBoxProvisioner` fail **at admit, not at exec**, for a runner lease without
    a cloud backend (or loudly warn).
- **P2.2 — Spec the Family-E2E wave against the now-closed seams (week 2).** Already fully
  scoped, seams CLOSED both sides (`clw-runner-contract-response`,
  `family-e2e-runner-TL-response`), sequenced after the vCPU-h ceiling wave (now merged).
  Test-first decompose: **MockBroker → CLW_* injection → digest-pinned `clw` in image →
  snapshot/run call-sites → live `BootCas` impl → flip live.** Land the runner-side D-9 mint
  client.
- **P2.3 — Wire the fail-closed guard into the live path (acceptance gate).** Once `BootCas` is
  live, prove cold-degrades-slow-from-CAS + CAS-unreachable raises the explicit error —
  closing the `interop.md:26` clause currently unenforceable.

**First target, one line:** *Get the CoreLink TL to answer "overlay-restore or per-job CAS
fetch?" — then everything else is decomposable; until then, harden the non-cache cold-start
S-class (the placeholder digest S1 and the un-enforced 1 GiB disk floor S3) so the first real
customer box can't break before the moat even exists.*

**Tense-discipline statement:** nothing above claims the cache moat is wired. It is
**decided-in-docs, seam-shaped-in-code, tested-against-fakes, unbuilt-in-production.** The
control plane (auth/cap), the exec backend, and the X4 floor **are** wired and tested.
Cross-tenant dedup is **staged, not live.**

---

## 4. OPERATIONAL CONTEXT (preserve verbatim)

### 4.1 The Northflank fleet + the disk-allowance saga (the S3 symptom, live)
- Northflank service `corelink-runners` (combined, git-backed) in project `corelink-runners`;
  runner boxes are ephemeral **JOBS**; Postgres addon `corelink-ledger`.
- ADR-0007 autoscaler: `workflow_job` webhook → `leases::acquire(runner)` → ephemeral
  Northflank JOB; label-gated (`FABRIC_AUTOSCALER_LABELS`, default `corelink`).
- **The #82 park:** flipping CI off the builder Mac onto the cloud fleet is **blocked** —
  `acquire(runner)` returns **503**: *"box provisioning failed: northflank create-job failed:
  HTTP 400 — Configured runtime ephemeral storage exceeds your project resource allowance."*
  i.e. our `NORTHFLANK_RUNNER_EPHEMERAL_STORAGE_MB` (tried 32768→8192→6144) **exceeds the
  Northflank account allowance**. **This is exactly S3 / the missing-CAS-offload symptom:** the
  box needs a fat disk *because* there is no CAS storage offload. The real fix is the moat
  (P3), not a bigger Northflank disk. A Northflank plan upgrade is an owner billing call.

### 4.2 The `!` (bang-command) mechanism — how Northflank API ops get run
The owner runs Northflank API scripts via `! <command>` in the prompt (user-initiated, lands
output in the conversation, bypasses the agent's PreToolUse hooks legitimately — their machine,
their auth). Helper scripts in `/tmp/` (token in `/tmp/nf_token`, rotation waived by owner):
- `nf_setenv.py` / `nf_fixstorage.py` — GET runtime-environment, merge one var into
  `data.runtimeEnvironment`, POST back **preserving `runtimeFiles`**, restart. (`STORAGE_MB`
  currently `'6144'`.)
- `nf_repro.py` — reproduces `acquire` with the autoscaler PAT + real runner image, auto-cancels
  on success (no stray box). Redacts tokens.
- `nf_cfg.py` — prints non-secret autoscaler config (presence-only for secrets).
- `nf_db.py` — Postgres addon status + DB/admit error log lines.
- `nf_allow.py` — probes the project/account resource allowance (the disk cap).
- `nf_logs.py` / `nf_health.py` — autoscaler/webhook log filter + health.

### 4.3 Waivers + the session fence (INVIOLABLE)
- **Session fence (owner mandate, mechanized):** NO sibling-repo mutation under
  `~/Documents/HuGR/` (`corelink-server`, `hugit`, `corelink-workspaces`, …). Single,
  composition-free, read-only Bash against siblings OK. Enforced by
  `.claude/hooks/forbid-sibling-paths.py` (default-deny, fail-closed).
- **hugit integration contract is FROZEN from hugit's side** — never edit unilaterally.
- **RunnerLease + conformance vectors are FROZEN** (drift tripwire).
- Waivers `TECHLEAD_ALLOW_SECRET_READ=1` / `TECHLEAD_ALLOW_EXFIL_RISK=1` only apply if set at
  Claude Code **launch-env** (NOT typed in chat). That's why Northflank ops go via `!`.
- Hook quirks: never `tail` on tasks-dir files (use `grep … | head`).
- Commits: `Signed-off-by: Gustavo Schneiter <gustavo@humangr.com>` +
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`. Never `gh pr merge --auto`
  (free plan = no branch protection; CI-green-before-merge is manual discipline).

### 4.4 Cross-TL coordination channel
The other TLs drop relay docs (I've received several at
`~/Documents/HuGR/corelink-workspaces/docs/…` paths the owner pastes). Prior runner-TL replies
live in `docs/handoff/2026-06-16-*.md` (family-e2e, clw-runner-contract, githugr-ci-on-fleet).
Cross-TL asks for P2 are itemized in §3.7 — these become relay docs the owner forwards.

---

## 5. PRIOR SHIPPED WORK + DEFERRED ITEMS

### 5.1 Shipped (do not redo)
- **`v0.1.0-seed`** (2026-06-12) — execution core (lease·isolation·teardown·boot·
  concurrency/expiry/recovery·Actions-YAML shim), fence enforcement, X4 oracle, §13 envelope.
- **vCPU-h hard compute ceiling** — **MERGED** (PR #86 → `main` `9635e31`). The
  loss-impossible enforcement wall (pricing.md §3): per-tenant monthly vCPU-h ceiling,
  default-off, fail-closed, atomic in the ledger admit under `pg_advisory_xact_lock(tenant)`;
  §8 invariant `actual ≤ reserved` (terminal accrual clamped). Audit-dry (22→34→3→0 across 4
  adversarial rounds), 817 workspace tests, DB-proven vs real Postgres 16. Full design record:
  `docs/handoff/2026-06-16-vcpu-ceiling-wave-plan.md`.
- **Pricing 40/60 ladder RATIFIED** (2026-06-16): $16/$40/$100/$200/$400 entry; ceilings
  100/240/600/1200/2400 vCPU-h at the conservative $0.10/vCPU-h basis (= nf-compute-400-16
  cost). The disk saga did NOT change the COGS basis. `docs/product/pricing.md`.

### 5.2 Deferred (lower priority until the moat work decides them)
- **#82 Mac-retirement** — blocked on the Northflank disk allowance (S3 symptom). Resolved
  either by the moat (P3, removes the fat-disk need) or an owner Northflank-plan-upgrade
  billing decision. Don't force a bigger disk.
- **githugr CI-on-fleet experiment** — gate 2 = add `humangr-labs/githugr` to
  `FABRIC_AUTOSCALER_REPO_ALLOWLIST` (CSV). (`docs/handoff/2026-06-16-githugr-ci-on-fleet-*`.)
- **CoreLink-introspect `max_vcpu_h` conformance vector** — owner/CoreLink-TL-gated (hugit-side
  PR first, never added unilaterally).
- **2-vCPU right-size test** — could lower COGS.
- **`hugit-c9-` container-prefix decision** — owner/hugit-techlead-gated.

---

## 6. KEY FILES (the map)

- **Vision:** `docs/whitepaper/corelink-runners-v1.md` (canonical) · `docs/product/product.md`
  · `docs/product/pricing.md` · `docs/interop.md` · `docs/spec/corelink-fabric-stub.md`
  (`§A3`/`§H1` `⟨FILL⟩`) · `docs/spec/hugit-integration-contract.md` v1.2.0 (frozen).
- **Cache seam (library, no live caller):** `crates/corelink-runner/src/boot/mod.rs`
  (`BootCas`, hydrate/cold_hydrate, `BoxHydrate`→`clw`) ·
  `crates/corelink-runner/src/materialize/mod.rs` (`materialize_sparse`).
- **Live path:** `crates/corelink-fabric-server/src/handlers/leases.rs` (acquire/finalize) ·
  `…/runner_inject.rs` (the inject seam to mirror) · `…/cloud_exec.rs` (provision; the hook
  point) · `…/exec.rs` (content-addressing, CAS write deferred) ·
  `crates/corelink-cloud-engine/src/northflank.rs` (box spawn, disk sizing, X4 floor).
- **Control plane:** `…/corelink_auth.rs` · `…/corelink_plans.rs` · `…/server.rs`.
- **Image:** `deploy/runner/Dockerfile` (S1 placeholder digest) · `deploy/runner/entrypoint.sh`.
- **CI:** `.github/workflows/ci.yml` (Swatinem, self-hosted) · `release.yml` · `dogfood-smoke.yml`.

---
*End of state. Resume at §0.*
