# MASTER-ASK → corelink-runners TL — everything I need from you, one shot (cold-sweep-verified). One BLOCKER, one check-host gate, two hardenings. Checklist; ping me per item.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06

## 🔴 BLOCKER — env-0 exit test not green yet (the SPAWN STALL is the real blocker)
Verified: #307 + #308 are merged; the **no-PAT half is CONFIRMED** (exit test proved `CLW_TOKEN` unset + ticket
reaching clw). But the **`[clw] cache hit` half has NEVER passed** — the pre-#308 run MISSED ("missing token"),
#308 fixes the ticket-clobber cause but its own commit body says it does **NOT** fix the underlying **spawn STALL**,
and no post-#308 run records a green HIT. Also: no in-repo/CI proof the post-#308 worker was actually deployed to prod.
- [ ] Deploy the post-#308 worker to prod (confirm the wrangler deploy ran).
- [ ] Root-cause + fix the **spawn STALL** (the exit test can't fully pass until a spawn succeeds).
- [ ] Re-run the exit test in a live lease → capture the **combined green line**: `/proc/self/environ` shows NO CAS
      PAT **AND** `[clw] cache hit` together. That single line closes the "no PAT in the untrusted env" pre-launch item.

## 🟡 CHECK-HOST live-flip gate — produce the toolchain snapshot
Image is BUILT + PUSHED + DIGEST-PINNED on clw v0.1.5 glibc (done). The remaining gate is the **real toolchain
snapshot** — today `toolchain_digest` is only test fixtures; no real snapshot is pinned.
- [ ] `clw snapshot $TOOLCHAIN_DIR --name check-host-toolchain-<ver>` under the runner `CLW_*` identity → pin
      `SnapshotReport.root` as the `toolchain_ref`.
- [ ] Ping me → I verify the snapshot→digest→hydrate round-trip (re-hash matches root + clean materialize, no
      path-escape). That + the owner deploy go = the last check-host live-flip gate.

## 🟢 O7 HARDENING (multi-tenant readiness — my flag, your build)
The sweep found the isolation posture is honest but has real residuals at multi-tenant scale:
- [ ] **Raise `wrangler.jsonc max_instances` above 2** (or an autoscaling ceiling) BEFORE multi-tenant go-live — at
      `max_instances:2` the account ceiling saturates before the per-tenant fairness gate ever binds (the #226 503
      class). This is the one with go-live weight.
- [ ] Add the runner's `ulimit -u` pids bound to `deploy/check-host/entrypoint.sh` (it has none today — asymmetry vs
      the runner path) OR document why check-host is exempt.
- [ ] Add a one-line caveat at `cutEgress()` that it's an SDK-proxy-layer cut (raw sockets bypass; `destroy()` is the
      only hard sever) — so operators aren't misled. (G2 metadata is genuinely closed at the platform — no action.)

## 🟢 AC create-only fast-follow (recipe already delivered)
Deny-DELETE is CLOSED on env-0. For the create-only/prefix narrowing: I handed you clw's byte-exact key derivation
`BLAKE3("clw/ref/runner/v1/" ++ name)` (raw 32B, lowercase-hex on the wire; 3 gotchas in that doc).
- [ ] Plumb `ac_output_name` into the mint → ping me → I verify the mint-blake3 == clw-write-key round-trip before flip.

**That's everything from you.** The env-0 spawn-stall is the one true blocker; the rest is check-host + hardening.
Ping me per checkbox.

— clw coordinator
