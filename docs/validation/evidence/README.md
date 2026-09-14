# Evidence ledger

Captured runs of the e2e validation suite — the G2 deliverable: every cell as
`atom → grade → cited artifact` (stimulus + behavioral assertion + captured result).

- Each run is a directory `<YYYYMMDD-HHMMSS>/` with one JSON per cell + an `index.json`.
- A cell is "pass" on its **behavioral** assertion, never a bare status code.
- Runs are reproducible: `scripts/e2e/run.sh ts2` (etc.). Raw run dirs are git-ignored by
  default (`.gitignore`); milestone runs are committed deliberately with `git add -f` as the
  cited proof-of-record.

Committed milestones (post evidence-audit, hardened assertions — see
[`../EVIDENCE-AUDIT-2026-07-19.md`](../EVIDENCE-AUDIT-2026-07-19.md)):
- `live-ts2/` — TS-2/TS-5 no-spawn probes (7/7): front 200, ed25519 32-byte pubkey verified,
  auth fail-closed w/ discriminating control, clean 4xx (parser-echo tracked), non-leak sweep
  (corelink_ regex, ≥4 bodies).
- `live-ts5/` — TS-5 security gates (5/5): /internal 401 (positive-control gap noted), fake vs
  real /v1 route byte-identical 401 (no oracle), forged ticket → 401 "invalid ticket",
  unknown-lease 404 (cross-tenant oracle deferred), non-leak sweep (≥5 bodies).
- `live-ts6/` — TS-6 multi-tenant (3/3): plan_cap value per tier (enforcement deferred),
  distinct tenant identity, usage-API reads echo the caller's tenant.
- `door-a-29696076686/` — Door-A E3: a real `runs-on: corelink` job ran on a **freshly-spawned
  CF ephemeral box** (`cf-runner-40d8276b` on `cloudchamber`), success. cache-warm hit and
  teardown recorded as observed-or-not (`false`), never assumed.
