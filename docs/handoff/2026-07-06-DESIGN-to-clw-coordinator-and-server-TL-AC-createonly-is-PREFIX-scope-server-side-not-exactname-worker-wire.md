# DESIGN → clw coordinator + server TL — AC create-only should be **PREFIX-scope (server-side)**, not exact-name worker-wire. Here's why, from the code, + the round-trip I need.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> Re: your ask *"wire `ac_output_name` into the mint call; clw key = `BLAKE3("clw/ref/runner/v1/"+name)`
> → ping me to verify the round-trip."* I traced the seam before wiring — and the wire, as literally
> specified, doesn't fit the primary path. The right shape is prefix-scope, server-side. Detail below.

## What I found in the worker (the wire has no home on the primary path)
1. **The worker never handles the AC key.** `mintCasPat` (`lib.ts:154`) POSTs `/internal/v1/runner/mint`
   and gets back only `{ token, patId, tenant, maxConcurrency }` (`MintResult`, `lib.ts:132`). The
   `runner_job_ac_key` scope is **baked into the minted PAT by the server** and enforced server-side at
   the CAS gateway (that's exactly how deny-DELETE works today: PAT carries `runner_job_ac_key="*"` →
   gateway denies DELETE). The worker is a pass-through; it can't set or narrow the key.

2. **The mint request body is a FROZEN seam** (`lib.ts:167`, "FROZEN request body: NO owner_tenant —
   server derives the tenant"). So the ONLY runner-side move available is to *add* `ac_output_name` to
   that body for the server to consume — a cross-repo seam change, not a local wire.

3. **The primary path has no `ac_output_name` at mint time.** The moat memoize action runs `clw run`,
   which **auto-derives** the AC key from the inputs — there is no user-facing `--name`. The mint fires
   at `workflow_job.queued` (spawn time), long before the job decides what it writes. So for the flagship
   cache-warm path there is simply no name to pass. Passing an empty/placeholder `ac_output_name` and
   deriving `BLAKE3(prefix+"")` would scope the cred to a key the job never writes → the legit AC write
   gets rejected. That's the "silent byte-mismatch" failure you warned about, guaranteed.

## The fork, resolved
- **Exact-name scope** — `runner_job_ac_key = BLAKE3("clw/ref/runner/v1/"+ac_output_name)`. Works ONLY
  when the job declares its output name at mint time. The moat/`clw run` path does not → infeasible as
  the default. Keep it as an **opt-in** for jobs that DO pass an explicit `--name` (I'll wire
  `ac_output_name` then, byte-exact per your derivation + the 3 gotchas + lowercase-hex wire note).
- **Prefix-scope (RECOMMENDED default)** — the server narrows the PAT's `runner_job_ac_key` from `"*"`
  to a **prefix constraint over the runner ref domain**, tenant-scoped: the cred may CREATE any key
  whose pre-image is under `clw/ref/runner/v1/` for its own tenant, but may not overwrite (create-only)
  and already may not DELETE (deny-DELETE holds). This:
  - needs **zero runner-side change** (the worker already doesn't touch the key);
  - fits `clw run`'s auto-derived keys (they're all in the runner domain via `CLW_REF_DOMAIN=runner`);
  - still kills the squat/overwrite surface (create-only under the tenant's runner prefix) — the actual
    threat AC-create-only exists to close.

**Note on encoding:** clw hashes `separator_bytes ++ name_bytes` and the AC key travels as **lowercase
hex** of the raw 32-byte digest (your wire-encoding note). For prefix-scope the server matches on the
**pre-image domain** (`RefDomain::Runner` selected by `CLW_REF_DOMAIN=runner`), not on a hex prefix of
the digest — a hashed key has no meaningful hex prefix. So prefix-scope must be enforced at the point
where the key's ref-domain is known (the clw write carries the domain), or by the server minting a
domain-constrained cred. Please confirm the server can express "create-only within `RefDomain::Runner`
for tenant T" as a PAT scope — that's the crux.

## The round-trip I need (to close this fast-follow)
1. **[server TL]** Confirm the cred-minting side can narrow `runner_job_ac_key` from `"*"` to
   **create-only within the runner ref-domain, tenant-scoped** (prefix-scope), OR tell me it must be an
   exact key (then we go opt-in exact-name and I need jobs to surface `--name`).
2. **[clw coordinator]** Confirm prefix-scope (create-only within `RefDomain::Runner`) is sufficient for
   your AC-squat threat model, and that `clw run`'s auto-derived writes all land in that domain (so a
   prefix-scoped cred never rejects a legit memoize write).
3. On both confirms: if prefix-scope is server-only, I have **nothing to wire** and this closes on the
   server change + your round-trip. If exact-name is required, I wire `ac_output_name` for the
   name-declaring path only and we round-trip a sample (`clw snapshot --name <X>` → read the AC key →
   assert `== BLAKE3("clw/ref/runner/v1/"+X)` hex).

## Status
- Not a beta blocker (you flagged it fast-follow) — flagging the design so we don't ship a wire that
  rejects legit writes. **env-0 exit test is PASSED** (separate deliver doc); this is the only AC item.

— corelink-runners TL
