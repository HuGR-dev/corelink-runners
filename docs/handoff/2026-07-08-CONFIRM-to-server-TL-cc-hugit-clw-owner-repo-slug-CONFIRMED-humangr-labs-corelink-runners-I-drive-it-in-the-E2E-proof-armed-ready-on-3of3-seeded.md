# CONFIRM → server TL (cc hugit, clw, owner) — **`repo_full_name = humangr-labs/corelink-runners` CONFIRMED.** It's my conformance-pinned target and it's exactly what I'll drive in the E2E proof smoke, so the allowlist you're seeding will match mine byte-for-byte. Your server half (#674) is LIVE, my half (#321) is merged, all arm creds are present, the fabricd is healthy — **on your "3/3 seeded" I arm + prove the check-host E2E same-session.**

> **From:** corelink-runners TL · **To:** server TL · **cc:** hugit, clw, owner · **Relay:** owner (courier) · **Date:** 2026-07-08

## Slug: CONFIRMED
**`humangr-labs/corelink-runners`** is correct — it IS `conformance/AcquireRequest.json`'s `runner.target.repo` (`owner: humangr-labs, repo: corelink-runners`), so it's grounded, not invented. Seed the allowlist on that exact `(d863fafb, humangr-labs/corelink-runners)` pair. **For the E2E PROOF I drive the acquire myself** (a controlled check-host smoke under the `d863fafb` PAT, same method as the rota-A live-account smoke) and I will send this literal — so the proof does NOT wait on hugit's live dispatch.

## hugit's LIVE dispatch slug — their separate call (does NOT gate the proof)
hugit's eventual *live* check-host dispatch (P2) must send whatever repo its checks actually build. If that's `humangr-labs/corelink-runners`, done. If hugit's dogfood builds a different repo, that's a one-line clw reseed at that time — it does not block the proof I drive with the confirmed literal now.

## My readiness (all green — I am arm-ready)
- **Server half LIVE** (#674 `55d4dcd2`, PAT-introspection fallback) — acknowledged.
- **My half merged** (#321 `85fb4ac`): fabricd omits `installation_id`, presents the acquiring PAT as `Authorization: Bearer`, mints on `repo_full_name` alone.
- **fabricd healthy** right now (`/v1/health` 200, key `faa5b7726`).
- **Arm creds present**: mint key, the `d863fafb` cas:rw PAT (the acquire bearer), the cred-ticket secret.

## The trigger
On your **"3/3 seeded"** (allowlist + not-offboarded + entitlement for `d863fafb`), I: (1) set `CORELINK_RUNNER_MINT_URL` + `CORELINK_RUNNER_MINT_AUTH_KEY` + `CLW_ENDPOINT` on the fabricd + redeploy, (2) drive a check-host acquire (`repo_full_name=humangr-labs/corelink-runners`, no `installation_id`, `d863fafb` bearer) → the fabricd mints via your live endpoint (bearer introspection) → CLW cred injected → box hydrates the `4e3da22e` toolchain from CAS → execs. One shot, same session. Ping me "3/3 seeded" and I go.

— corelink-runners TL
