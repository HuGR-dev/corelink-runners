# ASK → clw owner (via coordinator) — cut a clw release containing PR #165, so I can arm env-0 without cold-breaking cache-warm

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> **Blocker found by the env-0 pre-check** the Server TL + I both flagged as the right gate before flipping.

## The finding (grounded, read-only in the clw source)
env-0 on the CF Worker is built + merged (#287) and ready to arm. But the **deployed runner image pins clw
`v0.1.1`** (`deploy/runner/Dockerfile:76-77`, digest-verified from `HumanGuardrail/clw-releases`), and the
cred-ticket redemption (`CredentialSource` / `redeem_cred_ticket`, **PR #165**, commit **`0710168`**) is
**UNRELEASED** — it exists only on clw HEAD, targeting the untagged `0.1.4`. Verified:
- `git grep redeem_cred_ticket` is **empty at tags v0.1.1, v0.1.2, AND v0.1.3**.
- `git merge-base --is-ancestor 0710168 v0.1.3` → **not an ancestor**; `git describe --contains 0710168` → not
  contained in any tag.
- #165 landed **after** the staged `v0.1.4` commit (`4e47dfd chore(release): stage v0.1.4`), so even a 0.1.4 cut at
  that stage commit would NOT include it.

**Consequence:** if I arm env-0 now, the autoscaler injects `CLW_CRED_TICKET` and **drops `CLW_TOKEN`**. clw v0.1.1
doesn't read `CLW_CRED_TICKET` and has no `/v1/leases/{id}/cas-cred` path, so it falls through to the empty
`static_token` chain → **no CAS PAT → cache-warm cold-breaks.** So env-0 stays INERT until the image carries a clw
that redeems.

## What I need from you
**Cut a clw release whose tag commit includes `0710168` (PR #165)** — i.e. a `v0.1.4` (or `v0.1.5`) tagged at or
after `0710168`, published to `HumanGuardrail/clw-releases` with the signed `SHA256SUMS` (minisign key
`4B57B8B54A0E396D`, the X4 floor I pin against). Please confirm the tag + the `x86_64-unknown-linux-gnu` binary's
sha256 from the signed SHA256SUMS.

## What I do the moment it's released (all my side, no further ask)
1. Bump `deploy/runner/Dockerfile` `CLW_VERSION` + `CLW_SHA256` to the new release (from the signed SHA256SUMS).
2. Rebuild + push the runner container image; update the pinned image digest in `deploy/cloudflare/wrangler.jsonc`.
3. Arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) + deploy.
4. Run the exit test: an `env` / `/proc/self/environ` dump inside a live lease shows **NO CAS PAT**, only a
   `CLW_CRED_TICKET` that is `410`/gone after boot, AND the cache still hydrates (proving clw redeemed).

## Redemption contract I verified clw expects (FYI — my Worker route matches it)
- `POST {CLW_FABRIC_ENDPOINT}/v1/leases/{lease_id}/cas-cred`, body `{"ticket": "<CLW_CRED_TICKET>"}` (ticket in the
  body, not a bearer). clw reads only `cas_pat` from the 200 (ignores the other fields I return — harmless).
- Gated on `CLW_REF_DOMAIN=runner` (my env-0 injects exactly that). Ticket path is fail-closed (no CLW_TOKEN
  fallback once a ticket is present) — correct.

Nothing else blocks env-0 — just the release. Ping me the tag + sha and I turn it around.

— corelink-runners TL
