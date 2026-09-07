#!/bin/sh
# Offline T8-W4b boot bridge checks. No network, image build, or live calls.
set -eu
if [ -d /private/var/folders ]; then
    case "${TMPDIR:-}" in /var/*) TMPDIR="/private${TMPDIR}" ;; esac
fi
export TMPDIR

root=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/corelink-auth-bridge.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
bin="$tmp/bin"
mkdir -p "$bin"

cat >"$bin/clw" <<'EOF'
#!/bin/sh
[ "${CLW_FAIL:-0}" = 1 ] && exit 7
exit 0
EOF
cat >"$bin/exec-server" <<'EOF'
#!/bin/sh
env >"${AUTH_ENV_CAPTURE:?}"
cat "${EXEC_SERVER_AUTH_TOKEN_FILE:?}" >"${AUTH_FILE_CAPTURE:?}"
mode=$(stat -c '%a' "${EXEC_SERVER_AUTH_TOKEN_FILE}" 2>/dev/null || stat -f '%Lp' "${EXEC_SERVER_AUTH_TOKEN_FILE}")
printf '%s' "$mode" >"${AUTH_MODE_CAPTURE:?}"
ps -p $$ -o command= >"${AUTH_ARG_CAPTURE:?}"
case "${EXEC_MODE:-normal}" in
    term) sleep 30 & sleep_pid=$!; trap 'kill -TERM "$sleep_pid" 2>/dev/null; exit 0' TERM; wait "$sleep_pid" ;;
    fail) exit 42 ;;
esac
EOF
cat >"$bin/supervisord" <<'EOF'
#!/bin/sh
env >"${AUTH_ENV_CAPTURE:?}"
cat "${EXEC_SERVER_AUTH_TOKEN_FILE:?}" >"${AUTH_FILE_CAPTURE:?}"
mode=$(stat -c '%a' "${EXEC_SERVER_AUTH_TOKEN_FILE}" 2>/dev/null || stat -f '%Lp' "${EXEC_SERVER_AUTH_TOKEN_FILE}")
printf '%s' "$mode" >"${AUTH_MODE_CAPTURE:?}"
ps -p $$ -o command= >"${AUTH_ARG_CAPTURE:?}"
case "${EXEC_MODE:-normal}" in
    term) sleep 30 & sleep_pid=$!; trap 'kill -TERM "$sleep_pid" 2>/dev/null; exit 0' TERM; wait "$sleep_pid" ;;
    fail) exit 42 ;;
esac
EOF
cat >"$bin/dumb-init" <<'EOF'
#!/bin/sh
env >"${OUTER_ENV_CAPTURE:?}"
[ "${1:-}" = -- ] || exit 64
shift
exec "$@"
EOF
chmod 0755 "$bin"/*
check_script="$tmp/check-host.sh"
sed "s#/usr/local/bin/corelink-check-exec-server#\"$bin/exec-server\"#" \
    "$root/entrypoint.sh" >"$check_script"
chmod 0755 "$check_script"

auth="$tmp/run/corelink/exec-server-auth-token"
mkdir -p "$(dirname "$auth")"
AUTH_ENV_CAPTURE="$tmp/check.env" AUTH_FILE_CAPTURE="$tmp/check.file" AUTH_MODE_CAPTURE="$tmp/check.mode" AUTH_ARG_CAPTURE="$tmp/check.argv" \
    PATH="$bin:$PATH" EXEC_SERVER_AUTH_TOKEN='bridge-secret' \
    EXEC_SERVER_AUTH_TOKEN_FILE="$auth" TOOLCHAIN_DIGEST=digest \
    TOOLCHAIN_DIR="$tmp/toolchain" "$check_script"

test "$(cat "$tmp/check.file")" = bridge-secret
test "$(cat "$tmp/check.mode")" = 400
test ! -e "$auth"
! grep -q '^EXEC_SERVER_AUTH_TOKEN=' "$tmp/check.env"
grep -q "^EXEC_SERVER_AUTH_TOKEN_FILE=$auth$" "$tmp/check.env"
! grep -q 'bridge-secret' "$tmp/check.argv"

# A provider-injected marker cannot bypass a raw token bridge.
marked_auth="$tmp/marked/run/corelink/token"
AUTH_ENV_CAPTURE="$tmp/marked.env" AUTH_FILE_CAPTURE="$tmp/marked.file" AUTH_MODE_CAPTURE="$tmp/marked.mode" AUTH_ARG_CAPTURE="$tmp/marked.argv" \
    PATH="$bin:$PATH" CORELINK_AUTH_BRIDGED=1 EXEC_SERVER_AUTH_TOKEN=marked-secret \
    EXEC_SERVER_AUTH_TOKEN_FILE="$marked_auth" TOOLCHAIN_DIGEST=digest \
    TOOLCHAIN_DIR="$tmp/toolchain" "$check_script"
test "$(cat "$tmp/marked.file")" = marked-secret
! grep -q '^EXEC_SERVER_AUTH_TOKEN=' "$tmp/marked.env"

if CORELINK_AUTH_BRIDGED=1 EXEC_SERVER_AUTH_TOKEN='' EXEC_SERVER_AUTH_TOKEN_FILE="$tmp/marked-empty" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'marker plus empty token unexpectedly accepted' >&2; exit 1
fi
if CORELINK_AUTH_BRIDGED=1 EXEC_SERVER_AUTH_TOKEN_FILE="$tmp/marker-missing" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'marker without auth file unexpectedly accepted' >&2; exit 1
fi
if EXEC_SERVER_AUTH_TOKEN_FILE="$tmp/no-marker" TOOLCHAIN_DIGEST=digest \
    TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'missing marker unexpectedly accepted' >&2; exit 1
fi

# TERM is forwarded and still removes the ephemeral file.
term_auth="$tmp/term/run/corelink/token"
AUTH_ENV_CAPTURE="$tmp/term.env" AUTH_FILE_CAPTURE="$tmp/term.file" AUTH_MODE_CAPTURE="$tmp/term.mode" \
    PATH="$bin:$PATH" AUTH_ARG_CAPTURE="$tmp/term.argv" EXEC_MODE=term EXEC_SERVER_AUTH_TOKEN=term-secret EXEC_SERVER_AUTH_TOKEN_FILE="$term_auth" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" &
term_pid=$!
for _ in $(seq 1 100); do [ -e "$tmp/term.mode" ] && break; sleep 0.01; done
kill -TERM "$term_pid"
if wait "$term_pid"; then
    echo 'TERM unexpectedly changed child status' >&2; exit 1
else
    test "$?" = 143
fi
test ! -e "$term_auth"

# A durable child failure is returned while cleanup still runs.
fail_auth="$tmp/fail/run/corelink/token"
if AUTH_ENV_CAPTURE="$tmp/fail.env" AUTH_FILE_CAPTURE="$tmp/fail.file" AUTH_MODE_CAPTURE="$tmp/fail.mode" \
    PATH="$bin:$PATH" AUTH_ARG_CAPTURE="$tmp/fail.argv" EXEC_MODE=fail EXEC_SERVER_AUTH_TOKEN=fail-secret EXEC_SERVER_AUTH_TOKEN_FILE="$fail_auth" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script"; then
    echo 'child failure unexpectedly swallowed' >&2; exit 1
fi
test ! -e "$fail_auth"

# Hydration failure also removes the file before the process can become durable.
hydrate_fail_auth="$tmp/hydrate-fail/run/corelink/token"
if PATH="$bin:$PATH" CLW_FAIL=1 EXEC_SERVER_AUTH_TOKEN=hydrate-fail \
    EXEC_SERVER_AUTH_TOKEN_FILE="$hydrate_fail_auth" TOOLCHAIN_DIGEST=digest \
    TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'hydration failure unexpectedly accepted' >&2; exit 1
fi
test ! -e "$hydrate_fail_auth"

# Invalid inputs fail closed before the durable process is started.
if EXEC_SERVER_AUTH_TOKEN='' EXEC_SERVER_AUTH_TOKEN_FILE="$tmp/empty" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'empty token unexpectedly accepted' >&2; exit 1
fi
touch "$tmp/existing"
if EXEC_SERVER_AUTH_TOKEN=secret EXEC_SERVER_AUTH_TOKEN_FILE="$tmp/existing" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'pre-existing file unexpectedly accepted' >&2; exit 1
fi
ln -s "$tmp/existing" "$tmp/existing-link"
if EXEC_SERVER_AUTH_TOKEN=secret EXEC_SERVER_AUTH_TOKEN_FILE="$tmp/existing-link" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'symlink file unexpectedly accepted' >&2; exit 1
fi
ln -s "$tmp" "$tmp/unsafe-dir"
if EXEC_SERVER_AUTH_TOKEN=secret EXEC_SERVER_AUTH_TOKEN_FILE="$tmp/unsafe-dir/token" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'symlink directory unexpectedly accepted' >&2; exit 1
fi
mkdir -p "$tmp/real-parent"
ln -s "$tmp/real-parent" "$tmp/unsafe-parent"
if EXEC_SERVER_AUTH_TOKEN=secret EXEC_SERVER_AUTH_TOKEN_FILE="$tmp/unsafe-parent/deep/token" \
    TOOLCHAIN_DIGEST=digest TOOLCHAIN_DIR="$tmp/toolchain" "$check_script" 2>/dev/null; then
    echo 'parent symlink unexpectedly accepted' >&2; exit 1
fi

# The DevEnv entrypoint applies the same bridge before supervisord. Substitute
# fixed image paths so this remains an offline host-shell test.
# The current entrypoint requires the broker ticket tuple before it reaches
# the auth bridge; use inert fixture values because this smoke never contacts
# the broker.
export CLW_CRED_TICKET=fixture-ticket CLW_LEASE_ID=fixture-lease CLW_FABRIC_ENDPOINT=https://fixture.invalid
cloud_script="$tmp/cloudflare.sh"
sed -e "s#/usr/local/bin/clw#$bin/clw#g" \
    -e "s#/usr/bin/supervisord#$bin/supervisord#g" \
    -e "s#/usr/bin/dumb-init#$bin/dumb-init#g" \
    -e "s#/data/chrome#$tmp/chrome#g" -e "s#/data/workspace#$tmp/workspace#g" \
    "$root/../cloudflare/entrypoint.sh" >"$cloud_script"
chmod 0755 "$cloud_script"
cloud_auth="$tmp/cloud/run/corelink/token"
AUTH_ENV_CAPTURE="$tmp/cloud.env" AUTH_FILE_CAPTURE="$tmp/cloud.file" AUTH_MODE_CAPTURE="$tmp/cloud.mode" AUTH_ARG_CAPTURE="$tmp/cloud.argv" OUTER_ENV_CAPTURE="$tmp/cloud.outer.env" \
    PATH="$bin:$PATH" \
    EXEC_SERVER_AUTH_TOKEN='cloud-secret' EXEC_SERVER_AUTH_TOKEN_FILE="$cloud_auth" \
    CLW_TENANT=tenant WORKSPACE_NAME=workspace PROFILE_NAME=profile \
    "$cloud_script" >"$tmp/cloud.log" 2>&1 || { cat "$tmp/cloud.log" >&2; exit 1; }
test "$(cat "$tmp/cloud.file")" = cloud-secret
test "$(cat "$tmp/cloud.mode")" = 400
test ! -e "$cloud_auth"
! grep -q '^EXEC_SERVER_AUTH_TOKEN=' "$tmp/cloud.env"
grep -q "^EXEC_SERVER_AUTH_TOKEN_FILE=$cloud_auth$" "$tmp/cloud.env"
! grep -q 'cloud-secret' "$tmp/cloud.argv" "$tmp/cloud.log"
! grep -q '^EXEC_SERVER_AUTH_TOKEN=' "$tmp/cloud.outer.env"
grep -q '^ENTRYPOINT \["/entrypoint.sh"\]' "$root/../cloudflare/Dockerfile.runner-devenv"

# An injected dumb-init marker cannot suppress the clean outer-init re-exec.
marked_init_auth="$tmp/marked-init/run/corelink/token"
AUTH_ENV_CAPTURE="$tmp/marked-init.env" AUTH_FILE_CAPTURE="$tmp/marked-init.file" AUTH_MODE_CAPTURE="$tmp/marked-init.mode" AUTH_ARG_CAPTURE="$tmp/marked-init.argv" OUTER_ENV_CAPTURE="$tmp/marked-init.outer.env" \
    PATH="$bin:$PATH" CORELINK_DUMB_INIT=1 EXEC_SERVER_AUTH_TOKEN=marked-init-secret \
    EXEC_SERVER_AUTH_TOKEN_FILE="$marked_init_auth" CLW_TENANT=tenant \
    WORKSPACE_NAME=workspace PROFILE_NAME=profile "$cloud_script" >/dev/null 2>&1
test -s "$tmp/marked-init.outer.env"
! grep -q '^EXEC_SERVER_AUTH_TOKEN=' "$tmp/marked-init.outer.env"
grep -q '^user=coder$' "$root/../cloudflare/supervisord.conf"
grep -q '^USER coder$' "$root/../cloudflare/Dockerfile.runner-devenv"

# DevEnv TERM forwarding runs the existing snapshot path and removes auth.
cloud_term_auth="$tmp/cloud-term/run/corelink/token"
AUTH_ENV_CAPTURE="$tmp/cloud-term.env" AUTH_FILE_CAPTURE="$tmp/cloud-term.file" AUTH_MODE_CAPTURE="$tmp/cloud-term.mode" \
    AUTH_ARG_CAPTURE="$tmp/cloud-term.argv" OUTER_ENV_CAPTURE="$tmp/cloud-term.outer.env" EXEC_MODE=term EXEC_SERVER_AUTH_TOKEN=cloud-term EXEC_SERVER_AUTH_TOKEN_FILE="$cloud_term_auth" \
    CLW_TENANT=tenant WORKSPACE_NAME=workspace PROFILE_NAME=profile "$cloud_script" &
cloud_term_pid=$!
for _ in $(seq 1 100); do [ -e "$tmp/cloud-term.mode" ] && break; sleep 0.01; done
kill -TERM "$cloud_term_pid"
wait "$cloud_term_pid"
test ! -e "$cloud_term_auth"

# Supervisor failure remains observable and still cleans the bridge file.
cloud_fail_auth="$tmp/cloud-fail/run/corelink/token"
if AUTH_ENV_CAPTURE="$tmp/cloud-fail.env" AUTH_FILE_CAPTURE="$tmp/cloud-fail.file" AUTH_MODE_CAPTURE="$tmp/cloud-fail.mode" \
    AUTH_ARG_CAPTURE="$tmp/cloud-fail.argv" OUTER_ENV_CAPTURE="$tmp/cloud-fail.outer.env" EXEC_MODE=fail EXEC_SERVER_AUTH_TOKEN=cloud-fail EXEC_SERVER_AUTH_TOKEN_FILE="$cloud_fail_auth" \
    CLW_TENANT=tenant WORKSPACE_NAME=workspace PROFILE_NAME=profile "$cloud_script"; then
    echo 'supervisor failure unexpectedly swallowed' >&2; exit 1
fi
test ! -e "$cloud_fail_auth"

cloud_hydrate_fail_auth="$tmp/cloud-hydrate-fail/run/corelink/token"
if CLW_FAIL=1 OUTER_ENV_CAPTURE="$tmp/cloud-hydrate-fail.outer.env" EXEC_SERVER_AUTH_TOKEN=cloud-hydrate-fail \
    EXEC_SERVER_AUTH_TOKEN_FILE="$cloud_hydrate_fail_auth" \
    CLW_TENANT=tenant WORKSPACE_NAME=workspace PROFILE_NAME=profile "$cloud_script" \
    >/dev/null 2>&1; then
    echo 'cloud hydration failure unexpectedly accepted' >&2; exit 1
fi
test ! -e "$cloud_hydrate_fail_auth"

echo 'auth-secret-bridge: PASS'
