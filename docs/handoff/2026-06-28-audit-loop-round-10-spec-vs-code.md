# Autonomous audit loop — Round 10 (SPEC-vs-CODE / overclaim, 2026-06-28, ~08:55 local)

The CLAUDE.md "tense-discipline / no-overclaim" angle: do the whitepaper / product / pricing / contract / ADRs / READMEs / in-code doc-comments match the actual code? 3 Opus + 5 Sonnet → adversarial verify. **16 confirmed (6 medium, 10 low), 9 refuted.** All doc-accuracy (overclaim / drift / staleness) — no code defect. This round caught real **customer-facing** liabilities the code-logic rounds could not.

## FIXED (this PR) — the high-liability overclaims
| Sev | Finding | Fix |
|---|---|---|
| med | **The BANNED "cross-tenant dedup ✅ LIVE" overclaim** survived in `corelink-techlead-onboarding.md:33` (the 2026-06-09 review fixed the whitepaper + product.md but missed this file). | Qualified: "intra-tenant dedup at GA, cross-tenant STAGED [CAP-DEDUP-CROSS-TENANT]". |
| med | **README pricing table showed the SUPERSEDED pre-amendment ladder** ($8/$20/$50/$100/$200) — ~6× underwater at the real $0.10/vCPU-h basis — under a present-tense "**impossible to lose money**" guarantee. The ratified 2026-06-16 ladder + the code (`plans.rs`) have the corrected numbers. | README pricing replaced with the ratified ladder ($16/$40/$100/$200/$400, ceilings 100/240/600/1200/2400, COGS $10/$24/$60/$120/$240) + a "superseded" note. |
| med | **The vCPU-h ceiling claimed LIVE/structural** in `pricing.md §3` + the README, but it is **default-off** (`FABRIC_RUNNER_VCPU` unset ⇒ no ceiling) — self-contradicting `pricing.md §0 item 3` ("not yet enforced"). | Qualified the guarantee in the README + `pricing.md §3` ("once the wall is ARMED via `FABRIC_RUNNER_VCPU`"); updated `§0 item 3` (the wall SHIPPED, default-off). |
| low | README "**Seven crates**" — the workspace has **eight** (`corelink-check-exec-server` added). | Corrected to "Eight crates". |

## TRACKED (doc-accuracy follow-ups — low, internal, non-customer-facing)
- `northflank-postgres-runbook.md` calls `FABRIC_PG_TLS` a "landing work-pack" — the opt-in PG TLS shipped (stale tense).
- `ledger.rs:131-156` — a CP1-invariant doc-comment mis-attached to `ComputeGate` (code-comment drift; the round-7/10 comment sweeps catch these).
- `CLAUDE.md`: "182 tests" (outdated snapshot), integration-contract "v1.2.0" (the file is v1.4.0) — internal instruction-file drift; left for an owner-curated CLAUDE.md refresh (editing the canonical instructions warrants owner care).
- `contract` pins the result_binding_v2 vector to a specific sha (v1.4.0) — verify the vector matches.
- Aspirational/spine claims (whitepaper memoization "re-run costs ~0", "ephemeral microVMs" spine; interop credential-scan "live guarantee") — verifier rated **low**: largely the vision/spine framing; the substrate today is Cloudflare Containers (not microVMs) and the memo path is built default-off. A pass aligning the whitepaper's present-tense to the staged reality is a pre-public-material polish.

## Status
The customer-facing financial + the banned-dedup overclaims are FIXED (the load-bearing CLAUDE.md liabilities). The remaining drifts are internal/low doc-accuracy items, tracked. No code defect — the docs over-stated; the code is the more conservative truth.
