#!/bin/sh
# Offline T8-W4b boot bridge checks. No network, image build, or live calls.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/corelink-auth-bridge.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
bin="$tmp/bin"
mkdir -p "$bin"

cat >"$bin/clw" <<'EOF'
#!/bin/sh
exit 0
EOF
cat >"$bin/exec-server" <<'EOF'
#!/bin/sh
env >"${AUTH_ENV_CAPTURE:?}"
cat "${EXEC_SERVER_AUTH_TOKEN_FILE:?}" >"${AUTH_FILE_CAPTURE:?}"
EOF
cat >"$bin/supervisord" <<'EOF'
#!/bin/sh
env >"${AUTH_ENV_CAPTURE:?}"
cat "${EXEC_SERVER_AUTH_TOKEN_FILE:?}" >"${AUTH_FILE_CAPTURE:?}"
EOF
chmod 0755 "$bin"/*
file_mode() {
    stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1"
}

check_script="$tmp/check-host.sh"
sed "s#exec /usr/local/bin/corelink-check-exec-server#exec \"$bin/exec-server\"#" \
    "$root/entrypoint.sh" >"$check_script"
chmod 0755 "$check_script"

auth="$tmp/run/corelink/exec-server-auth-token"
mkdir -p "$(dirname "$auth")"
AUTH_ENV_CAPTURE="$tmp/check.env" AUTH_FILE_CAPTURE="$tmp/check.file" \
    PATH="$bin:$PATH" EXEC_SERVER_AUTH_TOKEN='bridge-secret' \
    EXEC_SERVER_AUTH_TOKEN_FILE="$auth" TOOLCHAIN_DIGEST=digest \
    TOOLCHAIN_DIR="$tmp/toolchain" "$check_script"

test "$(cat "$tmp/check.file")" = bridge-secret
test "$(file_mode "$auth")" = 400
! grep -q '^EXEC_SERVER_AUTH_TOKEN=' "$tmp/check.env"
grep -q "^EXEC_SERVER_AUTH_TOKEN_FILE=$auth$" "$tmp/check.env"

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

# The DevEnv entrypoint applies the same bridge before supervisord. Substitute
# fixed image paths so this remains an offline host-shell test.
cloud_script="$tmp/cloudflare.sh"
sed -e "s#/usr/local/bin/clw#$bin/clw#g" \
    -e "s#/usr/bin/supervisord#$bin/supervisord#g" \
    -e "s#/data/chrome#$tmp/chrome#g" -e "s#/data/workspace#$tmp/workspace#g" \
    "$root/../cloudflare/entrypoint.sh" >"$cloud_script"
chmod 0755 "$cloud_script"
cloud_auth="$tmp/cloud/run/corelink/token"
AUTH_ENV_CAPTURE="$tmp/cloud.env" AUTH_FILE_CAPTURE="$tmp/cloud.file" \
    EXEC_SERVER_AUTH_TOKEN='cloud-secret' EXEC_SERVER_AUTH_TOKEN_FILE="$cloud_auth" \
    CLW_TENANT=tenant WORKSPACE_NAME=workspace PROFILE_NAME=profile \
    "$cloud_script"
test "$(cat "$tmp/cloud.file")" = cloud-secret
test "$(file_mode "$cloud_auth")" = 400
! grep -q '^EXEC_SERVER_AUTH_TOKEN=' "$tmp/cloud.env"
grep -q "^EXEC_SERVER_AUTH_TOKEN_FILE=$cloud_auth$" "$tmp/cloud.env"

echo 'auth-secret-bridge: PASS'
