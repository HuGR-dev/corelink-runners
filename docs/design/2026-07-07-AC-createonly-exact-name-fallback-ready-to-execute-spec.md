# AC create-only — exact-name fallback: ready-to-execute spec (activate ONLY if the server gateway cannot do key-agnostic create-only)

> **From:** corelink-runners TL · **Date:** 2026-07-07 · **Status:** PRE-DECIDED, NOT built.
> This is the pre-decision for the *fallback* branch of the AC create-only fast-follow, so that IF the
> server TL replies "the CAS gateway can only express exact-key or domain-prefix, not key-agnostic
> create-only", the runner-side wire is a mechanical transcription, not a fresh design. It is written
> now (owner: "adianta tudo que puder") but **cannot be merged until the server accepts the seam field**
> — it touches the FROZEN `/internal/v1/runner/mint` request body, a cross-repo seam.

## When this activates (and when it does NOT)
- **Primary path (expected):** server confirms it can narrow `runner_job_ac_key` to **tenant-scoped
  create-only (deny-overwrite), key-agnostic** at the same chokepoint as deny-DELETE. Then **this whole
  doc is moot** — zero runner wire, nothing to build. (This is the likely outcome: deny-DELETE is already
  key-agnostic at that chokepoint, so create-only should be too.)
- **Fallback (this doc):** server says the gateway can ONLY do exact-key. Then the auto-derived `clw run`
  moat path CANNOT be exact-name-scoped (no name at mint time — the clw coordinator source-verified the
  moat write is a pure content-hash with no domain/name on the wire), so it stays key-agnostic and relies
  on deny-DELETE + tenant-scope. **Only jobs that DECLARE an output name at acquire time** get exact-name
  scoping via this wire. Opt-in, default-off.

## The wire (runner side)
1. **Acquire request** (`corelink-fabric-api::AcquireRequest`): add `ac_output_name: Option<String>`
   (default `None`). Present only for a check/agent lease whose caller knows the output name up front.
   Default-off: `None` ⇒ byte-identical to today.
2. **Fabric → server mint** (`/internal/v1/runner/mint`, the FROZEN seam — REQUIRES server TL sign-off):
   add `ac_output_name: Option<String>` to the request body. When present, the server derives
   `runner_job_ac_key = BLAKE3("clw/ref/runner/v1/" + ac_output_name)` (raw 32-byte digest, lowercase
   hex) and bakes it into the PAT scope; when absent, the server keeps the key-agnostic tenant-scoped
   create-only default. **The derivation is SERVER-SIDE** (the worker/fabric never handles the AC key).
3. **CF spawn-worker mint** (`deploy/cloudflare/src/lib.ts` `mintCasPat`): the `workflow_job.queued`
   webhook carries NO output name — the RUNNER path stays key-agnostic. So this wire is NOT added to the
   CF worker mint; it lives ONLY on the fabric-acquire path (check/agent leases). This is why the fallback
   only ever covers name-declaring leases, never the flagship `clw run` moat path.

## Derivation contract (byte-exact — from the clw coordinator)
- `AC_key = BLAKE3("clw/ref/runner/v1/" + name)` — the prefix and name are concatenated raw (no
  delimiter, no length prefix); output is the raw 32-byte digest, transmitted as lowercase hex.
- Domain is selected by `CLW_REF_DOMAIN=runner` on the clw write; the server matches the pre-image domain,
  not a hex prefix of the digest (a hashed key has no meaningful hex prefix).
- Round-trip verification (the acceptance gate before flipping on): `clw snapshot --name <X>` → read the
  AC key clw writes → assert `== BLAKE3("clw/ref/runner/v1/" + X)` in lowercase hex.

## Default-off proof obligations (when built)
- `ac_output_name = None` on every existing path ⇒ no behavior change (the mint body serializes without
  the field iff the server seam uses `skip_serializing_if = "Option::is_none"`; confirm with server TL so
  the frozen conformance vectors don't drift).
- A name-declaring lease scopes its cred to exactly `BLAKE3(prefix + name)` and is create-only; a write to
  any other key is denied server-side.
- The auto-derived `clw run` path (no `ac_output_name`) is UNCHANGED (key-agnostic + deny-DELETE +
  tenant-scope).

## Why it's not built now
The mint body is a FROZEN cross-repo seam ("FROZEN request body: NO owner_tenant"). Adding `ac_output_name`
requires the server TL to accept + consume it (and to update the byte-identical conformance vectors on both
sides). Shipping it unilaterally would break the drift tripwire. So this stays pre-decided until the server
answers the one open question in
`docs/handoff/2026-07-07-ASK-to-server-TL-AC-createonly-CORRECTED-shape-tenant-scoped-create-only-key-agnostic-domain-is-NOT-on-the-moat-wire.md`.

**Estimated runner-side effort once unblocked:** ~1 small PR (add the optional field to `AcquireRequest`
+ thread it to the mint body + a default-off test asserting absence ⇒ no wire change, presence ⇒ the field
is forwarded). The server-side derivation + enforcement is the server TL's PR.

— corelink-runners TL
