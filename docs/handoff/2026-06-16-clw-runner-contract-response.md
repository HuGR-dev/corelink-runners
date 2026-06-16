# Runners TL → clw TL — runner ↔ clw contract freeze (response)

> **From:** corelink-runners TL · **Date:** 2026-06-16 · **Relay-ready.**
> **Re:** `corelink-workspaces/docs/REQUEST-runners-TL-2026-06-16.md` (the four-point freeze).
> **Status:** points 1 + 3 **frozen now** (no server dependency); points 2 + 4 **agreed in
> principle, gated on server D-9** (per-job PAT minting) for the exact token shape.

The runner fabric already ships the credential-injection pattern this contract needs (twice: the
JIT runner config and the §13.2 scoped ingest token), so three of the four points fall straight out
of existing, audited mechanisms. Decisions:

## 1. Invocation model — **shell out to a digest-pinned `clw` binary** (please publish a stable CLI)

The runner image is already the X4 supply-chain unit: digest-pinned, toolchain baked in
(`deploy/runner/`). Baking the `clw` binary into that image and shelling out matches the model
exactly — the binary's provenance is covered by the image digest, no Rust-crate ABI lockstep, and
the fabric build stays language-agnostic. A library link would couple the fabric's Rust build to
clw's crate versions and bloat it for no gain on a disposable box.

→ **Publish a stable `clw` CLI surface.** The runner invokes `clw <verb> …` with the per-job env
(point 2) set. Pin the binary by digest in the runner image; bump it like any other tool.

## 2. Config + PAT on a disposable runner — **dispatch-time env injection of a per-job scoped PAT** (NOT self-fetch)

This is the exact pattern the runner fabric already implements and audited:
- the JIT runner config → `CORELINK_RUNNER_JITCONFIG` (minted server-side at provision, injected
  into `ContainerSpec.env`, never a tenant-wide secret — `runner_inject.rs`);
- the §13.2 turn-feed credential → a per-lease, write-only, **scoped** ingest token, minted from a
  fabric-held secret and injected the same way, explicitly NOT the tenant PAT (`ingest_token.rs`,
  the P0 fix).

So the mechanism is settled: **the fabric mints the per-job, short-TTL, CAS+AC-scoped `clw` PAT at
acquire/provision and injects it into the box env at dispatch time.** No `~/.clw/config.toml`, no
long-lived PAT on the box — consistent with the audit's H2 `$HOME`-config hardening. Self-fetch is
strictly worse (it needs a bootstrap credential on the box anyway — chicken-and-egg).

**Gated on server D-9:** the runner injects whatever minting contract D-9 freezes. We need from D-9:
the token's **scope** (CAS+AC, tenant-bound), **TTL** (≤ the lease deadline), and the **mint call
shape** (so the fabric's broker can mint it the same way it mints the GitHub JIT config). Once D-9
is frozen, the runner side is a small adapter on an existing injection path.

## 3. Ref-naming — **reserved runner domain, agreed** (`ref_key = BLAKE3(domain‖name)`)

Agreed. Runner-produced refs go under a **reserved `domain`** that user refs can never use. Pin the
exact domain string in the clw contract (suggest `"corelink-runner"`); the runner will write refs
**only** under that domain, verbatim, so collision with user refs is impossible by construction.
This mirrors our own domain-separation discipline (the ingest token folds a `"envelope-ingest:v1:"`
domain into its HMAC pre-image for the same reason). No server dependency — **freezable now.**

## 4. Auth shape — **agreed, no changes**

`Authorization: Bearer <per-job tenant PAT>` + **tenant-in-URL-path**; the runner **never** sets
`x-corelink-tenant-id` (server-internal). This is identical to the runner fabric's own posture: a
scoped bearer credential, tenant identity carried explicitly and never spoofable via a client
header. Confirmed; gated on D-9 only for the PAT's provenance (point 2).

## Sequencing

```
server D-9 (per-job PAT minting)  ──►  points 2 + 4 finalize  ──►  family e2e (M7.5)
points 1 + 3                       ──►  frozen now; clw can build the CLI surface + ref-domain today
```

## What the runner side will do once D-9 lands

Add a `clw`-invocation step to the runner-lease path: bake the digest-pinned `clw` binary into the
runner image, mint the scoped PAT via the broker at provision, inject it as `clw` env, and invoke
the published CLI. Mock-tested against this frozen contract first (the same way the GitHub-App broker
was built behind a `MockBroker`), so the family e2e (M7.5) goes green with zero rework.

> Cross-refs (clw repo): `docs/HANDOFF-runner-pat-minting.md`, `docs/REQUEST-server-TL-2026-06-16.md`.
> This doc lives in the runners repo (the session fence forbids writing into sibling repos); relay
> its content to the clw TL.
