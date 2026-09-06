import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthorizedDevenvStart } from "../src/types/devenv.js";
import { DEVENV_CREDENTIAL_KEY } from "../src/lib/devenv_credentials.js";

vi.mock("@cloudflare/containers", () => ({
  Container: class {
    constructor(public ctx: any, public env: any) {}
    start = vi.fn(async (_opts: unknown) => undefined);
    stop = vi.fn(async () => undefined);
    destroy = vi.fn(async () => undefined);
    schedule = vi.fn(async (_date: Date, _callback: string, _payload: unknown) => undefined);
    renewActivityTimeout() {}
    async containerFetch() { return new Response("unexpected proxy", { status: 502 }); }
  },
}));
vi.mock("../src/lib.js", async (importOriginal) => ({
  ...await importOriginal<typeof import("../src/lib.js")>(),
  revokeCasPatById: vi.fn(async () => undefined),
}));
import { revokeCasPatById } from "../src/lib.js";
import { RunnerDevEnvDO } from "../src/durable_objects/runner_dev_env.js";

const NOW = Date.parse("2026-09-05T12:00:00Z");
function grant(): AuthorizedDevenvStart {
  return {
    config: { workspaceName: "repo", profileName: "browser", tier: "standard-4" },
    grant: { tenantId: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3", sessionUuid: crypto.randomUUID(),
      patId: crypto.randomUUID(), casPat: "synthetic-cas-secret-never-in-provider-env", expiresAtMs: NOW + 60000 },
  };
}

function fixture() {
  const stored = new Map<string, any>();
  const stashed = new Map<string, unknown>();
  const wipe = vi.fn(async (id: string, _deadline?: number) => { stashed.delete(id); });
  const stash = vi.fn(async (id: string, ticket: string, cred: unknown, ttl: number, deadline: number) => {
    stashed.set(id, { ticket, cred, ttl, deadline });
    return ticket;
  });
  let gate = Promise.resolve();
  const ctx = {
    storage: {
      get: vi.fn(async (key: string) => structuredClone(stored.get(key))),
      put: vi.fn(async (key: string, value: unknown) => { stored.set(key, structuredClone(value)); }),
      delete: vi.fn(async (key: string) => { stored.delete(key); }),
    },
    blockConcurrencyWhile: (fn: () => Promise<any>) => {
      const result = gate.then(fn);
      gate = result.then(() => undefined, () => undefined);
      return result;
    },
    id: { toString: () => "devenv-test" },
  };
  const env: any = {
    SPAWN_WORKER_PUBLIC_URL: "https://spawn.test",
    CORELINK_RUNNER_MINT_AUTH_KEY: "synthetic-dispatcher-key",
    CRED_STASH: { idFromName: (name: string) => name,
      get: (id: string) => ({ stash: (ticket: string, cred: unknown, ttl: number, deadline: number) => stash(id, ticket, cred, ttl, deadline), wipe: (deadline?: number) => wipe(id, deadline) }) },
  };
  return { stored, stashed, stash, wipe, ctx, env,
    restart: async () => {
      const instance = new RunnerDevEnvDO(ctx, env);
      await ctx.blockConcurrencyWhile(async () => undefined);
      return instance;
    },
  };
}

beforeEach(() => {
  vi.spyOn(Date, "now").mockReturnValue(NOW);
  vi.mocked(revokeCasPatById).mockReset().mockResolvedValue(undefined);
});
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });

describe("authorized DevEnv credential lifecycle", () => {
  it("denies raw RPC and HTTP grants regardless of forged headers", async () => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    await expect(instance.startDevenv(payload)).rejects.toThrow("DEVENV_AUTHORIZED_RPC_REQUIRED");
    for (const path of ["/v1/customer/devenv", "/v1/devenv/", "/"]) {
      const response = await instance.fetch(new Request(`https://worker.test${path}`, {
        method: "POST", headers: { "x-corelink-tenant-id": payload.grant.tenantId, "x-corelink-internal-auth": "forged" },
        body: JSON.stringify({ ...payload, clw_token: payload.grant.casPat, clwCredTicket: "forged" }),
      }));
      expect(response.status).toBe(403);
      expect(await response.text()).not.toContain(payload.grant.casPat);
    }
    expect(instance.start).not.toHaveBeenCalled(); expect(f.stash).not.toHaveBeenCalled();
  });

  it("persists identifier ownership and schedules expiry before provider start, with ticket-only env", async () => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    (payload.config as any).clwEndpoint = "https://attacker.test";
    const ack = await instance.startAuthorizedDevenv(payload);
    expect(ack).toEqual({ sessionUuid: payload.grant.sessionUuid, status: "starting" });
    const leaseId = `devenv:${payload.grant.sessionUuid}`;
    expect(f.stash).toHaveBeenCalledWith(leaseId, expect.stringMatching(/^[0-9a-f]{64}$/), {
      token: payload.grant.casPat, tenant: payload.grant.tenantId, endpoint: "https://corelink-api.humangr.com",
    }, 60000, payload.grant.expiresAtMs);
    expect(instance.schedule).toHaveBeenCalledWith(new Date(payload.grant.expiresAtMs), "expireAuthorizedSession", { sessionUuid: payload.grant.sessionUuid });
    expect(vi.mocked(instance.schedule).mock.invocationCallOrder[0]).toBeLessThan(f.stash.mock.invocationCallOrder[0]);
    expect(vi.mocked(instance.schedule).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(instance.start).mock.invocationCallOrder[0]);
    expect(f.ctx.storage.put.mock.calls.find(([key]) => key === DEVENV_CREDENTIAL_KEY)?.[1]).toMatchObject({ providerMayExist: false, patId: payload.grant.patId });
    const env = vi.mocked(instance.start).mock.calls[0][0]?.envVars;
    expect(env).toMatchObject({ CLW_LEASE_ID: leaseId, CLW_FABRIC_ENDPOINT: "https://spawn.test", CLW_CRED_TICKET: expect.any(String) });
    expect(env).not.toHaveProperty("CLW_TOKEN"); expect(env).not.toHaveProperty("CORELINK_TOKEN");
    expect(JSON.stringify([...f.stored])).not.toContain(payload.grant.casPat);
    expect(JSON.stringify(env)).not.toContain(payload.grant.casPat);
  });

  it.each([
    ["tenantId", "00000000-0000-0000-0000-000000000000"], ["sessionUuid", "bad"], ["patId", "bad"],
    ["expiresAtMs", NOW], ["expiresAtMs", NOW + 8 * 3600000 + 1], ["expiresAtMs", Infinity],
    ["casPat", ""], ["casPat", "x".repeat(4097)], ["casPat", "token\nsecret"],
  ])("rejects invalid grant %s before stash or provider effects", async (key, value) => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    (payload.grant as any)[key] = value;
    await expect(instance.startAuthorizedDevenv(payload)).rejects.toThrow("DEVENV_INVALID_GRANT");
    expect(f.stash).not.toHaveBeenCalled(); expect(instance.start).not.toHaveBeenCalled();
  });

  it.each(["CRED_STASH", "SPAWN_WORKER_PUBLIC_URL", "CORELINK_RUNNER_MINT_AUTH_KEY"])("requires %s before accepting ownership", async (key) => {
    const f = fixture(); delete f.env[key]; const instance = await f.restart();
    await expect(instance.startAuthorizedDevenv(grant())).rejects.toThrow("DEVENV_CREDENTIAL_CONFIGURATION");
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false); expect(instance.start).not.toHaveBeenCalled();
  });

  it("rejects caller endpoint substitution and invalid names/tiers", async () => {
    const f = fixture(); const instance = await f.restart();
    for (const config of [{ tier: "unlimited" }, { workspaceName: "../bad" }, { profileName: "x/y" }]) {
      const payload = grant(); Object.assign(payload.config, config);
      await expect(instance.startAuthorizedDevenv(payload)).rejects.toThrow();
    }
    f.env.SPAWN_WORKER_PUBLIC_URL = "http://untrusted.test";
    await expect(instance.startAuthorizedDevenv(grant())).rejects.toThrow("DEVENV_CREDENTIAL_CONFIGURATION_INVALID");
    expect(instance.start).not.toHaveBeenCalled();
  });

  it("wipes and revokes exact ownership on stop and recovers failed cleanup after restart", async () => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    await instance.startAuthorizedDevenv(payload);
    vi.mocked(revokeCasPatById).mockRejectedValueOnce(new Error("upstream secret must not be logged"));
    await instance.onStop();
    expect(f.stored.get(DEVENV_CREDENTIAL_KEY)).toMatchObject({ stashWiped: true, revoked: false, providerMayExist: false });
    const restarted = await f.restart();
    await restarted.requestStop();
    expect(f.wipe).toHaveBeenCalledTimes(1);
    expect(revokeCasPatById).toHaveBeenLastCalledWith(f.env, payload.grant.patId, payload.grant.tenantId, expect.any(AbortSignal));
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false);
    await expect(restarted.startAuthorizedDevenv(grant())).resolves.toMatchObject({ status: "starting" });
  });

  it("blocks a new start until failed credential cleanup actually succeeds", async () => {
    const f = fixture(); const instance = await f.restart();
    await instance.startAuthorizedDevenv(grant());
    f.wipe.mockRejectedValue(new Error("offline"));
    await instance.onStop();
    await expect(instance.startAuthorizedDevenv(grant())).rejects.toThrow("DEVENV_CREDENTIAL_CLEANUP_PENDING");
    expect(instance.start).toHaveBeenCalledTimes(1);
    f.wipe.mockResolvedValue(undefined);
    await instance.requestStop();
    await expect(instance.startAuthorizedDevenv(grant())).resolves.toMatchObject({ status: "starting" });
  });

  it("keeps uncertain provider ownership after failed start and retries real destroy via public stop", async () => {
    const f = fixture(); const instance = await f.restart();
    vi.mocked(instance.start).mockRejectedValueOnce(new Error("provider partially created"));
    vi.mocked(instance.destroy).mockRejectedValueOnce(new Error("destroy unavailable"));
    await expect(instance.startAuthorizedDevenv(grant())).rejects.toThrow("DEVENV_AUTHORIZED_START_FAILED");
    expect(f.stored.get(DEVENV_CREDENTIAL_KEY)).toMatchObject({ providerMayExist: true, stashWiped: true, revoked: true });
    expect((await instance.getStatus()).status).toBe("stopping");
    await expect(instance.startAuthorizedDevenv(grant())).rejects.toThrow("DEVENV_START_REQUIRES_TERMINAL_STATE");
    await instance.requestStop();
    expect(instance.destroy).toHaveBeenCalledTimes(2);
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false);
    expect((await instance.getStatus()).status).toBe("stopped");
  });

  it("cleans a grant if scheduling fails, without starting a provider", async () => {
    const f = fixture(); const instance = await f.restart();
    vi.mocked(instance.schedule).mockRejectedValueOnce(new Error("scheduler unavailable"));
    await expect(instance.startAuthorizedDevenv(grant())).rejects.toThrow("DEVENV_AUTHORIZED_START_FAILED");
    expect(instance.start).not.toHaveBeenCalled(); expect(instance.destroy).not.toHaveBeenCalled();
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false); expect(revokeCasPatById).toHaveBeenCalledTimes(1);
  });

  it("enforces the persisted session deadline after restart and ignores stale callbacks", async () => {
    const f = fixture(); const instance = await f.restart(); const first = grant();
    await instance.startAuthorizedDevenv(first);
    const restarted = await f.restart();
    await restarted.expireAuthorizedSession({ sessionUuid: first.grant.sessionUuid });
    expect(restarted.destroy).toHaveBeenCalledTimes(1);
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false);
    const second = grant(); await restarted.startAuthorizedDevenv(second);
    await restarted.expireAuthorizedSession({ sessionUuid: first.grant.sessionUuid });
    expect(restarted.destroy).toHaveBeenCalledTimes(1);
    expect(f.stored.get(DEVENV_CREDENTIAL_KEY).sessionUuid).toBe(second.grant.sessionUuid);
  });

  it("does not let a stopped session reuse its previous grant", async () => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    await instance.startAuthorizedDevenv(payload); await instance.onStop();
    await expect(instance.startAuthorizedDevenv(payload)).rejects.toThrow("DEVENV_SESSION_REPLAY");
    expect(instance.start).toHaveBeenCalledTimes(1);
  });
  it("bounds failed cleanup and leaves durable ownership for a later public retry", async () => {
    const f = fixture(); const instance = await f.restart();
    await instance.startAuthorizedDevenv(grant());
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    vi.mocked(revokeCasPatById).mockImplementationOnce(() => new Promise(() => undefined));
    const stopped = instance.onStop();
    await vi.waitFor(() => expect(revokeCasPatById).toHaveBeenCalledTimes(1));
    await vi.advanceTimersByTimeAsync(5000);
    await stopped;
    expect(f.stored.get(DEVENV_CREDENTIAL_KEY)).toMatchObject({ revoked: false, providerMayExist: false });
    await instance.requestStop();
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false);
  });

  it("never sends a secret to stash or provider when the first cleanup-handle write fails", async () => {
    const f = fixture(); const instance = await f.restart(); const put = f.ctx.storage.put;
    let fail = true;
    f.ctx.storage.put = vi.fn(async (key: string, value: unknown) => {
      if (key === DEVENV_CREDENTIAL_KEY && fail) { fail = false; throw new Error("storage offline"); }
      return put(key, value);
    });
    await expect(instance.startAuthorizedDevenv(grant())).rejects.toThrow("DEVENV_AUTHORIZED_START_FAILED");
    expect(f.stash).not.toHaveBeenCalled(); expect(instance.start).not.toHaveBeenCalled();
    expect(revokeCasPatById).toHaveBeenCalledTimes(1);
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false);
  });

  it.each(["CLW_TOKEN", "CORELINK_TOKEN"])("entrypoint rejects inherited %s before any auth-file bridge", (name) => {
    const secret = "synthetic-raw-pat-not-for-output";
    const result = spawnSync("bash", [fileURLToPath(new URL("../entrypoint.sh", import.meta.url))], {
      encoding: "utf8", env: { PATH: process.env.PATH, [name]: secret,
        CLW_CRED_TICKET: "forged", CLW_LEASE_ID: "forged", CLW_FABRIC_ENDPOINT: "https://spawn.test" },
    });
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("raw CAS credentials are forbidden");
    expect(result.stdout + result.stderr).not.toContain(secret);
  });

  it("arms the deadline before a stalled stash RPC and bounds its response wait", async () => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    f.stash.mockImplementationOnce(() => new Promise(() => undefined));
    const launch = expect(instance.startAuthorizedDevenv(payload)).rejects.toThrow("DEVENV_AUTHORIZED_START_FAILED");
    await vi.waitFor(() => expect(f.stash).toHaveBeenCalledTimes(1));
    expect(instance.schedule).toHaveBeenCalledWith(new Date(payload.grant.expiresAtMs), "expireAuthorizedSession", { sessionUuid: payload.grant.sessionUuid });
    expect(f.stored.get(DEVENV_CREDENTIAL_KEY)).toMatchObject({ sessionUuid: payload.grant.sessionUuid, providerMayExist: false });
    await vi.advanceTimersByTimeAsync(5000);
    await launch;
    expect(instance.start).not.toHaveBeenCalled();
    expect(f.wipe).toHaveBeenCalledWith(`devenv:${payload.grant.sessionUuid}`, payload.grant.expiresAtMs);
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false);
  });

  it("allows the graceful shutdown snapshot to redeem until the confirmed stop callback", async () => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    await instance.startAuthorizedDevenv(payload); await instance.onStart();
    await instance.requestStop();
    expect(f.stashed.has(`devenv:${payload.grant.sessionUuid}`)).toBe(true);
    expect(f.wipe).not.toHaveBeenCalled(); expect(revokeCasPatById).not.toHaveBeenCalled();
    await instance.onStop();
    expect(f.stashed.has(`devenv:${payload.grant.sessionUuid}`)).toBe(false);
    expect(revokeCasPatById).toHaveBeenCalledTimes(1);
  });

  it("does not issue provider start when the grant expires during state persistence", async () => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    const put = f.ctx.storage.put;
    f.ctx.storage.put = vi.fn(async (key: string, value: any) => {
      await put(key, value);
      if (key === "state" && value.status === "starting") vi.mocked(Date.now).mockReturnValue(payload.grant.expiresAtMs);
    });
    await expect(instance.startAuthorizedDevenv(payload)).rejects.toThrow("DEVENV_AUTHORIZED_START_FAILED");
    expect(instance.start).not.toHaveBeenCalled();
    expect(f.stored.has(DEVENV_CREDENTIAL_KEY)).toBe(false);
  });

  it.each(["stop", "error"])("billing-state write failure cannot suppress credential cleanup on %s", async (callback) => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    await instance.startAuthorizedDevenv(payload);
    const put = f.ctx.storage.put;
    f.ctx.storage.put = vi.fn(async (key: string, value: any) => {
      if (key === "state" && value.terminalUsage) throw new Error("billing state unavailable");
      return put(key, value);
    });
    if (callback === "stop") await instance.onStop(); else await instance.onError(new Error("container failed"));
    expect(f.wipe).toHaveBeenCalledWith(`devenv:${payload.grant.sessionUuid}`, payload.grant.expiresAtMs);
    expect(revokeCasPatById).toHaveBeenCalledWith(f.env, payload.grant.patId, payload.grant.tenantId, expect.any(AbortSignal));
    expect(f.stashed.size).toBe(0);
    expect((instance as any).devenvState.terminalUsage.sessionId).toBe(payload.grant.sessionUuid);
    if (callback === "error") expect(f.stored.get(DEVENV_CREDENTIAL_KEY).providerMayExist).toBe(true);
  });

  it("retains failed deadline teardown ownership and emits only a categorical error", async () => {
    const f = fixture(); const instance = await f.restart(); const payload = grant();
    await instance.startAuthorizedDevenv(payload);
    vi.mocked(instance.destroy).mockRejectedValueOnce(new Error("provider detail with secret"));
    const log = vi.spyOn(console, "error").mockImplementation(() => undefined);
    await instance.expireAuthorizedSession({ sessionUuid: payload.grant.sessionUuid });
    expect(f.stored.get(DEVENV_CREDENTIAL_KEY)).toMatchObject({ providerMayExist: true, stashWiped: true, revoked: true });
    expect(JSON.stringify(log.mock.calls)).not.toContain("provider detail with secret");
    expect(log).toHaveBeenCalledWith(JSON.stringify({ event: "devenv_expiry_cleanup_pending" }));
  });

});
