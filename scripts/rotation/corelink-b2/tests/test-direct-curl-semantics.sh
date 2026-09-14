#!/usr/bin/env bash
# Assert the real curl behavior relied on by the expected-status auth probes.
set -euo pipefail

tmp="$(mktemp -d /private/tmp/corelink-direct-curl.XXXXXX)"
server='' tls_server=''
cleanup(){ [ -z "$server" ] || kill "$server" 2>/dev/null || true; [ -z "$tls_server" ] || kill "$tls_server" 2>/dev/null || true; rm -rf "$tmp"; }
trap cleanup EXIT

node -e '
  const http = require("http");
  const server = http.createServer((_, response) => {
    response.statusCode = 503;
    response.end();
    response.once("finish", () => server.close());
  });
  server.listen(0, "127.0.0.1", () => console.log(server.address().port));
' >"$tmp/port" 2>"$tmp/server.err" &
server=$!
for _ in $(seq 1 50); do [ -s "$tmp/port" ] && break; sleep 0.02; done
[ -s "$tmp/port" ] || { cat "$tmp/server.err" >&2; exit 1; }
port="$(<"$tmp/port")"

# No --fail: expected HTTP error statuses are returned on stdout while curl's
# exit code stays zero. This is the exact protocol needed for 503/401 proofs.
status="$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:$port/status")"
[ "$status" = 503 ]
wait "$server"; server=''

# A transport failure remains an error without --fail, so status capture does
# not convert connection or TLS failures into a passing auth proof.
if curl --connect-timeout 1 --silent --show-error --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:$port/unreachable" >/dev/null 2>&1; then
  printf '%s\n' 'curl unexpectedly accepted a refused transport connection' >&2
  exit 1
fi

# The same status-capture flags preserve curl's TLS verification. A self-signed
# local endpoint first proves it is listening with --insecure, then must fail
# when invoked as the production probe is invoked.
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=localhost \
  -keyout "$tmp/key.pem" -out "$tmp/cert.pem" >/dev/null 2>&1
tls_port=''
for candidate in $(seq 24000 24020); do
  openssl s_server -accept "$candidate" -cert "$tmp/cert.pem" -key "$tmp/key.pem" -www >"$tmp/tls.out" 2>"$tmp/tls.err" &
  tls_server=$!
  for _ in $(seq 1 25); do
    if curl --insecure --connect-timeout 1 --silent --output /dev/null "https://127.0.0.1:$candidate"; then tls_port="$candidate"; break 2; fi
    sleep 0.02
  done
  kill "$tls_server" 2>/dev/null || true; tls_server=''
done
[ -n "$tls_port" ] || { cat "$tmp/tls.err" >&2; exit 1; }
if curl --connect-timeout 1 --silent --show-error --output /dev/null --write-out '%{http_code}' "https://127.0.0.1:$tls_port/status" >/dev/null 2>&1; then
  printf '%s\n' 'curl unexpectedly accepted an untrusted TLS certificate' >&2
  exit 1
fi
printf '%s\n' 'real curl expected-status and transport semantics passed'
