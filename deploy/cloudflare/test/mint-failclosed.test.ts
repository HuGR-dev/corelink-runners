import { afterEach, describe, expect, it, vi } from "vitest";
import { buildContainerEnv, type MintEnv } from "../src/lib";

const env: MintEnv = {
  CORELINK_RUNNER_MINT_AUTH_KEY: "dispatcher-key",
  CORELINK_MINT_URL: "https://mint.example",
  CLW_ENDPOINT: "https://cas.example",
};
const params = { jobId: "7", repoFullName: "acme/api", installationId: "123" };
const stash = {
  stash: async () => "ticket-7",
};
const deps = { stash, fabricEndpoint: "https://runner.example" };

function mintResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}

afterEach(() => vi.restoreAllMocks());

describe("required mint fail-closed contract", () => {
  it.each(["", "  ", undefined])("missing job identity %s refuses before issuance", async jobId => {
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    const stashCall = vi.fn();
    await expect(buildContainerEnv(env, { ...params, jobId } as typeof params, {
      ...deps, stash: { stash: stashCall },
    })).resolves.toMatchObject({ authz: "forbidden", containerEnv: {} });
    expect(fetcher).not.toHaveBeenCalled();
    expect(stashCall).not.toHaveBeenCalled();
  });

  it("absent key, missing repo, and missing installation/PAT refuse", async () => {
    await expect(buildContainerEnv({}, params)).resolves.toMatchObject({ authz: "forbidden", coldReason: "mint_key_unarmed" });
    await expect(buildContainerEnv(env, { ...params, repoFullName: "" })).resolves.toMatchObject({ authz: "forbidden", coldReason: "no_repo" });
    await expect(buildContainerEnv(env, { ...params, installationId: "" })).resolves.toMatchObject({ authz: "forbidden", coldReason: "no_installation_or_pat" });
  });

  it.each([
    ["wrong key / 403", mintResponse({ error: "wrong secret" }, 403)],
    ["server error", mintResponse({ error: "database secret" }, 500)],
    ["null response", mintResponse(null)],
    ["array response", mintResponse([])],
    ["malformed response", mintResponse({ token_plaintext: "pat", pat_id: "p", tenant: "t" })],
    ["missing cap", mintResponse({ token_plaintext: "pat", pat_id: "p", tenant: "t" })],
    ["string cap", mintResponse({ token_plaintext: "pat", pat_id: "p", tenant: "t", max_concurrency: "3" })],
    ["unsafe integer cap", mintResponse({ token_plaintext: "pat", pat_id: "p", tenant: "t", max_concurrency: Number.MAX_SAFE_INTEGER + 1 })],
    ["whitespace token", mintResponse({ token_plaintext: "  ", pat_id: "p", tenant: "t", max_concurrency: 3 })],
    ["whitespace pat id", mintResponse({ token_plaintext: "pat", pat_id: "  ", tenant: "t", max_concurrency: 3 })],
    ["whitespace tenant", mintResponse({ token_plaintext: "pat", pat_id: "p", tenant: "  ", max_concurrency: 3 })],
  ])("%s is forbidden with no PAT env", async (_name, response) => {
    vi.stubGlobal("fetch", vi.fn(async () => response));
    const logs = vi.spyOn(console, "log");
    const result = await buildContainerEnv(env, params, deps);
    expect(result.authz).toBe("forbidden");
    expect(result.containerEnv).toEqual({});
    expect(JSON.stringify(logs.mock.calls)).not.toContain("wrong secret");
    expect(JSON.stringify(logs.mock.calls)).not.toContain("database secret");
  });

  it("bad cap retains known cleanup identity without retaining the PAT", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => mintResponse({
      token_plaintext: "secret-pat",
      pat_id: "pat-known",
      tenant: "tenant-known",
      max_concurrency: "three",
    })));
    const result = await buildContainerEnv(env, params, deps);
    expect(result).toMatchObject({ authz: "forbidden", patId: "pat-known", tenant: "tenant-known" });
    expect(result).not.toHaveProperty("maxConcurrency");
    expect(JSON.stringify(result)).not.toContain("secret-pat");
  });

  it("transport failure is forbidden without leaking the thrown error", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("PAT plaintext transport leak"); }));
    const logs = vi.spyOn(console, "log");
    await expect(buildContainerEnv(env, params, deps)).resolves.toMatchObject({ authz: "forbidden", containerEnv: {} });
    expect(JSON.stringify(logs.mock.calls)).not.toContain("PAT plaintext transport leak");
  });

  it("stash failure is forbidden while retaining cleanup metadata and no raw token", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => mintResponse({
      token_plaintext: "secret-pat",
      pat_id: "pat-7",
      tenant: "tenant-7",
      max_concurrency: 3,
    })));
    const result = await buildContainerEnv(env, params, {
      fabricEndpoint: "https://runner.example",
      stash: { stash: async () => { throw new Error("storage down"); } },
    });
    expect(result).toMatchObject({ authz: "forbidden", patId: "pat-7", tenant: "tenant-7", maxConcurrency: 3 });
    expect(result.containerEnv).toEqual({});
    expect(JSON.stringify(result)).not.toContain("secret-pat");
  });

  it("successful env-0 mint preserves server tenant/cap and never emits raw CLW_TOKEN", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => mintResponse({
      token_plaintext: "secret-pat",
      pat_id: "pat-7",
      tenant: "tenant-7",
      max_concurrency: 3,
      max_vcpu_h: 0,
    })));
    const result = await buildContainerEnv(env, params, deps);
    expect(result).toMatchObject({ authz: "ok", patId: "pat-7", tenant: "tenant-7", maxConcurrency: 3, maxVcpuH: 0 });
    expect(result.containerEnv).toMatchObject({ CLW_TENANT: "tenant-7", CLW_CRED_TICKET: "ticket-7" });
    expect(result.containerEnv).not.toHaveProperty("CLW_TOKEN");
    expect(JSON.stringify(result)).not.toContain("secret-pat");
  });
});
