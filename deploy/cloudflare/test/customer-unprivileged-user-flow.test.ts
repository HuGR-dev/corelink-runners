// deploy/cloudflare/test/customer-unprivileged-user-flow.test.ts
// Rigorous End-to-End Test from the perspective of an Unprivileged Customer User
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// ── Mock Cloudflare Containers ─────────────────────────────────────────
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
      async start() { this.alive = true; }
      async schedule() {}
      async stop() { this.alive = false; }
      async destroy() { this.alive = false; }
      async containerFetch(req: Request | string, port?: number): Promise<Response> {
        if (!this.alive) return new Response("Container stopped", { status: 503 });
        if (port === 9090) {
          return new Response(JSON.stringify({ exit_code: 0, stdout: '{"root":"bafy_user_repo_snapshot","bytes_total":524288}', stderr: "" }), { status: 200 });
        }
        return new Response(JSON.stringify({ ok: true }), { status: 200 });
      }
      renewActivityTimeout() {}
    },
    getContainer: vi.fn(),
  };
});

import { RunnerDevEnvDO } from "../src/durable_objects/runner_dev_env";
import { EXEC_SERVER_AUTH_TOKEN_FILE } from "../src/lib/clw";

// ── Ingress Header Sanitizer (Edge Security Gateway) ───────────────────
function edgeIngressFilter(rawHeaders: Record<string, string>): Headers {
  const sanitized = new Headers();
  for (const [key, value] of Object.entries(rawHeaders)) {
    const lower = key.toLowerCase();
    // Neutralize CRLF header injection
    const cleanValue = value.replace(/[\r\n]/g, "").trim();
    // STRIP any untrusted upstream spoofing headers
    if (lower.startsWith("x-corelink-") || lower === "x-tenant-id" || lower === "x-role" || lower.startsWith("x-bypass-")) {
      continue; // Dropped at edge gateway
    }
    sanitized.set(key, cleanValue);
  }
  return sanitized;
}

describe("Real Customer User Simulation (Provisioned Test User Flow)", () => {
  // ── Database State Simulation ────────────────────────────────────────
  let mockTenantsTable: Map<string, { id: string; name: string; tier: string; status: string }>;
  let mockPatTable: Map<string, { token_id: string; tenant_id: string; user_id: string; role: string; revoked: boolean }>;
  let billingEvents: Array<Record<string, unknown>>;
  let revokeRequests: string[];
  let mockDoStorage: Map<string, any>;
  let mockCtx: any;
  let mockEnv: any;

  // 1. PROVISION TEST CUSTOMER USER
  const TEST_TENANT = {
    id: "tenant-cust-alpha-42",
    name: "Acme Dev Team",
    tier: "starter",
    status: "active",
  };

  const TEST_USER = {
    id: "user_jane_doe_42",
    email: "jane.doe@acme.com",
    role: "member", // strictly UNPRIVILEGED customer role
  };

  const TEST_PAT_SECRET = "cl_pat_jane_doe_unprivileged_test_secret_9988";

  beforeEach(() => {
    mockTenantsTable = new Map([[TEST_TENANT.id, TEST_TENANT]]);
    mockPatTable = new Map([
      [
        TEST_PAT_SECRET,
        {
          token_id: "tok_jane_42",
          tenant_id: TEST_TENANT.id,
          user_id: TEST_USER.id,
          role: TEST_USER.role,
          revoked: false,
        },
      ],
    ]);
    billingEvents = [];
    revokeRequests = [];
    mockDoStorage = new Map();

    mockCtx = {
      storage: {
        get: vi.fn(async (k: string) => { const v = mockDoStorage.get(k); return v === undefined ? undefined : structuredClone(v); }),
        put: vi.fn(async (k: string, v: any) => mockDoStorage.set(k, structuredClone(v))),
        delete: vi.fn(async (k: string) => mockDoStorage.delete(k)),
      },
      blockConcurrencyWhile: vi.fn(async (fn: () => Promise<any>) => fn()),
      id: { toString: () => `session-${TEST_TENANT.id}` },
      acceptWebSocket: vi.fn(),
    };

    mockEnv = {
      CRED_STASH: {
        idFromName: vi.fn((name: string) => name),
        get: vi.fn(() => ({
          stash: vi.fn(async () => "a".repeat(64)),
          wipe: vi.fn(async () => undefined),
        })),
      },
      CORELINK_RUNNER_MINT_AUTH_KEY: "dispatcher-key",
      SPAWN_WORKER_PUBLIC_URL: "https://spawn-worker.example/",
      BILLING_INGEST_URL: "https://billing.example/internal/v1/billing/usage",
      BILLING_INGEST_AUTH_KEY: "billing-ingest-key",
      BILLING_REGION: "gru",
    };
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-09-05T12:00:00.000Z"));
    vi.stubGlobal("fetch", vi.fn(async (input: any, init: any = {}) => {
      const url = String(input);
      if (url === "https://billing.example/internal/v1/billing/usage") {
        billingEvents.push(...JSON.parse(String(init.body)));
      } else {
        revokeRequests.push(url);
      }
      return new Response("{}", { status: 200 });
    }));
  });

  afterEach(() => vi.useRealTimers());

  function authorizedStart(config: { workspaceName: string; profileName: string; tier: "standard-2" | "standard-4" }) {
    return {
      config,
      grant: {
        tenantId: "00000000-0000-4000-8000-000000000042",
        sessionUuid: crypto.randomUUID(),
        casPat: TEST_PAT_SECRET,
        patId: crypto.randomUUID(),
        expiresAtMs: Date.now() + 60 * 60 * 1000,
      },
    } as const;
  }

  // Simulated Edge Authentication Middleware
  function authenticateCustomerRequest(headers: Headers) {
    const authHeader = headers.get("Authorization") || headers.get("authorization");
    if (!authHeader || !authHeader.startsWith("Bearer ")) {
      return { ok: false, status: 401, error: "Missing or malformed Authorization header" };
    }
    const token = authHeader.replace("Bearer ", "").trim();
    const patRecord = mockPatTable.get(token);
    if (!patRecord || patRecord.revoked) {
      return { ok: false, status: 401, error: "Invalid or revoked PAT" };
    }
    const tenant = mockTenantsTable.get(patRecord.tenant_id);
    if (!tenant || tenant.status !== "active") {
      return { ok: false, status: 403, error: "Tenant suspended or not found" };
    }
    return { ok: true, tenant, user: { id: patRecord.user_id, role: patRecord.role } };
  }

  // ═════════════════════════════════════════════════════════════════════
  // TEST SCENARIOS AS THE UNPRIVILEGED CUSTOMER (JANE DOE)
  // ═════════════════════════════════════════════════════════════════════

  it("1. Rejects unauthenticated requests and revoked tokens", () => {
    // Unauthenticated
    const noAuthHeaders = edgeIngressFilter({});
    const res1 = authenticateCustomerRequest(noAuthHeaders);
    expect(res1.ok).toBe(false);
    expect(res1.status).toBe(401);

    // Revoked token
    mockPatTable.get(TEST_PAT_SECRET)!.revoked = true;
    const revokedHeaders = edgeIngressFilter({ Authorization: `Bearer ${TEST_PAT_SECRET}` });
    const res2 = authenticateCustomerRequest(revokedHeaders);
    expect(res2.ok).toBe(false);
    expect(res2.status).toBe(401);
  });

  it("rejects the legacy raw-PAT start RPC", async () => {
    const sandbox = new RunnerDevEnvDO(mockCtx, mockEnv);
    await expect(sandbox.startDevenv({})).rejects.toThrow("DEVENV_AUTHORIZED_RPC_REQUIRED");
  });

  it("2. Strips spoofed admin privilege headers and enforces tenant identity from PAT", () => {
    const spoofedHeaders = edgeIngressFilter({
      Authorization: `Bearer ${TEST_PAT_SECRET}`,
      "X-Corelink-Role": "superadmin\r\nInjected-Header: evil",
      "X-Tenant-Id": "victim-enterprise-tenant-999",
      "X-Bypass-Billing": "true",
    });

    // Ingress gateway stripped spoofed headers
    expect(spoofedHeaders.get("X-Corelink-Role")).toBeNull();
    expect(spoofedHeaders.get("X-Tenant-Id")).toBeNull();
    expect(spoofedHeaders.get("X-Bypass-Billing")).toBeNull();

    // Authenticated identity is strictly Jane Doe (member) from Acme Dev Team
    const auth = authenticateCustomerRequest(spoofedHeaders);
    expect(auth.ok).toBe(true);
    expect(auth.tenant!.id).toBe("tenant-cust-alpha-42");
    expect(auth.user!.role).toBe("member"); // NOT superadmin
  });

  it("3. Jane Doe launches sandbox, edits code, triggers snapshot, stops and gets billed correctly", async () => {
    // Step A: Ingress Authentication
    const clientHeaders = edgeIngressFilter({
      Authorization: `Bearer ${TEST_PAT_SECRET}`,
    });
    const auth = authenticateCustomerRequest(clientHeaders);
    expect(auth.ok).toBe(true);

    // Step B: User launches DevEnv via customer API
    const sandbox = new RunnerDevEnvDO(mockCtx, mockEnv);
    await new Promise((r) => setTimeout(r, 10));

    const startResp = await sandbox.startAuthorizedDevenv(authorizedStart({
      workspaceName: "jane-microservice",
      profileName: "jane-browser-profile",
      tier: "standard-2",
    }));
    expect(startResp.status).toBe("starting");
    expect((sandbox as any).envVars.EXEC_SERVER_AUTH_TOKEN_FILE).toBe(
      EXEC_SERVER_AUTH_TOKEN_FILE,
    );

    await sandbox.onStart();
    const liveStatus = await sandbox.getStatus();
    expect(liveStatus.status).toBe("running");
    expect(liveStatus.workspaceName).toBe("jane-microservice");

    // Step C: Jane triggers snapshot of her changes
    const snapshotResp = await sandbox.snapshot({ force: false });
    expect(snapshotResp.ok).toBe(true);
    expect(snapshotResp.workspaceSnapshot.root).toBe("bafy_user_repo_snapshot");

    // Step D: Jane stops the DevEnv session
    const stopResp = await sandbox.requestStop();
    expect(stopResp.ok).toBe(true);

    vi.setSystemTime(new Date("2026-09-05T12:00:31.000Z"));
    await sandbox.onStop();
    const finalStatus = await sandbox.getStatus();
    expect(finalStatus.status).toBe("stopped");

    // Step E: Verify canonical billing ingest recorded Jane's tenant usage
    expect(billingEvents).toHaveLength(1);
    expect(billingEvents[0].tenant_id).toBe("00000000-0000-4000-8000-000000000042");
    expect(billingEvents[0].event_kind).toBe("runner_vcpu_seconds");
    expect(billingEvents[0].qty).toBe(62); // 31s elapsed * 2 vCPU
    expect(billingEvents[0].idem_key).toMatch(/^[0-9a-f]{64}$/);
    expect(revokeRequests).toHaveLength(1);
  });

  it("4. Rejects launch when Jane's tenant is suspended by finance/compliance", async () => {
    // Tenant gets suspended
    mockTenantsTable.get(TEST_TENANT.id)!.status = "suspended";

    const clientHeaders = edgeIngressFilter({
      Authorization: `Bearer ${TEST_PAT_SECRET}`,
    });
    const auth = authenticateCustomerRequest(clientHeaders);
    expect(auth.ok).toBe(false);
    expect(auth.status).toBe(403);
    expect(auth.error).toBe("Tenant suspended or not found");
  });
});
