# clw required-hit patch artifact

This artifact is based on sibling clw source HEAD `57c2256e2c380efdeba25d19db2ee48c0fd8c11c`. The sibling workspace was inspected read-only. Apply `clw-required-hit.patch` from the clw repository root with `git apply`; it changes only `crates/clw-cli/src/subcmds/run.rs` and `crates/clw-run/src/lib.rs`.

The patch adds `clw run --require-hit` with visible alias `--no-exec`. It keeps the existing key derivation and normal `clw run` path unchanged. Required-hit performs the same authenticated AC lookup through `HttpClient`, parses the exact run record, fetches both output blobs, and independently checks both BLAKE3 digests before returning a hit. It returns the cached exit code, including a cached nonzero code, and never spawns the command. A missing, malformed, incomplete, digest-mismatched, or transport-failed lookup exits 78 with the child-not-run message. The existing default still executes on a miss and retains its current exit-code and caching behavior.

The added clw-run tests cover an empty miss, malformed AC record, cached nonzero replay without spawn, and CAS bytes substituted under the recorded digest. Existing tests at the sibling HEAD continue to cover default miss execution and ordinary hit replay. `cargo fmt --all -- --check`, `git diff --check`, and `git apply --check` passed in the disposable authoring clone. No build, dependency install, CI, provider call, credential read, or release was performed.

The actual CAS client verifies every 200 response by recomputing `Digest::of_bytes` before returning (`crates/clw-client/src/lib.rs:1097-1108`); the required-hit helper repeats that check so the policy remains fail-closed even for a weaker test or alternate transport. The runner must invoke the released binary with `--require-hit` (or `--no-exec`) and classify exit 78 as required-hit miss/error. A release carrying this patch is required; repinning the installed 0.1.5 binary cannot provide the flag.

This is a patch artifact for owner review. It does not claim product integration, binary release, or T6-W2 completion.
