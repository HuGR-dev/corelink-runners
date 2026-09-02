#!/usr/bin/env bash
# fabricd-preflight — classify an already-captured fabricd failure.
#
# This is a read-only evidence classifier.  It deliberately does not call curl,
# wrangler, Docker, or Cloudflare: a preflight must not wake, restart, deploy,
# delete, or re-arm the service while it is trying to explain a failure.
#
# Usage:
#   scripts/ops/fabricd-preflight.sh --selftest
#   scripts/ops/fabricd-preflight.sh --fixture /path/to/capture.txt
#   scripts/ops/fabricd-preflight.sh /path/to/capture.txt
#   command-that-captured-evidence | scripts/ops/fabricd-preflight.sh --stdin
#
# The capture may contain provider output, HTTP headers/body, or fabricd
# stderr.  A mode is emitted only when a mode-specific signal is present;
# otherwise UNKNOWN is returned (exit 2) rather than guessing from a generic
# "Failed to start container" response.

set -euo pipefail

SCRIPT_NAME="${0##*/}"
MAX_EVIDENCE_BYTES=1048576

usage() {
  cat <<'USAGE'
fabricd-preflight — classify captured fabricd boot evidence (read-only)

Usage:
  fabricd-preflight.sh --selftest
  fabricd-preflight.sh --fixture FILE
  fabricd-preflight.sh FILE
  command | fabricd-preflight.sh --stdin

Modes and the exact next read:
  boot-FATAL  wrangler tail corelink-fabricd --format pretty
  image-pull  wrangler containers info <APP_ID>
  port-bind   wrangler containers instances <APP_ID>
  Access-403  curl -sS -D - -o /dev/null -m 30 <URL>/health

The four next-read commands are read-only. Replace <APP_ID> and <URL> with
values obtained from the operator's approved, fixed preflight. This command
itself never executes them and never performs a live probe.
USAGE
}

die_usage() {
  printf 'fabricd-preflight: %s\n' "$1" >&2
  printf 'usage: %s --selftest | --fixture FILE | FILE | --stdin\n' "$SCRIPT_NAME" >&2
  exit 1
}

read_fixture() {
  local fixture="$1"

  if [[ "$fixture" == "-" ]]; then
    # `dd` bounds input so an accidentally supplied log stream cannot exhaust
    # the shell's memory.  The final byte is enough to make truncation visible
    # in the evidence note without changing classification.
    EVIDENCE="$(dd bs=1 count="$MAX_EVIDENCE_BYTES" 2>/dev/null || true)"
  else
    [[ -f "$fixture" ]] || die_usage "fixture is not a regular file: $fixture"
    EVIDENCE="$(dd if="$fixture" bs=1 count="$MAX_EVIDENCE_BYTES" 2>/dev/null || true)"
  fi

  [[ -n "$EVIDENCE" ]] || {
    printf 'fabricd-preflight: empty evidence\n' >&2
    exit 2
  }
}

classify() {
  local evidence="$1"
  local normalized status

  # Matching is case-insensitive and line-oriented evidence is flattened only
  # for matching.  The original capture is never rewritten or sent anywhere.
  normalized="$(printf '%s' "$evidence" | tr '\r\n\t' '   ' | tr '[:upper:]' '[:lower:]')"
  status="$(printf '%s' "$normalized" | sed -nE \
    's/.*http\/[0-9.]+[[:space:]]+([0-9]{3}).*/\1/p' | head -n 1 || true)"
  if [[ -z "$status" ]]; then
    status="$(printf '%s' "$normalized" | sed -nE \
      's/.*http[[:space:]]+([0-9]{3}).*/\1/p' | head -n 1 || true)"
  fi
  if [[ -z "$status" ]]; then
    status="$(printf '%s' "$normalized" | sed -nE \
      's/.*"?(status|http_code|httpstatus)"?[=:[:space:]]+"?([0-9]{3}).*/\2/p' | head -n 1 || true)"
  fi
  if [[ -z "$status" ]] && [[ "$normalized" =~ (^|[^0-9])403([^0-9]|$) ]]; then
    status=403
  fi

  MODE=UNKNOWN
  SIGNAL='no mode-specific signal'
  NEXT='do not infer a cause; obtain an approved capture with the evidence ladder'

  # Access must be a 403 with an edge/service-token signal.  A generic authz
  # 403 is intentionally UNKNOWN: it is not evidence of a Cloudflare Access
  # failure and must not be mistaken for one.
  if [[ "$status" == "403" ]] && [[ "$normalized" =~ (cf[-[:space:]]*access|cloudflare[[:space:]]+access|service[[:space:]]+token|cf[-[:space:]]*mitigated|access[[:space:]]+denied) ]]; then
    MODE=Access-403
    SIGNAL='HTTP 403 with a Cloudflare Access/service-token signal'
    NEXT='curl -sS -D - -o /dev/null -m 30 <URL>/health'
  # Pull failures are provider/image errors and must win over a generic Error:
  # line from a wrapper.
  elif [[ "$normalized" =~ (failed[[:space:]]+to[[:space:]]+(pull|resolve)[[:space:]]+image|image[[:space:]]+(pull|pullbackoff)|errimagepull|pull[[:space:]]+access[[:space:]]+denied|manifest[[:space:]]+(unknown|not[[:space:]]+found)|no[[:space:]]+such[[:space:]]+image|failed[[:space:]]+to[[:space:]]+fetch[[:space:]]+image) ]]; then
    MODE=image-pull
    SIGNAL='container image retrieval or registry resolution failed'
    NEXT='wrangler containers info <APP_ID>'
  elif [[ "$normalized" =~ (address[[:space:]]+already[[:space:]]+in[[:space:]]+use|failed[[:space:]]+to[[:space:]]+bind|cannot[[:space:]]+bind|bind\(\)[[:space:]]+failed|listen[[:space:]]+tcp|did[[:space:]]+not[[:space:]]+open[[:space:]]+port|port[[:space:]]+[0-9]+[[:space:]]+(is[[:space:]]+)?(unavailable|already[[:space:]]+in[[:space:]]+use)) ]]; then
    MODE=port-bind
    SIGNAL='the process could not bind/listen on its declared port'
    NEXT='wrangler containers instances <APP_ID>'
  elif [[ "$normalized" =~ (boot[-[:space:]]*fatal|fatal([:[:space:]]|$)|thread[[:space:]]+.*panicked|panic!|panic[[:space:]]+at|configuration[[:space:]]+(error|invalid)|cannot[[:space:]]+acquire[[:space:]]+connection|failed[[:space:]]+to[[:space:]]+(initialize|load)[[:space:]]+.*(before|during)[[:space:]]+boot|pre[-[:space:]]*bind) ]]; then
    MODE=boot-FATAL
    SIGNAL='fabricd initialization failed before the listener became available'
    NEXT='wrangler tail corelink-fabricd --format pretty'
  fi
}

emit_result() {
  printf 'MODE=%s\n' "$MODE"
  printf 'SIGNAL=%s\n' "$SIGNAL"
  printf 'NEXT_READ=%s\n' "$NEXT"
  if [[ "$MODE" == "UNKNOWN" ]]; then
    printf 'RESULT=UNCLASSIFIED (fail-closed; no cause inferred)\n'
  else
    printf 'RESULT=CLASSIFIED\n'
  fi
}

run_selftest() {
  local name expected i
  local -a names=(boot-fatal image-pull port-bind access-403)
  local -a expected_modes=(boot-FATAL image-pull port-bind Access-403)
  local -a fixtures

  fixtures[0]=$'2026-08-31T00:00:00Z ERROR: PgLedger: cannot acquire connection for DDL before bind'
  fixtures[1]=$'container: failed to pull image registry.example/fabricd@sha256:deadbeef (manifest unknown)'
  fixtures[2]=$'fabricd: failed to bind 0.0.0.0:8080: address already in use'
  fixtures[3]=$'HTTP/2 403 Forbidden\ncf-access: service token rejected\naccess denied'

  for i in "${!names[@]}"; do
    name="${names[$i]}"
    expected="${expected_modes[$i]}"
    classify "${fixtures[$i]}"
    if [[ "$MODE" != "$expected" ]]; then
      printf 'SELFTEST=FAIL fixture=%s expected=%s actual=%s\n' "$name" "$expected" "$MODE" >&2
      return 1
    fi
    printf 'SELFTEST=PASS fixture=%s mode=%s\n' "$name" "$MODE"
  done

  classify $'HTTP/2 500\nFailed to start container'
  if [[ "$MODE" != "UNKNOWN" ]]; then
    printf 'SELFTEST=FAIL fixture=generic-edge expected=UNKNOWN actual=%s\n' "$MODE" >&2
    return 1
  fi
  printf 'SELFTEST=PASS fixture=generic-edge mode=UNKNOWN\n'
  classify $'HTTP/2 403 Forbidden\nnot authorized for this tenant'
  if [[ "$MODE" != "UNKNOWN" ]]; then
    printf 'SELFTEST=FAIL fixture=generic-403 expected=UNKNOWN actual=%s\n' "$MODE" >&2
    return 1
  fi
  printf 'SELFTEST=PASS fixture=generic-403 mode=UNKNOWN\n'
  printf 'SELFTEST=PASS\n'
}

main() {
  local fixture=''

  [[ "$#" -gt 0 ]] || die_usage 'an evidence fixture is required (no network probe is implicit)'
  case "$1" in
    --help|-h)
      [[ "$#" -eq 1 ]] || die_usage '--help does not accept additional arguments'
      usage
      return 0
      ;;
    --selftest)
      [[ "$#" -eq 1 ]] || die_usage '--selftest does not accept additional arguments'
      run_selftest
      return $?
      ;;
    --fixture)
      [[ "$#" -eq 2 ]] || die_usage '--fixture requires exactly one file'
      fixture="$2"
      ;;
    --fixture=*)
      [[ "$#" -eq 1 ]] || die_usage '--fixture accepts exactly one file'
      fixture="${1#--fixture=}"
      [[ -n "$fixture" ]] || die_usage '--fixture requires a non-empty file'
      ;;
    --stdin)
      [[ "$#" -eq 1 ]] || die_usage '--stdin does not accept additional arguments'
      fixture='-'
      ;;
    --*)
      die_usage "unknown option: $1"
      ;;
    *)
      [[ "$#" -eq 1 ]] || die_usage 'a single fixture is accepted'
      fixture="$1"
      ;;
  esac

  read_fixture "$fixture"
  classify "$EVIDENCE"
  emit_result
  if [[ "$MODE" == "UNKNOWN" ]]; then
    return 2
  fi
  return 0
}

main "$@"
