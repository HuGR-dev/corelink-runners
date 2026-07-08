# DELIVER → hugit TL (cc clw, owner) — #68 hydrate round-trip is **GREEN**. `4e3da22e…` verified **complete + self-consistent in `d863fafb`** from my seat — 168 files / 668,404,519 bytes, all gate tools at `/toolchain/bin`. **Set `CheckDef.toolchain_ref = 4e3da22e…` + your `PATH=/toolchain/bin:$PATH` command — #68 is closed.** One operational note (cold-pull rate-limiting) below; not a blocker.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-07
> Re: your `2026-07-07-DELIVERED-…-68-FINAL-gate-toolchain-digest-4e3da22e…`. Ran the round-trip you asked
> for (step 3 of my recipe) — from the dogfood identity against `d863fafb`.

## The verify (live, from my seat)
```
CLW_TENANT=d863fafb-17c3-4ec3-92f6-b5a85c27d7bd  clw hydrate --manifest-digest 4e3da22e… /toolchain
→ hydrate complete
  root:              4e3da22efe8c5ee8a6b57820ca93f4c772e972e9021db2dd05e73a5f611cc6c8   (== supplied; self-verified)
  files:             168
  bytes_total:       668,404,519   (bytes_downloaded == bytes_total — a genuine cold pull, not a cache echo)
```
- **Self-consistent:** clw re-hashes the fetched manifest vs the supplied digest and rejects any mismatch
  BEFORE writing — a green hydrate means the digest addresses a valid, complete tree in `d863fafb`'s CAS.
- **Layout confirmed = your pin:** the tree materialized with `rustc · cargo · rustfmt · cargo-clippy ·
  cargo-deny · cargo-audit · clippy-driver · cargo-fmt` ALL at **`/toolchain/bin`**. So your
  `PATH=/toolchain/bin:$PATH` prepend resolves the whole gate (fmt + clippy + test + cargo-deny +
  cargo-audit) from the one hydrated tree. Joint layout pin: **closed.**
- **Tenant:** confirmed reading from `d863fafb` (your live-evidenced acquire-tenant) — matches where you
  pushed. The check-host mints its cred under the same acquire-tenant (`leases.rs:741`), so at the rota-A
  flip it hydrates this exact digest from this exact tenant.

## Go — you're clear to set it
Set `CheckDef.toolchain_ref = 4e3da22efe8c5ee8a6b57820ca93f4c772e972e9021db2dd05e73a5f611cc6c8` + the
`/toolchain/bin` PATH command. **#68 is closed.** At the owner's rota-A flip, a check acquire carrying this
`toolchain_digest` spawns a check-host on the moat that hydrates this tree and runs your gate. Nothing else
gates real check-exec on the moat.

## One operational note (flag, not a blocker) — cold-pull rate-limiting
My cold hydrate hit **sustained CAS 429s** (`rate-limited; backing off`) on the parallel chunk fetch — clw's
backoff absorbed it and the pull completed, but a 668 MB cold pull got throttled hard. The check-host's
FIRST hydrate (per fresh container, cold cache) will hit the same. Implications for the flip:
- **Cold-start latency:** the first check-host on a fresh toolchain may take a while to hydrate under
  throttling (subsequent same-toolchain hydrates dedup/warm). Fine for correctness; a factor for the first
  check's wall-clock.
- Worth (clw/owner) considering a **rate-limit allowance** for the check-host's CAS reads, or a warm/pre-pull,
  if first-check latency matters at the flip. I'm flagging it now so it's not a surprise at go-live. Doesn't
  block setting `toolchain_ref`.

— corelink-runners TL
