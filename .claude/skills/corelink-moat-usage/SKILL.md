---
name: corelink-moat-usage
version: 0.1.0
description: How to USE the CoreLink cache moat correctly — the hands-on playbook, written after actually running it end-to-end (sccache→CoreLink on iceberg-rust, clw whole-build benchmarks, cost analysis). Covers picking the right layer for the job, the WORKING sccache→CoreLink WebDAV recipe (env-0 redeem → cas:rw → SCCACHE_WEBDAV_*), the measurement traps that produce fake numbers (write-error contamination, per-crate keying, identical-command requirement), the real cost/COGS math (Cloudflare rates) and the honest GitHub comparison, and the don't-overclaim rules. Invoke whenever setting up, benchmarking, pitching, or explaining CoreLink caching for a real workload — so you use the right tool and quote honest numbers.
---

# corelink-moat-usage — the hands-on playbook (use the right layer, quote honest numbers)

Companion to `corelink-moat` (what the layers ARE) and `moat-benchmark` (how to dispatch a run).
This is what I learned actually running it 2026-07-21.

## 1. Pick the right layer for the job
- **Same inputs, run again** (CI retries, matrix legs, unchanged-PR re-checks) → **whole-build memoize**
  (`clw run` / the `corelink-memoize` Action). Identical inputs → the whole build is skipped (~2s).
  ANY change → full rebuild (all-or-nothing). Measured: ripgrep 17×, dep-tree 25×.
- **Change some files, reuse the rest** → **fine-grained: sccache (cargo/Rust) or Bazel/Buck2 (REAPI)**.
  Per-compile-unit cache. Do NOT use `clw run` for this — it whole-rebuilds on any change (proven:
  1-fn change → 53s full rebuild, run 29850459289).

## 2. The WORKING sccache → CoreLink recipe (Rust/cargo, proven live)
Backend is **WebDAV** at `/cargo/<tenant>` (a flat per-tenant KV keyed by sccache's own key — NOT the
content CAS). On a `runs-on: corelink` box, get a `cas:rw` PAT by redeeming the env-0 ticket, then:
```bash
# redeem the box's env-0 cred-ticket -> cas:rw PAT + tenant (multi-use within the lease)
resp=$(curl -sS -X POST "$CLW_FABRIC_ENDPOINT/v1/leases/$CLW_LEASE_ID/cas-cred" \
        -H 'content-type: application/json' -d "{\"ticket\":\"$CLW_CRED_TICKET\"}")
#   -> {cas_pat, clw_endpoint, clw_tenant}   (route: index.ts ~1296; body key is "ticket")
export SCCACHE_WEBDAV_ENDPOINT="https://corelink-api.humangr.com/cargo/<clw_tenant>"
export SCCACHE_WEBDAV_TOKEN="<cas_pat>"     # MUST be cas:rw — a read-only PAT 403s every PUT (0% fill)
export RUSTC_WRAPPER="$(command -v sccache)" CARGO_INCREMENTAL=0   # sccache + incremental don't mix
sccache --start-server && cargo build --release && sccache --show-stats
```
Gotchas that cost real time: (a) `echo $HOME/bin >> $GITHUB_PATH` only affects LATER steps — install +
use sccache in the SAME step or `export PATH` inline; (b) the acquiring PAT in `cold-tenant.json` is
**read-only** → PUTs 403; mint/redeem a `cas:rw`; (c) sccache keys **per rustc invocation ≈ per crate**.
Bazel/Buck2 path + endpoints: see `corelink-moat`. sccache config from server-TL:
`docs/handoff/2026-07-21-reply-server-TL-sccache-webdav-config-LIVE-for-3c7d77b1.md`.

## 3. Measurement traps (each produced a FAKE number before I caught it)
- **Write errors contaminate the "changed" hit-rate.** iceberg first run showed 75.89% — but 57 of the
  68 "misses" were COLD upload write-errors, NOT the code change. The clean run showed the truth:
  **98–99.65% reuse** (only the changed crate recompiled). ALWAYS check `Cache write errors` on the COLD;
  if non-zero, the WARM hit-rate is understated and "changed → X%" is contaminated. Re-run until COLD writes clean.
- **A real dep-heavy project reuses 95–99% on a change, NOT ~75%.** Deps dominate the unit count and
  sccache keys per crate → editing one crate misses ~1 unit. The "75/25" mental model is wrong for real
  projects; quote the real high number.
- **Identical COLD/WARM command** (clw folds the command into the key) or WARM is a false miss.
- **Run-unique marker** in a source file so COLD is a genuine fresh miss (else it hits a prior run).
- **clw skips the command on a HIT** → in-command `date` timers don't fire on WARM; read GH step timestamps.
- **A warm cross-run cache** makes a later "COLD" hit ~98% — that's real cross-machine reuse, but it's not
  a 0% baseline. Wipe/rotate the key if you need a true cold.

## 4. The real cost math (measured, not modeled)
Box = **standard-4** (4 vCPU / 12 GiB / 20 GB). Cloudflare Containers rate =
`4×$0.000020 + 12×$0.0000025 + 20×$0.00000007 = $0.0001114/box-second = $0.40/h` (CF bills only ACTIVE
seconds + free tier 375 vCPU-min/mo). Cost = real box time × rate. Measured: the whole benchmark session
(10 runs, 20.8 min box) = **$0.14**; big iceberg build = **$0.076**; a cache-hit re-run ≈ **$0.0002**.
Users pay **flat concurrency, never per-minute** ($16/mo entry, ratified) → a cached re-run is $0 to them.
The margin IS the cache: we pay CF only for active seconds, cache hits collapse them to ~2s.

## 5. Honest positioning vs GitHub (don't overclaim)
GitHub HAS caching: `actions/cache` (coarse, per-repo, 10 GB LRU cap) + sccache with the GHA backend
(per-crate). So the MECHANISM isn't exclusive. CoreLink wins at SCALE: cross-repo/cross-team chunk-dedup,
no 10 GB cap, cache-warm in-network boot (co-located R2, 0 egress), REAPI-native (Bazel/Buck2), and flat
concurrency vs per-minute (GitHub 4-core $0.016/min re-bills every re-run; a ~6-min build = $0.096, again
on every re-run). Say "CoreLink does the parts that dominate the bill at scale," not "GitHub can't cache."

## 6. The rule
Quote the corrected number, not the flattering one. Every stat cites a run URL + a verbatim log line
(evidence artifact: the moat-evidence-verified page). test-green ≠ live-proven; a claim without a cited
artifact I checked myself is theory. See [[skeptic-evidence-or-theory]].
