// Unit tests for the spawn-Worker's security-critical pure logic: constant-time
// auth compare, GitHub HMAC verification, and the FAIL-OPEN warm-mint env build.
// Plain vitest (node) — these functions don't need the Workers runtime
// (crypto.subtle + crypto.randomUUID are on Node 20+).
import { describe, it, expect, vi, afterEach } from "vitest";
import {
  safeEqual,
  verifyGithubHmac,
  buildContainerEnv,
  maybeRevokeCasPat,
} from "../src/lib";

describe("safeEqual (constant-time bearer compare)", () => {
  it("true for equal strings", () => expect(safeEqual("abc", "abc")).toBe(true));
  it("false for different strings of equal length", () =>
    expect(safeEqual("abc", "abd")).toBe(false));
  it("false for different lengths", () => expect(safeEqual("abc", "abcd")).toBe(false));
  it("false for empty vs non-empty", () => expect(safeEqual("", "x")).toBe(false));
});

// Sign a body exactly as GitHub does (HMAC-SHA256 → `sha256=<hex>`).
async function ghSign(secret: string, body: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  const hex = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `sha256=${hex}`;
}

describe("verifyGithubHmac", () => {
  const secret = "whsec-test";
  const body = '{"action":"queued"}';

  it("accepts a correctly-signed body", async () => {
    const sig = await ghSign(secret, body);
    expect(await verifyGithubHmac(secret, sig, body)).toBe(true);
  });
  it("rejects a tampered body", async () => {
    const sig = await ghSign(secret, body);
    expect(await verifyGithubHmac(secret, sig, body + " ")).toBe(false);
  });
  it("rejects a wrong secret", async () => {
    const sig = await ghSign("other", body);
    expect(await verifyGithubHmac(secret, sig, body)).toBe(false);
  });
  it("rejects a signature without the sha256= prefix", async () => {
    expect(await verifyGithubHmac(secret, "deadbeef", body)).toBe(false);
  });
  it("rejects an empty signature", async () => {
    expect(await verifyGithubHmac(secret, "", body)).toBe(false);
  });
});

describe("buildContainerEnv (warm-mint, FAIL-OPEN to cold)", () => {
  afterEach(() => vi.unstubAllGlobals());

  const JIT = "encoded-jit";
  const JOB = "987654321"; // GH workflow_job.id

  it("COLD when no mint key configured (only the JIT, no CLW_*)", async () => {
    const env = { CLW_TENANT: "t" } as never; // key absent
    const e = await buildContainerEnv(env, JIT, JOB);
    expect(e.CORELINK_RUNNER_JITCONFIG).toBe(JIT);
    expect(e.CLW_TOKEN).toBeUndefined();
    expect(e.CLW_ENDPOINT).toBeUndefined();
  });

  it("COLD when no tenant configured", async () => {
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never; // tenant absent
    const e = await buildContainerEnv(env, JIT, JOB);
    expect(e.CLW_TOKEN).toBeUndefined();
  });

  it("WARM when key+tenant present and the D-9 mint succeeds", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(JSON.stringify({ token_plaintext: "per-job-pat" }), { status: 200 })),
    );
    const env = {
      CORELINK_RUNNER_MINT_AUTH_KEY: "k",
      CLW_TENANT: "ee30f7ba",
      CLW_ENDPOINT: "https://corelink-api.humangr.com",
      CORELINK_MINT_URL: "https://corelink-api.humangr.com",
    } as never;
    const e = await buildContainerEnv(env, JIT, JOB);
    expect(e.CORELINK_RUNNER_JITCONFIG).toBe(JIT);
    expect(e.CLW_TOKEN).toBe("per-job-pat");
    expect(e.CLW_TENANT).toBe("ee30f7ba");
    expect(e.CLW_ENDPOINT).toBe("https://corelink-api.humangr.com");
    expect(e.CLW_REF_DOMAIN).toBe("runner");
  });

  it("mints under the GH workflow_job.id (so completion can revoke it)", async () => {
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify({ token_plaintext: "per-job-pat" }), { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    await buildContainerEnv(env, JIT, JOB);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/internal/v1/runner/mint");
    expect(JSON.parse((init as RequestInit).body as string).job_id).toBe(JOB);
  });

  it("FAIL-OPEN to COLD when the D-9 mint returns non-2xx", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 500 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    const e = await buildContainerEnv(env, JIT, JOB);
    expect(e.CORELINK_RUNNER_JITCONFIG).toBe(JIT); // job still runs
    expect(e.CLW_TOKEN).toBeUndefined(); // but cold — no CLW_*
  });

  it("FAIL-OPEN to COLD when the mint response has no token", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({}), { status: 200 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    const e = await buildContainerEnv(env, JIT, JOB);
    expect(e.CLW_TOKEN).toBeUndefined();
  });
});

describe("maybeRevokeCasPat (completion hardening, FAIL-OPEN)", () => {
  afterEach(() => vi.unstubAllGlobals());

  const JOB = "987654321";

  it("no-op (false) when the mint key is absent — nothing was minted", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const env = { CLW_TENANT: "ee30f7ba" } as never; // key absent
    expect(await maybeRevokeCasPat(env, JOB)).toBe(false);
    expect(fetchMock).not.toHaveBeenCalled(); // never even calls D-9
  });

  it("no-op (false) when the tenant is absent", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never;
    expect(await maybeRevokeCasPat(env, JOB)).toBe(false);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("revokes by (owner_tenant, job_id) and returns true on 2xx", async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    const env = {
      CORELINK_RUNNER_MINT_AUTH_KEY: "k",
      CLW_TENANT: "ee30f7ba",
      CORELINK_MINT_URL: "https://corelink-api.humangr.com",
    } as never;
    expect(await maybeRevokeCasPat(env, JOB)).toBe(true);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/internal/v1/runner/revoke");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(body.owner_tenant).toBe("ee30f7ba");
    expect(body.job_id).toBe(JOB);
    expect((init as RequestInit).headers).toMatchObject({ "x-corelink-internal-auth": "k" });
  });

  it("FAIL-OPEN (false, swallowed) when revoke returns non-2xx", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 500 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    expect(await maybeRevokeCasPat(env, JOB)).toBe(false); // never throws
  });

  it("FAIL-OPEN (false, swallowed) when fetch itself throws", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => {
      throw new Error("network down");
    }));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    expect(await maybeRevokeCasPat(env, JOB)).toBe(false);
  });
});
