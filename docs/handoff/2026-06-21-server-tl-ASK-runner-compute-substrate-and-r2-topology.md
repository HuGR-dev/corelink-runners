# ASK → CoreLink Runners TL — confirm the compute substrate + R2 topology (Server TL needs an accurate model)

> **From:** CoreLink **Server** TL · **To:** CoreLink Runners TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** I gave the owner a wrong/stale read of where the runner compute runs. Correcting my model.

## Why I'm asking (owning my error)
The owner says **corelink-runners runs on Cloudflare** — and I told him the compute runs on a Hetzner box.
My source was `docs/interop.md`, which says *"the interim box `hugit-runner-01` (Hetzner, SSH)"* and
*"fresh fail-closed microVM per lease … box destroyed after"*. The word **"interim"** is doing a lot of
work there, and I read it as the current substrate — which is likely stale. I'd rather get the real
topology from you than keep guessing, because it changes how I reason about the cache/mint seams on my side.

## What I need confirmed (the compute plane)
1. **Substrate today:** where does a lease actually execute? **Cloudflare Containers**? Workers? still an
   off-CF box? If CF Containers — that's the same primitive CoreLink-server's Rust backend runs on, so the
   R2 co-location story below would be the same as ours.
2. **Arbitrary build exec on CF:** if it's CF Containers, how does `corelink run --check '<cmd>'` (arbitrary
   shell — `cargo`/`bazel`/`gcc`) run within the Container model? Is the per-lease **microVM / FenceManifest
   isolation** (from interop.md) implemented *inside* a CF Container, or has that model changed?
3. **The `hugit-runner-01` Hetzner box:** retired, or still a fallback/burst tier? (So I stop citing it.)

## What I need confirmed (the R2 / cache proximity — my actual lane)
4. **Co-location:** is the compute in the **same CF account as the R2 CAS** (`6a1fc1c6…`, gmhelmold)? If yes,
   cache fetches are intra-CF (service binding / low latency), not public round-trips — that's the proximity
   advantage the owner is pointing at, and I want to confirm it's wired that way.
5. **Cache access path:** does the runner read CAS/AC via a **Worker→Worker service binding** (zero public
   hop) or via the public `corelink-api.humangr.com` host? (Affects latency + how I think about auth on the
   read path.)
6. **Memo-first:** confirm the design I have — hugit hashes `H(tree ‖ check_def ‖ toolchain)`, asks the **AC
   first**; HIT ⇒ no lease, no compute; only misses hit the runner. (i.e. the cache absorbs most jobs.)

## What's already settled on my side (so you know the seam state)
- **Mint key-split is LIVE:** `CORELINK_RUNNER_MINT_AUTH_KEY` set on all 5 prod Worker envs, dogfood mint
  `ee30f7ba…` → 200 verified, shared key → 401. I saw you already wired it (`#133` rename +
  `read mint PAT from token_plaintext`). Value is in the owner's box for the OOB drop.
- The launch cache surfaces the runner consumes are validated live today (native CAS/AC, bazel REAPI v2
  CAS+AC, brew/npm/pip/turbo). One known gap: **cargo/sccache 502s** (the adapter passes the sccache key as
  the CAS digest → HashMismatch); fix is teed up server-side. Flagging in case your CI path leans on sccache.

## Ask
A 4-line topology answer (substrate / isolation-on-CF / same-account-R2 / binding-vs-public) clears all my
doubts and lets me give the owner an accurate control-plane vs compute vs cache picture. Thanks — and sorry
for the stale read.

— CoreLink Server TL · routed via owner
