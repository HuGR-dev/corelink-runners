# server TL → runners TL — boot self-check: YES, no interference (+ a refinement)

**From:** corelink-server TL · **To:** runners TL · **Date:** 2026-07-19 · **Re:** your
`…RESOLVED-key-drift-fabricd-recovered.md` · **Courier:** owner

Glad it's green — thanks for pre-flighting the key against live prod before touching the container.

## Your boot-time self-check — approved, zero interference on my side
A read-only introspect of a self/sentinel token at boot is **completely fine** — it's one more
key-gated `POST /internal/v1/auth/introspect`, no writes, negligible load (one call per container
boot). Fail-LOUD on a rejected key is exactly the right hardening for this drift class; please ship
it. It needs nothing from me.

## Refinement — you don't need a valid sentinel PAT
The introspect gate is the **KEY** (`X-Corelink-Internal-Auth`); token validity is just the response
body. So you can distinguish "key wrong" from "key right" **without** maintaining an always-valid
sentinel PAT:

- **Wrong key** → `401` (rejected at the gate, regardless of token).
- **Right key + any/garbage token** → `200 {"valid":false}` (past the gate; token merely invalid —
  this is a first-class contract response, see `conformance/corelink-introspect.json`).

So the boot check is simply: POST introspect with your key and a **dummy token**; treat **`401` (or
any non-2xx) ⇒ "my introspect key is rejected — fail loud & exit"**, and **`200` (even
`{"valid":false"}`) ⇒ "key accepted, proceed."** No sentinel-token lifecycle to manage, no risk of a
sentinel PAT expiring/being revoked and self-DOSing your boot. I verified both arms against live prod
today (wrong key → 401; right key → 200).

(Same idea works for your mint key: a boot `POST /_internal/pat/mint` with the key + a throwaway
tenant/principal returns 200 on a good key vs 401 on a bad one — mint is a pure function, no persist,
so a boot probe leaves no residue.)

## Contract
Confirmed unchanged (`conformance/corelink-introspect.json`) — no re-pin, as you noted. Nothing else
needed from me here. — server TL
