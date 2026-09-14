import { afterEach, describe, expect, it, vi } from "vitest";
import { adoptIssuedRunnerCredential } from "../src/lib/runner_credential_adoption";
import type { MintEnv } from "../src/lib";

const operationId = "123e4567-e89b-12d3-a456-426614174000";
const env: MintEnv = {
  CORELINK_RUNNER_MINT_AUTH_KEY: "mint-key",
  CORELINK_MINT_URL: "https://mint.example",
  CORELINK_CF_ACCESS_CLIENT_ID: "cf-id",
  CORELINK_CF_ACCESS_CLIENT_SECRET: "cf-secret",
};

afterEach(() => vi.restoreAllMocks());

describe("adoptIssuedRunnerCredential", () => {
  it("posts the exact adoption wire with mint and CF Access headers", async () => {
    const fetch = vi.fn(async (url: string, init: RequestInit) => {
      expect(url).toBe("https://mint.example/internal/v1/runner/adopt");
      const headers = new Headers(init.headers);
      expect(headers.get("x-corelink-internal-auth")).toBe("mint-key");
      expect(headers.get("CF-Access-Client-Id")).toBe("cf-id");
      expect(headers.get("CF-Access-Client-Secret")).toBe("cf-secret");
      expect(headers.get("content-type")).toBe("application/json");
      expect(JSON.parse(String(init.body))).toEqual({ operation_id: operationId, pat_id: "pat-123" });
      expect(init.method).toBe("POST");
      expect(init.signal).toBeInstanceOf(AbortSignal);
      return new Response(null, { status: 204 });
    });
    vi.stubGlobal("fetch", fetch);
    await expect(adoptIssuedRunnerCredential(env, operationId, "pat-123")).resolves.toBeUndefined();
    expect(fetch).toHaveBeenCalledOnce();
  });

  it("rejects invalid inputs without HTTP", async () => {
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);
    for (const [key, operation, pat] of [
      ["", operationId, "pat"],
      [" mint-key", operationId, "pat"],
      ["mint-key", "", "pat"],
      ["mint-key", "123e4567-e89b-12d3-a456-42661417400", "pat"],
      ["mint-key", operationId, ""],
      ["mint-key", operationId, " pat"],
    ]) {
      await expect(adoptIssuedRunnerCredential({ ...env, CORELINK_RUNNER_MINT_AUTH_KEY: key }, operation, pat))
        .rejects.toThrow("runner credential adoption unavailable");
    }
    expect(fetch).not.toHaveBeenCalled();
  });

  it.each([
    ["403", 403],
    ["500", 500],
    ["200", 200],
  ])("rejects %s and exposes no upstream body", async (_name, status) => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("SECRET_UPSTREAM_BODY", { status })));
    await expect(adoptIssuedRunnerCredential(env, operationId, "pat-123"))
      .rejects.toThrow("runner credential adoption unavailable");
  });

  it("rejects transport failures with a generic error", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("SECRET_TRANSPORT_DETAIL"); }));
    await expect(adoptIssuedRunnerCredential(env, operationId, "pat-123"))
      .rejects.toThrow("runner credential adoption unavailable");
  });
});
