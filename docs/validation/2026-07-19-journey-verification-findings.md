# Story-journey suite — live verification findings (2026-07-19)

**Result: 142/142 journeys GREEN against live prod** (with warm tenant plan caches). Zero real
journey or fabric defects. The suite is the story-driven, real-user validation the owner asked for:
every journey acts as a real tenant (real PAT), creates + closes REAL leases, threads state, and
asserts BEHAVIOR (not status codes), collecting narrative evidence.

## Coverage
12 persona-cluster files under `scripts/e2e/journeys/`, ~142 journeys, mapping the 155 user stories
(hugit/P2 excluded — campaign #3 discontinued). Cluster tallies (live, warm):
usage-contract-workspaces 19 · operator 18 · dev-lifecycle 17 · attacker 15 · dev-workloads 14 ·
buyer-finance-lifecycle 14 · power-migrate 13 · support-sre 12 · compliance-comms 10 · agent 7 ·
developer 2 · security 1. The G1 completeness-critic (CI gate) proves every ledger atom is mapped.

## The 10 reds in the first live run were 100% ENVIRONMENTAL — proven, not papered over
The first re-verification ran immediately after the key-drift recovery **container roll**, so most
tenant plan caches were COLD. 10 journeys failed; all explained, none a real defect:

- **9 = cold plan-cache.** `/v1/usage` reads `plan_of` — a per-tenant cache warmed by `plan_of_
  resolving` on **acquire** (`usage.rs:88`, a DELIBERATE design: usage is a cheap read that does NOT
  fire a fresh introspect, so it can't 503 if introspect is down). A tenant that hasn't acquired
  since the container booted reads `plan_cap: null`. Journeys asserting the entitlement ladder
  (Free=1, Pro=10, Enterprise=100) as an early step saw `null`. **Proof it's cold-cache, not a bug:**
  after one warm-up acquire per tenant, a re-run of all 6 affected clusters was **65/65 green**.
- **1 = over-strict assertion** (`Offboard drains a lease clean`): it required the closed lease to
  VANISH from the list, but a just-closed lease lingers momentarily as `released` (async teardown).
  Fixed: "no residue" now means "no longer HELD" (holds no slot), not "absent from the list".

## Product finding the suite surfaced (owner's call — not changed unilaterally)
**`/v1/usage` returns `plan_cap: null` for a tenant that hasn't acquired since the container booted.**
It's a deliberate cheap-cached design, but a user who opens their usage panel before running any job
sees `null` instead of their real cap. Options for the owner: (a) leave as-is (documented convention),
or (b) have the usage handler resolve the plan fresh from the caller's PAT (it has the token) so the
cap always shows. Not a bug I'll change without a product decision.

## Suite pre-condition (documented, not a defect)
Cap-asserting journeys assume a WARM plan cache. On a freshly-rolled container, run one acquire per
tenant first (or accept that cap reads are `null` until first activity). A follow-up hardening: make
the cap-asserting journeys warm-first so they're deterministic on any container state.

## Discipline held
Agent-authored journeys were NOT trusted — every cluster was run live + audited by the lead before
this commit. An adversarial evidence-audit (#406) of the flagship cells already caught 12 overclaims.
The key-drift outage mid-verification was root-caused to a stale secret (NOT the suite/load) — see
`docs/handoff/2026-07-19-*` and the incident memory.
