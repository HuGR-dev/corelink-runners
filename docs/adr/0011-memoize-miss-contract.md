# ADR-0011 — Memoize miss policy

- **Status:** accepted; action/client source implemented, signed client release and live HIT evidence pending
- **Date:** 2026-09-04; implementation update 2026-09-06
- **Decision owner:** repository owner (D11 ratification)
- **Scope:** `actions/corelink-memoize` and its owned workflow tests

The action exposes `cache-policy` with exactly two values:

- `optional` (the default): cache absence, a miss, or an internal CoreLink
  failure runs the wrapped command COLD and keeps its exit status.
- `required-hit`: the action calls the client's atomic verified no-exec
  operation. A HIT replays the cached output and exit code; a miss, invalid
  result, transport or setup failure returns 78 without running the child.
  Cached nonzero exits, including 125, never trigger a COLD fallback.

The prepared clw 0.1.12 source implements `clw run --require-hit` (alias
`--no-exec`) using the same action key as ordinary `run`. Verification and cached
replay occur inside this operation; a separate preflight lookup is insufficient.
The action requires a successful `clw --version` reporting exactly `clw 0.1.12`.
Missing endpoint, credentials or binary, older/unknown versions and a failed
version command refuse with 78. It does not parse human HIT messages or `--json`.
Version selection identifies the supported interface; authenticity still depends
on the signed binary installation and the client's authenticated CAS/AC reads.

The previously installed clw 0.1.5 lacks this operation, so required-hit continues
to refuse on those images. **0.1.12 has not been published by this takeover.**
Signed release, verified consumer pins and a real authenticated HIT remain
required before production acceptance. Source preparation does not establish
those facts.

Focused shell fixtures cover optional COLD/fallback and required cached exit
preservation, miss/setup failures, missing dependencies and version refusals.
Root also ran the action with the actual locally built client: invalid setup and
missing endpoint returned 78 with no child; optional setup failure ran COLD.
These negative checks do not claim a production CAS HIT.
