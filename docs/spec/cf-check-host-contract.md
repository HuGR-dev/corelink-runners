# FROZEN CONTRACT — CF-native check-host (campaign B, owner go 2026-06-27)

> The anchor for the build wave. Every WP transcribes these interfaces verbatim; do NOT diverge.
> Design verdict + reuse map: `docs/design/2026-06-26-cf-native-check-host.md`. Resolver: clw option (b).

## Lifecycle (the load-bearing decision — toolchain at ACQUIRE, hydrate at SPAWN, validate at EXEC)
A check-host lease is acquired **with its toolchain**; the container hydrates it once at start; exec runs in
the already-hydrated box. This respects the FROZEN `Engine::exec_captured(c, argv)` seam (no toolchain param)
**and** closes the latent false-cache-hit bug (the hydrated toolchain is provably the memo axis).

```
acquire(toolchain_digest = D)              # D = the clw snapshot manifest digest (= CheckDef.toolchain_ref)
  → CloudflareEngine.spawn(check spec)     # spawns a CheckHostContainer; injects TOOLCHAIN_DIGEST=D + CLW_*
      → container entrypoint: clw hydrate --manifest-digest $TOOLCHAIN_DIGEST $TOOLCHAIN_DIR ; start exec-server
exec(CheckDef)                             # /v1/leases/{id}/exec
  → fabric ASSERTS CheckDef.toolchain_ref == lease.toolchain_digest  (mismatch → 400 fail-closed)
  → exec_captured(c, ["sh","-lc",cmd])     # runs in the hydrated $TOOLCHAIN_DIR; box already has the toolchain
close → teardown
```

## C1 — AcquireRequest gains `toolchain_digest` (additive, optional)
`crates/corelink-fabric-api/src/dto.rs` `AcquireRequest`:
```rust
/// Check-host lease only: the clw snapshot manifest digest of the toolchain to
/// hydrate at spawn (== the CheckDef.toolchain_ref the lease will exec). Absent
/// for runner + plain-hermetic leases. #[serde(default, skip_serializing_if="Option::is_none")]
pub toolchain_digest: Option<String>,
```
A check-host lease = `runner: None` + `toolchain_digest: Some(D)`. (Plain hermetic check = both None.)

## C2 — /v1/spawn extended for check-mode (Rust CloudflareEngine ↔ spawn-Worker)
`POST {spawn_worker_url}/v1/spawn` (bearer = CLOUDFLARE_SPAWN_AUTH_TOKEN). Additive request field:
```jsonc
{ "image_digest": "<check-host image, the FIXED wrangler image>",
  "mode": "check",                      // NEW: "runner" (default, back-comp) | "check"
  "toolchain_digest": "<D>",            // NEW: required when mode=="check"
  "env": { "CLW_ENDPOINT":..,"CLW_TENANT":..,"CLW_TOKEN":..,"TOOLCHAIN_DIGEST":"<D>", ... },
  "expiry_ms": <u64> }
→ 201 { "handle": "<url-safe>" }        // unchanged
```
mode=="check" → the Worker routes to `CHECK_HOST_CONTAINER` (not the runner DO).

## C3 — /v1/exec (NEW; Rust CloudflareEngine ↔ spawn-Worker)
`POST {spawn_worker_url}/v1/exec` (bearer = CLOUDFLARE_SPAWN_AUTH_TOKEN):
```jsonc
{ "handle": "<from spawn>", "argv": ["sh","-lc","<cmd>"], "timeout_ms": <u64> }
→ 200 { "exit_code": <i32|null>, "stdout": "<string>", "stderr": "<string>" }
```
Non-2xx → `CloudflareEngine::exec_captured` returns `Err` (fail-closed; no fabricated CmdOutput).
`exit_code: null` ⇒ `CmdOutput.code = None` (signal-killed) ⇒ run_check fails closed.

## C4 — in-container exec-server (spawn-Worker `containerFetch` ↔ the container)
The check-host image runs an HTTP server on **port 8080** (`defaultPort`), endpoint:
```jsonc
POST /exec  { "argv": ["sh","-lc","<cmd>"], "timeout_ms": <u64> }
→ 200 { "exit_code": <i32|null>, "stdout": "<string>", "stderr": "<string>" }   // byte-faithful, no trim
```
Runs `argv` with cwd = `$TOOLCHAIN_DIR` (PATH includes the hydrated toolchain). Captures stdout/stderr bytes
+ exit verbatim. A timeout kills the process group → `exit_code: null`. The server is reachable ONLY via the
Worker's `containerFetch` (not public).

## C5 — the entrypoint (hydrate-then-serve)
`deploy/check-host/entrypoint.sh`: `clw hydrate --manifest-digest "$TOOLCHAIN_DIGEST" "$TOOLCHAIN_DIR"`
(CLW_* injected as env, exactly as the runner path) → on success `exec` the exec-server. clw-hydrate failure
→ exit non-zero (the container dies; spawn → the lease fails closed). clw's `--manifest-digest` flag is
delivered by the clw TL on this campaign's W6 timeline.

## C6 — the W4↔W5 internal seam (env-carried check-host discriminator) [addendum 2026-06-28]
`ContainerSpec` (frozen seam in `corelink-runner`, NOT a WP owner-file) is **not** extended. The
check-host signal + digest ride the EXISTING additive `spec.env` channel — exactly as C2 routes
`TOOLCHAIN_DIGEST`, and exactly as the §13.2 ingest vars are already injected after `from_lease`:

- **W5 (fabric, acquire):** for a check lease with `AcquireRequest.toolchain_digest == Some(D)`,
  append `("TOOLCHAIN_DIGEST", D)` to `spec.env` (additive, after the §13.2 inject) AND record the
  acquire-time digest in a fabric-internal per-lease marker map (`app.rs`,
  mirror of `runner_leases`: `mark_toolchain_digest`/`toolchain_digest_of`, GC'd in `forget_lease`).
- **W4 (engine, spawn):** the check-host discriminator is `!spec.allow_egress && spec.env` contains
  key `TOOLCHAIN_DIGEST`. Such a spec is ADMITTED (bypasses the runner-only floor #198), spawns with
  top-level `mode:"check"` + `toolchain_digest:<D lifted from env>`. A `!allow_egress` spec WITHOUT
  `TOOLCHAIN_DIGEST` keeps the v0 runner-only rejection (plain hermetic check → Northflank, rota B).
- **exec-time assert (W5, `exec_handler`):** `CheckDef.toolchain_ref == toolchain_digest_of(lease)`
  (mismatch → 400 fail-closed) — the false-cache-hit guard; the marker map is its read source.
- **W4 and W5 compile independently:** W5 calls NO new W4 symbol (the `Engine` seam is frozen; `spawn`
  signature unchanged; the existing `CloudflareBoxProvisioner` is reused). Coupling is purely this
  runtime env-key convention → conflict-free parallel fanout.

DEFAULT-OFF holds: absent `CLOUDFLARE_SPAWN_*` ⇒ no check-host routing ⇒ byte-identical to rota B.

## WP table (disjoint files; contract-bound; W1→W2 build-dep; rest parallel)
| WP | Owner-files (disjoint) | Builds against |
|----|------------------------|----------------|
| **W1** exec-server | `crates/corelink-check-exec-server/**` (NEW Rust crate, static-musl bin, axum) | C4 |
| **W2** image+entrypoint | `deploy/check-host/{Dockerfile,entrypoint.sh}` (NEW) | C5 + W1's bin path + clw binary |
| **W3** Worker route+DO | `deploy/cloudflare/src/{index.ts,lib.ts}` + `wrangler.jsonc` (CheckHostContainer) | C2,C3,C4 |
| **W4** CloudflareEngine | `crates/corelink-cloud-engine/src/cloudflare.rs` (spawn check-mode + exec_captured + relax #198 floor for check-host) | C2,C3 |
| **W5** fabric wiring | `crates/corelink-fabric-api/src/dto.rs` (C1) + `crates/corelink-fabric-server/src/{handlers/leases.rs,handlers/exec_handler.rs,cloud_exec.rs}` (pass toolchain_digest to spawn; ASSERT ref==digest at exec; select check-host backend) | C1,C2 |

DEFAULT-OFF: the check-host backend wires only when its env is present; absent ⇒ byte-identical to today.
Live-flip gated (separate): clw `--manifest-digest` landed + a real toolchain snapshot exists + owner deploy go.

## Open cross-repo dependency
The check-host **image needs the `clw` binary** (clw TL's artifact, with `--manifest-digest`). Until clw
ships it, W2's Dockerfile pins a placeholder clw fetch; W6 (consume the flag) lands when clw delivers. The
Rust/Worker WPs (W1,W3,W4,W5) build + gate-green NOW against this contract (the clw dep is image-only).
