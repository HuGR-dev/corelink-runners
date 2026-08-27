import { Container } from "@cloudflare/containers";
import {
  DevenvState,
  DevenvTier,
  DEVENV_TIERS,
  StartPayload,
  StatusResponse,
  SnapshotRequest,
  SnapshotResponse,
  ResizeRequest,
  validateWorkspaceName,
  validateProfileName,
  validateClwToken,
  validateTenantId,
  validateStateTransition,
} from "../types/devenv.js";
import { pushUsageEvent } from "../lib.js";
import { hydrateViaClw, snapshotViaClw, acquireSnapshotLock, releaseSnapshotLock } from "../lib/clw.js";

/** State machine storage key */
const STATE_KEY = "state";
/** Last activity tracking key */
const ACTIVITY_KEY = "lastActivityAt";
/** Hard session timeout (ms) — prevents zombie container financial runaway */
const HARD_MAX_SESSION_MS = 8 * 3600 * 1000;
/** Exec-server loopback auth token key */
const EXEC_TOKEN_KEY = "execServerToken";
/** Exec server internal port */
const EXEC_SERVER_PORT = 9090;
/** RPC Timeout ms */
const EXEC_RPC_TIMEOUT_MS = 30_000;
/** Max bytes queued per WS (backpressure: 1 MiB) */
const MAX_WS_BUFFERED_BYTES = 1 << 20;

interface WsPair {
  readonly connId: string;
  readonly port: 6080 | 7681 | 8080;
  clientWs: WebSocket;
  containerWs: WebSocket;
}

export class RunnerDevEnvDO extends Container<any> {
  override defaultPort = 6080;
  override sleepAfter = "30m";
  override requiredPorts = [6080, 7681, 8080, 9090];
  override allowedHosts = [
    "corelink-api.humangr.com",
    "*.cloudflarestorage.com",
    "*.r2.cloudflarestorage.com",
  ];
  override enableInternet = true;
  
  private static readonly STATIC_ENV_VARS = {
    CLW_REF_DOMAIN: "runner",
    CLW_ENDPOINT: "https://corelink-api.humangr.com",
  } as const;

  private devenvState: DevenvState = { status: "stopped", createdAt: Date.now() };
  override envVars: Record<string, string> = {};
  private execToken: string = "default-token";
  private wsPairs: Map<string, WsPair> = new Map();

  constructor(ctx: any, env: any) {
    super(ctx, env);
    this.ctx.blockConcurrencyWhile(async () => {
      const stored = (await this.ctx.storage.get(STATE_KEY)) as DevenvState | undefined;
      if (stored) {
        this.devenvState = stored;
      } else {
        await this.ctx.storage.put(STATE_KEY, this.devenvState);
      }
      const token = (await this.ctx.storage.get(EXEC_TOKEN_KEY)) as string | undefined;
      if (token) {
        this.execToken = token;
      } else {
        this.execToken = crypto.randomUUID();
        await this.ctx.storage.put(EXEC_TOKEN_KEY, this.execToken);
      }
    });
  }

  private async persistState(): Promise<void> {
    await this.ctx.storage.put(STATE_KEY, this.devenvState);
  }

  private async transitionState(newState: DevenvState): Promise<void> {
    if (this.devenvState.status !== newState.status) {
      validateStateTransition(this.devenvState.status, newState.status);
    }
    this.devenvState = newState;
    await this.persistState();
  }

  private noteActivity(): void {
    this.ctx.storage.put(ACTIVITY_KEY, Date.now()).catch(() => {});
    this.renewActivityTimeout();
  }

  private validateStartPayload(payload: StartPayload): void {
    validateWorkspaceName(payload.config.workspaceName);
    validateProfileName(payload.config.profileName);
    validateClwToken(payload.config.clwToken);
    validateTenantId(payload.config.clwTenant);
  }

  private buildStatusResponse(): StatusResponse {
    const isRunning = this.devenvState.status === "running" || this.devenvState.status === "starting" || this.devenvState.status === "stopping";
    const tier: DevenvTier = (this.devenvState as any).tier ?? "standard-4";
    return {
      status: this.devenvState.status,
      workspaceName: isRunning ? (this.devenvState as any).workspaceName : null,
      profileName: isRunning ? (this.devenvState as any).profileName : null,
      tier,
      uptimeMs: isRunning && (this.devenvState as any).startedAt ? Date.now() - (this.devenvState as any).startedAt : null,
      ports: [6080, 7681, 8080],
      containerHandle: this.devenvState.status === "running" ? (this.devenvState as any).containerHandle : null,
    };
  }

  // ── Public RPC API ───────────────────────────────────────────────

  async startDevenv(payload: StartPayload): Promise<StatusResponse> {
    return await this.ctx.blockConcurrencyWhile(async () => {
      this.validateStartPayload(payload);
      
      const sessionUuid = crypto.randomUUID();
      const generationId = ((this.devenvState as any).generationId ?? 0) + 1;
      
      this.envVars = {
        ...RunnerDevEnvDO.STATIC_ENV_VARS,
        CLW_TENANT: payload.config.clwTenant,
        CLW_TOKEN: payload.config.clwToken,
        WORKSPACE_NAME: payload.config.workspaceName,
        PROFILE_NAME: payload.config.profileName,
        EXEC_SERVER_TOKEN: this.execToken,
        SESSION_UUID: sessionUuid,
        BILLING_TENANT_UUID: payload.config.clwTenant,
      };
      
      await this.transitionState({
        status: "starting",
        createdAt: this.devenvState.createdAt,
        startedAt: Date.now(),
        sessionUuid,
        billingSeq: 0,
        generationId,
        workspaceName: payload.config.workspaceName,
        profileName: payload.config.profileName,
        tier: payload.config.tier ?? "standard-4",
      });
      
      await this.start({
        envVars: this.envVars,
        enableInternet: true,
      });
      
      this.noteActivity();
      return this.buildStatusResponse();
    });
  }

  async requestStop(): Promise<{ readonly ok: true }> {
    return await this.ctx.blockConcurrencyWhile(async () => {
      if (this.devenvState.status === "stopped" || this.devenvState.status === "stopping") {
        return { ok: true };
      }
      
      if (this.devenvState.status === "errored") {
        await this.destroy();
        await this.transitionState({ status: "stopped", createdAt: this.devenvState.createdAt });
        return { ok: true };
      }
      
      await this.transitionState({
        status: "stopping",
        createdAt: this.devenvState.createdAt,
        startedAt: (this.devenvState as any).startedAt ?? Date.now(),
        sessionUuid: (this.devenvState as any).sessionUuid ?? crypto.randomUUID(),
        billingSeq: (this.devenvState as any).billingSeq ?? 0,
        generationId: (this.devenvState as any).generationId ?? 1,
        workspaceName: (this.devenvState as any).workspaceName ?? "",
        profileName: (this.devenvState as any).profileName ?? "",
        tier: (this.devenvState as any).tier ?? "standard-4",
      });
      
      await this.stop();
      this.noteActivity();
      return { ok: true };
    });
  }

  async getStatus(): Promise<StatusResponse> {
    return this.buildStatusResponse();
  }

  async snapshot(payload: SnapshotRequest): Promise<SnapshotResponse> {
    const traceId = crypto.randomUUID();
    const isRunning = this.devenvState.status === "running";
    if (!isRunning) {
      throw new Error("CANNOT_SNAPSHOT_STOPPED_CONTAINER");
    }
    const wsName = (this.devenvState as any).workspaceName;
    const profName = (this.devenvState as any).profileName;
    const genId = (this.devenvState as any).generationId ?? 1;

    const profileSnap = await this.execClwSnapshot("/data/chrome", profName, payload.force, genId, traceId);
    const workspaceSnap = await this.execClwSnapshot("/data/workspace", wsName, payload.force, genId, traceId);

    return {
      ok: true,
      profileSnapshot: { root: profileSnap.root ?? "", bytesTotal: profileSnap.bytesTotal },
      workspaceSnapshot: { root: workspaceSnap.root ?? "", bytesTotal: workspaceSnap.bytesTotal },
    };
  }

  async resize(payload: ResizeRequest): Promise<{ ok: true }> {
    const req = new Request(`http://localhost:${EXEC_SERVER_PORT}/resize`, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Exec-Token": this.execToken },
      body: JSON.stringify({ width: payload.width, height: payload.height }),
    });
    const resp = await this.containerFetch(req, EXEC_SERVER_PORT);
    if (!resp.ok) {
      throw new Error(`RESIZE_FAILED: ${resp.status}`);
    }
    return { ok: true };
  }

  // ── Lifecycle Hooks ──────────────────────────────────────────────

  override async onStart(): Promise<void> {
    if (this.devenvState.status === "starting") {
      await this.transitionState({
        status: "running",
        createdAt: this.devenvState.createdAt,
        startedAt: (this.devenvState as any).startedAt,
        sessionUuid: (this.devenvState as any).sessionUuid,
        billingSeq: (this.devenvState as any).billingSeq,
        generationId: (this.devenvState as any).generationId,
        workspaceName: (this.devenvState as any).workspaceName,
        profileName: (this.devenvState as any).profileName,
        tier: (this.devenvState as any).tier,
        containerHandle: this.ctx.id.toString(),
        lastHealthCheckAt: Date.now(),
        healthCheckFailures: 0,
      });
    }
    this.noteActivity();
  }

  override async onStop(): Promise<void> {
    await this.recordUsage();
    await this.transitionState({
      status: "stopped",
      createdAt: this.devenvState.createdAt,
      generationId: (this.devenvState as any).generationId,
    });
  }

  override async onError(error: unknown): Promise<void> {
    const errMsg = error instanceof Error ? error.message : String(error);
    await this.recordUsage();
    await this.transitionState({
      status: "errored",
      createdAt: this.devenvState.createdAt,
      lastError: errMsg.slice(0, 256),
      lastWorkspaceName: (this.devenvState as any).workspaceName ?? "",
      generationId: (this.devenvState as any).generationId,
      tier: (this.devenvState as any).tier,
    });
  }

  // ── In-Container Exec Client ─────────────────────────────────────

  private async containerExec(argv: readonly string[]): Promise<{ exitCode: number; stdout: string; stderr: string }> {
    const req = new Request(`http://localhost:${EXEC_SERVER_PORT}/clw`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Exec-Token": this.execToken,
      },
      body: JSON.stringify({ argv }),
      signal: AbortSignal.timeout(EXEC_RPC_TIMEOUT_MS),
    });
    const resp = await this.containerFetch(req, EXEC_SERVER_PORT);
    if (!resp.ok) {
      throw new Error(`EXEC_RPC_FAILED: ${resp.status} ${await resp.text()}`);
    }
    const body = (await resp.json()) as { exit_code: number; stdout: string; stderr: string };
    return { exitCode: body.exit_code, stdout: body.stdout, stderr: body.stderr };
  }

  private async execClwSnapshot(dir: string, name: string, force: boolean, generationId: number, traceId: string) {
    const args = [
      "snapshot", dir,
      "--name", name,
      "--concurrency", "8",
      "--json",
    ];
    if (force) args.push("--force");

    const res = await this.containerExec(args);
    if (res.exitCode !== 0) {
      throw new Error(`clw snapshot failed: ${res.stderr}`);
    }
    const report = JSON.parse(res.stdout.trim());
    return {
      root: report.root as string | null,
      bytesTotal: Number(report.bytes_total) || 0,
    };
  }

  // ── Billing / Metering ───────────────────────────────────────────

  private async recordUsage(): Promise<void> {
    const startedAt = (this.devenvState as any).startedAt;
    if (!startedAt) return;

    const MIN_BILLABLE_SECONDS = 30;
    const periodEndMs = Date.now();
    const rawWallSeconds = Math.max(0, (periodEndMs - startedAt) / 1000);
    const wallSeconds = Math.max(MIN_BILLABLE_SECONDS, Math.ceil(rawWallSeconds));

    const tier: DevenvTier = (this.devenvState as any).tier ?? "standard-4";
    const vcpuMultiplier = tier === "standard-2" ? 2 : tier === "power-8" ? 8 : tier === "ultra-16" ? 16 : 4;
    const vcpuSeconds = wallSeconds * vcpuMultiplier;

    const tenantUuid = this.envVars.BILLING_TENANT_UUID ?? this.envVars.CLW_TENANT ?? "00000000-0000-0000-0000-000000000000";
    const billingPeriod = new Date(startedAt).toISOString().slice(0, 7);

    const sessionUuid = (this.devenvState as any).sessionUuid ?? crypto.randomUUID();
    const billingSeq = (this.devenvState as any).billingSeq ?? 1;

    // 1. In-Worker D1 direct tally (when CONFIG_DB is available)
    if ((this.env as any).CONFIG_DB) {
      const monthStartMs = new Date(billingPeriod + "-01T00:00:00Z").getTime();
      let attempts = 0;
      while (attempts < 3) {
        try {
          const jitter = Math.floor(Math.random() * 200) + 50;
          await new Promise((r) => setTimeout(r, jitter));
          await (this.env as any).CONFIG_DB.prepare(
            `INSERT INTO devenv_monthly_vcpu (tenant_id, month_at, vcpu_seconds, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (tenant_id, month_at) DO UPDATE SET
                 vcpu_seconds = vcpu_seconds + excluded.vcpu_seconds,
                 updated_at   = excluded.updated_at`
          ).bind(tenantUuid, monthStartMs, vcpuSeconds, Date.now()).run();
          break;
        } catch {
          attempts++;
          await new Promise((r) => setTimeout(r, attempts * 300));
        }
      }
    }

    // 2. Canonical HTTP usage push (when BILLING_INGEST_URL is configured)
    if ((this.env as any).BILLING_INGEST_URL) {
      try {
        await pushUsageEvent(this.env as any, {
          tenant_id: tenantUuid,
          event_kind: "runner_vcpu_seconds",
          qty: vcpuSeconds,
          billing_period: billingPeriod,
          region: "wnam",
          source: "corelink/devenv",
          time_ms: periodEndMs,
          idem_key: `devenv:${sessionUuid}:${billingSeq}`,
        });
      } catch (err) {
        console.error("devenv_billing_push_failed", err);
      }
    }
  }

  // ── WebSocket Proxying & Hibernation ─────────────────────────────

  override async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);

    if (request.headers.get("Upgrade") === "websocket") {
      return this.handleWsUpgrade(request, url);
    }

    return await this.containerFetch(request, this.defaultPort);
  }

  private async handleWsUpgrade(request: Request, url: URL): Promise<Response> {
    const port = url.port ? Number(url.port) : (url.pathname.includes("/vnc") ? 6080 : url.pathname.includes("/tty") ? 7681 : 8080);
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);

    const connId = crypto.randomUUID();
    (server as any).serializeAttachment({ connId, port });
    this.ctx.acceptWebSocket(server);

    const containerReq = new Request(`http://localhost:${port}${url.pathname}${url.search}`, {
      method: "GET",
      headers: { Upgrade: "websocket" },
    });
    const containerResp = await this.containerFetch(containerReq, port);
    if (!containerResp.webSocket) {
      return new Response("Container WS Upgrade Failed", { status: 502 });
    }

    const containerWs = containerResp.webSocket;
    containerWs.accept();

    this.wsPairs.set(connId, { connId, port: port as 6080 | 7681 | 8080, clientWs: server, containerWs });
    this.noteActivity();

    containerWs.addEventListener("message", (e: MessageEvent) => {
      if (server.readyState === WebSocket.OPEN) {
        if ((server as any).bufferedAmount > MAX_WS_BUFFERED_BYTES) return;
        server.send(e.data);
      }
    });

    containerWs.addEventListener("close", () => {
      if (server.readyState === WebSocket.OPEN) server.close();
      this.wsPairs.delete(connId);
    });

    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    this.noteActivity();
    const attachment = (ws as any).deserializeAttachment() as { connId: string; port: number } | null;
    if (!attachment) return;

    const pair = this.wsPairs.get(attachment.connId);
    if (pair && pair.containerWs.readyState === WebSocket.OPEN) {
      if ((pair.containerWs as any).bufferedAmount > MAX_WS_BUFFERED_BYTES) return;
      pair.containerWs.send(message);
    }
  }

  async webSocketClose(ws: WebSocket, code: number, reason: string): Promise<void> {
    const attachment = (ws as any).deserializeAttachment() as { connId: string } | null;
    if (attachment) {
      const pair = this.wsPairs.get(attachment.connId);
      if (pair && pair.containerWs.readyState === WebSocket.OPEN) {
        pair.containerWs.close(code, reason);
      }
      this.wsPairs.delete(attachment.connId);
    }
  }
}
