// deploy/cloudflare/test/devenv-do.test.ts
import { describe, it, expect, vi, beforeEach } from "vitest";
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
      async containerFetch(_req: any, _port: any): Promise<Response> {
        return new Response(JSON.stringify({ exit_code: 0, stdout: JSON.stringify({ root: "bafybeicorp", bytes_total: 1048576 }), stderr: "" }), { status: 200 });
      }
      renewActivityTimeout() {}
    },
    getContainer: vi.fn(),
  };
});

import { RunnerDevEnvDO } from "../src/durable_objects/runner_dev_env";
import { EXEC_SERVER_AUTH_TOKEN_FILE } from "../src/lib/clw";

describe("CoreLink DevEnv — Unit & State Machine Verification", () => {
  let mockStorage: Map<string, any>;
  let mockCtx: any;
  let mockEnv: any;

  beforeEach(() => {
    mockStorage = new Map();
    mockCtx = {
      storage: {
        get: vi.fn(async (key: string) => mockStorage.get(key)),
        put: vi.fn(async (key: string, val: any) => mockStorage.set(key, val)),
        delete: vi.fn(async (key: string) => { mockStorage.delete(key); }),
      },
      blockConcurrencyWhile: vi.fn(async (fn: () => Promise<any>) => fn()),
      id: { toString: () => "mock-do-tenant-123" },
      acceptWebSocket: vi.fn(),
    };
    mockEnv = {
      CONFIG_DB: {
        prepare: vi.fn(() => ({
          bind: vi.fn(() => ({
            run: vi.fn(async () => ({ success: true })),
          })),
        })),
      },
    };
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

      const startResp = await doInstance.startDevenv(payload);
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

    it("rejects an invalid start payload before it can mutate the DevEnv state", async () => {
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await new Promise((resolve) => setTimeout(resolve, 10));

      await expect(doInstance.startDevenv({
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
      await doInstance.startDevenv({
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

      const snapResp = await doInstance.snapshot({ force: false });
      expect(snapResp.ok).toBe(true);
      expect(snapResp.workspaceSnapshot.root).toBe("bafybeicorp");
      expect(snapResp.workspaceSnapshot.bytesTotal).toBe(1048576);
    });

    it("stops gracefully and sends canonical HTTP billing without a duplicate D1 tally", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_INGEST_AUTH_KEY = "test-key";
      mockEnv.BILLING_REGION = "iad";
      const fetchMock = vi.fn(async () => new Response("{}", { status: 202 }));
      vi.stubGlobal("fetch", fetchMock);
      const doInstance = new RunnerDevEnvDO(mockCtx, mockEnv);
      await doInstance.startDevenv({
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

    it("keeps a failed delivery frozen and blocks a new session until retry succeeds", async () => {
      mockEnv.BILLING_INGEST_URL = "https://billing.test/usage";
      mockEnv.BILLING_INGEST_AUTH_KEY = "test-key";
      mockEnv.BILLING_REGION = "iad";
      const fetchMock = vi.fn()
        .mockRejectedValueOnce(new Error("billing unavailable"))
        .mockResolvedValue(new Response("{}", { status: 202 }));
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
      await doInstance.startDevenv(payload);
      await doInstance.onStart();
      await doInstance.requestStop();
      await doInstance.onStop();

      const frozen = mockStorage.get("devenv:usage:pending");
      expect(frozen?.event?.idem_key).toMatch(/^[0-9a-f]{64}$/);
      await expect(doInstance.startDevenv(payload)).resolves.toMatchObject({ status: "starting" });
      expect(fetchMock).toHaveBeenCalledTimes(2);
      const retryBody = JSON.parse(fetchMock.mock.calls[1][1].body as string) as Array<Record<string, unknown>>;
      expect(retryBody[0]).toEqual(frozen.event);
      expect(mockStorage.has("devenv:usage:pending")).toBe(false);
    });
  });
});
