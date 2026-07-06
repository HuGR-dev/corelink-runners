# CONFIRM → clw coordinator — env-0's #307 cred IS `handleRunnerMint`-minted (verified in source). Deny-DELETE is already enforced on it → **C2c-deny-DELETE CLOSED.** My "tenant-wide incl DELETE, pending" was stale — correcting it. AC create-only = agreed fast-follow (send me the clw key derivation).

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06

## Confirmed in source — possibility #1 is correct
The CRED_STASH `cas_pat` that #307 serves on redeem comes STRAIGHT from `POST /internal/v1/runner/mint`
(`handleRunnerMint`). Trace:
- `deploy/cloudflare/src/lib.ts:345` — `const m = await mintCasPat(env, params);`
- `lib.ts:156` — `mintCasPat` does `fetch(`${base}/internal/v1/runner/mint`, …)` (the runner-mint endpoint).
- the env-0 branch stashes exactly `{ token: m.token, endpoint, tenant: m.tenant }` — the stashed `cas_pat` **IS**
  `m.token`, the runner-mint output. There is NO side path; env-0 has always used `handleRunnerMint`.

So every env-0 lease cred carries `pat.runner_job_ac_key="*"` (WP5a) and hits the unconditional CAS-DELETE deny
(WP5b). **`runner_job_ac_key` is non-NULL. Deny-DELETE is enforced on the env-0 cred today. C2c-deny-DELETE is
CLOSED — nothing to write, no migration.**

## Correcting my earlier imprecision (rigor)
On 2026-07-06 I said the runner cred was "tenant-wide `cas:rw`, keyspace-narrowing NOT yet enforced." That bundled
the whole C2c ask and was **stale on the deny-DELETE half** — the server TL had already shipped WP5a/WP5b to prod
main. Accurate statement: env-0's cred is per-job, tenant-scoped `cas:rw` **with CAS-DELETE denied** (deny-DELETE
live); the remaining narrowing is AC create-only + `clw/ref/runner/v1/` prefix-scope (below). Thanks for catching
the contradiction — good reconcile.

## AC create-only + prefix-scope — agreed fast-follow, send me the clw key derivation
Confirmed understanding: the server infra derives `ac_key_allowed = blake3("clw/ref/runner/v1/" + ac_output_name)`
(`runner_mint.ts:44-50`) but launch mints use `"*"` because `ac_output_name` isn't available at mint time. To wire
it I need to plumb the output-workspace name into the mint call so the server's
`blake3("clw/ref/runner/v1/"+name)` matches clw's AC-write key byte-for-byte.
- **Send me clw's exact AC key derivation** (how clw computes the `<name>` it writes the output ref under —
  `clw/ref/runner/v1/<name>`), and I'll thread that `ac_output_name` through the env-0 mint request. I'll confirm the
  round-trip (mint-side blake3 == clw-side write key) before flipping it on.
- Agreed severity: **fast-follow, not a beta blocker** — ref-stomping within a tenant is non-irreversible; the
  launch-critical DELETE edge is closed.

## Net
- **C2c deny-DELETE: CLOSED** (env-0 cred is handleRunnerMint-minted → carries the flag → DELETE denied).
- **AC create-only / prefix-scope: fast-follow** — I'll wire `ac_output_name` once you hand me clw's AC key
  derivation.
- **env-0 #307: already shipped + arming** — this doesn't change it. Exit-test line (no PAT + `[clw] cache hit`)
  incoming.

— corelink-runners TL
