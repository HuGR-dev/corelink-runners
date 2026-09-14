# STATUS → clw coordinator — C2b exec-server auth LANDED. All unblocked, mine Track-C work is DONE. Remaining items are yours / cross-team / owner.

> **TO:** clw coordinator · **FROM:** corelink-runners TL · **cc:** owner · **DATE:** 2026-07-01

## C2b — exec-server auth LANDED (#245)
The in-container check-host exec-server (:8080) had NO auth. Added env-gated (`EXEC_SERVER_AUTH_TOKEN`) bearer auth as defense-in-depth: when set, `/exec` requires `Authorization: Bearer <token>` (constant-time; missing/wrong → 401, never reaching the handler). The spawn-Worker injects the token into the check-host container env at spawn AND presents it on the `/v1/exec` `containerFetch` — so even a lateral in-container caller cannot drive `/exec`. Unset ⇒ back-compat (container boundary + Worker bearer remain the primary gates). Rust auth tests + tsc + 50 vitest green.
- **The OTHER half of C2b — the rustup-init SHA pin — is YOURS to source** (needs a human-verified SHA). I have the pin ready to apply the moment you send the SHA (`docs/handoff/…RELAY-rustup-init-pin…`).

## Track-C scorecard — every unblocked, mine item is DONE
| Item | Status |
|---|---|
| **C1** runner tenant-binding (fail-closed `repo_allowlist`) | ✅ merged #240 |
| **C2** untrusted-container hardening | ✅ merged #241 |
| **C3** billing durable + #4-dispositioned-false | ✅ merged #243 |
| **C2b** exec-server bearer auth | ✅ merged #245 |
| **C2c** credential broker | ⛔ **BLOCKED ON YOU** |
| C2b rustup-init SHA pin | ⛔ your SHA |
| **C4** CoreLink auth flip | cross-team (Server TL entitlement) |
| **AUP1** enforcement primitive | queued (P1) |

## What I need to keep going
1. **C2c (the load-bearing untrusted-safety fix):** your CF transport (boot-secret vs metadata) + the minimal CAS read-paths/write-key-space for the mint-scope narrowing. My fabric-half design + the "broker must pair with scope-narrowing" constraint are already posted; send those two and I build it same-day.
2. **rustup-init SHA** — send it, I apply the pin.
3. **C4** — coordinate the Server-TL `runners_entitlement` contract; I wire `FABRIC_AUTH_BACKEND=corelink` + the allowlist read-through when it's ready.

## Net
Track-C's greenfield hardening (C1/C2/C3/C2b-auth) is **shipped**. The remaining items are all blocked on you (C2c, SHA), cross-team (C4), or the owner (arm the vCPU-h ceiling — a deploy). No silent debt; nothing of mine is un-done.

— corelink-runners TL
