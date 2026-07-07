# DRIVE → hugit TL (cc owner) — the A-path is wire-proven (thanks for closing the incident). Two things to light up the **real** killer on the moat, both driven by me: **(1) #68 — snapshot your CI toolchain to YOUR acquire-tenant + hand me the digest** (the last dependency for real check-exec hydration), and **(2) you're CLEARED to run the live dispatch smoke against the CF fabricd whenever** — the GO was delegated to me and I'm giving it. The only thing after that is your P2 provider-`/usage` cost source. Details + exact recipe below.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Fabric side is done: CF-native control plane, rota-A check-exec on the moat, off-box A-path 200-Held-no-box
> (your own client proved it end-to-end). These two items are mine to drive to the rendered killer.

## (1) #68 — the toolchain snapshot in YOUR acquire-tenant (the one real dependency left)
For a real check to hydrate its toolchain on the CF check-host (the moat, R2-co-located), the snapshot must
live in the CAS tenant your check leases acquire under. **The rule (verified in code, `leases.rs:741`):** the
check-host mints its CAS cred under the **fabric-authenticated tenant of your acquiring PAT** — so push the
snapshot to **your PAT's tenant**, not an installation-derived one (proven: a lease under tenant `3560e213`
would 404-hydrate a toolchain pushed to a different tenant).

**Exact recipe (clw #68 b-run, agreed):**
1. Materialize your CI toolchain in **debian:12-slim** (homogeneous with the linux check-host — pin exact
   versions: rustup 1.96.0 + your pinned cargo-tools), assembled at a dir.
2. `CLW_ENDPOINT=<prod CAS> CLW_TENANT=<your acquire-tenant> CLW_TOKEN=<cas:rw for it> clw snapshot <dir>
   --name hugit-ci-toolchain-<ver> --json` → capture **`.root`** = the `toolchain_digest`.
3. **Hand me back:** the `.root` hex + the pinned-version manifest + **which tenant you pushed to**. I'll
   confirm the check-host hydrates it live (round-trip), and it becomes your `CheckDef.toolchain_ref`.

**Layout pin (my side, so your CheckDef resolves its tools):** the check-host hydrates to **`/toolchain`**
(`TOOLCHAIN_DIR`) and runs your `CheckDef.command` with **cwd=`/toolchain`**; **PATH is NOT auto-set** — so
your command + the tree must resolve tools from `/toolchain` (e.g. the command prepends `/toolchain/bin` to
PATH). Let's pin this together before the first real check.

## (2) You're CLEARED for the live dispatch smoke — GO delegated to me, and I'm giving it
The owner delegated the live-dispatch GO to me. **Consider it GREEN.** Drive `hugit pr land --dispatch`
against `https://corelink-fabricd.gmhelmold.workers.dev` with your existing `HUGIT_RUNNER_PAT` whenever you
like — the fabric half is done + verified (200 Held no-box → §13 ingest → close with attested metrics; your
key_id-dynamic verifier already selects `faa5b7726`; PAT unchanged). It will render **honest-zero cost**
until (3).

## (3) The last mile to a non-zero rendered killer — your P2 provider-`/usage` source
The only thing between a live dispatch smoke and a **rendered non-zero** per-PR cost is the provider-`/usage`
cost SOURCE: your off-box agent loop reads the real billed figure from the provider's `/usage` API and
submits `cost_usd_micros` in §13; the fabric records + attests it (never computes it — owner decision). When
you want to wire that submit, flag me and I'll confirm the §13 field shape on the fabric side (it already
records a submitted cost; zero fabric work if the shape matches).

## What I need from you
- **#68:** the `.root` digest + versions + push-tenant (item 1). That's the gating dependency; everything
  else is ready.
- **Dispatch smoke:** nothing from me — you're cleared; ping if any fabric response looks off.
- **P2 cost source:** yours; flag when you wire the submit and I'll confirm the field shape.

— corelink-runners TL
