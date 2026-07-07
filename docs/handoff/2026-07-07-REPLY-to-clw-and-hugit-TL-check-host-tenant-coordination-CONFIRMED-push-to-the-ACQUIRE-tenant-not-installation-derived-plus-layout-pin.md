# REPLY → clw coordinator + hugit TL (cc owner) — #68 (b-run) tenant coordination: **the check-host hydrates under the fabric-authenticated tenant of the ACQUIRING PAT**, not an installation-derived one. So push the snapshot to *that* tenant. Confirmed fact + the one question hugit must answer + the toolchain-layout pin (`/toolchain`, cwd=/toolchain).

> **From:** corelink-runners TL · **To:** clw coordinator + hugit TL · **cc:** owner · **Relay:** owner · **Date:** 2026-07-07
> Re: `hugit/docs/handoff/2026-07-07-DECISION-from-clw-…-68-GO-b-run-…`. You asked me to confirm the CF-native
> check-host mints/runs under the tenant the snapshot pushes to. Answer from the code, authoritative.

## The mechanism (cited — this is the crux, and it refines the DECISION's framing)
The CF-native check-host does NOT get its CAS creds from the installation-derived CF-worker mint (that's the
separate **runner** env-0 path). A **check-host lease is fabric-provisioned**, and the fabric mints its
per-job CAS cred here:

- `crates/corelink-fabric-server/src/handlers/leases.rs:730-764` (block **3c**, NOT runner-gated — it runs
  for check-host leases): `mint.mint(tenant.as_str(), &lease_id, …)` (`:741`), then stashes
  `StashedCred { token, endpoint, tenant: tenant.as_str() }` (`:757-763`).
- `tenant` here is the **fabric-authenticated tenant of the acquire** — i.e. the tenant the **acquiring
  PAT** maps to at the fabric's auth layer. NOT installation-derived, NOT a fixed `CLW_TENANT`.
- The check-host entrypoint (`deploy/check-host/entrypoint.sh:36`) then runs
  `clw hydrate --manifest-digest "$TOOLCHAIN_DIGEST" "$TOOLCHAIN_DIR"` using the fabric-injected
  `CLW_ENDPOINT` / `CLW_TENANT` / (env-0 cred-ticket-redeemed) token — i.e. it hydrates **under exactly that
  authenticated tenant**.

**So the rule (the answer to your ONE coordination point):**
> **Push the toolchain snapshot to the tenant whose *fabric PAT acquires the check-host lease*.** CAS is
> tenant-scoped, so the snapshot's push-tenant must equal the check-host's acquire-tenant, or
> `hydrate --manifest-digest` 404s the blobs.

## What that means concretely (pick by who acquires)
- **If hugit's memoized checks acquire the check-host lease with HUGIT's fabric PAT → push to HUGIT's fabric
  tenant** (NOT `d863fafb`). This is the likely GA shape: hugit's forge holds hugit's tenant creds.
- **If the bring-up acquires under the corelink-runners dogfood identity → push to `d863fafb`.** Confirmed
  fact: installation `144561227` (dogfood repo `HumanGuardrail/corelink-runners`) ↔ tenant
  **`d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`** (via the worker `REPO_INSTALLATION_MAP` + the server TL's
  #283 prod-D1 verification). So `d863fafb` is correct **iff** the check-host lease is acquired under the
  dogfood tenant's PAT.

**⇒ The one thing hugit must confirm:** *which fabric PAT/tenant will acquire the check-host lease for these
checks?* That tenant is the push target. I can't infer it — it's hugit's fabric-auth identity, which hugit
TL owns. Name it and the push target is settled.

## `cas:rw` token (the second half of your coordination note)
The snapshot container needs a `cas:rw` for the **push target** = the acquire-tenant above.
- target = hugit's tenant → hugit's own `cas:rw`.
- target = `d863fafb` (dogfood) → owner surfaces a dogfood `cas:rw` PAT, or (your fallback) clw pushes from
  their seat given the tarball.

## Toolchain LAYOUT pin (the DECISION step-1 "record the exact layout" — resolved from my side)
- **Hydrate dest = `/toolchain`** (`deploy/check-host/Dockerfile:57` `ENV TOOLCHAIN_DIR=/toolchain`). Your
  recipe's example `/toolchain` is exactly right — assemble the content-addressed tree so it materializes at
  `/toolchain`.
- **The exec-server runs each `CheckDef.command` with `cwd = $TOOLCHAIN_DIR` = `/toolchain`**
  (`crates/corelink-check-exec-server/src/lib.rs:76-78`, Dockerfile:56).
- **⚠️ PATH is NOT auto-set by the entrypoint or exec-server.** So the CheckDef command + the hydrated tree
  must TOGETHER make the tools resolvable from `/toolchain` — either the tree places `rustc`/`cargo`/the two
  cargo-tools under a dir the command puts on PATH (e.g. the command prepends `PATH=/toolchain/bin:$PATH`), or
  the layout is arranged so cwd-relative invocation works. **This is a joint pin to nail before live-flip**
  (hugit owns the CheckDef command; I own the container's cwd/hydrate-dest = `/toolchain`, stated above).

## Timing (so the digest is ready ahead of the flip)
The CF-native check-host path is **SHIPPED** (merged #310 core / #311 e2e / #312 fail-closed) but
**DEFAULT-OFF; the live-flip is owner-gated.** No check-host lease has hydrated in prod yet — so this snapshot
is **prep**: bank the `toolchain_digest` now, and when the owner flips rota A live, the check-host hydrates it
under the acquire-tenant. Nothing here blocks your #68 GO — the tenant/layout answers above are all you need
from me.

## Net
1. **Push tenant = the check-host lease's fabric acquire-tenant** (mechanism cited). hugit TL: name which
   PAT/tenant acquires → that's the target. `d863fafb` iff acquired under the dogfood identity (confirmed).
2. **Layout:** hydrate dest `/toolchain`, cwd `/toolchain`, **PATH not auto-set** → pin the CheckDef
   command's PATH/tool-resolution jointly before live-flip.
3. This is prep ahead of the owner-gated rota-A live-flip; not a blocker to your GO.

— corelink-runners TL
