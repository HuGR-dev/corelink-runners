# Exposed-secret rotation harness v2

This isolated emergency harness defaults to plan-only. That path does not read
inputs or run external commands. Mock mode accepts every remote-facing binary
from an explicit mock root, so the test suite cannot resolve or invoke a real
Wrangler.

Live mode is implemented but was not executed. It requires these exact values:

    --live-ack-primary ACK-PRIMARY-FORWARD-ONLY-EXPOSED-SECRET-ROTATION-V2
    --live-ack-recovery ACK-RECOVERY-PAIR-ONLY-NEVER-RESTORE-EXPOSED-SECRETS-V2
    --live-guard CORELINK-EXPOSED-SECRET-ROTATION-V2-LIVE-GUARD
    --control-manifest <owner-only mode-0600 local-controller manifest>
    --control-controller-sha256 <lowercase 64-hex controller hash>
    --control-manifest-sha256 <lowercase 64-hex manifest hash>

An optional lifecycle test additionally requires:

    --lifecycle run
    --lifecycle-ack ACK-BOUNDED-SYNTHETIC-ACQUIRE-CLOSE-V2

There is no old signing-seed argument or file. A primary attempt creates four
independent mode-0600 files in an already-created, invoking-owner-owned, non-symlink mode-0700 OOB directory: primary and
recovery Ed25519 seeds, plus their paired spawn-authentication tokens. The only
legacy input is an old spawn-token file, accepted solely for the one
post-cutover 401 probe. The files are created with an atomic hard-link commit that refuses an existing or symlink destination. The evidence directory only receives public facts and
status proofs.

The pin manifest is strict and has exactly these fields:

    schema=corelink-exposed-secret-rotation-v2
    spawn_worker=<exact deployed Worker name>
    fabricd_worker=<exact deployed Worker name>
    spawn_config=<absolute local config path>
    fabricd_config=<absolute local config path>
    spawn_runner_image=<repo@sha256:...>
    spawn_check_image=<repo@sha256:...>
    fabricd_image=<repo@sha256:...>
    spawn_config_sha256=<lowercase 64-hex>
    fabricd_config_sha256=<lowercase 64-hex>

The injected control bridge has fixed secret-free operations. It must reject
unknown output fields.

- preflight NONCE proves a deployed, armed global admission freeze; the exact
  /internal/v1/fleet/busy result with busy=0 and unverifiable=0; the expected
  in-memory ledger posture; explicit terminalization evidence for Pending and
  Held; a recorded maintenance impact; zero remaining Pending/Held/active
  boxes; and fresh matching remote pins.
- corelink-stage NONCE KEY_ID PUBKEY verifies that the Ed25519 key_id and
  public key were derived from the selected seed by the package-owned secure
  derivation process. It records the Corelink v1/v2 key-contract version and
  invalidation of prior attestations. It runs before Fabricd changes while
  admission remains frozen; it has no deployment or replica gate.
- postflight ATTEMPT NONCE KEY_ID PUBKEY proves health, the exact one-current
  key shape keys:[{key_id,pubkey_b64,expires_ms:null}], the selected Corelink
  key, a changed provider version, a fresh container boot containing the
  selected key, and fresh Corelink-native v1/v2 binding conformance from the
  real Fabricd canary or reviewed Corelink conformance implementation. An
  extra field or non-null expiry is refused. Old-key rejection is asserted only
  through the Corelink key contract when that contract exposes the result.
- The direct auth probe is injected separately. After both deployments, it
  reads the supplied old spawn-token file only once into memory, receives the
  selected new token over stdin, and sends exactly two POST /v1/spawn requests
  with an empty JSON object. Under the armed admission freeze, the new token
  must reach 503 after authentication and the old one must reach 401; both must
  prove zero spawns. Neither token may be
logged, written, restored, or accepted as a signing-key input.

The included direct probe may be passed as the auth-probe binary. It holds the
two bearer values only in shell and curl process memory and uses a curl config
pipe, so neither authorization header is an argument or a temporary file:

    --auth-probe-bin scripts/rotation/corelink-b2/bin/direct-auth-probe.sh
- release-freeze NONCE ATTEMPT is called only after all frozen proofs pass.
  If it succeeds, the declared-safe lifecycle is one serialized, allowlisted
  acquire/close canary. Failure triggers automatic refreeze, then requires
  teardown-complete and fleet busy=0/unverifiable=0 proof.

After a failed primary attempt, recovery is the only permitted next attempt.
It reuses the already-created recovery pair and reruns preflight, Corelink key
contract, health, auth, key, verifier, and digest checks. It never restores exposed
material.

Run tests with:

    bash tests/test-rotation.sh

The live control binary is fixed to bin/control-bridge.sh; live mode rejects an
injected control binary. The package-owned bridge invokes the fixed
`scripts/rotation/corelink-b2/bin/rotation-controller.sh`, passing the exact
dual acknowledgements and independently supplied controller and manifest hash
pins. The controller validates the owner-only mode-0600 manifest, source commit,
operation-script hashes, token-file permissions, and fixed B2 identities before
running any operation. It emits strict JSON; the bridge accepts only the exact
canonical JSON shape for the selected operation and converts it to the v2
line-oriented k=v proof contract. There is no remote control endpoint or
caller-selected live executable. Postflight requires three health results
(fabricd health, fabricd ready, spawn health), the exact one-key attestation,
provider-version/container-boot selected-key proof, and Corelink-native v1/v2
binding conformance.

## Repository-owned layout

The complete package lives under `scripts/rotation/corelink-b2/`:

- `bin/rotate-exposed-secrets.sh` is the forward-only harness.
- `bin/rotation-controller.sh` is the immutable local controller.
- `bin/op_corelink_stage.sh` and `bin/op_postflight.sh` are the fixed
  Corelink-owned stage and postflight operations. They cannot be redirected by
  a manifest.
- `fixtures/corelink-key-contract-v1-v2.json` is the nonsecret contract
  fixture used by both operations.
- `manifests/corelink-rotation.manifest.template` is a nonsecret template;
  replace `<repo-root>` and owner-controlled placeholders when constructing a
  local mode-0600 manifest.
- `tests/test-rotation.sh` and `tests/test-controller.sh` are the focused
  harness and controller suites. Their mock binaries are under
  `tests/mock-bin/`.

All package paths are resolved from the package directory. No external product
or deployment package is a stage, postflight, provenance, or replica gate.
