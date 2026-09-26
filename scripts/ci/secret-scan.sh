#!/usr/bin/env bash
# Scan an explicit commit range with the pinned gitleaks release.
set -euo pipefail
readonly GITLEAKS_VERSION='8.30.1'
readonly GITLEAKS_ASSET="gitleaks_${GITLEAKS_VERSION}_linux_x64.tar.gz"
readonly GITLEAKS_URL="https://github.com/gitleaks/gitleaks/releases/download/v${GITLEAKS_VERSION}/${GITLEAKS_ASSET}"
readonly GITLEAKS_SHA256='551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb'
usage() { printf 'usage: %s --repo DIR --base SHA --head SHA [--scanner PATH]\n' "$0"; }
repo=''; base=''; head=''; scanner=''; scanner_override=0
while (($#)); do
  case "$1" in
    --repo) [[ $# -ge 2 ]] || { usage >&2; exit 2; }; repo=$2; shift 2 ;;
    --base) [[ $# -ge 2 ]] || { usage >&2; exit 2; }; base=$2; shift 2 ;;
    --head) [[ $# -ge 2 ]] || { usage >&2; exit 2; }; head=$2; shift 2 ;;
    --scanner) [[ $# -ge 2 ]] || { usage >&2; exit 2; }; scanner=$2; scanner_override=1; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done
[[ -n "$repo" && -n "$base" && -n "$head" ]] || { usage >&2; exit 2; }
[[ "$base" =~ ^[0-9a-fA-F]{40}$ && "$head" =~ ^[0-9a-fA-F]{40}$ ]] || { printf 'secret-scan: invalid commit IDs\n' >&2; exit 2; }
repo="$(cd -- "$repo" && pwd)"
git -C "$repo" rev-parse --git-dir >/dev/null 2>&1 || { printf 'secret-scan: repository is not a git checkout\n' >&2; exit 2; }
if [[ "$base" != '0000000000000000000000000000000000000000' ]]; then
  git -C "$repo" cat-file -e "${base}^{commit}" 2>/dev/null || { printf 'secret-scan: base commit is unavailable\n' >&2; exit 2; }
fi
git -C "$repo" cat-file -e "${head}^{commit}" 2>/dev/null || { printf 'secret-scan: head commit is unavailable\n' >&2; exit 2; }
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/corelink-secret-scan.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
trap 'trap - HUP; exit 129' HUP
trap 'trap - INT; exit 130' INT
trap 'trap - TERM; exit 143' TERM
config="$work_dir/gitleaks.toml"; ignore_file="$work_dir/gitleaksignore"
report="$work_dir/report.json"; log="$work_dir/scanner.log"
printf '[extend]\nuseDefault = true\n' >"$config"
# These 20 fingerprints are the reviewed deterministic fixture/reference
# findings from the frozen cda90940..52197fd0 range. Fingerprints are the
# narrowest supported exception: a changed value, commit, path or detector
# produces a different fingerprint and remains visible to the scanner.
# #604's shared billing ACK fixture uses a synthetic repeated-character
# idempotency key; these exact findings are fixture data, not credentials.
# Its devenv test fixture has this separately reviewed synthetic finding.
cat >"$ignore_file" <<'EOF'
1400ebbc5c3f0e95d0d813e412bdf15757fccc92:deploy/cloudflare/test/compute-terminal-test-helpers.ts:generic-api-key:3
014c07ce43b51c9a17b88bf9f915faeb4a499cc3:docs/handoff/2026-09-06-compaction-checkpoint.md:generic-api-key:43
10181a778dfc7c5479d240d43b4b1fe341884414:deploy/cost-monitor/test/fixtures/trusted-time/pki/tsa-bad-eku.key:private-key:1
10181a778dfc7c5479d240d43b4b1fe341884414:deploy/cost-monitor/test/fixtures/trusted-time/pki/tsa-good.key:private-key:1
090c5f3e1b483f9729d2bdc914b5157f37bd00b6:conformance/spawn-worker-billing-wire.json:generic-api-key:1
1c73050d5c5e9377e711eb64534142970ef7804b:crates/corelink-fabric-server/conformance/fabric-billing-wire.json:generic-api-key:1
16b2b5c2968b91282d4ad41d52002b5ae961cfb4:crates/corelink-fabric-server/conformance/fabric-billing-wire.json:generic-api-key:1
49896886f8425aef7d49326fa8cc89849bc00d1b:deploy/cloudflare/test/runner-credential-adoption.test.ts:generic-api-key:41
67fa575d0d3fecd88bf98849f942b48b002b9660:docs/handoff/2026-09-06-techlead-takeover.md:generic-api-key:26
67fa575d0d3fecd88bf98849f942b48b002b9660:docs/handoff/2026-09-06-techlead-takeover.md:generic-api-key:30
64a83e4d3a03f3cbc4e89683eeb662ba7ea3c77b:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:161
64a83e4d3a03f3cbc4e89683eeb662ba7ea3c77b:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:263
64a83e4d3a03f3cbc4e89683eeb662ba7ea3c77b:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:305
64a83e4d3a03f3cbc4e89683eeb662ba7ea3c77b:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:344
64a83e4d3a03f3cbc4e89683eeb662ba7ea3c77b:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:467
9b87d3b5e588fe19357741fb76c270bad3fe7437:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:467
d741e623c22c5bd6220d862bfcad6964ff2dfaa8:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:151
d741e623c22c5bd6220d862bfcad6964ff2dfaa8:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:294
d741e623c22c5bd6220d862bfcad6964ff2dfaa8:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:330
91af0ac8a08ae8199bf9ed7f57a6bbdb8d7bd3e0:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:234
982f5300a5005689f0ded5026f2e80a282981b3b:conformance/billing-ingest-ack-v1.json:generic-api-key:11
982f5300a5005689f0ded5026f2e80a282981b3b:conformance/billing-ingest-ack-v1.json:generic-api-key:20
982f5300a5005689f0ded5026f2e80a282981b3b:deploy/cloudflare/test/devenv-do.test.ts:generic-api-key:386
EOF
install_scanner() {
  local archive checksum_tool
  command -v curl >/dev/null 2>&1 || { printf 'secret-scan: curl is required\n' >&2; return 2; }
  if command -v sha256sum >/dev/null 2>&1; then checksum_tool='sha256sum';
  elif command -v shasum >/dev/null 2>&1; then checksum_tool='shasum -a 256';
  else printf 'secret-scan: sha256 utility is required\n' >&2; return 2; fi
  archive="$work_dir/$GITLEAKS_ASSET"
  curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --connect-timeout 10 --max-time 120 --retry 3 --retry-delay 1 "$GITLEAKS_URL" --output "$archive"
  printf '%s  %s\n' "$GITLEAKS_SHA256" "$archive" | $checksum_tool --check --status - || { printf 'secret-scan: download checksum mismatch\n' >&2; return 2; }
  mkdir "$work_dir/bin"; tar -xzf "$archive" -C "$work_dir/bin" gitleaks
  printf '%s\n' "$work_dir/bin/gitleaks"
}
if ((scanner_override)); then
  [[ "${SECRET_SCAN_TEST_MODE:-}" == 1 ]] || { printf 'secret-scan: --scanner is test-only\n' >&2; exit 2; }
else
  scanner="$(install_scanner)" || exit $?
fi
[[ -x "$scanner" ]] || { printf 'secret-scan: scanner is not executable\n' >&2; exit 2; }
scanner_version="$("$scanner" version 2>/dev/null || true)"
[[ "$scanner_version" == "$GITLEAKS_VERSION" ]] || { printf 'secret-scan: scanner version mismatch\n' >&2; exit 2; }
if [[ "$base" == '0000000000000000000000000000000000000000' ]]; then
  log_opts="${head} -m"; range_label='history through head'
else
  log_opts="${base}..${head} -m"; range_label="${base}..${head}"
fi
set +e
(
  cd "$repo"
  env -u GITLEAKS_CONFIG -u GITLEAKS_CONFIG_TOML "$scanner" git . --log-opts="$log_opts" --redact=100 --config "$config" --gitleaks-ignore-path "$ignore_file" --ignore-gitleaks-allow --report-format json --report-path "$report" --no-banner --log-level error
) >"$log" 2>&1
status=$?
set -e
case "$status" in
  0) printf 'secret-scan: PASS (gitleaks %s; %s)\n' "$GITLEAKS_VERSION" "$range_label"; exit 0 ;;
  1) printf 'secret-scan: FAIL (gitleaks found one or more secrets; values redacted)\n' >&2; exit 1 ;;
  *) printf 'secret-scan: ERROR (gitleaks exited %d; scanner output withheld)\n' "$status" >&2; exit "$status" ;;
esac
