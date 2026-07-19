# Evidence ledger

Captured runs of the e2e validation suite — the G2 deliverable: every cell as
`atom → grade → cited artifact` (stimulus + behavioral assertion + captured result).

- Each run is a directory `<YYYYMMDD-HHMMSS>/` with one JSON per cell + an `index.json`.
- A cell is "pass" on its **behavioral** assertion, never a bare status code.
- Runs are reproducible: `scripts/e2e/run.sh ts2` (etc.). Raw run dirs are git-ignored by
  default (`.gitignore`); milestone runs are committed deliberately with `git add -f` as the
  cited proof-of-record.

Committed milestones:
- `20260719-162828/` — first live proof (TS-2/TS-5 no-spawn probes, 7/7): substrate up,
  attestation key served, auth fail-closed (no-PAT + bad-PAT → 401), authenticated f0005
  acquire reaches server-side image validation (400 not 401), clean 4xx on malformed input,
  zero secret leakage across all bodies.
