# Owner action board — every remaining pendency, one action each

**Date:** 2026-06-15 · **Author:** corelink-runners techlead · **Baseline:** `main` after the adoption + close-out waves.

Everything that could be closed *inside this repo* is closed (core is feature-complete, zero open P0/P1, zero in-code debt). What remains is **owner-gated or cross-repo** — work the fabric cannot do for itself. This board reduces each to a single concrete command, decision, or relay. Ordered by leverage.

> Convention reminder: redeploy must be a **New Build of `main`**, never a Restart (a restart re-runs the old image). Never `gh pr merge --auto`. Secrets are deprioritized by the owner ("por último do último").

---

## A. Redeploy the live fabric to current `main`  — **SAFE, do anytime**

**Why:** the live Northflank service predates the `result_binding_sig_v2` P0 attestation fix + all hardening. A shipped security fix should not sit undeployed.

**Risk:** audited zero. Every env var introduced since the last deploy is **default-off or default-unchanged**; no new *required* config. A redeploy with the current live env reproduces today's behavior exactly and never fail-opens. (Evidence: `AUDIT-REDEPLOY`, this wave — every `std::env::var` read enumerated; the only hard-required vars — `FABRIC_SIGNING_KEY` / `FABRIC_PAT` / `FABRIC_TENANT` / `FABRIC_TENANT_MAX_CONCURRENCY` + the pg pair — are already in the live env. Ledger is Postgres, so `instances=2` stays cap-safe; pg-without-`DATABASE_URL` is a hard boot error, never a silent in-memory fallback.)

**Action (owner):**
1. Northflank → org `human-guardrail` / team `humangr` → service `corelink-runners` → **New Build** of `main`. Do **not** add/change any env var.
2. Smoke (side-effect-free) once healthy:
   ```sh
   export CORELINK_PAT=<live FABRIC_PAT>
   corelink smoke --url https://p01--corelink-runners--pmk6nf8xbcjb.code.run
   ```
   Expect: health ok · attestation key served · unpinned→400 · bad-PAT→401 · exit 0.
3. Full path (provisions + tears down one real box):
   ```sh
   corelink smoke --url https://p01--corelink-runners--pmk6nf8xbcjb.code.run --full
   ```
   Expect: real acquire → microVM → teardown clean, **and the CloseResponse carries `result_binding_sig_v2`** (its presence = the P0 fix is live). Boot log shows `ledger backend: Postgres … pool=8`.

Reference: `docs/deploy/post-redeploy-smoke-checklist.md`, `docs/deploy/northflank-postgres-runbook.md`.

---

## B. hugit adds the `result_binding_sig_v2` verifier  — **relay (SECURITY)**

**Why:** the fix is backward-compat (v1 still emitted), so the verdict-forgery window stays open on hugit's v1-only verify path until they verify v2. P0 security, no flag-day pressure.

**Action (owner):** forward **`docs/handoff/2026-06-15-hugit-v2-verifier-options.md`** to the hugit techlead. It gives them two concrete paths — (1) transcribe the v2 formula (spelled out, vector-locked), or (2) **depend on our reference SDK** `@corelink/verify` / `corelink_verify` (new this wave). Ask back: which path + when. (Frozen-from-hugit's-side; we propose, never edit their code.)

---

## C. CoreLink slot-billing flip (M2)  — **1 decision + 1 command, our side READY**

**Why:** our side is built (`CoreLinkPlanStore` + `CoreLinkTokenStore`, conformance-pinned introspect shape, durable billing exporter — all DEFAULT-OFF). Blocked only on (a) corelink shipping the `runners_entitlement` lookup + minting a real tenant PAT, and (b) the owner picking the first dogfood tenant.

**Action (owner):**
1. **Decide:** which tenant gets the first `runners_entitlement` row (so a tenant can actually use Runners). An empty table = all tenants cap-absent, so the flip validates the 3 fail-closed arms immediately even with nothing sold.
2. When corelink confirms the lookup is live + you have the real tenant PAT, flip:
   ```
   FABRIC_AUTH_BACKEND=corelink   (was: static)
   # CORELINK_INTROSPECT_URL + FABRIC_INTROSPECT_AUTH_KEY already prepared
   ```
   then redeploy (New Build). Validate per `docs/deploy/corelink-flip-runbook.md`.

> Note: a tenant can ALSO be onboarded today with **zero** CoreLink dependency via the static backend + `POST /internal/v1/admin/tenants` (`FABRIC_ADMIN_KEY`) — so dogfood/real workloads do **not** block on this flip.

---

## D. `hugit-c9-` container-prefix rename  — **1 decision**

**Why:** an ops-visible seam naming change (not a local cleanup); needs an owner/hugit call before anyone renames.

**Action (owner):** decide keep-or-rename and, if rename, the target prefix + the cutover window (it touches live container naming on both sides). No code moves until decided.

---

## E. Publish the SDKs + binary  — **prep DONE; gated on secrets + 2 decisions**

**Why:** the GitHub Action + Buildkite plugin currently expect `corelink` on PATH. A tagged release makes them real for outside users.

**Action (owner), when ready (secrets are deprioritized):**
1. Tag a release: `git tag vX.Y.Z && git push origin vX.Y.Z` → `.github/workflows/release.yml` builds the `corelink` binary and attaches it (+ sha256) to a GitHub Release. (Inert on every non-tag push.)
2. To publish SDKs, add repo secrets `NPM_TOKEN` / `PYPI_TOKEN` (publish jobs are **skipped, not failed**, until present).
3. **Two open decisions** (see `docs/release.md` §3): npm access scope (private vs public `@corelink`) + PyPI name; and the **SDK license** — currently proprietary (`UNLICENSED` / `Proprietary`) by safe default; a permissive license needs an explicit decision + a top-level `LICENSE` file before any public publish.

Follow-up (deliberate, ours): once a release exists, wire the GH Action + Buildkite plugin to download the released binary instead of PATH-locate.

---

## F. Deprioritized / blocked — no action now (tracked for honesty)

- **Secret rotation** (Northflank token, `FABRIC_SIGNING_KEY`, `FABRIC_PAT`, Postgres pw, `FABRIC_INTROSPECT_AUTH_KEY`) — owner deprioritized ("por último do último"). Checklist ready: `docs/deploy/secret-rotation-checklist.md`.
- **ATT3 secrets seam** — awaits the hugit payload contract (decision #7); also secret-adjacent, so deprioritized.
- **FC1–FC5 (Firecracker / own-metal)** — blocked on a KVM bare-metal buy; off the critical path.
- **M2 identity / self-serve onboarding** — a future campaign (ADR-0002: HuGR account / Clerk pool), not this phase.

---

### Summary

| # | Pendency | Owner action | Blocking? |
|---|---|---|---|
| A | Redeploy `main` | New Build + `corelink smoke` | no — safe anytime |
| B | hugit v2 verifier | forward the relay doc | no — backward-compat |
| C | Billing flip | pick dogfood tenant; flip env when corelink ready | cross-repo |
| D | `hugit-c9-` prefix | decide keep/rename | low |
| E | Publish SDKs/binary | tag; add secrets; 2 decisions | no — prep done |
| F | Secrets / FC / M2 | none now | deprioritized/blocked |
