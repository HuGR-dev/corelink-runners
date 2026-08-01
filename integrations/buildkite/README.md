# CoreLink Runners — Buildkite Plugin

A Buildkite plugin that wraps the `corelink run` CLI so a CI team adds **one
`plugins:` entry** to run a memoized, attested job on the CoreLink fabric.

> **Pricing:** flat by concurrency, never per-minute. You buy N parallel
> runners; minutes are unlimited. A re-run whose result is memoized returns
> from cache in milliseconds — you are never billed as if it re-ran. See
> [`docs/product/pricing.md`](../../docs/product/pricing.md).

> **Trust model:** the plugin fails **closed** if the fabric attestation does
> not verify. A `verified: false` result is always a hard step failure — it
> is never silently swallowed.

---

## Quickstart

```yaml
# pipeline.yml
steps:
  - label: ":corelink: Run tests on CoreLink fabric"
    plugins:
      - HuGR-Labs/corelink#v0.1.0:
          url: "https://runners.corelink.dev"
          check: "cargo test --workspace --locked"
          check-id: "cargo-test"
          # Image MUST be pinned with a sha256 digest.
          # Unpinned images are rejected before box contact (X4 floor).
          image: "ubuntu@sha256:a6d2b38300ce017add71440577d5b0a90460d0602a9f4920e2cae943d53c8fac"
          verify: true
    env:
      # Inject via Buildkite secrets — never hard-code in pipeline.yml.
      CORELINK_URL: "https://runners.corelink.dev"
```

The `CORELINK_PAT` secret **must** be set as a Buildkite secret (see [Secret
setup](#secret-setup) below). The plugin reads it from the environment; it is
never logged, echoed, or written to any file.

A complete, runnable example is at [`examples/pipeline.yml`](examples/pipeline.yml).

---

## Binary availability

The `corelink` binary is not yet published to crates.io and has no public
release artifact. Until a release exists, the plugin expects the binary to be
on `PATH` — built and cached in an earlier pipeline step:

```yaml
steps:
  - label: "Build corelink CLI"
    commands:
      - cargo build -p corelink-cli --release --locked
      - echo "export PATH=\"\$PWD/target/release:\$PATH\"" >> ~/.bashrc

  - label: ":corelink: Run on CoreLink fabric"
    depends_on: "build-corelink-cli"
    plugins:
      - HuGR-Labs/corelink#v0.1.0:
          check: "cargo test --workspace --locked"
```

When a release is published, the plugin will download the binary automatically.
The release URL pattern will be:

```
https://github.com/HuGR-Labs/corelink-runners/releases/download/v<version>/corelink-<os>-<arch>
```

---

## Options

| Option | Required | Default | Description |
|---|---|---|---|
| `url` | no | `$CORELINK_URL` | CoreLink fabric base URL (e.g. `https://runners.corelink.dev`). Overrides the `CORELINK_URL` environment variable when set. |
| `check` | **yes** | — | Shell command to run on the fabric. |
| `check-id` | no | `"ci"` | Stable identifier for this check; used in memoization and attestation. |
| `image` | no | `""` | Container image, **must** include a `sha256` digest. Unpinned images are rejected by the fabric before a box is contacted (X4 supply-chain floor). |
| `verify` | no | `true` | Verify the fabric attestation. Set to `false` only for local dev where the fabric runs with `FABRIC_DEV_UNSAFE=1`. A verification failure is always a hard step failure. |

## Environment variables

| Variable | Required | Description |
|---|---|---|
| `CORELINK_PAT` | **yes** | CoreLink Personal Access Token (Bearer). Must be a Buildkite secret — never hard-code. |
| `CORELINK_URL` | no | Fabric base URL fallback (overridden by the `url` plugin option). |

---

## Exit semantics

The `corelink run` CLI uses three exit codes; the plugin maps them as follows:

| CLI exit | Meaning | Plugin behaviour |
|---|---|---|
| `0` | Ran, verified, check passed | Step passes |
| `1` | Ran, verified, check exited non-zero | Step fails (legitimate job failure) |
| `2` | Attestation failed / wire / auth / unpinned image | Hard step failure — never swallowed |

The plugin always surfaces CLI stderr (fabric diagnostics) in the Buildkite log.
On success, it annotates the build with the lease ID, verified status, and check
exit code (requires `buildkite-agent` on PATH, which is standard in hosted
Buildkite runners — guarded, so local test runs still work without it).

---

## Security notes

- `CORELINK_PAT` is consumed via the environment. It is never passed on the
  command line, never interpolated into an `echo`, and never written to any
  file or log.
- The `image` option **must** include a `sha256` digest — the fabric enforces
  this at the wire level before any box is provisioned (X4 supply-chain floor).
- Setting `verify: false` disables attestation checking. Only use this against
  a local dev fabric booted with `FABRIC_DEV_UNSAFE=1`; attestations from that
  instance are forgeable.

---

## Secret setup

1. Go to your Buildkite organization **Settings → Secrets**.
2. Add a secret named `CORELINK_PAT` with your CoreLink Personal Access Token.
3. Reference it in your pipeline or agent environment so it is available as
   `CORELINK_PAT` when the plugin hook runs.

For pipeline-level secrets (Buildkite Enterprise), add the secret to the
pipeline's **Environment** section. For agent-level, inject it via the agent's
environment configuration.

---

## References

- [`docs/cli.md`](../../docs/cli.md) — full `corelink` CLI reference
- [`docs/product/pricing.md`](../../docs/product/pricing.md) — pricing model
- [`docs/spec/hugit-integration-contract.md`](../../docs/spec/hugit-integration-contract.md) — fabric wire/envelope contract (historical hugit framing; hugit discontinued)
- [`docs/deploy/fabric-server.md`](../../docs/deploy/fabric-server.md) — self-hosted fabric setup
- [GitHub Actions integration](../github-actions/) — the first CI front door
