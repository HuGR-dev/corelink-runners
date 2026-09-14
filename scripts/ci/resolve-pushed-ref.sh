#!/usr/bin/env bash
# resolve-pushed-ref.sh — turn a `wrangler containers push` transcript into an
# immutable registry reference to pin in wrangler.jsonc (X4 floor).
#
# `wrangler containers push <image>:<tag>` prints, among the buildkit push
# progress, a manifest line like:
#     manifest-<tag>@sha256:<64hex>: done  |++++…++|
# and a final:
#     Pushed image: registry.cloudflare.com/<account>/<image>:<tag>
#
# The resolver emits only the immutable digest form
#     registry.cloudflare.com/<account>/<image>@sha256:<64hex>
# A missing or malformed digest is an error. Falling back to the tag would make
# a successful push look pin-ready while leaving the subsequent deployment
# mutable, violating the X4 floor.
#
# Usage: resolve-pushed-ref.sh <image-name> <tag> <push-output-file>
set -euo pipefail

IMAGE="${1:?image name required}"
TAG="${2:?tag required}"
OUT="${3:?push-output file required}"
ACCOUNT="${CLOUDFLARE_ACCOUNT_ID:?CLOUDFLARE_ACCOUNT_ID must be set}"

base="registry.cloudflare.com/${ACCOUNT}/${IMAGE}"

# 1) manifest digest emitted by the push (the exact bytes that landed).
digest="$(grep -oE 'manifest-[^@[:space:]]*@sha256:[0-9a-f]{64}' "$OUT" 2>/dev/null \
            | grep -oE 'sha256:[0-9a-f]{64}' | tail -n1 || true)"

# 2) or a digest form already present in the "Pushed image:" line.
if [ -z "$digest" ]; then
  digest="$(grep -oE 'registry\.cloudflare\.com/[^[:space:]]+@sha256:[0-9a-f]{64}' "$OUT" 2>/dev/null \
              | grep -oE 'sha256:[0-9a-f]{64}' | tail -n1 || true)"
fi

if printf '%s' "$digest" | grep -qE '^sha256:[0-9a-f]{64}$'; then
  printf '%s@%s\n' "$base" "$digest"
else
  printf 'resolve-pushed-ref: push output did not contain an immutable sha256 digest for %s:%s\n' \
    "$base" "$TAG" >&2
  exit 1
fi
