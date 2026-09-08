# Human-authorized waivers

This file records narrow, explicit waivers required by the go-live plan. A waiver
changes only the named execution mechanism or deferral; it never converts missing
production evidence into a PASS or removes an acceptance outcome.

WAIVER (human-authorized) — T2-W4 GitHub Actions hosted-execution mechanism
  authorized-by: gustavomalleths@gmail.com | 2026-09-08
  reason: GitHub billing is unavailable to the owner at this time. The owner explicitly authorized equivalent local Docker-based CI and bundle merges without GitHub Actions execution.
  remediation: Run the documented local CI substitution for the complete Sprint 2 gate while billing is unavailable; retain the real T2-W4 acceptance outcomes in full: A2.8 requires the rebuilt and published image plus a job booting on the new digest, and A2.9 requires 3/3 real fixes reaching verified deployed versions within 15 minutes of merge. Re-run the hosted workflow when billing is restored. | tracking: T2-W4 · A2.8 · A2.9 · Sprint 2
