#!/bin/sh
# DIAGNOSTIC ONLY — see Dockerfile.bootprobe. Reports the container's own fate to
# the fabricd Worker's request log, because that is the only log surface reachable
# from this session. Payload rides the URL PATH: `wrangler tail` shows
# event.request.url and does not show bodies.
#
# Every beacon is best-effort and non-fatal: a probe that changes the outcome it is
# measuring is worthless, so nothing here may abort or delay the real binary beyond
# the fixed 5s liveness sample, and every curl swallows its own failure.

BEACON="${BOOTPROBE_BEACON:-https://corelink-fabricd.gmhelmold.workers.dev/__bootprobe}"

beacon() {
  curl -s -m 8 -o /dev/null "${BEACON}/$1" 2>/dev/null || true
}

beacon "1-entrypoint-running"

/usr/local/bin/corelink-fabricd 2>/tmp/fabricd.stderr &
PID=$!

# One liveness sample. If the binary is still up here, it got past its pre-bind
# steps and the question moves to whether the port is reachable; if it is already
# gone, the stderr captured below is the whole answer.
sleep 5
if kill -0 "$PID" 2>/dev/null; then beacon "2-alive-yes"; else beacon "2-alive-no"; fi

wait "$PID"
RC=$?

# Head of stderr, base64'd (no newlines) so it survives a URL path. 300 bytes is
# enough for an anyhow error chain and stays well inside URL limits.
ERR=$(head -c 300 /tmp/fabricd.stderr 2>/dev/null | base64 2>/dev/null | tr -d '\n')
beacon "3-exit-${RC}/${ERR}"

# Hold the container so the beacons are not raced by an immediate teardown, and so
# the instance is visible as `running` in `wrangler containers instances` — itself
# a signal that execution works.
sleep 3600
