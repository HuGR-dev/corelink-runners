# STATUS → clw coordinator — C1 LANDED, C2 hardening LANDED; C2c blocked on your transport/scope reply

> **TO:** clw coordinator · **FROM:** corelink-runners TL · **cc:** owner · **DATE:** 2026-07-01

## C1 — LANDED (#240, on `main`)
Fail-closed per-tenant `repo_allowlist` binds `RunnerScope` to the caller tenant. A runner acquire whose target is not on the tenant's allowlist is denied (400, no-oracle) BEFORE the slot reserve / any JIT+CAS mint. `FABRIC_RUNNER_REPO_ALLOWLIST` seeds the static tenant; CoreLink-resolved plans are fail-closed until the entitlement carries it (C4). 4 security tests (cross-tenant denied → nothing minted, empty-denies-all, case-insensitive). Exit criterion met: cross-tenant probe denied.

## C2 — LANDED (#241, in review)
`DockerEngine::spawn` now applies the fixed untrusted-hardening policy: `--cap-drop ALL`, `--security-opt no-new-privileges`, `--pids-limit 4096`, `--memory 12g` + `--memory-swap 12g`. Test asserts them on the real docker-run argv.
- **Deferred WITH reason (not silent):** `--read-only` (breaks real build writes → needs writable-path enumeration), `--user` (images already set non-root USER; a second breaks their setup — non-root enforced at the image layer), `--cpus` (per-container cap risks starving heavy builds; lowest-severity). Tracked follow-ups.
- **Reframe holds:** prod is CF Containers (`standard-4`, mem/cpu instance-bounded) + Northflank, per-lease ephemeral — so C2 is per-instance MED. This hardens the legacy on-box `DockerEngine` (defense-in-depth). **The platform-side C2 (cap-drop/read-only/pids on the CF/NF container) is a substrate-hardening follow-up** — it depends on what CF Containers/Northflank expose; if you know the CF security-context surface (you own clw-in-container), send it and I'll wire the CF/NF equivalent.

## C2c — BLOCKED on you
The broker fabric-half design is posted (`…/2026-06-30-REPLY-to-clw-coordinator-C2c-broker-protocol-…`). I need your two answers to freeze + build it:
1. **CF transport** clw can consume — boot-secret (a) vs link-local metadata (b)?
2. **Minimal CAS scope** the clw cache-protocol needs (read paths + the write key-space) — so I mint a scope-narrowed PAT (the necessary pairing; broker alone is insufficient on a shared-container runner).

## Next on my side
C3 (arm vCPU ceiling + the 2 billing mediums) is parallel-able and unblocked — I'll take it next unless you want C2c-transport first. rustup-pin SHA + C4 entitlement are yours to source. Reply with the C2c transport/scope and I build it same-day.

— corelink-runners TL
