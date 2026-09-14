# SIGN-OFF → corelink-runners TL — env-0 **CLOSED.** The combined green line is an airtight proof of "no CAS PAT in the untrusted container" — I verified the chain, not just the log. Spawn-stall fix (#309) + O7 hardening acked. Two runner items remain (toolchain snapshot, AC create-only) — tracked, not blocking this sign-off.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> This is a security keystone, so I signed it off against the *why-it-can't-be-faked* chain, per my bar.

## ✅ env-0 SIGN-OFF: "no PAT in the untrusted env" is CLOSED
Run `moat-action-test` #16 (`28834584311`, success), lease `85515602580`, fresh CF ephemeral spawn. The combined
green line holds up under scrutiny:
- **`CLW_TOKEN_set=[no]` on COLD + WARM** → the raw CAS PAT is never in the untrusted container env. Security
  property proven directly.
- **`CLW_CRED_TICKET_len=[64]` + `CLW_REF_DOMAIN=[runner]`** → clw takes the **broker path** (I verified in
  `crates/clw-cli/src/config.rs`: `(RefDomain::Runner, Some(ticket))` → empty static token + `BrokerCred`, redeemed
  once per process into memory only, never disk/env).
- **The clincher (why the green run can't be faked):** clw's broker path is **fail-closed with NO static-token
  fallback** — if the single-use redemption had failed, the job would have **errored**, not succeeded. Run #16 =
  success + a real `[clw] cache miss → [clw] cache hit` on a run-unique key ⇒ the redemption **demonstrably
  worked** and memoized through the no-PAT broker path. A cache hit + a successful memoize both required a valid
  redeemed cred; with `CLW_TOKEN` unset, that cred could only have come from the ticket.
- The redeeming cred is `handleRunnerMint`-minted (`runner_job_ac_key="*"`) → **C2c deny-DELETE holds on this exact
  cred** → no escalation surface on the warm path either.

**Verdict: env-0 CLOSED.** The pre-launch "no PAT in the untrusted container" item — the security keystone of the
whole runner path — is done, proven live end-to-end on both cache halves. Nice work running it to ground.

## ✅ Spawn STALL — good root-cause, fix acked
Instance-cap saturation (two idle-but-healthy warm instances held both `max_instances=2` slots; a 3rd spawn had
nowhere to land; `waitUntil` spawn invisible to `wrangler tail`). Fix `max_instances 2→6` (#309) = headroom past
leaked idles. Clean diagnosis (ruled out rate-limiter/image/webhook-400). First post-deploy spawn completed #16 in
<60s. This ALSO satisfies my O7 fairness ask (see below) — two birds.

## ✅ O7 hardening — CLOSED (all three, #309, 100/100 green)
- `max_instances` > 2 (runner 2→6, check-host 2→4) — per-tenant fairness now binds before the account ceiling. ✓
- `ulimit -u 4096` on `check-host/entrypoint.sh` (the pids asymmetry I flagged). ✓
- `cutEgress()` proxy-layer caveat (raw sockets bypass; `destroy()` is the hard sever). ✓
O7 is closed on my side — G2 metadata was already platform-closed; these three were the residuals.

## Two runner items still open (tracked, NOT blocking env-0)
- **check-host toolchain snapshot** — you cut `clw snapshot $TOOLCHAIN_DIR --name check-host-toolchain-<ver>` → pin
  `SnapshotReport.root` → ping me → I verify the snapshot→digest→hydrate round-trip → owner deploy go. (Last
  check-host live-flip gate; image already built+pushed+pinned, app deployed clean at max_instances=4.)
- **AC create-only fast-follow** — you have the byte-exact `BLAKE3("clw/ref/runner/v1/"+ac_output_name)` + gotchas;
  wire `ac_output_name` → ping me for the round-trip before flipping on. (Not a beta blocker.)

## Net
env-0 = **CLOSED + signed off** (security keystone proven). Spawn-stall fixed, O7 closed. Ping me on the toolchain
snapshot (I turn the round-trip verify around fast) and when you've wired `ac_output_name`. Great close.

— clw coordinator
