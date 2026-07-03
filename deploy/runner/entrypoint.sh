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
  echo "cache-warm: CLW_* injected — hydrating build cache from the CoreLink CAS (in-network)…"
  if command -v clw >/dev/null 2>&1; then
    if clw hydrate "${CLW_CACHE_DEST:-$HOME/.cache/corelink}" \
         --name "${CLW_CACHE_KEY:-runner-cache}"; then
      echo "cache-warm: hydrate OK — warm run."
    else
      echo "cache-warm: hydrate failed — proceeding COLD (north-star: slow, never broken)." >&2
    fi
  else
    echo "cache-warm: clw not found in image — proceeding COLD." >&2
  fi
else
  echo "cache-warm: CLW_* not injected — cold run (moat off)."
fi

# ── App-layer resource caps (Track-C C2 in-image equivalent) ──────────────────
# The microVM (per-lease Firecracker on the CF substrate) is the isolation
# BOUNDARY, but the microVM-alone posture was REFUSED: a runaway job inside our
# VM must be BOUNDED at the app layer too (defense-in-depth), mirroring the
# on-box DockerEngine hardening (crates/corelink-runner/src/isolation.rs:179-197:
# --cap-drop ALL / --pids-limit / --memory). We cannot pass `docker run` flags on
# the CF substrate (the container IS the VM), so we apply the equivalent ceilings
# with `ulimit` in this shell BEFORE dropping into the untrusted job. Because we
# `exec ./run.sh` below, these limits are inherited by the runner and every job
# step it spawns.
#
#   ulimit -u  (max user processes)  → fork-bomb bound. Mirrors PIDS_LIMIT=4096.
#   ulimit -v  (max virtual memory)  → memory bound set NEAR the standard-4
#                                      envelope (12 GiB, MEMORY_LIMIT) so a
#                                      runaway is capped JUST UNDER the VM without
#                                      breaking a normal heavy build.
#
# TUNABLE (flag for the TL): these are grounded to the standard-4 instance
# (12 GiB RAM / 4 vCPU — the same envelope the on-box MEMORY_LIMIT/PIDS_LIMIT are
# validated against). If the instance type changes, re-ground both. `ulimit -v`
# is a per-process ADDRESS-SPACE cap (KiB), not a cgroup total; 12 GiB is the
# ceiling — set slightly under the VM so the OOM killer inside the VM is the last
# resort, not the first. Overridable via the two env vars below.
#
# Fail-OPEN, matching the moat north-star: a ulimit that the platform refuses to
# set (e.g. a hard limit already lower) is logged and swallowed — never a broken
# job. `|| true` guards each set; we lower toward the ceiling, never raise.
RUNNER_ULIMIT_NPROC="${RUNNER_ULIMIT_NPROC:-4096}"        # pids/fork-bomb bound
RUNNER_ULIMIT_AS_KIB="${RUNNER_ULIMIT_AS_KIB:-12582912}"  # 12 GiB = 12*1024*1024 KiB
if ulimit -u "$RUNNER_ULIMIT_NPROC" 2>/dev/null; then
  echo "caps: ulimit -u ${RUNNER_ULIMIT_NPROC} (fork-bomb bound) set."
else
  echo "caps: ulimit -u refused (kept inherited limit) — proceeding (fail-open)." >&2
fi
if ulimit -v "$RUNNER_ULIMIT_AS_KIB" 2>/dev/null; then
  echo "caps: ulimit -v ${RUNNER_ULIMIT_AS_KIB} KiB (~12 GiB memory bound) set."
else
  echo "caps: ulimit -v refused (kept inherited limit) — proceeding (fail-open)." >&2
fi

# ── Launch: ephemeral one-shot JIT mode ───────────────────────────────────────
# --jitconfig   : modern JIT path (runner ≥ v2.294.0); the token encodes
#                 registration, org, repo, labels, and a one-time use secret.
# Ephemeral + self-deregistering: the runner exits cleanly after one job and
# removes itself from the runner pool.  The fabric tears down the box on exit.
#
# We exec (replace shell) so signals pass cleanly to the runner process.
# The value of CORELINK_RUNNER_JITCONFIG is passed as an argument — it is
# NEVER echoed, logged, or expanded into a visible string here.
exec ./run.sh --jitconfig "$CORELINK_RUNNER_JITCONFIG"
