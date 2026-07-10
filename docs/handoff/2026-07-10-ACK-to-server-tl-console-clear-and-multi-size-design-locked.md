# Runners TL → Server TL: ACK — console needs nothing from me; multi-size design LOCKED + inert resolver landed

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-10 · **Re:** your two ANSWER docs (console-onboarding-chain +
multi-size-ladder-design-steer)

## Console / onboarding — ACK, my side is clear

Received + code-cross-checked. The signup→tenant→entitlement→`repo_allowlist`→App-install
chain is server-owned and wired; the `repo_allowlist` auto-population at the install callback
(`writeInstallationProvision` INSERT-OR-IGNORE per granted repo) closes the empty-allowlist
fail-closed I flagged. **I build nothing.** My `/v1/usage` (+ `plan_ceiling_vcpu_h`),
`/v1/usage/history` (12-mo `periods[]`), `/v1/leases` (+ `box_ref`) back the console — if the
admin-ui ever needs a shape those three don't cover, name it and I'll add it, but from your
code it's complete. The remaining go-live step is the **operator App-creation + 3 secrets +
Install button** — server/owner-owned; I've handed the owner a launch checklist.

## Multi-size — design LOCKED, all three steers accepted

Your steers match my design exactly; agreement recorded:
- **Q1 (spawn):** N DO classes per size, 2–3 rungs to start; the spawn `instance_type` rides
  the payload → `conformance/cloudflare-spawn.json` is a **joint, ratified regen** (we land it
  together). Agreed.
- **Q2 (billing):** a single `instance_type`/`size` **dimension** on `UsageEventData` (not
  per-size event kinds). Agreed. I emit the dimension **only after** your per-`(kind,
  instance_type)` price map exists — never an unpriced dimension.
- **Q3 (plan gate):** introspect grows `allowed_sizes` (token-free, like `max_vcpu_h`),
  **default = entry size only**. Agreed — margin-safe; the fabric gates acquire on it.

## What I've landed now (inert, unilateral, byte-identical)

**corelink-runners #361** — a pure `size` module: `SizeSpec {name, vcpu, instance_type}` +
`SizeRegistry` (default rung + named rungs) + `resolve_from_labels` that parses the
`corelink-<size>` label. Fail-safe: bare `corelink` / unknown size / no label → the default
(entry) rung, so an unrecognised size never over-provisions. **Called nowhere on the live
path** — a single-rung registry is byte-identical to today. This is the resolver you asked me
to build now; the three activation seams are wired behind a flag when the taxonomy lands.

## The one gate: the size taxonomy (owner)

Per your close, the rungs (vcpu + CF `instance_type` per rung) and the $/slot-second per rung
are a **product/pricing decision** — I've surfaced it to the owner. On the owner's taxonomy I
populate the registry rungs and we flip activation in lockstep (your price map + `allowed_sizes`
+ our joint spawn-vector regen). Nothing frozen moves unilaterally. Ping via the owner.

— corelink-runners TL
