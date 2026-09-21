import { afterEach, describe, expect, it, vi } from "vitest";
import { authorizeRunner, RunnerAuthorizationError } from "../src/lib/runner_authorization";
import type { MintEnv, MintParams } from "../src/lib";

const env: MintEnv = {
  CORELINK_RUNNER_MINT_AUTH_KEY: "dispatch-key",
  CORELINK_MINT_URL: "https://mint.example",
  FABRIC_COMPUTE_URL: "https://fabric.example",
  CORELINK_CF_ACCESS_CLIENT_ID: "cf-id",
  CORELINK_CF_ACCESS_CLIENT_SECRET: "cf-secret",
};
const params: MintParams = { jobId: "42", repoFullName: "acme/api", installationId: "123" };
const good = { tenant: "tenant-1", max_concurrency: 4, max_vcpu_h: 12, compute_grant: "fixture.grant" };

afterEach(() => vi.restoreAllMocks());

describe("authorizeRunner", () => {
  it("rejects missing inputs without fetch", async () => {
    const fetch = vi.fn();
    vi.stubGlobal("fetch", fetch);
    for (const [e, p] of [
      [{ ...env, CORELINK_RUNNER_MINT_AUTH_KEY: "" }, params],
      [env, { ...params, jobId: " " }],
      [env, { ...params, repoFullName: " acme/api" }],
      [env, { ...params, installationId: "", acquiringPat: "" }],
    ] as [MintEnv, MintParams][]) await expect(authorizeRunner(e, p)).rejects.toBeInstanceOf(RunnerAuthorizationError);
    expect(fetch).not.toHaveBeenCalled();
  });

  it.each([
    ["403", new Response("secret response", { status: 403 })],
    ["5xx", new Response("database details", { status: 503 })],
    ["null", new Response("null", { status: 200 })],
    ["array", new Response("[]", { status: 200 })],
    ["malformed", new Response(JSON.stringify({ tenant: "tenant-1", max_concurrency: "4" }), { status: 200 })],
    ["missing cap", new Response(JSON.stringify({ tenant: "tenant-1" }), { status: 200 })],
  ])("fails closed on %s without exposing upstream details", async (_name, response) => {
    vi.stubGlobal("fetch", vi.fn(async () => response));
    await expect(authorizeRunner(env, params)).rejects.toBeInstanceOf(RunnerAuthorizationError);
  });

  it.each([0, 12.5, -1, 0x100000000])("refuses an invalid metered entitlement %s", async (cap) => {
    vi.stubGlobal("fetch", vi.fn(async () => Response.json({ ...good, max_vcpu_h: cap })));
    await expect(authorizeRunner(env, params)).rejects.toBeInstanceOf(RunnerAuthorizationError);
  });

  it("requires a grant for metered authorization and binds the request attempt", async () => {
    const id = "22222222-2222-4222-8222-222222222222";
    vi.stubGlobal("fetch", vi.fn(async (_url, init) => {
      expect(JSON.parse(init.body).compute_reservation_id).toBe(id);
      return Response.json({ tenant: "tenant-1", max_concurrency: 4, max_vcpu_h: 12 });
    }));
    await expect(authorizeRunner(env, { ...params, computeReservationId: id })).rejects.toBeInstanceOf(RunnerAuthorizationError);
  });

  it("accepts max_vcpu_h without a grant while compute admission is unarmed", async () => {
    const fetch = vi.fn(async (_url: string, init: RequestInit) => {
      expect(JSON.parse(String(init.body))).not.toHaveProperty("compute_reservation_id");
      return Response.json({ tenant: "tenant-1", max_concurrency: 4, max_vcpu_h: 12 });
    });
    vi.stubGlobal("fetch", fetch);
    await expect(authorizeRunner({ ...env, FABRIC_COMPUTE_URL: undefined }, { ...params, computeReservationId: "reservation-1" })).resolves.toEqual({
      tenant: "tenant-1", maxConcurrency: 4, maxVcpuH: 12,
    });
  });

  it("rejects an unexpected compute grant while compute admission is unarmed", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => Response.json({
      tenant: "tenant-1", max_concurrency: 4, max_vcpu_h: 12, compute_grant: "fixture.grant",
    })));
    await expect(authorizeRunner({ ...env, FABRIC_COMPUTE_URL: undefined }, params)).rejects.toBeInstanceOf(RunnerAuthorizationError);
  });

  it("rejects a compute grant when the authorization has no ceiling", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => Response.json({
      tenant: "tenant-1", max_concurrency: 4, compute_grant: "fixture.grant",
    })));
    await expect(authorizeRunner(env, params)).rejects.toBeInstanceOf(RunnerAuthorizationError);
  });

  it("fails closed on transport errors", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("secret transport detail"); }));
    await expect(authorizeRunner(env, params)).rejects.toBeInstanceOf(RunnerAuthorizationError);
  });

  it("maps valid authorization and sends installation wire with access headers", async () => {
    const fetch = vi.fn(async (_url: string, init: RequestInit) => {
      expect(_url).toBe("https://mint.example/internal/v1/runner/authorize");
      expect(new Headers(init.headers).get("x-corelink-internal-auth")).toBe("dispatch-key");
      expect(new Headers(init.headers).get("CF-Access-Client-Id")).toBe("cf-id");
      expect(JSON.parse(String(init.body))).toEqual({ job_id: "42", repo_full_name: "acme/api", installation_id: "123" });
      return new Response(JSON.stringify(good), { status: 200 });
    });
    vi.stubGlobal("fetch", fetch);
    await expect(authorizeRunner(env, params)).resolves.toEqual({ tenant: "tenant-1", maxConcurrency: 4, maxVcpuH: 12, computeGrant: "fixture.grant" });
  });

  it("omits installation only when the acquiring PAT caller has none", async () => {
    const fetch = vi.fn(async (_url: string, init: RequestInit) => {
      const headers = new Headers(init.headers);
      expect(headers.get("authorization")).toBe("Bearer tenant-pat");
      expect(JSON.parse(String(init.body))).toEqual({ job_id: "42", repo_full_name: "acme/api" });
      return new Response(JSON.stringify({ tenant: "tenant-1", max_concurrency: 1 }), { status: 200 });
    });
    vi.stubGlobal("fetch", fetch);
    await expect(authorizeRunner(env, { ...params, installationId: "", acquiringPat: "tenant-pat" })).resolves.toEqual({ tenant: "tenant-1", maxConcurrency: 1 });
  });

  it("preserves installation alongside acquiring PAT for server ownership validation", async () => {
    const fetch = vi.fn(async (_url: string, init: RequestInit) => {
      expect(new Headers(init.headers).get("authorization")).toBe("Bearer tenant-pat");
      expect(JSON.parse(String(init.body))).toEqual({ job_id: "42", repo_full_name: "acme/api", installation_id: "123" });
      return new Response(JSON.stringify(good), { status: 200 });
    });
    vi.stubGlobal("fetch", fetch);
    await expect(authorizeRunner(env, { ...params, acquiringPat: "tenant-pat" })).resolves.toMatchObject({ tenant: "tenant-1" });
  });
});
