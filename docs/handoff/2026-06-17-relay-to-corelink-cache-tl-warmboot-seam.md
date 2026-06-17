# Relay → CoreLink Cache TL — close the warm-boot seam (CT-Q1 / CT-Q2)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Cache** TL (CAS / AC / R2)
> **Date:** 2026-06-17 · **Forwarded by:** owner (gustavo@humangr.com)
> **Status:** **BLOCKING** — this gates the entire Runners cache-moat build (P3). Nothing
> is being built on the cache axis until you answer **§2**. Everything you need to answer
> is in this doc; it is self-contained.

---

## 0. TL;DR — the one decision that forks everything

> **CT-Q1 — When a runner box boots, how does the job's working set become local from the
> CAS: a pre-seeded overlay / snapshot-restore mounted at provision time, OR a per-job fetch
> over the wire during boot (driven by `clw hydrate`)?**

Pick one (or describe a third). **Everything downstream — whether the Runners side writes a
network `BootCas` client or a mount-and-restore path, what the per-job CAS PAT is scoped to,
where the fail-closed guard sits — forks on this single answer.** It is the "core technical
bet" the fabric stub (`docs/spec/corelink-fabric-stub.md` §A3) has left `⟨FILL⟩` since day one.

Second, smaller, but also blocking the wire format:

> **CT-Q2 — What is the remote-cache protocol/endpoint shape? REAPI v2 (Bazel Remote
> Execution API) reused, or a custom CoreLink protocol? What does `clw` talk to?**

If you can answer **only CT-Q1 today**, that already unblocks the bulk of decomposition — send it.

---

## 1. Context (why you're getting this, and why you can trust the seam is real)

**CoreLink Runners** is expansion campaign #1: ephemeral, cache-warm CI/build compute, billed
by concurrency. The product's *one* differentiating thesis is **cache-warm by construction** —
"a runner that isn't warm off the CAS is just rented metal" (whitepaper §10). It **consumes**
your CAS/AC; it does **not** fork it (whitepaper §14: "a layer on the cache, not a parallel
system").

**Honest status (tense-disciplined):** today the runner is *rented metal*. We completed a
full drift assessment (2026-06-17). The entire cache-warm architecture on the Runners side —
the `BootCas` trait, `hydrate`/`cold_hydrate`, `BoxHydrate`→`clw hydrate`, `materialize_sparse`
— is **real, fail-closed, tested library code with ZERO production callers** (proven only
against in-memory fakes in our `acceptance_c3`/`acceptance_c9` suites). There is **no live CAS
or AC anywhere on the runtime path** — no fetch, no lookup, no memoization short-circuit. We
are **not** claiming warm boot is wired; we are asking you to decide the seam so we can build
the live data plane behind it.

**What IS wired + tested on our side (the seam is ready to receive your answer):**
- Control plane: introspect-auth → tenant + concurrency cap (conformance-pinned).
- Exec backend: managed-microVM provisioner (Northflank Engine) with spawn/teardown/probe.
- The **X4 supply-chain floor**: every image is `sha256:`-digest-pinned and verified *before*
  provider contact — so **any binary you ask us to bake (e.g. `clw`) must come with a digest
  to pin.**
- The proven **per-job env-injection seam** (`inject_runner_jitconfig` injects one box env var
  per job) — this is *exactly* the seam your `CLW_TOKEN`/`CLW_ENDPOINT`/`CLW_TENANT` will copy.
- Full lease lifecycle: acquire + cap-gate + atomic admit *before any box*, finalize with
  rollback-on-failure.

**What is frozen (do not expect us to move it):** the hugit integration contract
(`hugit-integration-contract.md` v1.2.0) is frozen from hugit's side; our `RunnerLease` +
conformance vectors are frozen (drift tripwire). The `clw` CLI surface + `CLW_*` namespace are
already PINNED on the Workspaces side. **All remaining cache debt is Runners-side — the only
thing blocking us is your two answers below.**

**The hard constraint that shapes every answer (the north star):** a customer must **never**
have a "can't run a job" hiccup. A **cold first run — empty CAS, nothing warm — MUST work.**
Cache is an optimization *on top of* a correct cold path; **cache absent ⇒ slow, never broken.**
So whatever mechanism you pick must degrade to "fetch-everything-cold, write-back, slow" — and
the contract clause we must be able to honor is `interop.md:26`: *"CAS/AC unreachable ⇒ an
explicit fail-closed error — never a silent cold result dressed as warm."*

---

## 2. THE BLOCKING DECISIONS

### CT-Q1 — Warm-boot mechanism (fabric-stub §A3, `[hugit-required §2]`)

Our `boot` module assumes (`boot/mod.rs:28`) that the working-set layers are present in the CAS
*"before a container is started"* — **but by whom, and via what mechanism?** The two candidate
shapes, with what each forces on our side:

| | **Option A — Pre-seeded overlay / snapshot-restore** | **Option B — Per-job CAS fetch over the wire** |
|---|---|---|
| **Mechanism** | At provision, the box's disk is a CAS-backed overlay / a restored snapshot; the working set is already mounted when the container starts. | At boot, the box runs `clw hydrate` which pulls the path-set from the CAS over the network into a thin local view. |
| **Runners builds** | A *mount-and-restore* path: provision asks the fabric/cache for the overlay handle; little-to-no network client in `BootCas`. | A network `BootCas` client (REAPI/custom) + the live `clw hydrate` call-site; `materialize_sparse` writes only in-fence entries. |
| **Where "warm" lives** | In the storage layer you (or the fabric) prepare ahead of the box. | In the CAS endpoint `clw` fetches from per job. |
| **Cold-first-run degrade** | Empty overlay → box falls back to cold fetch/build; must still be correct. | Empty CAS → `clw hydrate` is a no-op/miss → cold build, write-back for next job. |
| **Who owns the disk shape** | Mostly fabric/cache (overlay provisioning). | Mostly Runners (`clw` drives it) + cache (serves bytes). |

Our existing code leans toward **Option B** (the `BoxHydrate`→`clw hydrate` call shape, the
`materialize_sparse`/`select_in_fence` path-set writer) — but that is just where the seam
*happens* to be shaped today; **we will build whichever you choose** and have deliberately
written **zero** `BootCas` implementation until you decide. If you have a snapshot-restore
capability that is faster/cheaper, Option A may win — your call, you own the metal + storage.

**What we need from you:** A | B | other — and one paragraph on the mechanism.

### CT-Q2 — Remote-cache protocol / endpoint (fabric-stub §C3)

`§C3` asks: *REAPI v2 reuse? Custom?* — and it must carry the frozen `CheckDef`/`CheckResult`
types on the wire. We need:
1. **Protocol:** REAPI v2 (so we reuse battle-tested CAS/AC semantics + tooling) or a custom
   CoreLink protocol?
2. **Endpoint shape:** the URL/host `clw` (and any Runners-side client) targets for CAS read/write
   and AC lookup. Tenant-in-URL-path, or header?
3. **Auth posture:** we expect `Authorization: Bearer <per-job PAT>` with the tenant in the URL
   path (the runner never sets `x-corelink-tenant-id`). Confirm or correct.

---

## 3. SUPPORTING ASKS (not all blocking, but needed before P3 lands)

3. **Per-job CAS PAT — the D-9 mint (blocks the live hydrate path).** We already mint a GitHub
   Actions JIT config per job via a synchronous broker and inject it as one box env var; we want
   to mint the CAS PAT **the same way**. Please specify `POST /internal/v1/runner/mint`: request
   shape, **scope** (`cas:rw` vs `cas+ac`), **TTL** (we propose ≤ lease deadline, e.g. 5400 s),
   and the auth key the runner presents (we hold `CORELINK_PAT_MINT_AUTH_KEY`).
4. **AC lookup contract (memoized exec — AC hit ⇒ no runner spent).** Whitepaper §11 wants an
   AC pre-flight: resolve the action key, ask the AC first, and on a hit return the stored result
   so **the job never runs** (this is the structural basis of "never charge for the customer's
   own compute twice"). Is that lookup **runner-side pre-lease** or **fabric-side**? What's the
   key derivation + the `CheckResult` store-after-miss path (`§C2`)?
5. **Cold-run owner-gated blockers (FYI — flagging, not asking you to flip).** Prod
   `runners_entitlement` rows are empty by our ratified fail-closed default (mint 403s, acquire
   0-slots until seeded); prod `CORELINK_PAT_MINT_AUTH_KEY` is unset. These are owner ops, but
   they sit on the critical path for the first *real* cold run — flag if your side needs anything
   to seed them.
6. **Slot / billing SKU.** Can `corelink-billing` carry a flat **licensed-slot** SKU
   (quantity = slots), pushed by plan tier (or pulled)? Runners is priced flat-concurrency; we
   never expose per-minute. (Open item from `auth-billing-integration-request §3`.)
7. **Tense discipline (shared house rule).** Cross-tenant dedup is **staged**
   (`CAP-DEDUP-CROSS-TENANT`), not live. We will only ever claim **intra-tenant** dedup on warm
   boot. Please confirm nothing in your answer assumes cross-tenant dedup is live at GA.

---

## 4. What the Runners side commits to (the division of labor)

Once CT-Q1 lands, **we** own and will build: the live `BootCas` impl (network client or
mount-restore per your answer), the `clw` binary baked + **digest-pinned** into our runner image
(send us the digest — §CT-Q2/Workspaces), the `CLW_*` per-job injection (mirroring our JIT seam),
the snapshot/hydrate/run call-sites, the runner-side D-9 mint client, and making the
fail-closed-on-substrate-down guard **prod-reachable + proven** (cold-degrades-slow-from-CAS,
CAS-unreachable raises the explicit error). **You** own: the CAS/AC endpoint + protocol, the
warm-boot storage mechanism (if Option A), the mint endpoint, and the AC store/lookup semantics.

---

## 5. How to reply

Drop a reply doc (the owner will route it back to us). Minimum viable answer:
**(1) CT-Q1: A | B | other + the mechanism in a paragraph.** That alone unblocks our
decomposition. CT-Q2 + §3 can follow. If anything in the hugit contract looks infeasible from
your side, push back via the owner / hugit techlead — it's frozen, not unilateral.

*Anchors, for precision:* `docs/spec/corelink-fabric-stub.md` §A3 (line 26), §C3 (line 48),
§C2 (line 46), §H1 (line 84) · `docs/interop.md:26` (the fail-closed clause) ·
`docs/spec/hugit-integration-contract.md` v1.2.0 `[hugit-required §2]` (cache-warm boot).
