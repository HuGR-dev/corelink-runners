#!/usr/bin/env bash
# scripts/dogfood.sh — dogfood tenant onboarding + smoke
#
# Onboards a tenant onto a RUNNING fabric instance (static backend, no CoreLink
# dependency), then runs `corelink smoke` to verify the deployment.
#
# ── Required env vars ──────────────────────────────────────────────────────────
#
#   CORELINK_URL        The fabric base URL (e.g. https://fabric.example.com or
#                       http://127.0.0.1:8080 for local dogfood).
#
#   CORELINK_ADMIN_KEY  The FABRIC_ADMIN_KEY the fabric server was started with.
#                       If this is absent/empty the admin route returns 404 and
#                       tenant onboarding cannot proceed.
#
# ── Optional env vars ─────────────────────────────────────────────────────────
#
#   DOGFOOD_TENANT      Tenant id to register (default: dogfood).
#                       Must match [a-z0-9-] (TenantId validation on the server).
#
#   DOGFOOD_PLAN        Plan tier to set (default: pro).
#                       Valid values: starter | pro | team | scale | max.
#
#   DOGFOOD_PAT         The PAT that the `corelink smoke` step will present as a
#                       Bearer token. See the STATIC-BACKEND CAVEAT below.
#
# ── Static-backend PAT caveat ─────────────────────────────────────────────────
#
# In static-auth mode (FABRIC_AUTH_BACKEND=static, the default) the server
# maintains a single FABRIC_PAT → FABRIC_TENANT bootstrap mapping set at
# startup.  The admin endpoint (Step 1 of this script) writes the PLAN/CONCURRENCY
# cap for a tenant into the live in-memory registry — but it does NOT mint a new
# PAT or add a PAT→tenant entry.
#
# Consequence: for Step 2 (smoke) to pass, DOGFOOD_PAT must ALREADY be recognised
# by the running server as a valid token for DOGFOOD_TENANT.  With the static
# backend that means the server was started with:
#
#   FABRIC_PAT=$DOGFOOD_PAT FABRIC_TENANT=$DOGFOOD_TENANT
#
# True self-serve PAT minting for arbitrary tenants requires the CoreLink auth
# backend (FABRIC_AUTH_BACKEND=corelink) — that is the M1 CoreLink flip and is
# outside the scope of this script.
#
# In practice for local/CI dogfood: start the fabric server with
# FABRIC_PAT + FABRIC_TENANT matching DOGFOOD_PAT + DOGFOOD_TENANT, then run
# this script.  The admin call upgrades the plan cap; smoke uses the same PAT.
#
# ── Plan caps (for reference) ─────────────────────────────────────────────────
#
#   starter  → max_concurrency  20, rate_ceiling_per_min  200
#   pro      → max_concurrency  40, rate_ceiling_per_min  400
#   team     → max_concurrency  80, rate_ceiling_per_min  800
#   scale    → max_concurrency 160, rate_ceiling_per_min 1600
#   max      → max_concurrency 320, rate_ceiling_per_min 3200
#
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

# ── Defaults ──────────────────────────────────────────────────────────────────
DOGFOOD_TENANT="${DOGFOOD_TENANT:-dogfood}"
DOGFOOD_PLAN="${DOGFOOD_PLAN:-pro}"

# ── Validate required inputs ───────────────────────────────────────────────────
if [[ -z "${CORELINK_URL:-}" ]]; then
    echo "ERROR: CORELINK_URL is required (e.g. http://127.0.0.1:8080)" >&2
    exit 1
fi

if [[ -z "${CORELINK_ADMIN_KEY:-}" ]]; then
    echo "ERROR: CORELINK_ADMIN_KEY is required (the FABRIC_ADMIN_KEY the server was started with)" >&2
    exit 1
fi

# Trim trailing slash from URL so path concatenation is unambiguous.
CORELINK_URL="${CORELINK_URL%/}"

echo "==> Step 1: onboard tenant '${DOGFOOD_TENANT}' with plan '${DOGFOOD_PLAN}'"
echo "    fabric: ${CORELINK_URL}"

# POST /internal/v1/admin/tenants
# Auth: X-Corelink-Internal-Auth header (constant-time compared on server)
# Body: {"tenant":"<id>","plan":"<tier>"}
# Response 200: {"tenant","tier","max_concurrency","rate_ceiling_per_min"}
# Response 400: invalid tenant id or unknown plan tier
# Response 401: admin key mismatch
# Response 404: FABRIC_ADMIN_KEY not set on the server (admin route invisible)
ADMIN_RESPONSE=$(
    curl --silent --show-error --fail-with-body \
        --request POST \
        --url "${CORELINK_URL}/internal/v1/admin/tenants" \
        --header "Content-Type: application/json" \
        --header "X-Corelink-Internal-Auth: ${CORELINK_ADMIN_KEY}" \
        --data "{\"tenant\":\"${DOGFOOD_TENANT}\",\"plan\":\"${DOGFOOD_PLAN}\"}"
)

echo "    resolved caps: ${ADMIN_RESPONSE}"

# Lightweight sanity check: response must contain the tenant field.
if ! echo "${ADMIN_RESPONSE}" | grep -q "\"tenant\""; then
    echo "ERROR: admin response did not contain expected 'tenant' field" >&2
    echo "       raw response: ${ADMIN_RESPONSE}" >&2
    exit 1
fi

echo "    OK — tenant onboarded"
echo ""

# ── Step 2: smoke ──────────────────────────────────────────────────────────────
echo "==> Step 2: corelink smoke --url ${CORELINK_URL}"

# DOGFOOD_PAT caveat: see top-of-file documentation.
# The static auth backend resolves the PAT via the FABRIC_PAT bootstrap entry
# set at server startup — DOGFOOD_PAT must match that value.
if [[ -z "${DOGFOOD_PAT:-}" ]]; then
    echo "WARNING: DOGFOOD_PAT is unset; smoke will use whatever CORELINK_PAT is in the environment." >&2
    echo "         If that is also unset the smoke's 401-check will use a blank token and the" >&2
    echo "         authenticated checks may fail. See static-backend PAT caveat in this script." >&2
    echo ""
fi

CORELINK_PAT="${DOGFOOD_PAT:-${CORELINK_PAT:-}}" \
    corelink smoke --url "${CORELINK_URL}"

echo ""
echo "==> Dogfood complete."
