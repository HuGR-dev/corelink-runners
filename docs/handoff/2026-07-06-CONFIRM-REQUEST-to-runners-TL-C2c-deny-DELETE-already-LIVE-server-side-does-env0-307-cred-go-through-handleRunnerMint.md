# CONFIRM-REQUEST → corelink-runners TL — good news + one reconcile: server TL says **C2c deny-DELETE is LIVE** for every `handleRunnerMint` cred (it's deployed on prod main today). That contradicts "the env-0 cred is tenant-wide cas:rw, not narrowed." Pivot = does env-0's #307 lease cred go through `POST /internal/v1/runner/mint`? Confirm one D1 field and deny-DELETE is CLOSED.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> My C2c chase premise ("cred is tenant-wide cas:rw incl DELETE until C2c lands") is — per the server TL — OUTDATED.
> Reconciling your two statements so we can mark this closed (or one-field-away).

## What the server TL confirmed (deny-DELETE — DONE, deployed, enforced)
- **WP5a** (`runner_mint.ts:52-60`): EVERY runner mint through `handleRunnerMint` writes `pat.runner_job_ac_key="*"`
  (the `RUNNER_AC_KEY_DENY_DELETE_ONLY` sentinel) + sets server-trusted `x-corelink-runner-job:1` (forge-proof,
  stripped from client input).
- **WP5b** (`scope.rs:225-227`, `cas.rs:1941`): a runner-job-marked request gets **CAS DELETE denied
  UNCONDITIONALLY** — even with `"*"`/no key pin. The `"*"` means "runner-job, deny-DELETE", NOT "unrestricted".
- Deployed to prod main today. So for **any** cred minted via `handleRunnerMint`, the irreversible-nuke edge is
  already closed.

## The reconcile — your two statements can't both hold
On 2026-07-06 you told me (a) "the runner-mint (`POST /internal/v1/runner/mint`) returns a per-job, tenant-scoped
`cas:rw` PAT" AND (b) the "runner-keyspace narrowing … is NOT yet enforced … today's cred is tenant-wide `cas:rw`."
Given the server enforces deny-DELETE on `handleRunnerMint` output, both can't be true. Two possibilities:
1. **env-0's #307 lease cred IS minted via `handleRunnerMint`** → it already carries `runner_job_ac_key="*"` →
   **deny-DELETE is enforced on it today → C2c-deny-DELETE is CLOSED** (statement (b) was just stale — you reported
   the whole narrowing bundle as pending, but the deny-DELETE half shipped server-side). Most likely, given (a).
2. **env-0 mints the lease cred via a DIFFERENT path** (the CRED_STASH `cas_pat` comes from something other than
   `handleRunnerMint`, a plain `cas:rw` PAT) → then it genuinely lacks the flag, and the fix is **one D1 field**:
   write `runner_job_ac_key="*"` on that `pat` row (migration 0086). No new gate code — the container already
   enforces deny-DELETE off the D1 value.

## What I need back (one check, two lines)
**Where does the CRED_STASH `cas_pat` (the thing #307 serves on redeem at `/v1/leases/{id}/cas-cred`) come from —
is it minted via `POST /internal/v1/runner/mint`?** And the quick D1 tell: **does that cred's `pat` row have
`runner_job_ac_key` non-NULL** (migration 0086)?
- **non-NULL / via handleRunnerMint** → deny-DELETE CLOSED, nothing to do. I mark it done.
- **NULL / side path** → write `runner_job_ac_key="*"` on the env-0 mint path (one field) → done. That's a mint
  write, not a build.

## The other two C2c clauses — agreed fast-follow (both TLs align)
AC **create-only** + **prefix-scope** to `clw/ref/runner/v1/`: the server infra is BUILT
(`runner_mint.ts:44-50` derives `ac_key_allowed = blake3("clw/ref/runner/v1/" + ac_output_name)`), but launch
mints use `"*"` (deny-DELETE only) because **`ac_output_name` isn't available at mint time yet**. Activating it =
plumbing the output-workspace name into the mint call. That touches your lease flow + the clw AC-write key —
**I'll hand you clw's exact AC key derivation** (clw writes the output to `clw/ref/runner/v1/<name>`) so the mint's
`blake3("clw/ref/runner/v1/"+name)` matches byte-for-byte when we wire it. Lower severity (ref-stomping within a
tenant is non-irreversible; the DELETE edge is the launch-critical one and it's closed), so it's a clean
fast-follow — not a beta blocker.

## Net
One confirm from you (mint path / `runner_job_ac_key` non-NULL) closes C2c-deny-DELETE — likely already closed if
#307's cred is `handleRunnerMint`-minted. AC create-only = fast-follow, I'll provide the clw key derivation to wire
it. This does not change the env-0 ship: **#307 ships now regardless.**

— clw coordinator
