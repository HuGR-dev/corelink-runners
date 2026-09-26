// deploy/cloudflare/test/devenv-do.test.ts
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  validateWorkspaceName,
  validateProfileName,
  validateClwToken,
  validateTenantId,
  DEVENV_TIERS,
  DevenvStatus,
  type StartPayload,
} from "../src/types/devenv";

// ── Test doubles ──────────────────────────────────────────────────────
vi.mock("@cloudflare/containers", () => {
  return {
    Container: class {
      ctx: any;
      env: any;
      defaultPort = 6080;
      sleepAfter = "30m";
      requiredPorts = [6080, 7681, 8080, 9090];
      allowedHosts = ["*"];
      enableInternet = true;
      envVars = {};

      constructor(ctx: any, env: any) {
        this.ctx = ctx;
        this.env = env;
      }
      async start(_opts: any) {}
      async stop() {}
      async destroy() {}
      async schedule(_when: Date, _callback: string, _payload: unknown) {}
      async containerFetch(req: Request, _port: any): Promise<Response> {
        const body = await req.clone().json() as { argv?: string[] };
        const argv = body.argv ?? [];
        if (!argv.includes("snapshot")) {
          return new Response(JSON.stringify({ exit_code: 0, stdout: "", stderr: "" }), { status: 200 });
        }
        const name = argv[argv.indexOf("--name") + 1];
        return new Response(JSON.stringify({
          exit_code: 0,
          stdout: JSON.stringify({
            name,
            root: "a".repeat(64),
            files: 1,
            bytes_total: 1048576,
            chunks_total: 1,
            chunks_uploaded: 1,
            unchanged: false,
            skipped_external_symlinks: [],
          }),
          stderr: "",
        }), { status: 200 });
      }
      renewActivityTimeout() {}
    },
    getContainer: vi.fn(),
  };
});

vi.mock("../src/lib.js", async (importOriginal) => ({
  ...await importOriginal<typeof import("../src/lib.js")>(),
  revokeCasPatById: vi.fn(async () => undefined),
}));

import { RunnerDevEnvDO } from "../src/durable_objects/runner_dev_env";
import { EXEC_SERVER_AUTH_TOKEN_FILE } from "../src/lib/clw";
import { acceptedBillingResponse } from "./helpers/billing-ack";

describe("CoreLink DevEnv — Unit & State Machine Verification", () => {
  let mockStorage: Map<string, any>;
  let mockCtx: any;
  let mockEnv: any;

  beforeEach(() => {
    mockStorage = new Map();
    let gate = Promise.resolve();
    mockCtx = {
      storage: {
        get: vi.fn(async (key: string) => structuredClone(mockStorage.get(key))),
        put: vi.fn(async (key: string, val: any) => mockStorage.set(key, structuredClone(val))),
        delete: vi.fn(async (key: string) => { mockStorage.delete(key); }),
      },
      blockConcurrencyWhile: vi.fn((fn: () => Promise<any>) => {
        const result = gate.then(fn);
        gate = result.then(() => undefined, () => undefined);
        return result;
      }),
      id: { toString: () => "mock-do-tenant-123" },
      acceptWebSocket: vi.fn(),
    };
    mockEnv = {
      CRED_STASH: { idFromName: (name: string) => name, get: () => ({ stash: async (ticket: string) => ticket, wipe: async () => undefined }) },
      SPAWN_WORKER_PUBLIC_URL: "https://spawn.test",
      CORELINK_RUNNER_MINT_AUTH_KEY: "synthetic-mint-key",
      CONFIG_DB: {
        prepare: vi.fn(() => ({
          bind: vi.fn(() => ({
            run: vi.fn(async () => ({ success: true })),
          })),
        })),
      },
    };
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  describe("1. Validators & Invariants", () => {
    it("accepts valid workspace and profile names", () => {
      expect(validateWorkspaceName("my-dev-env-01")).toBe("my-dev-env-01");
      expect(validateProfileName("custom_profile")).toBe("custom_profile");
    });

    it("rejects invalid workspace names with special characters", () => {
      expect(() => validateWorkspaceName("invalid/name")).toThrow();
      expect(() => validateWorkspaceName("")).toThrow();
    });

    it("validates CoreLink PAT tokens", () => {
      expect(validateClwToken("cl_pat_1234567890abcdef1234567890")).toBe("cl_pat_1234567890abcdef1234567890");
      expect(() => validateClwToken("invalid_token")).toThrow();
    });

    it("validates tenant IDs", () => {
      expect(validateTenantId("ee30f7ba-fc25-4d71-939e-ebe130b4c6a3")).toBe("ee30f7ba-fc25-4d71-939e-ebe130b4c6a3");
      expect(() => validateTenantId("short")).toThrow();
    });

    it("provides all scale-to-infinity hardware tiers", () => {
      expect(DEVENV_TIERS["standard-2"]).toEqual({ vcpus: 2, memoryMb: 4096, label: "Standard (2 vCPU, 4 GB)" });
      expect(DEVENV_TIERS["standard-4"]).toEqual({ vcpus: 4, memoryMb: 8192, label: "Standard (4 vCPU, 8 GB)" });
      expect(DEVENV_TIERS["power-8"]).toEqual({ vcpus: 8, memoryMb: 16384, label: "Power (8 vCPU, 16 GB)" });
      expect(DEVENV_TIERS["ultra-16"]).toEqual({ vcpus: 16, memoryMb: 32768, label: "Ultra (16 vCPU, 32 GB)" });
    });
  });

  describe("2. RunnerDevEnvDO Lifecycle & RPC API", () => {
    it("initializes in stopped state with generated exec token", async () => {
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await new Promise((r) => setTimeout(r, 10));
      const status = await doInstance.getStatus();
      expect(status.status).toBe("stopped");
      expect(status.workspaceName).toBeNull();
      expect(status.uptimeMs).toBeNull();
      expect(mockStorage.has("execServerToken")).toBe(true);
    });

    it("starts container with validated payload and transitions to starting", async () => {
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      const payload: StartPayload = {
        config: {
          workspaceName: "frontend-repo",
          profileName: "browser-profile-1",
          tier: "power-8",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      };

      const startResp = await startTest(doInstance, payload);
      expect(startResp.status).toBe("starting");
      expect((doInstance as any).envVars.EXEC_SERVER_AUTH_TOKEN).toBeDefined();
      expect((doInstance as any).envVars.EXEC_SERVER_AUTH_TOKEN_FILE).toBe(
        EXEC_SERVER_AUTH_TOKEN_FILE,
      );
      expect(startResp.workspaceName).toBe("frontend-repo");
      expect(startResp.profileName).toBe("browser-profile-1");
      expect(startResp.tier).toBe("power-8");

      // Verify onStart hook moves to running
      await doInstance.onStart();
      const runningStatus = await doInstance.getStatus();
      expect(runningStatus.status).toBe("running");
      expect(runningStatus.containerHandle).toBe("mock-do-tenant-123");
    });

    it("does not overwrite an active session on a repeated start", async () => {
      const payload: StartPayload = {
        config: {
          workspaceName: "active-session",
          profileName: "default",
          tier: "standard-4",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      };
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(doInstance, payload);
      const firstSession = (doInstance as any).devenvState.sessionUuid;
      await expect(startTest(doInstance, payload)).rejects.toThrow("DEVENV_START_REQUIRES_TERMINAL_STATE");
      expect((doInstance as any).devenvState.sessionUuid).toBe(firstSession);
    });

    it("stops only the matching authorized tenant/session and is idempotent", async () => {
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(instance, testPayload());
      const active = (instance as any).devenvState;

      await expect(instance.stopAuthorizedDevenv({ tenantId: active.tenantId, sessionUuid: crypto.randomUUID() }))
        .resolves.toEqual({ sessionUuid: expect.any(String), status: "not_current" });
      expect((instance as any).devenvState.status).toBe("starting");

      await expect(instance.stopAuthorizedDevenv({ tenantId: active.tenantId, sessionUuid: active.sessionUuid }))
        .resolves.toEqual({ sessionUuid: active.sessionUuid, status: "stopped" });
      await expect(instance.stopAuthorizedDevenv({ tenantId: active.tenantId, sessionUuid: active.sessionUuid }))
        .resolves.toEqual({ sessionUuid: active.sessionUuid, status: "already_stopped" });
    });

    it("fails closed on provider destroy failure and retains cleanup ownership", async () => {
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(instance, testPayload());
      const active = (instance as any).devenvState;
      vi.spyOn(instance as any, "destroy").mockRejectedValue(new Error("provider unavailable"));

      await expect(instance.stopAuthorizedDevenv({ tenantId: active.tenantId, sessionUuid: active.sessionUuid }))
        .rejects.toThrow("DEVENV_PROVIDER_STOP_FAILED");
      expect((instance as any).devenvState.status).toBe("stopping");
      expect(mockStorage.get("devenv:credential-cleanup")).toMatchObject({
        tenantId: active.tenantId, sessionUuid: active.sessionUuid, providerMayExist: true,
      });
    });

    it("cancels a stop-before-start race without affecting other sessions", async () => {
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      const payload = {
        config: { workspaceName: "delayed", profileName: "default", tier: "standard-4" as const },
        grant: { tenantId: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3", sessionUuid: crypto.randomUUID(),
          patId: crypto.randomUUID(), casPat: "cl_pat_delayed", expiresAtMs: Date.now() + 3600000 },
      };
      await expect(instance.stopAuthorizedDevenv({ tenantId: payload.grant.tenantId, sessionUuid: payload.grant.sessionUuid }))
        .resolves.toEqual({ sessionUuid: payload.grant.sessionUuid, status: "not_current" });
      await expect(instance.startAuthorizedDevenv(payload)).rejects.toThrow("DEVENV_AUTHORIZED_START_CANCELED");
    });

    it("rejects malformed stop identities without creating a tombstone", async () => {
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await expect(instance.stopAuthorizedDevenv({ tenantId: "not-a-uuid", sessionUuid: crypto.randomUUID() }))
        .rejects.toThrow("DEVENV_INVALID_STOP_IDENTITY");
      expect([...mockStorage.keys()].some((key) => key.startsWith("devenv:authorized-stop:"))).toBe(false);
    });

    it("accepts non-RFC UUID-shaped identities used by legacy server records", async () => {
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      const tenantId = "ee30f7ba-fc25-0d71-739e-ebe130b4c6a3";
      const sessionUuid = "ee30f7ba-fc25-0d71-739e-ebe130b4c6a3";
      await expect(instance.stopAuthorizedDevenv({ tenantId, sessionUuid }))
        .resolves.toEqual({ sessionUuid, status: "not_current" });
    });

    it("expires tombstones, refuses a live full bound, and protects renewed entries from stale cleanup", async () => {
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      const tenantId = "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3";
      const firstSession = crypto.randomUUID();
      await instance.stopAuthorizedDevenv({ tenantId, sessionUuid: firstSession });
      const first = structuredClone(mockStorage.get(`devenv:authorized-stop:${encodeURIComponent(tenantId)}:${encodeURIComponent(firstSession)}`));
      vi.setSystemTime(new Date(first.canceledAt + 1));
      await instance.stopAuthorizedDevenv({ tenantId, sessionUuid: firstSession });
      const renewed = mockStorage.get(`devenv:authorized-stop:${encodeURIComponent(tenantId)}:${encodeURIComponent(firstSession)}`);
      await instance.expireAuthorizedStop({ tenantId, sessionUuid: firstSession, canceledAt: first.canceledAt });
      expect(mockStorage.has(`devenv:authorized-stop:${encodeURIComponent(tenantId)}:${encodeURIComponent(firstSession)}`)).toBe(true);
      await instance.expireAuthorizedStop({ tenantId, sessionUuid: firstSession, canceledAt: renewed.canceledAt });
      expect(mockStorage.has(`devenv:authorized-stop:${encodeURIComponent(tenantId)}:${encodeURIComponent(firstSession)}`)).toBe(false);

      const entries = Array.from({ length: 64 }, (_, i) => {
        const session = `ee30f7ba-fc25-4d71-739e-${String(i + 1).padStart(12, "0")}`;
        const key = `devenv:authorized-stop:${encodeURIComponent(tenantId)}:${encodeURIComponent(session)}`;
        mockStorage.set(key, { canceledAt: 1 });
        return { key, canceledAt: 1, expiresAt: Date.now() + 3600000 };
      });
      mockStorage.set("devenv:authorized-stop-index", entries);
      await expect(instance.stopAuthorizedDevenv({ tenantId, sessionUuid: crypto.randomUUID() }))
        .rejects.toThrow("DEVENV_AUTHORIZED_STOP_CAPACITY");
      vi.useRealTimers();
    });

    it("fails closed on corrupt tombstone index and mismatched expired values", async () => {
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      const tenantId = crypto.randomUUID();
      const sessionUuid = crypto.randomUUID();
      await instance.stopAuthorizedDevenv({ tenantId, sessionUuid });
      const key = `devenv:authorized-stop:${encodeURIComponent(tenantId)}:${encodeURIComponent(sessionUuid)}`;
      mockStorage.set("devenv:authorized-stop-index", { corrupt: true });
      await expect(instance.expireAuthorizedStop({ tenantId, sessionUuid, canceledAt: mockStorage.get(key).canceledAt }))
        .rejects.toThrow("DEVENV_AUTHORIZED_STOP_INDEX_CORRUPT");
      expect(mockStorage.has(key)).toBe(true);
    });

    it("preflights all expired tombstones before mutating any durable state", async () => {
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      const tenantId = crypto.randomUUID();
      const firstSession = crypto.randomUUID();
      const secondSession = crypto.randomUUID();
      const key = (sessionUuid: string) =>
        `devenv:authorized-stop:${encodeURIComponent(tenantId)}:${encodeURIComponent(sessionUuid)}`;
      const firstKey = key(firstSession);
      const secondKey = key(secondSession);
      const index = [
        { key: firstKey, canceledAt: 11, expiresAt: 0 },
        { key: secondKey, canceledAt: 22, expiresAt: 0 },
      ];
      mockStorage.set(firstKey, { tenantId, sessionUuid: firstSession, canceledAt: 11, expiresAt: 0 });
      mockStorage.set("devenv:authorized-stop-index", index);

      await expect(instance.stopAuthorizedDevenv({ tenantId, sessionUuid: crypto.randomUUID() }))
        .rejects.toThrow("DEVENV_AUTHORIZED_STOP_INDEX_CORRUPT");
      expect(mockStorage.get(firstKey)).toEqual({ tenantId, sessionUuid: firstSession, canceledAt: 11, expiresAt: 0 });
      expect(mockStorage.has(secondKey)).toBe(false);
      expect(mockStorage.get("devenv:authorized-stop-index")).toEqual(index);
    });

    it("rejects an invalid start payload before it can mutate the DevEnv state", async () => {
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await new Promise((resolve) => setTimeout(resolve, 10));

      await expect(startTest(doInstance, {
        config: {
          workspaceName: "invalid/workspace",
          profileName: "default",
          tier: "standard-4",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      })).rejects.toThrow();

      expect(await doInstance.getStatus()).toMatchObject({
        status: "stopped",
        workspaceName: null,
      });
      expect((doInstance as any).envVars).toEqual({});
    });

    it("executes snapshot on running container", async () => {
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(doInstance, {
        config: {
          workspaceName: "my-workspace",
          profileName: "my-profile",
          tier: "standard-4",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      });
      await doInstance.onStart();

      const owner = (doInstance as any).devenvState;
      const execSpy = vi.spyOn(doInstance as any, "containerFetch");
      const snapResp = await doInstance.snapshot({ force: false });
      expect(snapResp.ok).toBe(true);
      expect(snapResp.workspaceSnapshot.root).toBe("a".repeat(64));
      expect(snapResp.workspaceSnapshot.bytesTotal).toBe(1048576);
      const requests = await Promise.all(execSpy.mock.calls.map(async ([request]) =>
        await (request as Request).clone().json() as Record<string, unknown>));
      expect(requests).toHaveLength(2);
      for (const request of requests) {
        expect(request.expected_session_uuid).toBe(owner.sessionUuid);
        expect(request.expected_generation_id).toBe(owner.generationId);
      }
      expect((doInstance as any).envVars.DEVENV_GENERATION_ID).toBe(String(owner.generationId));
    });

    it.each([
      ["the same names", "recovery", "default"],
      ["different names", "replacement-workspace", "replacement-profile"],
    ])("refuses a pending snapshot after stop and restart with %s", async (_label, nextWorkspace, nextProfile) => {
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      const initialPayload = testPayload();
      await startTest(doInstance, initialPayload);
      await doInstance.onStart();
      const initial = (doInstance as any).devenvState;
      let reachedFirstExec!: () => void;
      const firstExecReached = new Promise<void>((resolve) => { reachedFirstExec = resolve; });
      let releaseFirstExec!: (response: Response) => void;
      const firstExecResponse = new Promise<Response>((resolve) => { releaseFirstExec = resolve; });
      const executed: Array<{ sessionUuid: string; generationId: number; name: string }> = [];

      vi.spyOn(doInstance as any, "containerFetch").mockImplementation(async (request: Request) => {
        const body = await request.clone().json() as {
          argv: string[]; expected_session_uuid?: string; expected_generation_id?: number;
        };
        const current = (doInstance as any).devenvState;
        if (body.expected_session_uuid !== current.sessionUuid ||
            body.expected_generation_id !== current.generationId) {
          return new Response(JSON.stringify({ error: "devenv_session_identity_mismatch" }), { status: 409 });
        }
        const name = body.argv[body.argv.indexOf("--name") + 1];
        executed.push({ sessionUuid: current.sessionUuid, generationId: current.generationId, name });
        if (executed.length === 1) {
          reachedFirstExec();
          return firstExecResponse;
        }
        return snapshotExecResponse(name);
      });

      const pendingSnapshot = doInstance.snapshot({ force: false });
      await firstExecReached;
      await doInstance.stopAuthorizedDevenv({ tenantId: initial.tenantId, sessionUuid: initial.sessionUuid });
      await startTest(doInstance, {
        ...initialPayload,
        config: { ...initialPayload.config, workspaceName: nextWorkspace, profileName: nextProfile },
      });
      await doInstance.onStart();
      const replacement = (doInstance as any).devenvState;
      expect(replacement.sessionUuid).not.toBe(initial.sessionUuid);
      expect(replacement.generationId).toBeGreaterThan(initial.generationId);

      releaseFirstExec(snapshotExecResponse(initial.profileName));
      await expect(pendingSnapshot).rejects.toThrow("DEVENV_SNAPSHOT_SESSION_CHANGED");
      expect(executed).toEqual([{
        sessionUuid: initial.sessionUuid,
        generationId: initial.generationId,
        name: initial.profileName,
      }]);
      expect((doInstance as any).devenvState.sessionUuid).toBe(replacement.sessionUuid);
    });

    it("routes HTTP snapshots through the same session-bound exec contract", async () => {
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(doInstance, testPayload());
      await doInstance.onStart();
      const owner = (doInstance as any).devenvState;
      const execSpy = vi.spyOn(doInstance as any, "containerFetch");
      const response = await doInstance.fetch(new Request("https://runner.test/v1/customer/devenv/snapshot", {
        method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ force: false }),
      }));
      expect(response.status).toBe(200);
      expect((await response.json() as { ok: boolean }).ok).toBe(true);
      const requests = await Promise.all(execSpy.mock.calls.map(async ([request]) =>
        await (request as Request).clone().json() as Record<string, unknown>));
      expect(requests).toHaveLength(2);
      expect(requests.every((request) => request.expected_session_uuid === owner.sessionUuid &&
        request.expected_generation_id === owner.generationId)).toBe(true);
    });

    it("rejects malformed snapshot roots without returning an ok response", async () => {
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(doInstance, {
        config: {
          workspaceName: "my-workspace",
          profileName: "my-profile",
          tier: "standard-4",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: `cl_${"a".repeat(24)}`,
        },
      });
      await doInstance.onStart();
      const execSpy = vi.spyOn(doInstance as any, "containerFetch").mockImplementation(async (req: Request) => {
        const { argv } = await req.json() as { argv: string[] };
        const name = argv[argv.indexOf("--name") + 1];
        const report = {
          name,
          root: argv[1] === "/data/workspace" ? "/tmp/invalid-root" : "a".repeat(64),
          files: 0,
          bytes_total: 0,
          chunks_total: 0,
          chunks_uploaded: 0,
          unchanged: false,
          skipped_external_symlinks: [],
        };
        return new Response(JSON.stringify({ exit_code: 0, stdout: JSON.stringify(report), stderr: "" }), { status: 200 });
      });

      await expect(doInstance.snapshot({ force: false })).rejects.toThrow("CLW_SNAPSHOT_REPORT_INVALID_FIELDS");
      expect(execSpy).toHaveBeenCalledTimes(2);
    });

    it("stops gracefully and sends canonical HTTP billing without a duplicate D1 tally", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_INGEST_AUTH_KEY = "test-key";
      mockEnv.BILLING_REGION = "iad";
      const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => acceptedBillingResponse(init));
      vi.stubGlobal("fetch", fetchMock);
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(doInstance, {
        config: {
          workspaceName: "billing-test",
          profileName: "default",
          tier: "ultra-16", // 16x multiplier
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      });
      await doInstance.onStart();

      const stopResp = await doInstance.requestStop();
      expect(stopResp.ok).toBe(true);

      await doInstance.onStop();
      const finalStatus = await doInstance.getStatus();
      expect(finalStatus.status).toBe("stopped");

      expect(mockEnv.CONFIG_DB.prepare).not.toHaveBeenCalled();
      expect(fetchMock).toHaveBeenCalledTimes(1);
      const body = JSON.parse(fetchMock.mock.calls[0][1].body as string) as Array<Record<string, unknown>>;
      expect(body[0]).toMatchObject({ event_kind: "runner_vcpu_seconds", region: "iad" });
      expect(body[0].qty).toBeLessThan(480);
    });

    it("does not settle or delete pending usage from a status-only 202", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_INGEST_AUTH_KEY = "test-key";
      mockEnv.BILLING_REGION = "iad";
      vi.stubGlobal("fetch", vi.fn(async () => new Response("{}", { status: 202 })));
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(doInstance, {
        config: {
          workspaceName: "status-only-ack",
          profileName: "default",
          tier: "standard-4",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      });
      await doInstance.onStart();
      await doInstance.requestStop();
      await doInstance.onStop();
      expect(mockStorage.has("devenv:usage:settled")).toBe(false);
      expect(mockStorage.has("devenv:usage:pending")).toBe(true);
    });

    it("keeps a failed delivery frozen and blocks a new session until retry succeeds", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_INGEST_AUTH_KEY = "test-key";
      mockEnv.BILLING_REGION = "iad";
      const fetchMock = vi.fn()
        .mockRejectedValueOnce(new Error("billing unavailable"))
        .mockRejectedValueOnce(new Error("billing still unavailable"))
        .mockImplementation(async (_url: string, init?: RequestInit) => acceptedBillingResponse(init));
      vi.stubGlobal("fetch", fetchMock);
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      const payload: StartPayload = {
        config: {
          workspaceName: "pending-billing",
          profileName: "default",
          tier: "standard-4",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      };
      await startTest(doInstance, payload);
      await doInstance.onStart();
      await doInstance.requestStop();
      await doInstance.onStop();

      const frozen = mockStorage.get("devenv:usage:pending");
      expect(frozen?.event?.idem_key).toMatch(/^[0-9a-f]{64}$/);
      await expect(startTest(doInstance, payload)).rejects.toThrow("DEVENV_BILLING_PENDING");
      expect(mockStorage.get("state").terminalUsage.sessionId).toBe(frozen.sessionUuid);
      await expect(startTest(doInstance, payload)).resolves.toMatchObject({ status: "starting" });
      expect(fetchMock).toHaveBeenCalledTimes(3);
      const retryBody = JSON.parse(fetchMock.mock.calls[2][1].body as string) as Array<Record<string, unknown>>;
      expect(retryBody[0]).toEqual(frozen.event);
      expect(mockStorage.has("devenv:usage:pending")).toBe(false);
    });

    it("keeps pending delivery after a settled-marker/delete interleaving", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_INGEST_AUTH_KEY = "test-key";
      mockEnv.BILLING_REGION = "iad";
      const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => acceptedBillingResponse(init));
      vi.stubGlobal("fetch", fetchMock);
      let failDelete = true;
      const storageDelete = mockCtx.storage.delete;
      mockCtx.storage.delete = vi.fn(async (key: string) => {
        if (key === "devenv:usage:pending" && failDelete) {
          failDelete = false;
          throw new Error("delete interrupted");
        }
        return storageDelete(key);
      });
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(doInstance, {
        config: {
          workspaceName: "delete-interleave",
          profileName: "default",
          tier: "standard-4",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      });
      await doInstance.onStart();
      await doInstance.requestStop();
      await doInstance.onStop();
      expect(mockStorage.has("devenv:usage:settled")).toBe(true);
      expect(mockStorage.has("devenv:usage:pending")).toBe(true);
      const restarted = new RunnerDevEnvDO(mockCtx, mockEnv);
      await restarted.requestStop();
      expect(mockStorage.has("devenv:usage:pending")).toBe(false);
      expect(fetchMock).toHaveBeenCalledTimes(1);
    });

    it("retries a failed pending write through public stop after restart without billing the next month", async () => {
      let now = Date.parse("2026-09-30T23:59:40Z");
      vi.spyOn(Date, "now").mockImplementation(() => now);
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_INGEST_AUTH_KEY = "test-key";
      mockEnv.BILLING_REGION = "iad";
      const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => acceptedBillingResponse(init));
      vi.stubGlobal("fetch", fetchMock);
      let failPut = true;
      const storagePut = mockCtx.storage.put;
      mockCtx.storage.put = vi.fn(async (key: string, value: unknown) => {
        if (key === "devenv:usage:pending" && failPut) {
          failPut = false;
          throw new Error("snapshot interrupted");
        }
        return storagePut(key, value);
      });
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(doInstance, {
        config: {
          workspaceName: "snapshot-failure",
          profileName: "default",
          tier: "standard-4",
          clwEndpoint: "https://corelink-api.humangr.com",
          clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
          clwToken: "cl_pat_1234567890abcdef1234567890",
        },
      });
      await doInstance.onStart();
      await doInstance.requestStop();
      now = Date.parse("2026-09-30T23:59:50Z");
      await doInstance.onStop();
      expect((await doInstance.getStatus()).status).toBe("stopped");
      expect(mockStorage.has("devenv:usage:pending")).toBe(false);
      expect(fetchMock).not.toHaveBeenCalled();
      now = Date.parse("2026-10-01T03:00:00Z");
      const restarted = new RunnerDevEnvDO(mockCtx, mockEnv);
      await restarted.requestStop();
      expect((await restarted.getStatus()).status).toBe("stopped");
      expect(fetchMock).toHaveBeenCalledTimes(1);
      const [event] = JSON.parse(fetchMock.mock.calls[0][1].body);
      expect(event).toMatchObject({ qty: 40, billing_period: "2026-09", time_ms: Date.parse("2026-09-30T23:59:50Z") });
    });

    it("retains callback time in memory after the first terminal-state write fails", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_REGION = "iad";
      const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => acceptedBillingResponse(init));
      vi.stubGlobal("fetch", fetchMock);
      let now = 1000;
      vi.spyOn(Date, "now").mockImplementation(() => now);
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(instance, testPayload());
      await instance.onStart();
      const put = mockCtx.storage.put;
      let fail = true;
      mockCtx.storage.put = vi.fn(async (key: string, value: any) => {
        if (key === "state" && value.terminalUsage && fail) {
          fail = false;
          throw new Error("storage unavailable");
        }
        return put(key, value);
      });
      now = 11000;
      await instance.onStop();
      expect(fetchMock).not.toHaveBeenCalled();
      now = 900000;
      await instance.requestStop();
      expect(fetchMock).toHaveBeenCalledTimes(1);
      expect(JSON.parse(fetchMock.mock.calls[0][1].body)[0]).toMatchObject({ qty: 40, time_ms: 11000 });
    });

    it("public stop retries delivery while concurrent terminal callbacks share one request", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_REGION = "iad";
      let resolveDelivery!: (response: Response) => void;
      let pendingInit: RequestInit | undefined;
      const fetchMock = vi.fn()
        .mockRejectedValueOnce(new Error("offline"))
        .mockImplementationOnce((_url: string, init?: RequestInit) => new Promise<Response>((resolve) => {
          pendingInit = init;
          resolveDelivery = resolve;
        }));
      vi.stubGlobal("fetch", fetchMock);
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(instance, testPayload());
      await instance.onStart();
      await instance.onError(new Error("container stopped"));
      expect((await instance.getStatus()).status).toBe("errored");
      const retry = instance.requestStop();
      await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
      const lateCallback = instance.onStop();
      resolveDelivery(acceptedBillingResponse(pendingInit));
      await Promise.all([retry, lateCallback]);
      expect(fetchMock).toHaveBeenCalledTimes(2);
      await expect(startTest(instance, testPayload())).resolves.toMatchObject({ status: "starting" });
    });

    it("default-disabled billing emits nothing after a later configuration change", async () => {
      const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => acceptedBillingResponse(init));
      vi.stubGlobal("fetch", fetchMock);
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(instance, testPayload());
      await instance.onStop();
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_REGION = "iad";
      const restarted = new RunnerDevEnvDO(mockCtx, mockEnv);
      await restarted.requestStop();
      expect(fetchMock).not.toHaveBeenCalled();
      expect(mockEnv.CONFIG_DB.prepare).not.toHaveBeenCalled();
    });

    it("disabling billing preserves pending delivery and blocks replacement", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_REGION = "iad";
      vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(instance, testPayload());
      await instance.onStop();
      const pending = mockStorage.get("devenv:usage:pending");
      delete mockEnv.BILLING_INGEST_URL;
      await expect(instance.requestStop()).resolves.toEqual({ ok: true });
      await expect(startTest(instance, testPayload())).rejects.toThrow("DEVENV_BILLING_PENDING");
      expect(mockStorage.get("devenv:usage:pending")).toEqual(pending);
    });

    it("retains an unresolved legacy errored session without inventing its stop time", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_REGION = "iad";
      const fetchMock = vi.fn();
      vi.stubGlobal("fetch", fetchMock);
      const instance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await startTest(instance, testPayload());
      const active = mockStorage.get("state");
      mockStorage.set("state", { ...active, status: "errored", lastError: "old runtime", lastWorkspaceName: "recovery" });
      const restarted = new RunnerDevEnvDO(mockCtx, mockEnv);
      await restarted.requestStop();
      await restarted.onStop();
      await expect(startTest(restarted, testPayload())).rejects.toThrow("DEVENV_BILLING_PENDING");
      expect(mockStorage.get("state").sessionUuid).toBe(active.sessionUuid);
      expect(fetchMock).not.toHaveBeenCalled();
      mockStorage.set("devenv:usage:settled", active.sessionUuid);
      await expect(startTest(restarted, testPayload())).resolves.toMatchObject({ status: "starting" });
    });
  });
});

function snapshotExecResponse(name: string): Response {
  return new Response(JSON.stringify({
    exit_code: 0,
    stdout: JSON.stringify({
      name,
      root: "a".repeat(64),
      files: 1,
      bytes_total: 1048576,
      chunks_total: 1,
      chunks_uploaded: 1,
      unchanged: false,
      skipped_external_symlinks: [],
    }),
    stderr: "",
  }), { status: 200 });
}

function testPayload(): StartPayload {
  return { config: {
    workspaceName: "recovery", profileName: "default", tier: "standard-4",
    clwEndpoint: "https://corelink-api.humangr.com",
    clwTenant: "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3",
    clwToken: "cl_pat_1234567890abcdef1234567890",
  } };
}

async function startTest(instance: RunnerDevEnvDO, payload: StartPayload) {
  await instance.startAuthorizedDevenv({
    config: { workspaceName: payload.config.workspaceName, profileName: payload.config.profileName, tier: payload.config.tier },
    grant: { tenantId: payload.config.clwTenant, sessionUuid: crypto.randomUUID(), patId: crypto.randomUUID(),
      casPat: payload.config.clwToken, expiresAtMs: Date.now() + 3600000 },
  });
  return instance.getStatus();
}
