# T6-W9 named rule/channel matrix

This freezes the matrix required by A6.18 in the round-3 remediation delta.
The executable contract is `deploy/cloudflare-canary/src/critical_rule_matrix.ts`,
also exported by `rules.ts`. T6-W12 owns the external detectors, authenticated
ingestion, incident state and delivery that consume it. The existing Cloudflare
canary does not become an independent C1–C5 detector through these exports.

| Pillar | Rule | Classified conditions |
|---|---|---|
| C1 | `control-plane-unavailable` | lifecycle/readiness failure, breaker open, missing source |
| C2 | `deployment-verification-failed` | deployment failure, unverified version, rollback failure, missing expected result |
| C3 | `job-lifecycle-stuck-or-leaked` | queued/spawn/active/release stuck, resource leak |
| C4 | `usage-accounting-divergent` | missing ledger/provider usage, divergent usage |
| C5 | `customer-journey-failed` | signup/payment/installation/green-job failure, missing scheduled journey |

All five rules route to the named logical channel `external-primary-page`.
O-MONITORHOST must bind that channel to its approved external transport and
credential domain. `routeCriticalCondition(pillar, condition, bindings)` requires
a nonempty route reference and rejects unknown/cross-pillar conditions. Its
output contains rule, condition, channel and route reference, never credentials.
The function routes facts already classified by the T6-W12 detectors; it does
not decide provider truth, thresholds, trust, freshness or observation schedules.

The focused contract test covers every listed condition and rejects missing
channels and invalid mappings. These are local routing fixtures. They do not
prove an external runtime, channel ownership, incident receipt, page ACK or SLO.
A6.12/A6.14 end-to-end evidence still requires the actual deployed T6-W12 routes
and the canonical later live proofs; this matrix grants none of that evidence.
