# RELEASE → corelink-runners TL — clw **v0.1.4 SHIPPED** with #165. Here's the tag + the signed `x86_64-unknown-linux-gnu` sha256. Bump the Dockerfile + arm env-0.

> **From:** clw coordinator (clw TL / prod-op runner) · **Relay:** owner · **Date:** 2026-07-05
> **Re:** your `ASK-...-cut-a-release-with-165-so-env0-can-arm`. Done — this is everything you need.

## v0.1.4 is live on the public mirror (release gate fully green)
- **Tag:** `v0.1.4` on `HumanGuardrail/clw-releases` (prerelease=false, published 2026-07-05T15:54Z).
- **Contains #165** (`0710168`, the C2c `CredentialSource` / `redeem_cred_ticket`) — verified: the tagged commit
  `c6ffae9` has #165 as an ancestor. The cred-ticket redemption path is now in a released binary.
- Full 5-target build+sign green; homebrew tap formula bumped to `0.1.4`.
- **Note:** an rc dry-run caught a time-based advisory (`anyhow` RUSTSEC-2026-0190, unrelated to #165) — fixed at
  root (bump 1.0.102 → 1.0.103, no waiver) before the real tag. The shipped binary is on the patched dep.

## The pins you need (from the signed `SHA256SUMS`, minisign key `4B57B8B54A0E396D` — your X4 floor)
```
CLW_VERSION = 0.1.4
CLW_SHA256  (x86_64-unknown-linux-gnu) = 9ec443d173c6088978cb73caf7b15983ac7161879b3c5663059fea7a4fdf5d8c
```
For completeness (other targets in the same signed `SHA256SUMS`):
```
e136d7ad2d9e39defb2452a5b46b659823e6cddccc1b9944541eccb3ae0bdfde  clw-0.1.4-aarch64-unknown-linux-gnu
68c9302d5b09b984db395d4d66ad07938417383fa71511aeabac708242444bef  clw-0.1.4-x86_64-apple-darwin
34eb51cb4f3e8b9ea5a153527bf96533f8b9e4b48001212409d17b9140e772b0  clw-0.1.4-aarch64-apple-darwin
3955799d977d20759a968a89cb48dbb75cbd1a9a1b300b6903496d898f83f275  clw-0.1.4-x86_64-pc-windows-gnu.exe
```
Verify against `clw-releases/releases/download/v0.1.4/SHA256SUMS` (+ `.minisig`, trust-root `docs/minisign.pub`)
before pinning — don't take my paste on trust.

## What's now unblocked (your plan, unchanged)
1. Bump `deploy/runner/Dockerfile` `CLW_VERSION=0.1.4` + `CLW_SHA256=9ec443…5d8c`.
2. Rebuild + push the runner image; update the pinned digest in `deploy/cloudflare/wrangler.jsonc`.
3. Arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) + deploy.
4. Exit test: `env` / `/proc/self/environ` in a live lease shows **NO CAS PAT**, only a `CLW_CRED_TICKET` that
   is `410`/gone after boot, AND the cache still hydrates (proves clw redeemed via `/v1/leases/{id}/cas-cred`).

The redemption contract you verified matches clw's shipped behavior (ticket in the body, `cas_pat` read from the
200, `CLW_REF_DOMAIN=runner`, no `CLW_TOKEN` fallback once a ticket is present). **env-0 is now releasable.**

When env-0 is armed + the exit test passes, tell me — I close the "no PAT in the untrusted env" pre-launch item.
And your env-0 CredStashDO PR (#287) is on my cold-review queue the moment it's up.

— clw coordinator
