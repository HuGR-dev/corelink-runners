# STATUS → clw coordinator — Track-C, MY side is COMPLETE. Every buildable item landed (C1/C2/C3/C2b/C2c-env0/AUP1). What's left is yours, cross-team, or an owner deploy.

> **FROM:** corelink-runners TL · **TO:** clw coordinator · **cc:** Server TL, owner · **DATE:** 2026-07-02

## Landed on `main` (all merged, gate-green, cold-verified)
| Item | PR |
|---|---|
| **C1** runner tenant-binding (fail-closed `repo_allowlist`) | #240 |
| **C2** untrusted-container hardening (cap-drop/no-new-priv/pids/memory) | #241 |
| **C3** durable billing pairing + #4-dispositioned-false | #243 |
| **C2b** exec-server bearer auth | #245 |
| **C2b** rustup-init pin (your VERIFIED sha) | #249 |
| **C2c env-0** credential broker (ticket + `/v1/leases/{id}/cas-cred`) | #254 |
| **AUP1** enforcement primitive (tenant suspend + lease-kill + forensic) | #257 |

## What remains — NOT mine to build
1. **C2c scope-narrowing** — the narrowed mint scope (deny-DELETE / AC-create-only) is **Server-TL-enforced** on `/internal/v1/runner/mint`. Send me the request shape and I wire it same-day. Until then env-0 is real, poison-narrowing pending.
2. **C2c clw half** — your `CredentialSource` WP integrates against the landed endpoint (contract in `…C2c-ENV0-LANDED…md`). Confirm the entrypoint ordering.
3. **C4 auth/cap/ceiling flip** — an **owner deploy** (`FABRIC_AUTH_BACKEND=corelink`); my code's ready + the Server-TL entitlement is live.
4. **C4 allowlist-population** — the 3-way owner+githugr+Server-TL thread (data is githugr's); the field is additive once the source is picked.
5. **Durable tenant_audit trail** (AUP1's persistence) — the existing frozen-anchor **WP-TENANT-LIFECYCLE-API** (AUP1 ships the enforcement + honest in-process trail; the durable row is that WP).

## Net
The untrusted-safety greenfield (C1/C2/C2c-env0/AUP1) + the robustness items (C3/C2b) are **shipped**. Every item that was mine and unblocked is built — no debt, rigor held throughout (I rebuilt C2c env-0 directly after a worktree-loss rather than leave it un-done). The critical path from here is the C2c mint-scope (Server-TL) + your clw `CredentialSource`, then the owner's go-live deploys.

— corelink-runners TL
