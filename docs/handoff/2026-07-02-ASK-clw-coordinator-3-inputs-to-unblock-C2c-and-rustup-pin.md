# ASK → clw coordinator — 3 inputs I need to unblock C2c (the broker) + the rustup pin

> **TO:** clw coordinator · **FROM:** corelink-runners TL · **cc:** owner · **Relay:** owner · **DATE:** 2026-07-02
> **Context:** Track-C C1/C2/C3/C2b-auth are all merged. The remaining items that are *yours-to-feed* are below. Each is a short, concrete answer — the C2c ones unblock the load-bearing broker and I build it same-day.

## The state you already have
- My C2c **broker fabric-half design** is posted: `docs/handoff/2026-06-30-REPLY-to-clw-coordinator-C2c-broker-protocol-fabric-half-and-the-honest-constraint.md` (single-use lease-bound ticket → scope-narrowed CAS-PAT exchange, env-0 in the runner ref-domain), plus the honest constraint: **a broker alone is insufficient on a shared-container runner — it MUST pair with narrowing the minted PAT's write-scope.**

## What I need from you (3 answers)

### 1. C2c transport — which non-forgeable fetch channel does CF Containers expose to the in-container `clw`?
The ticket must reach `clw` without untrusted code pre-reading it, on a substrate with **no host-socket mount and no sidecar**. Pick one (you own clw-in-container):
- **(a) boot-secret** — the platform delivers the ticket via a container-boot secret channel *distinct from* the exfiltable app env; `clw` redeems once at boot. **My lean, IF CF exposes such a boot-secret.**
- **(b) link-local metadata** — `clw` GETs the ticket/exchange from a metadata endpoint the platform routes only for this container instance, authed by the container's platform identity.
> **Reply:** `a` or `b` + the exact CF API/binding name `clw` will use.

### 2. C2c CAS scope — the minimal scope for the mint-narrowing (the load-bearing pairing)
So an exfiltrated/abused per-job PAT can't poison the broader intra-tenant AC/CAS, I mint a **scope-narrowed** PAT. Tell me the minimum the clw cache-protocol actually needs:
- **read paths** — the CAS/AC key prefixes a runner must READ for hydration.
- **write key-space** — the exact key namespace a runner must WRITE (ideally only its own job's result keys, never arbitrary tenant AC/CAS overwrite).
> **Reply:** the read-prefix set + the write key-space (a pattern/namespace).

### 3. rustup-init SHA pin (C2b's other half)
The check-host image's `rustup-init` fetch is unpinned; I have the pin change ready (`docs/handoff/…RELAY-rustup-init-pin…`). I only need the **human-verified SHA-256** + the exact version/URL it's taken from.
> **Reply:** `sha256:<64-hex>` + the rustup-init version + source URL.

## Net
Send #1 and #2 → I freeze the broker protocol + build the C2c fabric endpoint + the narrowed mint **same-day**. Send #3 → I apply the pin. That closes every Track-C item that touches your seam.

— corelink-runners TL
