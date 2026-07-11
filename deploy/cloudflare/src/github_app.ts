// GitHub-App installation-token minting — the CUSTOMER-repo credential path.
//
// The dogfood autoscaler mints its JIT runner config with a STATIC first-party
// token (`GITHUB_MINT_TOKEN`) that only has rights on HumanGuardrail repos. A
// stranger's `runs-on: corelink` job lives on THEIR repo, where that token 404s.
// To mint a JIT runner on a customer repo the Worker must present an INSTALLATION
// ACCESS TOKEN for the GitHub App on THAT installation. This module builds it:
//   1. `appJwt`            — a short-lived (≤10 min) RS256 App JWT from the App's
//                            PKCS#8 private key (proves "I am the App").
//   2. `installationToken` — exchanges that JWT for a per-installation access
//                            token (`POST /app/installations/{id}/access_tokens`),
//                            cached in KV keyed by installationId so each mint
//                            reuses a live token instead of re-signing every time.
//
// PURE + runtime-agnostic (like lib.ts): NO `cloudflare:workers` /
// `@cloudflare/containers` imports — only WebCrypto (`crypto.subtle`) + `fetch`
// (Node 20+ / Workers globals), so it is unit-testable in plain vitest.
//
// NOTE (operational): WebCrypto's `importKey("pkcs8", …)` requires a PKCS#8 PEM
// (`-----BEGIN PRIVATE KEY-----`). GitHub hands you a PKCS#1 key
// (`-----BEGIN RSA PRIVATE KEY-----`); it must be converted to PKCS#8 before it
// is bound as `GITHUB_APP_PRIVATE_KEY` (`openssl pkcs8 -topk8 -nocrypt …`). A
// PKCS#1 PEM throws at import here → the mint FAILS CLOSED (never a silent wrong
// credential). The owner runbook (WP-4) documents the conversion.

import type { KvLike } from "./lib";

// The subset of Env this module reads. `RUNNER_JOB_PATS` (a `KVNamespace` in the
// live Worker) satisfies `KvLike` structurally — kept runtime-agnostic here.
export interface GithubAppEnv {
  // The GitHub App's numeric id (the JWT `iss`). `wrangler secret`/`var`.
  GITHUB_APP_ID?: string;
  // The App's PKCS#8 RSA private key PEM. Worker secret — NEVER logged, NEVER in
  // an error body, NEVER injected into a container.
  GITHUB_APP_PRIVATE_KEY?: string;
  // The installation-token cache lives here (`ghtok:<installationId>`). Absent ⇒
  // every mint re-fetches (correct, just un-cached).
  RUNNER_JOB_PATS?: KvLike;
}

// The App JWT lifetime. GitHub caps it at 10 min; 9 min leaves headroom under the
// cap even after the 60s clock-drift back-date below (`exp − iat = 540s ≤ 600s`).
const APP_JWT_TTL_S = 540;
// GitHub-recommended: back-date `iat` 60s to tolerate clock drift between us and
// GitHub (a JWT with `iat` in GitHub's future is rejected).
const APP_JWT_IAT_BACKDATE_S = 60;

// KV cache for installation tokens. GitHub installation tokens live ~1h; we
// refresh this many ms BEFORE the stated expiry so a cached token is never handed
// out inside its final window (a mint that then races expiry would 401).
const GHTOK_CACHE_PREFIX = "ghtok:";
const INSTALLATION_TOKEN_SAFETY_MS = 5 * 60 * 1000; // 5 min
// Cloudflare KV's floor for `expirationTtl`. A token whose safe lifetime is under
// this is returned but NOT cached (re-fetched next time) rather than rejected by KV.
const KV_MIN_TTL_S = 60;

// base64url (no padding) of raw bytes / a UTF-8 string — the JWT segment encoding.
function b64url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
function b64urlStr(str: string): string {
  return b64url(new TextEncoder().encode(str));
}

// Decode a PEM body (BEGIN/END lines + whitespace stripped) to its DER bytes.
function pemToDer(pem: string): Uint8Array {
  const b64 = pem
    .replace(/-----BEGIN [^-]+-----/g, "")
    .replace(/-----END [^-]+-----/g, "")
    .replace(/\s+/g, "");
  const bin = atob(b64);
  const der = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) der[i] = bin.charCodeAt(i);
  return der;
}

/**
 * Build a short-lived RS256 App JWT (`iss = appId`, `iat` back-dated 60s, `exp ≤
 * 10 min`), signed with the App's PKCS#8 private key via WebCrypto
 * (`RSASSA-PKCS1-v1_5` / SHA-256). Deterministic given `appId`, the key, and
 * `nowMs` (up to RSA-PKCS1's deterministic signature) — so a test can verify the
 * signature against the public key. The private key stays inside this call; it is
 * NEVER returned, logged, or placed in an error.
 */
export async function appJwt(appId: string, privateKeyPem: string, nowMs: number): Promise<string> {
  const nowS = Math.floor(nowMs / 1000);
  const iat = nowS - APP_JWT_IAT_BACKDATE_S;
  const exp = iat + APP_JWT_TTL_S; // ≤ 600s lifetime (GitHub's 10-min cap)
  const header = { alg: "RS256", typ: "JWT" };
  const payload = { iat, exp, iss: appId };
  const signingInput = `${b64urlStr(JSON.stringify(header))}.${b64urlStr(JSON.stringify(payload))}`;
  const key = await crypto.subtle.importKey(
    "pkcs8",
    pemToDer(privateKeyPem),
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign(
    "RSASSA-PKCS1-v1_5",
    key,
    new TextEncoder().encode(signingInput),
  );
  return `${signingInput}.${b64url(new Uint8Array(sig))}`;
}

/** A minted installation access token + its GitHub-stated expiry (ISO-8601). */
export interface InstallationToken {
  token: string;
  expires_at: string;
}

/**
 * Get an installation access token for `installationId`, cached in KV.
 *
 * - Cache HIT (a stored token still > `INSTALLATION_TOKEN_SAFETY_MS` from expiry)
 *   ⇒ return it, NO GitHub fetch, NO JWT signed.
 * - Cache MISS / near-expiry ⇒ sign an `appJwt`, POST
 *   `/app/installations/{id}/access_tokens`, cache the result (TTL = safe
 *   remaining lifetime), and return it.
 *
 * Isolation: the cache key is EXACTLY `ghtok:<installationId>`, so installation X
 * can only ever be served installation X's token (I3).
 *
 * FAILS CLOSED: a non-2xx (or a malformed body) THROWS — the caller (mintJit)
 * must let it propagate so the spawn fails, never silently falling back to the
 * first-party token for a foreign repo (I4). The error carries only the status +
 * GitHub's own response body — NEVER the JWT or the private key (I2).
 */
export async function installationToken(
  env: GithubAppEnv,
  installationId: string,
  nowMs: number,
): Promise<InstallationToken> {
  const cacheKey = `${GHTOK_CACHE_PREFIX}${installationId}`;
  const kv = env.RUNNER_JOB_PATS;
  if (kv) {
    const cached = await kv.get(cacheKey);
    if (cached) {
      try {
        const rec = JSON.parse(cached) as InstallationToken;
        const expMs = Date.parse(rec.expires_at);
        if (Number.isFinite(expMs) && nowMs < expMs - INSTALLATION_TOKEN_SAFETY_MS) {
          return rec; // live cache hit ⇒ skip the fetch entirely
        }
      } catch {
        /* corrupt cache entry ⇒ fall through and re-mint */
      }
    }
  }

  const jwt = await appJwt(env.GITHUB_APP_ID ?? "", env.GITHUB_APP_PRIVATE_KEY ?? "", nowMs);
  const resp = await fetch(
    `https://api.github.com/app/installations/${installationId}/access_tokens`,
    {
      method: "POST",
      headers: {
        authorization: `Bearer ${jwt}`,
        accept: "application/vnd.github+json",
        "user-agent": "corelink-spawn-worker",
        "x-github-api-version": "2022-11-28",
      },
    },
  );
  if (!resp.ok) {
    // The App JWT / private key are NEVER included — only the status + GitHub's
    // own error body (I2).
    throw new Error(
      `installation token ${installationId}: ${resp.status} ${await resp.text().catch(() => "")}`,
    );
  }
  const j = (await resp.json()) as { token?: string; expires_at?: string };
  if (!j.token || !j.expires_at) {
    throw new Error(
      `installation token ${installationId}: malformed response (keys: ${Object.keys(j).join(",")})`,
    );
  }
  const rec: InstallationToken = { token: j.token, expires_at: j.expires_at };

  if (kv) {
    const expMs = Date.parse(rec.expires_at);
    const ttlS = Number.isFinite(expMs)
      ? Math.floor((expMs - nowMs) / 1000) - Math.floor(INSTALLATION_TOKEN_SAFETY_MS / 1000)
      : 0;
    if (ttlS >= KV_MIN_TTL_S) {
      await kv.put(cacheKey, JSON.stringify(rec), { expirationTtl: ttlS }).catch(() => {
        /* best-effort cache: a put failure just means the next mint re-fetches */
      });
    }
  }
  return rec;
}
