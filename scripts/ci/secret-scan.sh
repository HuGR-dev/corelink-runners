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
cleanup() { rm -rf "$work_dir"; }
on_signal() { local signal="$1" code="$2"; trap - "$signal"; exit "$code"; }
trap cleanup EXIT
trap 'on_signal HUP 129' HUP
trap 'on_signal INT 130' INT
trap 'on_signal TERM 143' TERM
config="$work_dir/gitleaks.toml"; ignore_file="$work_dir/gitleaksignore"
report="$work_dir/report.json"; log="$work_dir/scanner.log"
printf '[extend]\nuseDefault = true\n' >"$config"; : >"$ignore_file"
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
