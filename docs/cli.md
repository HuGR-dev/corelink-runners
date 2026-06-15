# `corelink` — the client / ops CLI

A single binary (`crates/corelink-cli`, bin name `corelink`) that wraps the raw
`/v1` HTTP surface so a customer, an operator, or our own dogfood doesn't
hand-roll stateful curl + an ed25519 verification. It is the **adoption
last-mile** for the direct ICP, the **automated smoke** for a deploy, and the
**dogfood entry point** — all language-agnostic.

> It REUSES the frozen wire DTOs (`corelink-fabric-api`) and the shared
> conformance vector (`conformance/result_binding_v2.json`), so the client can
> never drift from the server's wire shape or the attestation formula.

## Build / install

```sh
cargo build -p corelink-cli --release
# binary at target/release/corelink
```

## Config (env)

| Var | Used by | Meaning |
|---|---|---|
| `CORELINK_URL` | `smoke`, `verify` | fabric base URL (fallback for `--url` / `--pubkey-url`) |
| `CORELINK_PAT` | `smoke`, `verify --pubkey-url` | the tenant PAT (Bearer); `verify` needs it only when fetching the key via `--pubkey-url` (the key endpoint is authenticated) — not when passing `--pubkey` directly |

## `corelink smoke` — verify a live deployment

Automates `docs/deploy/post-redeploy-smoke-checklist.md`. The **default** checks
are side-effect-free (nothing provisions a box):

- `GET /v1/health` → 200
- `GET /v1/attestation/key` → a 32-byte ed25519 pubkey
- fail-closed: an **unpinned** image → `400` (X4 supply-chain floor, before box contact)
- fail-closed: a **bad PAT** → `401`

```sh
CORELINK_PAT=<tenant-pat> corelink smoke --url https://<fabric>
```

Add `--full` to additionally do a **real `acquire → cancel`** (provisions and
tears down a real box on the cloud backend); `--image <name@sha256:…>` overrides
the pinned image used there. Exit `0` iff every check passed.

## `corelink verify` — trust a verdict

The customer-trust primitive: verify a fabric's `result_binding_sig_v2` (the v2
attestation that binds the **verdict** `exit` + output `artifacts`, closing the
v1 forgeable-verdict gap) against the published key.

```sh
# Verify a CloseResponse / ExecResponse JSON, fetching the key from the fabric:
corelink verify --pubkey-url https://<fabric> --input close-response.json

# …or pass the key directly and pipe the payload on stdin:
cat close-response.json | corelink verify --pubkey <ed25519-b64>
```

It reads the payload's `check_result` (CloseResponse) or `result` (ExecResponse)
plus `result_binding_sig_v2`, recomputes the v2 pre-image, and verifies. Prints
`✓ VERIFIED` (exit 0) or `✗ FAILED … do NOT trust this verdict` (exit 1). A
malformed key/sig or a pre-v2 (empty-sig) payload is a loud error (exit 2), never
a silent pass.

## Why a CLI (not a per-language SDK / Actions shim) first

A language-agnostic CLI is the smallest surface that unblocks all three needs
(adoption, smoke, dogfood) at once. A GitHub-Actions/Buildkite shim and language
SDKs remain a deliberate follow-up — the CLI's `client`/`binding` modules are the
reference the SDKs transcribe.

## Dogfood note — no CoreLink flip required

A tenant can be onboarded on the **static** auth backend (`FABRIC_AUTH_BACKEND`
default) + the admin endpoint (`POST /internal/v1/admin/tenants`, `FABRIC_ADMIN_KEY`)
with **zero** CoreLink dependency. So `corelink smoke --full` / a real workload
can run on the live fabric today — the CoreLink entitlement flip is only needed
for self-serve multi-tenant billing.
