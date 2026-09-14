# Story journeys — the real user, with state, end-to-end

The story-driven layer of the e2e suite. Where `journey/`, `security/`, `tenants/` hold isolated
probes (one request, one assertion), **journeys are narratives**: a named user story, run by a
persona (a real tenant PAT), as an ordered sequence of steps that thread state — acquire a real
lease, watch it become held, see it in the list, hit the cap, release it, watch the account settle.
A story that breaks mid-way is a failure, exactly as it would be for a user.

Each journey:
- **acts as a real user** — public `/v1` API, a real tenant PAT, real leases (a valid pinned image
  → a genuinely HELD lease that provisions a box);
- **threads state** across steps via `ctx` (lease ids, caps, observed values);
- **asserts the DEFINED behavior** at each step, not a bare status code;
- **captures the narrative** — `docs/validation/evidence/journeys/journey-*.json` records every step
  with its assertion + artifact;
- **always cleans up** — `onCleanup` closes every lease it opened, even on failure, so no real box
  is leaked.

## Run

```sh
scripts/e2e/run.sh journeys      # sources the tenant PATs, runs every journey live
```

## The stories (each maps to S-ids and exercises real combinations)

| Journey | Persona | Proves | Closes audit gap |
|---|---|---|---|
| Pro developer runs a job end-to-end | P1 Pro | full lease lifecycle: acquire → held → list → inspect → usage → close → released | — |
| Free tenant hits the concurrency ceiling | P1 Free | the cap is **ENFORCED** at admission (2nd acquire → 429), and the slot recycles on close | **F6** (value vs enforced) |
| A second tenant cannot see/touch the first's live lease | P7 attacker | cross-tenant isolation on a **LIVE foreign lease** (404, no oracle, no mutation, untampered) | **F2 / F3** (random-uuid tautology) |

More journeys are added the same way — one file per persona/theme, each a `new Journey(...).step(...).run()`.
The completeness-critic still enforces that every S-id maps to a coverage cell; journeys are the
richest way to satisfy that for the live, stateful stories.
