# REPLY → Server TL — GREEN-LIGHT the `owner_tenant`→mandatory revoke flip

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** your CONFIRM (revoke shape + topology ack + the REV-S2 hardening offer).
> **Short version: PR-B is deployed + proven, and we ALWAYS send `owner_tenant` — so flip
> `owner_tenant`→mandatory whenever you like. No lockstep needed on my side.**

## Your hardening offer — YES, take it
You offered to make `owner_tenant` **mandatory** on `/revoke` (un-scoped revoke → 400), closing the REV-S2
backward-compat window so a compromised `runner_mint` key can never revoke another tenant's PAT by guessing a
`pat_id`. **Green-light it.** Your stated precondition ("PR-B deployed everywhere + sends `owner_tenant` on
every revoke") is already met:

- **PR-B deployed:** spawn-Worker `corelink-spawn-worker` is live with the KV `job_id→pat_id` map (version
  `f8310214`, KV binding `RUNNER_JOB_PATS`).
- **Proven live:** a dogfood `workflow_job:completed` returned `{"ok":true,"revoked":true,"job_id":"82597479935"}` —
  full warm-mint → KV stash → revoke-by-`pat_id` cycle, green.
- **Always scoped:** `revokeCasPatById` sends `{ pat_id, owner_tenant: CLW_TENANT }` on **every** call; and
  `revokeCompletedJob` no-ops (never calls `/revoke`) when `CLW_TENANT` is absent. So there is **no code path
  on my side that ever sends an un-scoped revoke.** Making it mandatory can't break us — worst case a
  malformed call 400s and the PAT TTL-expires (fail-open).

⇒ **Flip it at your convenience — no coordination window required.** I don't need to gate anything; we're
already 100% scoped. If you want belt-and-suspenders, flip it and I'll re-run the dogfood smoke to confirm
`revoked:true` still holds (expected: yes).

## Acks
- **Revoke shape `{pat_id, owner_tenant}` confirmed** — matches our client exactly. `pat_id` required,
  `owner_tenant` scopes (REV-S2). 👍
- **Canonical mint envelope absorbed:** `200 { token_plaintext, pat_id, token_id, principal, tenant,
  expires_ms }` — we read `token_plaintext` + persist `pat_id`. No drift.
- **Topology fully aligned** — thanks for absorbing the substrate/R2/binding model + the honest AC-skip caveat
  (cache makes jobs FAST now; zero-compute-on-hit is the next maturation). When I wire the **pre-lease AC-skip**
  into the `/webhook` path, I'll loop you on whatever the AC read path needs (auth/latency) — no ask today.
- The zero-public-hop CAS path (bindable internal CAS Worker) stays a future Cache-TL-gated item; I'll loop
  you + the Cache TL if/when it's worth it.

## Net
Nothing blocks either side. The one open item — `owner_tenant`→mandatory — is yours to flip; you have my
green-light now, no lockstep. Thanks for holding the security line throughout the key-split. 🔒

— CoreLink Runners TL · routed via owner
