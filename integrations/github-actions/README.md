# CoreLink Runners — GitHub Action

A composite GitHub Action that wraps the `corelink run` CLI so a CI team
adds **one `uses:` step** to run a memoized, attested job on the CoreLink
fabric.

> **Pricing:** flat by concurrency, never per-minute.  You buy N parallel
> runners; minutes are unlimited.  A re-run whose result is memoized returns
> from cache in milliseconds — you are never billed as if it re-ran.  See
> [`docs/product/pricing.md`](../../docs/product/pricing.md).

> **Trust model:** the action fails **closed** if the fabric attestation does
> not verify.  A `verified: false` result is always a hard step failure — it
> is never silently swallowed.

---

## Quickstart

```yaml
# .github/workflows/ci.yml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # Until a published release exists, build the CLI first (see below).
      - name: Build corelink CLI
        run: |
          cargo build -p corelink-cli --release --locked
          echo "$PWD/target/release" >> $GITHUB_PATH

      - name: Run tests on CoreLink
        id: run-tests
        uses: HuGR-Labs/corelink-runners/integrations/github-actions@v0.1.0
        with:
          url:      ${{ vars.CORELINK_URL }}
          pat:      ${{ secrets.CORELINK_PAT }}
          check:    "cargo test --workspace --locked"
          check-id: "cargo-test"
          # Image MUST be pinned with a sha256 digest.
          # Unpinned images are rejected before box contact (X4 floor).
          image:    "ubuntu@sha256:a6d2b38300ce017add71440577d5b0a90460d0602a9f4920e2cae943d53c8fac"
          verify:   "true"

      - name: Show attestation
        if: always()
        run: |
          echo "lease : ${{ steps.run-tests.outputs.lease-id }}"
          echo "exit  : ${{ steps.run-tests.outputs.exit }}"
          echo "attest: ${{ steps.run-tests.outputs.verified }}"
```

A complete, runnable example is at [`examples/ci.yml`](examples/ci.yml).

---

## Binary availability

The `corelink` binary is not yet published to crates.io and has no public
release artifact.  Until a release exists, the action expects the binary to
be on `PATH` — built and cached in an earlier workflow step:

```yaml
- name: Build corelink CLI
  run: |
    cargo build -p corelink-cli --release --locked
    echo "$PWD/target/release" >> $GITHUB_PATH
```

When a release is published, the `version` input will download the binary
automatically (Linux/macOS, amd64/arm64).  The release URL pattern will be:

```
https://github.com/HuGR-Labs/corelink-runners/releases/download/v<version>/corelink-<os>-<arch>
```

---

## Inputs

| Input | Required | Default | Description |
|---|---|---|---|
| `url` | yes | — | CoreLink fabric base URL (e.g. `https://runners.corelink.dev`). Overrides `CORELINK_URL`. |
| `pat` | yes | — | CoreLink Personal Access Token.  **Must** be a repository secret — it is never echoed or logged. |
| `check` | yes | — | Shell command to run on the fabric. |
| `check-id` | no | `"ci"` | Stable identifier for this check; used in memoization and attestation. |
| `image` | no | `""` | Container image, **must** include a `sha256` digest. Unpinned images are rejected by the fabric before a box is contacted (X4 supply-chain floor). |
| `verify` | no | `"true"` | Verify the fabric attestation. Set to `"false"` only for local dev where the fabric runs with `FABRIC_DEV_UNSAFE=1`. A verification failure is always a hard step failure. |
| `version` | no | `"0.1.0"` | CLI release version to install. Currently unused (see Binary availability above). |

## Outputs

| Output | Description |
|---|---|
| `exit` | Exit code returned by the check command on the fabric (`0` = passed, `1` = check failed). |
| `verified` | `"true"` if the fabric attestation was cryptographically verified. |
| `lease-id` | The CoreLink lease ID for this run — use for audit trails and support requests. |

---

## Exit semantics

The `corelink run` CLI uses three exit codes; the action maps them as follows:

| CLI exit | Meaning | Action behaviour |
|---|---|---|
| `0` | Ran, verified, check passed | Step passes |
| `1` | Ran, verified, check exited non-zero | Step fails (job failure) |
| `2` | Attestation failed / wire / auth / unpinned image | Hard step failure — `verified: false` is never swallowed |

The action always surfaces CLI stderr (fabric diagnostics) in the runner log.

---

## Security notes

- The PAT is injected via an environment variable (`CORELINK_PAT`).  It is
  never passed on the command line, never interpolated into an `echo`, and
  never written to a file or log.
- The `image` input **must** include a `sha256` digest — the fabric enforces
  this at the wire level before any box is provisioned.
- Setting `verify: "false"` disables attestation checking.  Only use this
  against a local dev fabric booted with `FABRIC_DEV_UNSAFE=1`; attestations
  from that instance are forgeable.

---

## Repository secret setup

1. Go to **Settings → Secrets and variables → Actions → New repository
   secret**.
2. Name: `CORELINK_PAT`, value: your CoreLink Personal Access Token.
3. Add a repository **variable** `CORELINK_URL` with your fabric URL.

---

## References

- [`docs/cli.md`](../../docs/cli.md) — full `corelink` CLI reference
- [`docs/product/pricing.md`](../../docs/product/pricing.md) — pricing model
- [Legacy fabric wire/envelope contract (historical framing; the §13/attestation mechanisms are now owned and served by CoreLink)
- [`docs/deploy/fabric-server.md`](../../docs/deploy/fabric-server.md) — self-hosted fabric setup
