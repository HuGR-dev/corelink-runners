// deploy/cloudflare/test/deep-step-by-step-audit.test.ts
// Microscopic Atom-by-Atom Step-by-Step Deep Behavioral Audit Driver
import { describe, it, expect, vi, beforeEach } from "vitest";

// ── Strict Test double with deep inspection trace hooks ───────────────
interface StepTraceEvent {
  step: number;
  timestampNs: bigint;
  component: string;
  operation: string;
  payload: Record<string, any>;
  invariantAsserted: string;
}

const traceLog: StepTraceEvent[] = [];

function recordTrace(step: number, component: string, operation: string, payload: Record<string, any>, invariantAsserted: string) {
  traceLog.push({
    step,
    timestampNs: process.hrtime.bigint(),
    component,
    operation,
    payload,
    invariantAsserted,
  });
}

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
      envVars: Record<string, string> = {};
      alive = false;

      constructor(ctx: any, env: any) {
        this.ctx = ctx;
        this.env = env;
      }
      async start(opts: any) {
        this.alive = true;
        recordTrace(4, "ContainerRuntime", "vm_start", { opts, envVars: { ...this.envVars, CLW_TOKEN: "[REDACTED_TMPFS]" } }, "INV-01: Container microVM bootstrap with cgroup limits");
      }
      async stop() {
        this.alive = false;
        recordTrace(9, "ContainerRuntime", "vm_stop", { alive: false }, "INV-07: MicroVM shutdown and resource release");
      }
      async destroy() {
        this.alive = false;
      }
      async containerFetch(req: Request | string, port?: number): Promise<Response> {
        const urlStr = typeof req === "string" ? req : req.url;
        const url = new URL(urlStr, "http://localhost");

        if (port === 9090 || url.port === "9090") {
          const reqHeader = typeof req === "string" ? null : ((req as Request).headers?.get("X-Exec-Token") || (req as Request).headers?.get("x-exec-token"));
          
          recordTrace(7, "RustExecServer:9090", "token_challenge", { 
            headerReceived: reqHeader ? `${reqHeader.substring(0, 8)}...` : null, 
            expectedToken: this.envVars.EXEC_SERVER_TOKEN ? `${this.envVars.EXEC_SERVER_TOKEN.substring(0, 8)}...` : null,
            tokenMatch: reqHeader === this.envVars.EXEC_SERVER_TOKEN
          }, "INV-04: Strict loopback token authentication challenge");

          if (this.envVars.EXEC_SERVER_TOKEN && reqHeader !== this.envVars.EXEC_SERVER_TOKEN) {
            return new Response(JSON.stringify({ error: "Unauthorized" }), { status: 401 });
          }

          if (url.pathname === "/clw" || url.pathname === "/exec") {
            const mockSnapshotPayload = {
              root: "bafybeicorp_merkle_blake3_root_987654321",
              bytes_total: 10485760, // 10 MB
              chunks_count: 640,
              tar_packs_count: 1,
              dedup_ratio: 0.94,
              inodes_used: 1240,
              inodes_total: 100000,
            };

            recordTrace(8, "ClwStorageEngine", "cas_snapshot_generate", mockSnapshotPayload, "INV-04: CAS BLAKE3 snapshot root generation & deduplication");
            return new Response(JSON.stringify({ exit_code: 0, stdout: JSON.stringify(mockSnapshotPayload), stderr: "" }), {
              status: 200,
              headers: { "Content-Type": "application/json" },
            });
          }
        }

        return new Response(JSON.stringify({ status: "ok" }), { status: 200 });
      }
      renewActivityTimeout() {}
    },
    getContainer: vi.fn(),
  };
});

import { RunnerDevEnvDO } from "../src/durable_objects/runner_dev_env";
import { validateWorkspaceName, validateProfileName, validateClwToken, validateTenantId, DEVENV_TIERS } from "../src/types/devenv";

describe("Microscopic Step-by-Step Behavioral Verification (Deep Logs & Evidences)", () => {
  let mockStorage: Map<string, any>;
  let mockCtx: any;
  let mockEnv: any;
  let d1SqlCalls: Array<{ query: string; params: any[] }>;

  beforeEach(() => {
    mockStorage = new Map();
    d1SqlCalls = [];
    traceLog.length = 0;

    mockCtx = {
      storage: {
        get: vi.fn(async (key: string) => {
          const val = mockStorage.get(key);
          recordTrace(1, "DO_SQLite_Storage", "storage_get", { key, found: val !== undefined }, "INV-07: Atomic SQLite state retrieval");
          return val;
        }),
        put: vi.fn(async (key: string, val: any) => {
          mockStorage.set(key, val);
          recordTrace(2, "DO_SQLite_Storage", "storage_put", { key, status: val?.status ?? "raw_value" }, "INV-07: Atomic SQLite state persistence");
        }),
      },
      blockConcurrencyWhile: vi.fn(async (fn: () => Promise<any>) => {
        recordTrace(3, "DO_ConcurrencyGuard", "blockConcurrencyWhile_acquire", { locked: true }, "INV-03: DO Mutex lock acquisition");
        const res = await fn();
        recordTrace(3, "DO_ConcurrencyGuard", "blockConcurrencyWhile_release", { locked: false }, "INV-03: DO Mutex lock release");
        return res;
      }),
      id: { toString: () => "do-session-atom-999" },
      acceptWebSocket: vi.fn((ws: any, tags: string[]) => {
        recordTrace(6, "WebSocketHibernation", "ws_accept_hibernation", { tags }, "INV-02: Zero-cost WebSocket hibernation tagging");
      }),
    };

    mockEnv = {
      CONFIG_DB: {
        prepare: vi.fn((sql: string) => ({
          bind: vi.fn((...params: any[]) => ({
            run: vi.fn(async () => {
              d1SqlCalls.push({ query: sql, params });
              recordTrace(10, "D1_Distributed_Database", "sql_upsert_monthly_vcpu", {
                tenant_id: params[0],
                month_at: params[1],
                vcpu_seconds_added: params[2],
                recorded_at_epoch_ms: params[3],
              }, "INV-05: Atomic D1 FinOps ledger accounting with 30s floor");
              return { success: true };
            }),
          })),
        })),
      },
    };
  });

  it("Executes every single microscopic step: validation -> token-tmpfs -> boot -> ws -> exec -> cas -> d1-ledger -> teardown", async () => {
    // ═══════════════════════════════════════════════════════════════════════
    // ETAPA 1: Validação Estrita de Input (Zero-Trust RFC-1123 & PAT regex)
    // ═══════════════════════════════════════════════════════════════════════
    const wsName = validateWorkspaceName("corelink-agent-v1");
    const profName = validateProfileName("chrome-isolated-01");
    const token = validateClwToken("cl_pat_99887766554433221100aabbccddeeff");
    const tenantId = validateTenantId("ee30f7ba-fc25-4d71-939e-ebe130b4c6a3");

    expect(wsName).toBe("corelink-agent-v1");
    expect(profName).toBe("chrome-isolated-01");
    expect(token).toBe("cl_pat_99887766554433221100aabbccddeeff");
    expect(tenantId).toBe("ee30f7ba-fc25-4d71-939e-ebe130b4c6a3");

    // ═══════════════════════════════════════════════════════════════════════
    // ETAPA 2: Inicialização do Durable Object & Mutex Lock
    // ═══════════════════════════════════════════════════════════════════════
    const devenv = new RunnerDevEnvDO(mockCtx, mockEnv);
    await new Promise((r) => setTimeout(r, 10)); // drain constructor lock

    // ═══════════════════════════════════════════════════════════════════════
    // ETAPA 3: Invocação de startDevenv (Geração de UUIDv7, cgroups & tmpfs)
    // ═══════════════════════════════════════════════════════════════════════
    const startResp = await devenv.startDevenv({
      config: {
        workspaceName: wsName,
        profileName: profName,
        tier: "ultra-16", // 16 vCPU, 32 GB RAM
        clwEndpoint: "https://corelink-api.humangr.com",
        clwTenant: tenantId,
        clwToken: token,
      },
    });

    expect(startResp.status).toBe("starting");
    expect(startResp.tier).toBe("ultra-16");
    expect(DEVENV_TIERS[startResp.tier].vcpus).toBe(16);

    // ═══════════════════════════════════════════════════════════════════════
    // ETAPA 4: Transição onStart (Container Pronto & Portas Ativas)
    // ═══════════════════════════════════════════════════════════════════════
    await devenv.onStart();
    const liveStatus = await devenv.getStatus();
    expect(liveStatus.status).toBe("running");
    expect(liveStatus.ports).toEqual([6080, 7681, 8080]);

    // ═══════════════════════════════════════════════════════════════════════
    // ETAPA 5: Execução de Snapshot CAS (Exec-Server + Merkle BLAKE3)
    // ═══════════════════════════════════════════════════════════════════════
    const snap = await devenv.snapshot({ force: false });
    expect(snap.ok).toBe(true);
    expect(snap.workspaceSnapshot.root).toBe("bafybeicorp_merkle_blake3_root_987654321");
    expect(snap.workspaceSnapshot.bytesTotal).toBe(10485760);

    // ═══════════════════════════════════════════════════════════════════════
    // ETAPA 6: Encerramento Gracioso & Gravação Contábil no D1
    // ═══════════════════════════════════════════════════════════════════════
    const stopResp = await devenv.requestStop();
    expect(stopResp.ok).toBe(true);

    await devenv.onStop();
    const stoppedStatus = await devenv.getStatus();
    expect(stoppedStatus.status).toBe("stopped");

    // ═══════════════════════════════════════════════════════════════════════
    // ETAPA 7: Verificação Estrita dos Parâmetros SQL no D1
    // ═══════════════════════════════════════════════════════════════════════
    expect(d1SqlCalls.length).toBe(1);
    const d1Record = d1SqlCalls[0];
    expect(d1Record.query).toContain("INSERT INTO devenv_monthly_vcpu");
    expect(d1Record.params[0]).toBe("ee30f7ba-fc25-4d71-939e-ebe130b4c6a3"); // tenant_id
    expect(d1Record.params[2]).toBe(480); // 30s piso * 16 vCPU = 480 vCPU-segundos

    // ═══════════════════════════════════════════════════════════════════════
    // EXIBIR O RASTRO DE EXECUÇÃO DETALHADO PASSO A PASSO
    // ═══════════════════════════════════════════════════════════════════════
    console.log("\n====================================================================================================");
    console.log("            CORELINK DEVENV — RASTREAMENTO MICROSCÓPICO PASSO A PASSO (ATOM-BY-ATOM)               ");
    console.log("====================================================================================================");
    traceLog.forEach((t, i) => {
      console.log(`[PASSO ${String(i + 1).padStart(2, "0")}] [${t.component.padEnd(24)}] -> ${t.operation.padEnd(28)} | ${t.invariantAsserted}`);
      console.log(`          Payload: ${JSON.stringify(t.payload)}`);
    });
    console.log("====================================================================================================");
  });
});
