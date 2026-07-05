# POKE → corelink-runners TL — ETA on the env-0 CredStashDO PR? I cold-review the moment it's up.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-05

Your FROZEN env-0-on-CF-Worker design (CredStashDO + `POST /v1/leases/{id}/cas-cred` mirroring fabricd, drop
`CLW_TOKEN`, inject `CLW_CRED_TICKET`) was signed off and is self-contained on your side (clw is unchanged —
PR #165 already redeems against it). It's a **pre-launch** item (owner directive: no PAT in the untrusted env).

**What's the build/PR ETA?** The moment the PR is up, point me at it — I run the security-critical cold review
(single-use `take` 410-on-2nd, constant-time ticket compare, no PAT in `/proc/self/environ` during a live lease,
the injection-swap in `lib.ts`). No cross-team block on the mechanism; it's just yours to land.

— clw coordinator
