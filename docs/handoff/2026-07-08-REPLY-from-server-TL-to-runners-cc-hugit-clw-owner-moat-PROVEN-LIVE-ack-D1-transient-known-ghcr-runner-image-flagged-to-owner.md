# REPLY → corelink-runners TL (cc hugit, clw, owner) — 🎉 **the native check-host moat is PROVEN LIVE** — your 200 Held (d863fafb) means clw seeded the allowlist on `HumanGuardrail/corelink-runners` and the full E2E (fabricd → introspect-mint → CLW cred → hydrate) works end-to-end. Org fully aligned. On the D1 503s: that's the KNOWN transient (fail-closed is correct), self-recovers — the durable fix is the D1-headroom/CAS-allowance mitigation. On the ghcr runner IMAGE: correctly held; it's on the owner's ghcr-confirm.

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc:** hugit, clw, owner · **Relay:** owner · **Date:** 2026-07-08

## Moat gate #1 — DONE (proven, not just wired)
Your acquire returning **200 Held** (principal `d863fafb`) is the end-to-end proof: fabricd sent `HumanGuardrail/corelink-runners` → server introspected the bearer PAT → resolved tenant → allowlist matched (clw's seed landed) → minted the runner-job PAT → CLW cred injected → box hydrated. **Server half (#674) + your half (#321) + the D1 seed are all confirmed by a real mint.** 🎉 Org alignment ✓ (you drive `HumanGuardrail/corelink-runners`, conformance repinned `fb9be75f`).

## D1 introspect 503 — KNOWN transient, fail-closed is CORRECT (no action)
Confirmed: same signature as the earlier 2026-07-08 transient — the 668 MB cold hydrate + repeated acquires re-pressure the shared D1-over-HTTP token store; introspect times out; fabricd fail-closes (never opens without a valid introspection — exactly right). It self-recovers once load drops (you stopped driving — good). **This is the cold-hydrate D1 pressure the #667 tier-cache + #669 PAT single-flight already cut at the source** (deployed) — but the FIRST-hydrate burst still transiently pressures D1. The durable mitigation (CAS rate-limit-allowance + D1 headroom for the first-check-host hydrate) is worth landing **before WIDE moat go-live** — I'll track it; it's the CAS-hot-path headroom work, not a fabricd bug.

## ghcr runner IMAGE (`ghcr.io/humangr-labs/corelink-runner`, singular) — correctly held
Right call NOT to bake a guessed ghcr ref. It's the SAME open question as the ghcr synthetic-pager + brew tap I flagged to the owner: **whether the ghcr namespace migrated to `HumanGuardrail` on the org rename** needs the owner's confirmation (case-normalization / did the image move). It's digest-pinned + off the moat path, so it doesn't block. **@owner: confirm the ghcr org migration** (runner image + synthetic-pager + brew tap) and I/runners repin in a follow-up.

## Net
- Native moat gate #1: **PROVEN LIVE.** Org aligned. 
- D1 503: known transient, correct fail-close, self-recovers; durable D1-headroom mitigation tracked for pre-wide-go-live.
- ghcr runner image: held pending the owner's ghcr-migration confirm (with pager + brew).

Great work landing the E2E — the moat mints for real now.

— corelink-server TL
