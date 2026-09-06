import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({
  Container: class {
    ctx: any; env: any;
    constructor(ctx: any, env: any) { this.ctx = ctx; this.env = env; }
    defaultPort = 6080;
    start = vi.fn(async () => undefined);
    stop = vi.fn(async () => undefined);
    destroy = vi.fn(async () => undefined);
    schedule = vi.fn(async () => undefined);
    renewActivityTimeout() {}
    async containerFetch() { return new Response("unexpected", { status: 502 }); }
  },
}));
vi.mock("../src/lib.js", async (importOriginal) => ({ ...await importOriginal<typeof import("../src/lib.js")>(), revokeCasPatById: vi.fn(async () => undefined) }));
import { RunnerDevEnvDO } from "../src/durable_objects/runner_dev_env.js";
import type { AuthorizedDevenvStart } from "../src/types/devenv.js";
import { DEVENV_CREDENTIAL_KEY } from "../src/lib/devenv_credentials.js";

const NOW = Date.parse("2026-09-05T12:00:00Z");
const tenantId = "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3";
const sessionUuid = "11111111-1111-4111-8111-111111111111";

function grant(): AuthorizedDevenvStart {
  return { config: { workspaceName: "repo", profileName: "browser", tier: "standard-4" }, grant: { tenantId, sessionUuid, patId: "22222222-2222-4222-8222-222222222222", casPat: "synthetic-cas-secret", expiresAtMs: NOW + 60_000, computeReservationId: sessionUuid } };
}
function token() {
  const payload = { v: 1, key_id: "key", tenant_id: tenantId, workload_kind: "devenv", workload_id: sessionUuid, reservation_id: sessionUuid, period_key: 202609, ceiling_vcpu_ms: "1000000", vcpu_count: 4, maximum_wall_ms: 28_800_000, issued_at_ms: NOW - 1_000, expires_at_ms: NOW + 60_000 };
  return `${btoa(JSON.stringify(payload)).replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_")}.signature`;
}
function obligationToken(id: string, workloadId: string, expiresAtMs = NOW + 60_000) {
  const payload = { v: 1, key_id: "key", tenant_id: tenantId, workload_kind: "devenv", workload_id: workloadId, reservation_id: id, period_key: 202609, ceiling_vcpu_ms: "1000000", vcpu_count: 4, maximum_wall_ms: 28_800_000, issued_at_ms: expiresAtMs - 60_000, expires_at_ms: expiresAtMs };
  return `${btoa(JSON.stringify(payload)).replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_")}.signature`;
}
function fixture(fetcher: typeof fetch) {
  const stored = new Map<string, unknown>(); const stashed = new Map<string, unknown>(); let gate = Promise.resolve();
  const ctx = { storage: {
    get: vi.fn(async (key: string) => structuredClone(stored.get(key))), put: vi.fn(async (key: string, value: unknown) => { stored.set(key, structuredClone(value)); }), delete: vi.fn(async (key: string) => { stored.delete(key); }), list: vi.fn(async (options: { prefix: string; limit: number; startAfter?: string }) => new Map([...stored].filter(([key]) => key.startsWith(options.prefix) && (!options.startAfter || key > options.startAfter)).sort(([a], [b]) => a.localeCompare(b)).slice(0, options.limit).map(([key, value]) => [key, structuredClone(value)]))),
  }, blockConcurrencyWhile: (fn: () => Promise<unknown>) => { const result = gate.then(fn); gate = result.then(() => undefined, () => undefined); return result; }, id: { toString: () => "devenv-test" } };
  const env: any = { FABRIC_COMPUTE_URL: "https://fabric.example", SPAWN_WORKER_PUBLIC_URL: "https://spawn.test", CORELINK_RUNNER_MINT_AUTH_KEY: "synthetic-dispatcher-key", CRED_STASH: { idFromName: (id: string) => id, get: (id: string) => ({ stash: vi.fn(async (ticket: string, cred: unknown) => { stashed.set(id, { ticket, cred }); return "a".repeat(64); }), wipe: vi.fn(async () => { stashed.delete(id); }) }) } };
  vi.stubGlobal("fetch", fetcher);
  const instance = new RunnerDevEnvDO(ctx, env);
  return { instance, stored, stashed, ctx, env };
}
function response(url: string, state = url.endsWith("/reserve") ? "prepared" : url.endsWith("/cancel") ? "cancelled" : "active") { return new Response(JSON.stringify({ reservation_id: sessionUuid, state }), { status: 200 }); }

beforeEach(() => vi.spyOn(Date, "now").mockReturnValue(NOW));
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("authorized DevEnv compute composition", () => {
  it("prepares metered compute before credential/provider start and claims once", async () => {
    const fetcher = vi.fn(async (url: string) => response(url)); const f = fixture(fetcher);
    await f.instance.prepareAuthorizedCompute({ token: token(), reservationId: sessionUuid, tenantId, workloadKind: "devenv", workloadId: sessionUuid, vcpuCount: 4, maximumWallMs: 28_800_000 });
    vi.mocked(f.instance.start).mockImplementation(async () => { expect((f.stored.get(`compute:obligation:${sessionUuid}`) as { phase: string }).phase).toBe("dispatched"); });
    await expect(f.instance.startAuthorizedDevenv(grant())).resolves.toMatchObject({ status: "starting" });
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(f.instance.start).toHaveBeenCalledTimes(1);
    expect((f.stored.get(`compute:obligation:${sessionUuid}`) as { phase: string }).phase).toBe("dispatched");
  });

  it.each([["reserve", 429], ["activate", 503]])("does not start a provider when %s admission fails", async (operation, status) => {
    const fetcher = vi.fn(async (url: string) => url.endsWith(`/${operation}`) ? new Response("rejected", { status }) : response(url)); const f = fixture(fetcher);
    await expect(f.instance.prepareAuthorizedCompute({ token: token(), reservationId: sessionUuid, tenantId, workloadKind: "devenv", workloadId: sessionUuid, vcpuCount: 4, maximumWallMs: 28_800_000 })).rejects.toThrow();
    expect(f.instance.start).not.toHaveBeenCalled(); expect(f.stashed.size).toBe(0);
  });

  it("does not spend the same reservation on a duplicate start", async () => {
    const fetcher = vi.fn(async (url: string) => response(url)); const f = fixture(fetcher); const b = { token: token(), reservationId: sessionUuid, tenantId, workloadKind: "devenv" as const, workloadId: sessionUuid, vcpuCount: 4, maximumWallMs: 28_800_000 };
    await f.instance.prepareAuthorizedCompute(b); const payload = grant();
    await f.instance.startAuthorizedDevenv(payload);
    await expect(f.instance.startAuthorizedDevenv(payload)).rejects.toThrow("DEVENV_START_REQUIRES_TERMINAL_STATE");
    expect(f.instance.start).toHaveBeenCalledTimes(1); expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("retains a dispatched reservation when provider start fails", async () => {
    const fetcher = vi.fn(async (url: string) => response(url)); const f = fixture(fetcher); const b = { token: token(), reservationId: sessionUuid, tenantId, workloadKind: "devenv" as const, workloadId: sessionUuid, vcpuCount: 4, maximumWallMs: 28_800_000 };
    await f.instance.prepareAuthorizedCompute(b); vi.mocked(f.instance.start).mockRejectedValueOnce(new Error("provider failed"));
    await expect(f.instance.startAuthorizedDevenv(grant())).rejects.toThrow("DEVENV_AUTHORIZED_START_FAILED");
    expect((f.stored.get(`compute:obligation:${sessionUuid}`) as { phase: string }).phase).toBe("dispatched");
    expect(f.instance.start).toHaveBeenCalledTimes(1); expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false);
  });

  it("carries the drain cursor across restart before reaching an expired preparation", async () => {
    const fetcher = vi.fn(async (url: string) => new Response(JSON.stringify({ reservation_id: "11111111-1111-4111-8111-999999999999", state: url.endsWith("/cancel") ? "cancelled" : "active" }), { status: 200 })); const f = fixture(fetcher);
    await f.ctx.blockConcurrencyWhile(async () => undefined);
    for (let n = 1; n <= 25; n++) {
      const id = `11111111-1111-4111-8111-${String(n).padStart(12, "0")}`;
      await f.ctx.storage.put(`compute:obligation:${id}`, { binding: { token: obligationToken(id, id), reservationId: id, tenantId, workloadKind: "devenv", workloadId: id, vcpuCount: 4, maximumWallMs: 28_800_000 }, phase: "terminal", deadlineMs: NOW + 60_000, terminalKind: "cancelled" });
    }
    const expiredId = "11111111-1111-4111-8111-999999999999";
    await f.ctx.storage.put(`compute:obligation:${expiredId}`, { binding: { token: obligationToken(expiredId, expiredId, NOW - 1), reservationId: expiredId, tenantId, workloadKind: "devenv", workloadId: expiredId, vcpuCount: 4, maximumWallMs: 28_800_000 }, phase: "preparing", deadlineMs: NOW - 1 });
    await f.instance.retryUnusedCompute({ reservationId: expiredId });
    expect(fetcher).not.toHaveBeenCalled();
    expect(f.stored.get("compute:drain-cursor")).toMatch(/compute:obligation:/);
    const restarted = new RunnerDevEnvDO(f.ctx, f.env);
    await f.ctx.blockConcurrencyWhile(async () => undefined);
    await restarted.retryUnusedCompute({ reservationId: expiredId });
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(fetcher.mock.calls[0]?.[0]).toMatch(/\/cancel$/);
    expect(f.stored.has("compute:drain-cursor")).toBe(false);
    expect((f.stored.get(`compute:obligation:${expiredId}`) as { terminalKind: string }).terminalKind).toBe("cancelled");
    expect(restarted.start).not.toHaveBeenCalled();
    expect(restarted.schedule).not.toHaveBeenCalled();
  });
});
