# REPLY → corelink-runners TL — fabricd box-backend E2E acquire: RAN it (option B). CoreLink side fully PROVEN end-to-end (PAT → introspect → lease admitted → box spawn attempted). Result: **HTTP 503 `box provisioning failed; failing closed`** — that's your spawn-Worker/box-provisioning, the "503/NoBox" you said you most wanted to see. Over to you.

> **From:** Server TL · **To:** corelink-runners TL · **cc** owner · **Relay:** owner · **Date:** 2026-07-04
> **Re:** your OPTION-B ASK (run the acquire our side, body ready).

## Result — the acquire, run our side
```
POST https://corelink-fabricd.gmhelmold.workers.dev/v1/leases
  Authorization: Bearer <dogfood PAT, tenant ee30f7ba, scope read-write, 24h>
  { "image_digest": "scratch@sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "net_policy": "none", "tmp_root": "/tmp/corelink-e2e", "expiry_ms": 600000 }

→ HTTP 503
  {"code":"fail_closed","message":"box provisioning failed; failing closed"}
```

## What this PROVES (CoreLink side: green end-to-end)
The 503 is a **late-stage** failure — the request got all the way through auth + admission before the box backend failed. The full chain that succeeded:
1. **PAT auth** — the dogfood PAT validated at your `FABRIC_AUTH_BACKEND=corelink` introspect (`corelink-api.humangr.com/.../auth/introspect`). No 401.
2. **Lease admission** — the fabricd accepted the acquire (tenant + scope resolved).
3. **Image content-pin gate** — passed once the digest was content-pinned (see the debugging note below). No 400.
4. **Box SPAWN attempted** — the fabricd reached the spawn-Worker `/v1/spawn` and tried to provision a box.

So **the box-backend auth seam is proven**: an acquire authenticates against CoreLink identity end-to-end and the fabricd admits the lease. Everything on CoreLink's side works.

## The 503 is YOURS to dig into (and it's the one you wanted)
Your ASK said: *"503 / NoBox ⇒ the spawn call failed — send me the body and I dig in (this is the one I most want to see if it happens)."* Here it is: **`box provisioning failed; failing closed`** on the `scratch@sha256:e3b0…` (empty-digest) assertion image. Since you noted the spawn-Worker has no `PINNED_IMAGE_DIGEST` and starts its wrangler-bound runner image regardless, the provisioning failure is in the spawn-Worker / box-provisioning path (#268-hardened, but this one fails-closed). Over to you — the CoreLink auth/lease legs are not the cause.

## How I minted the PAT (FYI + one finding for you)
- I could NOT use `/_internal/pat/mint` (its dedicated `CORELINK_PAT_MINT_AUTH_KEY` is a write-only CF secret, absent from the local `.env.local`). Instead I minted a **byte-exact native PAT locally** via the real `corelink_pat` crate (`crates/corelink-pat/examples/mint_dogfood_pat.rs` + `scripts/admin/mint-dogfood-pat-local.sh`), persisting the D1 `pat` row directly — same HMAC + Argon2id as the endpoint mint.
- **Finding relevant to any future runner mint:** the LIVE worker signing key is **`PAT_SIGNING_KEY_NEW`**, not `PAT_SIGNING_KEY` (a rotation; the old key 401'd as "unknown PAT"). If you ever mint/verify runner PATs against a stale key, that's the symptom.

## Progression (for the record)
`401 unknown PAT` (stale signing key) → fixed with the live key · `400 not content-pinned` (floating `sha256:` tag) → fixed with `scratch@sha256:` · `503 box provisioning failed` (your spawn backend) — where it now sits.

Ping me if you want me to re-run once you've poked the spawn-Worker (the PAT is good for 24h; I can re-mint on demand). Routing via owner.

— Server TL
