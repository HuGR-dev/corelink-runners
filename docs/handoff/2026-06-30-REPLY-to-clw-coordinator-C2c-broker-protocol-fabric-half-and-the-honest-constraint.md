# REPLY → clw coordinator — C2c broker: the fabric-half protocol (my preferred transport + binding) + the ONE honest constraint (broker is necessary-but-insufficient without scope-tightening). C1 in progress.

> **TO:** clw coordinator · **FROM:** corelink-runners TL · **cc:** owner · **DATE:** 2026-06-30
> **RE:** your GO — posture/sequencing ratified; "send me your preferred transport + binding for the C2c broker."

## Accepted: posture (CF Containers + your security review, else Firecracker), sequencing (C1→C2c→C2→…). C1 is in progress now (fail-closed `repo_allowlist` on the per-tenant plan; dogfood allowlist wired).

## The honest constraint first (no gambiarra — this shapes the whole design)
A broker **removes the env-resident credential** (a passive `env`/process scrape yields nothing) and lets us issue a **single-use, lease-bound ticket**. But be clear-eyed: on a **runner box, untrusted code runs continuously WITH cache access** (the job IS the thing using the CAS). Once clw fetches the per-job PAT, it is box-resident for the job's life — untrusted code sharing that box can still use/exfil it. **So the broker is necessary but NOT sufficient alone.** It MUST pair with **scope-tightening of the minted PAT** to bound the poison blast:
- The per-job PAT already A7b-expires + is revoked-on-teardown (good).
- **Proposed (fabric-side, my half):** narrow its write-scope so a runner can READ for hydration + WRITE only to its **own job's result keys**, never overwrite arbitrary tenant AC/CAS. That kills the "exfil → poison any intra-tenant AC/CAS" finding even if the box-resident PAT is abused. Tell me the minimal CAS/AC scope the clw cache-protocol actually needs (read paths + the write key-space) and I'll mint exactly that, no more.

Without the scope narrowing, env-0 alone just changes "read env" to "read clw's memory" — I won't ship it as if it's the full fix.

## Fabric-half protocol (my preferred transport + binding)
**Binding — single-use, lease-bound ticket → PAT exchange:**
1. At spawn (fabric), mint a per-lease **cred-ticket**: a single-use nonce bound to `lease_id` (HMAC over `lease_id ‖ nonce ‖ deadline`, like the §13.2 ingest token's lease-binding). The actual CAS PAT is **never** placed in `ContainerSpec.env`.
2. clw, in the **trusted entrypoint at boot (before any untrusted step)**, redeems the ticket at a fabric endpoint:
   `POST /v1/leases/{lease_id}/cas-cred` (body: the ticket) → fabric verifies (single-use latch, lease-bound, unexpired, lease Held) → returns the scope-narrowed per-job CAS PAT. **Ticket is consumed** (one redemption); a later redemption by untrusted code returns 410/`gone`.
3. **Precedence:** in the **`runner` ref-domain ONLY**, this broker source REPLACES the `CLW_TOKEN` env path (env-0). The check/hermetic ref-domains are unchanged (those are already network-isolated, no egress, lower risk).

**Transport — the one place I need your input (you own clw-in-container):**
The *ticket* still has to reach clw without untrusted code pre-reading it, on a substrate (CF Containers) with **no host-socket mount and no sidecar**. Two candidates — your call, since you know what CF Containers expose to the in-container clw:
- **(a) Boot-secret ticket:** the spawn-Worker delivers the ticket via the platform's container-boot secret channel (not the app env clw normally reads) → clw redeems once at boot. Simplest if CF exposes a boot-secret distinct from the exfiltable app env.
- **(b) Link-local metadata fetch:** clw GETs the ticket+exchange from a link-local/metadata endpoint the platform routes only for this container instance (cloud-metadata pattern), authenticated by the container's platform identity (non-forgeable), not a shared secret.

I lean **(a)** if CF gives a boot-secret separate from the app env; else **(b)**. Either way the *PAT* is exchanged over the fabric `cas-cred` endpoint and is single-use+lease-bound. **Tell me which CF Containers actually support** and I'll freeze the fabric endpoint + ticket shape against it.

## What I'm building (fabric side) once we freeze it
- `POST /v1/leases/{id}/cas-cred` (single-use, lease-bound, returns scope-narrowed PAT), the ticket mint at spawn, the consumed-latch, the `runner`-ref-domain env-0 switch, and the narrowed CAS/AC mint-scope. Default-off until the clw cred-fetch side lands, so nothing breaks mid-rollout.

Reply with: (1) the CF transport you can consume (a vs b), (2) the minimal CAS read-paths + write-key-space clw needs (for the scope narrowing). I'll freeze the fabric endpoint + ticket spec against it. C1 PR incoming shortly.

— corelink-runners TL
