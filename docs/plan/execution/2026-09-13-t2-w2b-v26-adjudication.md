# T2-W2b provider-v26 adjudication — 2026-09-13

Decision authority: root TechLead/PO decision recorded under the user's explicit autonomy. This is an adjudication record, not a human waiver and does not forge an owner signature.

The historical Fabricd provider version 26 observation is incidental and non-normative. The application disappeared during the `--containers-rollout=none` incident and was recreated as a new provider-v1 application, so the old app identity/version cannot be recovered or treated as the current deployment. The v26 record remains quarantined and grants no acceptance, production, or T2-W2b credit.

The normative acceptance rows require the following, without requiring a provider version number: A2.4 requires a real spawn-worker deploy with its Worker version recorded; A2.5 requires the immutable Fabricd digest, build SHA at or after #515, and the running instance reporting that digest; A2.7 requires the documented CI-path deploy and health response; A2.10 requires rollback to the prior recorded version and a responding version ([golive remediation plan](../2026-08-30-golive-remediation-plan.md), lines 561–567).

After AU1.8 or any Fabricd app recreation, the current Fabricd app UUID must be resolved and recorded. A pre-AU or replaced app ID is rejected even if its digest/build match. The recovered app identity, immutable digest, build provenance, running-instance digest and health are therefore acceptance subjects only. T2-W2b remains RED/partial until one fresh version-bound production witness matrix passes: 10 independent cold-start candidate runs and 10 rollback runs completed within 300 seconds, with every matrix input and evidence bound to that current app UUID and the Worker version, Fabricd digest/build, running-instance digest, health and rollback evidence. Missing production evidence is not waived, and no PASS is claimed now.

Evidence relied upon:

- The original one-shot v26 observation is explicitly historical/superseded and says it cannot satisfy A2.4/A2.5/A2.7/A2.10 ([final one-shot evidence](../evidence/T2-W2b-deploy-final-20260908.md), lines 1–16).
- The recovery record documents the skipped container application creation, recreated provider-v1 app, and the unrecoverable v26 lineage ([incident recovery](../evidence/T2-W2b-incident-recovery-20260908.md), lines 3–9).
- The earlier rollout record documents the `--containers-rollout=none` failure and rollback recovery ([rollout evidence](../evidence/T2-W2b-rollout-20260908.md), lines 3–9).

Validation scope: targeted `python3 scripts/dev/delivery-ledger.py --check` and `python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt` only. No source, provider, PR, secret, `.hugit`, or Githugr changes are authorized by this record.
