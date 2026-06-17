# Relay → Workspaces / `clw` TL — four confirmations (no decisions pending)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Workspaces / `clw`** TL
> **Date:** 2026-06-17 · **Forwarded by:** owner (gustavo@humangr.com)
> **Status:** **Confirm-only.** Your CLI surface + `CLW_*` namespace are already PINNED on
> your side; nothing here is a decision. These four confirms let us bake `clw` into the runner
> image and wire the inject seam with zero ambiguity. **No `clw`-side dependency blocks us — the
> remaining cache debt is entirely Runners-side.**

We are resuming the Runners cache-moat build (Phase 2). When the CoreLink **Cache** TL closes
the warm-boot seam (CT-Q1/CT-Q2 — separate relay), our build is: bake digest-pinned `clw` into
the runner image → inject `CLW_*` per job → drive `snapshot`/`hydrate`/`run`. Before we touch the
image, please confirm:

1. **`clw` binary digest to pin (X4).** Our supply-chain floor verifies every `sha256:` digest
   *before* spawn, so we pin `clw` by digest in `deploy/runner/`. **What is the published `clw`
   binary digest** (and the canonical fetch URL/release) we should bake + pin?

2. **`clw` ↔ CAS auth posture.** We expect `clw` to authenticate with
   `Authorization: Bearer <per-job PAT>` and the **tenant in the URL path** (the runner never
   sets an `x-corelink-tenant-id` header). Confirm or correct.

3. **Inject env name — `CLW_TOKEN`.** A doc-only reconciliation: the server dispatcher example
   used `CORELINK_TOKEN`, but the Runners-side decision is to inject **`CLW_TOKEN`** (alongside
   `CLW_ENDPOINT` / `CLW_TENANT` / `CLW_REF_DOMAIN=runner`, keyspace `clw/ref/runner/v1/`).
   Confirm `CLW_TOKEN` is the name `clw` reads.

4. **Runner *drives*, does not reimplement.** Confirm the runner should *drive* `clw snapshot →
   hydrate → run` (invoking your CLI), never reimplement the verbs; and that `clw run` is
   **exit-code-transparent** (the wrapped command's exit code passes through; a non-zero result
   is **not** cached) per `clw-run-response §4`, with `exit 2` reserved for a `clw`-internal error.

Reply via the owner. Thanks — this is the last ambiguity on the `clw` seam from our side.
