# T8-W4b focused acceptance

Status: `READY-PENDING-T3`

Exact integration target: `486b57357813e5c3630d21701d72e309d32c5ff7`
(`delivery/techlead-takeover-20260906`).

The auth-file path is source-reachable across Rust, boot, Worker and DevEnv:

- Rust `ExecAuth::from_env` rejects process-env credentials, requires
  `EXEC_SERVER_AUTH_TOKEN_FILE`, opens it without symlink following, and
  requires a regular owner-only `0400` file.
- `deploy/check-host/entrypoint.sh` and `deploy/cloudflare/entrypoint.sh`
  bridge the provider token to that ephemeral file, clear the raw token before
  child startup, validate the marker/file, and remove the file on exit.
- Worker `buildContainerEnv` supplies the lease-bound
  `CLW_CRED_TICKET`, `CLW_LEASE_ID`, and `CLW_FABRIC_ENDPOINT`; the raw CAS
  credential stays brokered and the exec bearer is converted at container boot.

Focused proof on this target:

- `bash deploy/check-host/test/auth-secret-bridge.sh`: `auth-secret-bridge: PASS`.
- Rust `cargo test -p corelink-check-exec-server --test exec_e2e`: 12/12 passed
  on the unchanged T8-W4b Rust surface (the target workspace was under memory
  pressure during the fresh rebuild; no test failure occurred).
- Worker/DevEnv focused Vitest files: 62 tests passed on this target (the
  current `check-host` suite contains 38 tests; with `devenv-do` 19 and
  unprivileged flow 5).
- Image pin/conformance files: 8 tests passed.

Version/digest binding remains available: Wrangler pins the runner and
check-host container images by immutable `@sha256` references; the check-host
Dockerfile pins its Ubuntu base and verifies `clw` with SHA-256; and the Worker
enforces `@sha256:` plus optional exact `PINNED_IMAGE_DIGEST` matching.

T3-W18 live evidence remains in RCA, so this result is pending that dependency
and does not claim live deployment acceptance.
