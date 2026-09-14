// Unit tests for GitHub-App installation-token minting (WP-1): the App JWT is
// deterministic + signature-verifiable, `installationToken` caches by
// installationId (hit skips fetch, expiry re-fetches, non-2xx throws), and
// `mintJit`'s auth-selection picks the App token only when App creds AND an
// installationId are present (else the first-party GITHUB_MINT_TOKEN, byte-
// identical to today). Plain vitest (node) — WebCrypto (crypto.subtle) is a
// Node 20+ global; a real RSA key pair is generated IN the test (never hardcoded).
import { describe, it, expect, vi, afterEach, beforeAll } from "vitest";
import { appJwt, installationToken, type GithubAppEnv } from "../src/github_app";
import type { KvLike } from "../src/lib";

// `mintJitAuthToken` lives in index.ts, which imports `@cloudflare/containers`
// (Workers-only) → mock it (mirroring cred-stash-do.test.ts) so index is
// importable under node, THEN import the selection function.
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));
import { mintJitAuthToken, type Env } from "../src/index";

// ── A real RSA key pair for the App-JWT tests (generated, never hardcoded) ─────
let keyPair: CryptoKeyPair;
let privateKeyPem: string;

function derToPem(der: ArrayBuffer, label: string): string {
  const bin = String.fromCharCode(...new Uint8Array(der));
  const b64 = btoa(bin);
  const lines = b64.match(/.{1,64}/g)?.join("\n") ?? b64;
  return `-----BEGIN ${label}-----\n${lines}\n-----END ${label}-----\n`;
}

beforeAll(async () => {
  keyPair = (await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  )) as CryptoKeyPair;
  const pkcs8 = await crypto.subtle.exportKey("pkcs8", keyPair.privateKey);
  privateKeyPem = derToPem(pkcs8, "PRIVATE KEY");
});

// Decode a base64url JWT segment to its UTF-8 string.
function b64urlDecode(seg: string): string {
  const b64 = seg.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((seg.length + 3) % 4);
  return atob(b64);
}

describe("appJwt (RS256 App JWT)", () => {
  const APP_ID = "123456";
  const NOW = 1_800_000_000_000; // fixed instant

  it("has the RS256 header, iss=appId, and a ≤10min exp back-dated 60s", async () => {
    const jwt = await appJwt(APP_ID, privateKeyPem, NOW);
    const [h, p] = jwt.split(".");
    const header = JSON.parse(b64urlDecode(h)) as { alg: string; typ: string };
    const payload = JSON.parse(b64urlDecode(p)) as { iss: string; iat: number; exp: number };
    expect(header).toEqual({ alg: "RS256", typ: "JWT" });
    expect(payload.iss).toBe(APP_ID);
    const nowS = Math.floor(NOW / 1000);
    expect(payload.iat).toBe(nowS - 60); // 60s clock-drift back-date
    expect(payload.exp - payload.iat).toBeLessThanOrEqual(600); // ≤ 10 min lifetime
    expect(payload.exp).toBeGreaterThan(payload.iat);
  });

  it("is deterministic for a fixed key + now (RSA-PKCS1 signature)", async () => {
    const a = await appJwt(APP_ID, privateKeyPem, NOW);
    const b = await appJwt(APP_ID, privateKeyPem, NOW);
    expect(a).toBe(b);
  });

  it("produces a signature verifiable with the public key", async () => {
    const jwt = await appJwt(APP_ID, privateKeyPem, NOW);
    const [h, p, s] = jwt.split(".");
    const sigBin = b64urlDecode(s);
    const sig = new Uint8Array([...sigBin].map((c) => c.charCodeAt(0)));
    const ok = await crypto.subtle.verify(
      "RSASSA-PKCS1-v1_5",
      keyPair.publicKey,
      sig,
      new TextEncoder().encode(`${h}.${p}`),
    );
    expect(ok).toBe(true);
  });
});

// ── A minimal in-memory KvLike for the installation-token cache ────────────────
function makeKv(seed: Record<string, string> = {}): KvLike & { store: Map<string, string> } {
  const store = new Map<string, string>(Object.entries(seed));
  return {
    store,
    async get(key: string) {
      return store.get(key) ?? null;
    },
    async put(key: string, value: string) {
      store.set(key, value);
    },
    async delete(key: string) {
      store.delete(key);
    },
  };
}

describe("installationToken (mint + KV cache by installationId)", () => {
  afterEach(() => vi.unstubAllGlobals());

  const NOW = 1_800_000_000_000;
  const INST = "150584374";
  const baseEnv = (): GithubAppEnv => ({
    GITHUB_APP_ID: "123456",
    GITHUB_APP_PRIVATE_KEY: privateKeyPem,
  });

  function ghTokenResponse(token: string, expiresAt: string): Response {
    return new Response(JSON.stringify({ token, expires_at: expiresAt }), { status: 201 });
  }

  it("mints on a miss (201) and caches under ghtok:<installationId>", async () => {
    const kv = makeKv();
    const fetchMock = vi.fn(async () =>
      ghTokenResponse("ghs_installtoken", new Date(NOW + 3600_000).toISOString()),
    );
    vi.stubGlobal("fetch", fetchMock);
    const r = await installationToken({ ...baseEnv(), RUNNER_JOB_PATS: kv }, INST, NOW);
    expect(r.token).toBe("ghs_installtoken");
    expect(fetchMock).toHaveBeenCalledOnce();
    // POSTed to the installation's access_tokens endpoint.
    expect(fetchMock.mock.calls[0][0]).toContain(`/app/installations/${INST}/access_tokens`);
    // Cached under the installation-scoped key (I3).
    expect(kv.store.has(`ghtok:${INST}`)).toBe(true);
  });

  it("cache HIT (live token) skips the fetch entirely", async () => {
    const kv = makeKv({
      [`ghtok:${INST}`]: JSON.stringify({
        token: "ghs_cached",
        expires_at: new Date(NOW + 3600_000).toISOString(),
      }),
    });
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const r = await installationToken({ ...baseEnv(), RUNNER_JOB_PATS: kv }, INST, NOW);
    expect(r.token).toBe("ghs_cached");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("near-expiry cache entry RE-FETCHES (safety margin)", async () => {
    const kv = makeKv({
      // Expires in 2 min — inside the 5-min safety margin ⇒ treated as stale.
      [`ghtok:${INST}`]: JSON.stringify({
        token: "ghs_stale",
        expires_at: new Date(NOW + 2 * 60_000).toISOString(),
      }),
    });
    const fetchMock = vi.fn(async () =>
      ghTokenResponse("ghs_fresh", new Date(NOW + 3600_000).toISOString()),
    );
    vi.stubGlobal("fetch", fetchMock);
    const r = await installationToken({ ...baseEnv(), RUNNER_JOB_PATS: kv }, INST, NOW);
    expect(r.token).toBe("ghs_fresh");
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  it("per-installation isolation: two installations use distinct cache keys", async () => {
    const kv = makeKv();
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(ghTokenResponse("tok-A", new Date(NOW + 3600_000).toISOString()))
      .mockResolvedValueOnce(ghTokenResponse("tok-B", new Date(NOW + 3600_000).toISOString()));
    vi.stubGlobal("fetch", fetchMock);
    const a = await installationToken({ ...baseEnv(), RUNNER_JOB_PATS: kv }, "111", NOW);
    const b = await installationToken({ ...baseEnv(), RUNNER_JOB_PATS: kv }, "222", NOW);
    expect(a.token).toBe("tok-A");
    expect(b.token).toBe("tok-B");
    expect(kv.store.has("ghtok:111")).toBe(true);
    expect(kv.store.has("ghtok:222")).toBe(true);
  });

  it("non-2xx THROWS (fails the mint — never a silent fallback) [I4]", async () => {
    const fetchMock = vi.fn(async () => new Response("bad installation", { status: 404 }));
    vi.stubGlobal("fetch", fetchMock);
    await expect(installationToken(baseEnv(), INST, NOW)).rejects.toThrow(/installation token/);
  });

  it("error body carries NO key material [I2]", async () => {
    const fetchMock = vi.fn(async () => new Response("gh error body", { status: 401 }));
    vi.stubGlobal("fetch", fetchMock);
    const err = await installationToken(baseEnv(), INST, NOW).catch((e: Error) => e);
    const msg = (err as Error).message;
    expect(msg).not.toContain("PRIVATE KEY");
    expect(msg).not.toContain(privateKeyPem.slice(40, 80)); // no key body substring
  });
});

describe("mintJitAuthToken (auth-selection: App vs first-party)", () => {
  afterEach(() => vi.unstubAllGlobals());
  const NOW_ISO = new Date(Date.now() + 3600_000).toISOString();

  it("App creds ABSENT ⇒ GITHUB_MINT_TOKEN, byte-identical [I1]", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const env = { GITHUB_MINT_TOKEN: "first-party-static" } as unknown as Env;
    const tok = await mintJitAuthToken(env, "150584374");
    expect(tok).toBe("first-party-static");
    expect(fetchMock).not.toHaveBeenCalled(); // no App-token exchange
  });

  it("App creds present but NO installationId ⇒ GITHUB_MINT_TOKEN [I1]", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const env = {
      GITHUB_MINT_TOKEN: "first-party-static",
      GITHUB_APP_ID: "123456",
      GITHUB_APP_PRIVATE_KEY: privateKeyPem,
    } as unknown as Env;
    const tok = await mintJitAuthToken(env, "");
    expect(tok).toBe("first-party-static");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("App creds present AND installationId ⇒ installation token (NOT the static one)", async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ token: "ghs_install", expires_at: NOW_ISO }), { status: 201 }),
    );
    vi.stubGlobal("fetch", fetchMock);
    const env = {
      GITHUB_MINT_TOKEN: "first-party-static",
      GITHUB_APP_ID: "123456",
      GITHUB_APP_PRIVATE_KEY: privateKeyPem,
    } as unknown as Env;
    const tok = await mintJitAuthToken(env, "150584374");
    expect(tok).toBe("ghs_install");
    expect(fetchMock).toHaveBeenCalledOnce();
  });
});
