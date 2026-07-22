#!/usr/bin/env bash
# entrypoint.sh — CoreLink ephemeral runner container entrypoint (ADR-0007 Stage A)
#
# CONTRACT:
#   - $CORELINK_RUNNER_JITCONFIG must be set (JIT config token from the fabric).
#   - Exits non-zero immediately if the env var is absent or empty.
#   - Launches the runner in one-shot ephemeral JIT mode; self-deregisters on exit.
#   - NEVER prints the value of $CORELINK_RUNNER_JITCONFIG to stdout/stderr.
#
# Usage (by the CoreLink fabric — not by humans):
#   docker run --rm \
#     -e CORELINK_RUNNER_JITCONFIG="<jit-config-token>" \
#     <image>
set -euo pipefail

# ── Guard: JIT config must be present ─────────────────────────────────────────
if [[ -z "${CORELINK_RUNNER_JITCONFIG:-}" ]]; then
  echo "ERROR: CORELINK_RUNNER_JITCONFIG is not set or is empty." >&2
  echo "       The CoreLink fabric must inject this env var at provision time." >&2
  echo "       This image will NOT idle — exiting with code 1." >&2
  exit 1
fi

# ── Safety: ensure we're in the runner directory ──────────────────────────────
cd "$(dirname "$0")"

# ── Cache-warm preflight (the moat) — fail-OPEN to a COLD run ─────────────────
# When the fabric injects the CLW_* moat env (in-network CoreLink CAS endpoint +
# the per-job CAS PAT), warm the build cache from the CAS via `clw` BEFORE the
# job. This is the differentiator: cache-warm by construction.
#
# NORTH STAR (hard invariant): the cache is an OPTIMIZATION over a correct cold
# run. Cache absent / unreachable / clw-error ⇒ a SLOW (cold) run, NEVER a broken
# one. EVERY failure here is logged and SWALLOWED; the job always proceeds. The
# moat is off entirely when CLW_* is not injected (today's cold dogfood path, and
# until the D-9 per-job mint + the in-network CAS endpoint are wired).
#
# CLW_TOKEN is a sensitive per-job PAT (A6) — `clw` reads it from the env; it is
# NEVER echoed or expanded into a visible string here.
#
# Track-C C2c ticket posture: the moat now arms on EITHER credential form —
#   • CLW_TOKEN     : a per-job CAS PAT (the original A6 path), OR
#   • CLW_CRED_TICKET: a single-use, lease-bound ticket (C2c env-0). clw OWNS
#     redemption: it selects the broker/redeem path iff CLW_REF_DOMAIN=runner AND
#     CLW_CRED_TICKET is present, redeeming against
#     {CLW_FABRIC_ENDPOINT}/v1/leases/{id}/cas-cred to obtain the real CAS cred.
# We do NOT redeem here; we only gate on presence and hand the env to clw.
#
# HARD INVARIANT — at most ONE credential-consuming clw process per container.
# The ticket is SINGLE-USE: the fabric returns 410 (Gone) on a 2nd redeem, so a
# second clw invocation that tries to redeem the same ticket would fail closed.
# This container runs exactly one such clw process (the hydrate below) before the
# untrusted job; the job itself must not be handed a live redeemable ticket.
if [[ -n "${CLW_ENDPOINT:-}" && ( -n "${CLW_CRED_TICKET:-}" || -n "${CLW_TOKEN:-}" ) ]]; then
  echo "cache-warm: CLW_* injected — pre-warming build cache from the CoreLink CAS (in-network, background)…"
  if command -v clw >/dev/null 2>&1; then
    # Run the BOOT pre-warm in the BACKGROUND. It is a best-effort pre-warm; the WARM
    # job step's OWN `clw run` is the AUTHORITATIVE memoize (the `[clw] cache hit`
    # capstone). Two failure modes forced this (both root-caused 2026-07-21):
    #   1. A slow/retrying FOREGROUND hydrate (3× redemptions, ~18s) starved the GH
    #      runner's registration+claim window → the box was reaped before claiming the
    #      job → the job never ran.
    #   2. A `timeout` cap was WORSE: SIGKILL mid-op leaked clw's per-container lock and
    #      the job's own `clw run` then hung on it (COLD step stuck → job failure).
    # Backgrounding fixes both: `./run.sh` starts AT ONCE (fast, reliable registration),
    # and the pre-warm completes + releases the lock on its OWN — the job's `clw run`
    # serialises behind it, never leaked, never killed. Fail-OPEN (north star).
    ( clw hydrate "${CLW_CACHE_DEST:-$HOME/.cache/corelink}" \
        --name "${CLW_CACHE_KEY:-runner-cache}" \
      && echo "cache-warm: background hydrate OK." \
      || echo "cache-warm: background hydrate failed — COLD (harmless, north star)." >&2 ) &
  else
    echo "cache-warm: clw not found in image — proceeding COLD." >&2
  fi
else
  echo "cache-warm: CLW_* not injected — cold run (moat off)."
fi

# ── App-layer resource caps (Track-C C2 in-image equivalent) ──────────────────
# The microVM (per-lease Firecracker on the CF substrate) is the isolation
# BOUNDARY. We add ONE app-layer bound as defense-in-depth, mirroring the fork
# bomb cap of the on-box DockerEngine hardening (the `DockerEngine`
# `Engine::run` impl in crates/corelink-runner/src/isolation.rs, which passes
# `--cap-drop ALL` / `--pids-limit` (the PIDS_LIMIT const) / `--memory` (the
# MEMORY_LIMIT const)). We cannot pass `docker run` flags on the CF substrate
# (the container IS the VM), so we apply the pids ceiling with `ulimit` in this
# shell BEFORE dropping into the untrusted job. Because we `exec ./run.sh`
# below, this limit is inherited by the runner and every job step it spawns.
#
#   ulimit -u  (max user processes)  → fork-bomb bound. Mirrors PIDS_LIMIT=4096.
#
# MEMORY — there is intentionally NO app-layer per-process RSS cap here, and this
# is deliberate (an earlier `ulimit -v` 12 GiB line was REMOVED per cold review):
#   • `ulimit -v` caps per-process VIRTUAL address space, NOT resident memory.
#     Legitimate JVM / Go / ASAN / heavy-linker jobs RESERVE huge virtual space
#     far above their real RSS and would be KILLED spuriously — it breaks real
#     jobs while still NOT containing a true runaway (it is per-process, not
#     container-total; N processes each just under the cap blow past it).
#   • `RLIMIT_RSS` (ulimit -m) is a NO-OP on modern Linux kernels — the kernel
#     ignores it, so it cannot cap real memory either.
#   • The CF substrate exposes NO per-container cgroup memory knob we can set
#     from inside the guest.
# There is therefore NO valid app-layer per-process RSS cap on this substrate.
# Memory containment lives ENTIRELY at the isolation boundary: each lease is its
# OWN Firecracker microVM sized to the instance envelope (standard-4: 12 GiB /
# 4 vCPU), with the in-VM OOM-killer as the backstop. A memory bomb OOMs its OWN
# VM and dies — the blast radius is that single lease; it cannot reach a
# neighbor. (`--memory`/MEMORY_LIMIT in isolation.rs is the on-box Docker
# analogue of that same per-instance envelope, not a per-process cap.)
#
# TUNABLE (flag for the TL): the pids bound is grounded to the standard-4
# instance (12 GiB RAM / 4 vCPU — the same envelope the on-box PIDS_LIMIT is
# validated against). Overridable via the env var below.
#
# Fail-OPEN, matching the moat north-star: a ulimit that the platform refuses to
# set (e.g. a hard limit already lower) is logged and swallowed — never a broken
# job. We lower toward the ceiling, never raise.
RUNNER_ULIMIT_NPROC="${RUNNER_ULIMIT_NPROC:-4096}"        # pids/fork-bomb bound
if ulimit -u "$RUNNER_ULIMIT_NPROC" 2>/dev/null; then
  echo "caps: ulimit -u ${RUNNER_ULIMIT_NPROC} (fork-bomb bound) set."
else
  echo "caps: ulimit -u refused (kept inherited limit) — proceeding (fail-open)." >&2
fi

# ── Launch: ephemeral one-shot JIT mode ───────────────────────────────────────
# --jitconfig   : modern JIT path (runner ≥ v2.294.0); the token encodes
#                 registration, org, repo, labels, and a one-time use secret.
# Ephemeral + self-deregistering: the runner exits cleanly after one job and
# removes itself from the runner pool.  The fabric tears down the box on exit.
#
# Diagnostic-capable launch: run under `tee` (not `exec`) so a non-zero exit —
# e.g. a JIT registration failure, which is otherwise INVISIBLE (the container has
# only CF-internal egress + no `wrangler containers logs`) — can be surfaced. On
# failure we POST the tail of run.sh's output to the Worker's /runner-diag sink
# (reachable via CLW_FABRIC_ENDPOINT, the same host the cred-ticket redeems against),
# which `logEvent`s it into `wrangler tail`. run.sh NEVER echoes the jitconfig value,
# so the captured output carries no secret. The value is still passed as an argument
# only — never expanded into a visible string here.
set +e
./run.sh --jitconfig "$CORELINK_RUNNER_JITCONFIG" 2>&1 | tee /tmp/runsh.out
rc=${PIPESTATUS[0]}
# Keepable observability: on a NON-ZERO runner exit (e.g. a JIT registration failure)
# POST the output tail to the Worker's /runner-diag sink so it surfaces in `wrangler
# tail` — the container has no external log path (this is exactly how the 2026-07-21
# box-registration root-cause was found). Carries no secret (run.sh never echoes the
# jitconfig). No-op on success or when the env-0 vars are absent.
if [[ "$rc" -ne 0 && -n "${CLW_FABRIC_ENDPOINT:-}" && -n "${CLW_LEASE_ID:-}" ]]; then
  { printf 'run.sh exit=%s\n---output tail---\n' "${rc}"; tail -c 2800 /tmp/runsh.out 2>/dev/null; } | curl -s -m 10 -X POST \
    "${CLW_FABRIC_ENDPOINT}/v1/leases/${CLW_LEASE_ID}/runner-diag" \
    -H "content-type: text/plain" --data-binary @- >/dev/null 2>&1 || true
fi
exit "$rc"
