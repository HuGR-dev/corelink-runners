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
import { buildDevenvUsageEvent, type DevenvUsageInput } from "../lib/devenv_usage.js";
import {
  DEVENV_USAGE_PENDING_KEY,
  DEVENV_USAGE_SETTLED_KEY,
  freezeDevenvUsage,
  nextDevenvUsageAttempt,
  type DevenvUsagePending,
} from "../lib/devenv_usage_outbox.js";
import {
  hydrateViaClw,
  snapshotViaClw,
  acquireSnapshotLock,
  releaseSnapshotLock,
  EXEC_SERVER_AUTH_TOKEN_FILE,
} from "../lib/clw.js";

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

type DevenvUsageOutcome =
  | { readonly outcome: "sent" | "disabled" | "pending" | "no_session" }
  | { readonly outcome: "invalid"; readonly code: string };

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
  private settlementPromise: Promise<DevenvUsageOutcome> | null = null;

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
      if (this.devenvState.status !== "stopped" && this.devenvState.status !== "errored") {
        throw new Error("DEVENV_START_REQUIRES_TERMINAL_STATE");
      }
      const settlement = await this.recordUsage();
      if (settlement.outcome === "pending" || settlement.outcome === "invalid") {
        throw new Error("DEVENV_BILLING_PENDING");
      }
      
      const sessionUuid = crypto.randomUUID();
      const generationId = ((this.devenvState as any).generationId ?? 0) + 1;
      
      this.envVars = {
        ...RunnerDevEnvDO.STATIC_ENV_VARS,
        CLW_TENANT: payload.config.clwTenant,
        CLW_TOKEN: payload.config.clwToken,
        WORKSPACE_NAME: payload.config.workspaceName,
        PROFILE_NAME: payload.config.profileName,
        // Provider ingress token is consumed by entrypoint.sh only. The bridge
        // writes this mode-0400 path, unsets EXEC_SERVER_AUTH_TOKEN, and the
        // supervisor passes only the path to the durable exec-server.
        EXEC_SERVER_AUTH_TOKEN_FILE,
        EXEC_SERVER_AUTH_TOKEN: this.execToken,
        SESSION_UUID: sessionUuid,
        BILLING_TENANT_UUID: payload.config.clwTenant,
      };
      
      await this.transitionState({
        status: "starting",
        createdAt: this.devenvState.createdAt,
        startedAt: Date.now(),
        sessionUuid,
        tenantId: payload.config.clwTenant,
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
      if (this.devenvState.status === "stopped" || this.devenvState.status === "errored") {
        // A terminal container stays stoppable even while billing is unavailable.
        // Preserve its identity; startDevenv requires successful settlement.
        if (this.devenvState.status === "errored") await this.destroy();
        await this.recordUsage();
        return { ok: true };
      }
      if (this.devenvState.status === "stopping") {
        // A lost provider callback must not make public stop permanently inert.
        await this.stop();
        return { ok: true };
      }
      
      await this.transitionState({
        status: "stopping",
        createdAt: this.devenvState.createdAt,
        startedAt: (this.devenvState as any).startedAt,
        sessionUuid: (this.devenvState as any).sessionUuid,
        tenantId: (this.devenvState as any).tenantId,
        billingSeq: (this.devenvState as any).billingSeq,
        generationId: (this.devenvState as any).generationId,
        workspaceName: (this.devenvState as any).workspaceName,
        profileName: (this.devenvState as any).profileName,
        tier: (this.devenvState as any).tier,
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
        tenantId: (this.devenvState as any).tenantId,
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

  /** Freeze callback time before any storage or network await. */
  private terminalUsageSnapshot(): DevenvUsageInput | undefined {
    const state = this.devenvState;
    if (state.status === "stopped" || state.status === "errored") return state.terminalUsage;
    return {
      tenantId: state.tenantId,
      sessionId: state.sessionUuid,
      tier: state.tier,
      startedAtMs: state.startedAt,
      completedAtMs: Date.now(),
      region: this.env.BILLING_REGION,
    };
  }

  override async onStop(): Promise<void> {
    const terminalUsage = this.terminalUsageSnapshot();
    if (!terminalUsage && this.devenvState.status === "errored") {
      // An old errored record still owns its session even without a timestamp.
      await this.recordUsage();
      return;
    }
    // Retain the snapshot in memory even when the first durable write fails.
    // recordUsage persists this state before constructing/delivering an event.
    this.devenvState = {
      status: "stopped",
      createdAt: this.devenvState.createdAt,
      generationId: this.devenvState.generationId,
      terminalUsage,
    };
    await this.recordUsage();
  }

  override async onError(error: unknown): Promise<void> {
    const terminalUsage = this.terminalUsageSnapshot();
    const state = this.devenvState;
    if (state.status !== "stopped" && state.status !== "errored") {
      this.devenvState = {
        status: "errored",
        createdAt: state.createdAt,
        startedAt: state.startedAt,
        sessionUuid: state.sessionUuid,
        tenantId: state.tenantId,
        billingSeq: state.billingSeq,
        lastError: (error instanceof Error ? error.message : String(error)).slice(0, 256),
        lastWorkspaceName: state.workspaceName,
        generationId: state.generationId,
        tier: state.tier,
        terminalUsage,
      };
    }
    await this.recordUsage();
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

  private async recordUsage(): Promise<DevenvUsageOutcome> {
    if (this.settlementPromise) return this.settlementPromise;
    this.settlementPromise = this.settleUsage().catch(() => {
      console.error(JSON.stringify({ event: "devenv_billing_outbox_unavailable" }));
      return { outcome: "pending" as const };
    }).finally(() => { this.settlementPromise = null; });
    return this.settlementPromise;
  }

  private async settleUsage(): Promise<DevenvUsageOutcome> {
    // Persist the terminal timestamp even if writing the pending event fails.
    // A restarted DO can then retry without charging time after the callback.
    await this.persistState();
    const pending = await this.ctx.storage.get(DEVENV_USAGE_PENDING_KEY) as DevenvUsagePending | undefined;
    if (pending) return this.deliverPendingUsage(pending);
    const state = this.devenvState;
    if (state.status !== "stopped" && state.status !== "errored") return { outcome: "pending" };
    const snapshot = state.terminalUsage;
    if (!snapshot) {
      if (state.status === "errored") {
        const settledSession = await this.ctx.storage.get(DEVENV_USAGE_SETTLED_KEY) as string | undefined;
        if (settledSession === state.sessionUuid) return { outcome: "no_session" };
      }
      // Legacy errored sessions have no trustworthy completion time. Keep them
      // blocked instead of inventing a later timestamp or losing the identity.
      return { outcome: state.status === "errored" ? "pending" : "no_session" };
    }
    const settledSession = await this.ctx.storage.get(DEVENV_USAGE_SETTLED_KEY) as string | undefined;
    if (settledSession === snapshot.sessionId) return { outcome: "no_session" };
    if (!this.env.BILLING_INGEST_URL) {
      await this.ctx.storage.put(DEVENV_USAGE_SETTLED_KEY, snapshot.sessionId);
      console.info(JSON.stringify({ event: "devenv_billing_disabled", reason: "BILLING_INGEST_URL_unset" }));
      return { outcome: "disabled" };
    }
    const result = await buildDevenvUsageEvent(snapshot);
    if (!result.ok) {
      console.error(JSON.stringify({ event: "devenv_billing_invalid", code: result.error.code, field: result.error.field }));
      return { outcome: "invalid", code: result.error.code };
    }
    const frozen = freezeDevenvUsage(result.event, snapshot.sessionId, snapshot.completedAtMs);
    await this.ctx.storage.put(DEVENV_USAGE_PENDING_KEY, frozen);
    return this.deliverPendingUsage(frozen);
  }

  private async deliverPendingUsage(pending: DevenvUsagePending): Promise<DevenvUsageOutcome> {
    const settledSession = await this.ctx.storage.get(DEVENV_USAGE_SETTLED_KEY) as string | undefined;
    if (settledSession === pending.sessionUuid) {
      await this.ctx.storage.delete(DEVENV_USAGE_PENDING_KEY);
      return { outcome: "sent" };
    }
    // Disabling delivery must not discard an event that was already queued.
    if (!this.env.BILLING_INGEST_URL) return { outcome: "pending" };
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 5000);
    try {
      await pushUsageEvent(this.env as any, pending.event, controller.signal);
      await this.ctx.storage.put(DEVENV_USAGE_SETTLED_KEY, pending.sessionUuid);
      await this.ctx.storage.delete(DEVENV_USAGE_PENDING_KEY);
      return { outcome: "sent" };
    } catch {
      await this.ctx.storage.put(DEVENV_USAGE_PENDING_KEY, nextDevenvUsageAttempt(pending));
      console.error(JSON.stringify({ event: "devenv_billing_delivery_failed", attempt: pending.attempts + 1 }));
      return { outcome: "pending" };
    } finally {
      clearTimeout(timer);
    }
  }

  // ── HTTP→RPC Router & WebSocket Proxy ──────────────────────────────
  //
  // The corelink-server worker forwards requests via the cross-worker
  // RUNNER_DEVENV_DO binding using `devStub.fetch()`.  This override
  // maps API paths to the internal RPC methods so the control-plane
  // endpoints work end-to-end.
  //
  // Path matrix (all under /v1/customer/devenv):
  //   GET  /                     → list / getStatus
  //   POST /                     → startDevenv
  //   GET  /status               → getStatus
  //   POST /stop                 → requestStop
  //   DELETE /  or DELETE /:id   → requestStop
  //   POST /snapshot             → snapshot
  //   POST /resize               → resize
  //   WS   /vnc | /tty | /code  → WebSocket proxy

  override async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const method = request.method.toUpperCase();

    // ── WebSocket upgrades ───────────────────────────────────────
    if (request.headers.get("Upgrade") === "websocket") {
      return this.handleWsUpgrade(request, url);
    }

    // ── Devenv control-plane routing ─────────────────────────────
    // Strip the /v1/customer/devenv prefix to get the sub-path.
    const devenvBase = "/v1/customer/devenv";
    const devenvAlt = "/v1/devenv";
    let subPath = "";
    if (url.pathname.startsWith(devenvBase)) {
      subPath = url.pathname.slice(devenvBase.length);
    } else if (url.pathname.startsWith(devenvAlt)) {
      subPath = url.pathname.slice(devenvAlt.length);
    }

    // Normalise: strip trailing slash, strip leading /:id segment for DELETE
    const normSub = subPath.replace(/\/+$/, "");

    try {
      // POST /v1/customer/devenv → startDevenv
      if (method === "POST" && (normSub === "" || normSub === "/")) {
        const body = await request.json() as any;
        const tenantId = request.headers.get("x-corelink-tenant-id") ?? "";
        const clwToken = body.clw_token ?? "";
        const result = await this.startDevenv({
          config: {
            workspaceName: body.workspace_name ?? "",
            profileName: body.profile_name ?? "default",
            tier: body.tier ?? "standard-4",
            clwEndpoint: "https://corelink-api.humangr.com",
            clwTenant: tenantId,
            clwToken,
          },
        });
        return new Response(JSON.stringify(result), {
          status: 201,
          headers: { "Content-Type": "application/json" },
        });
      }

      // GET /v1/customer/devenv → list (wraps getStatus in devenvs array)
      if (method === "GET" && (normSub === "" || normSub === "/")) {
        const status = await this.getStatus();
        return new Response(JSON.stringify({ devenvs: [status] }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }

      // GET /v1/customer/devenv/status → getStatus
      if (method === "GET" && normSub === "/status") {
        const status = await this.getStatus();
        return new Response(JSON.stringify(status), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }

      // POST /v1/customer/devenv/stop → requestStop
      if (method === "POST" && normSub === "/stop") {
        const result = await this.requestStop();
        return new Response(JSON.stringify(result), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }

      // DELETE /v1/customer/devenv or DELETE /v1/customer/devenv/:id → requestStop
      if (method === "DELETE") {
        const result = await this.requestStop();
        return new Response(JSON.stringify(result), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }

      // POST /v1/customer/devenv/snapshot → snapshot
      if (method === "POST" && normSub === "/snapshot") {
        const body = (await request.json().catch(() => ({}))) as any;
        const result = await this.snapshot({ force: body.force ?? false });
        return new Response(JSON.stringify(result), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }

      // POST /v1/customer/devenv/resize → resize
      if (method === "POST" && normSub === "/resize") {
        const body = await request.json() as any;
        const result = await this.resize({ width: body.width, height: body.height });
        return new Response(JSON.stringify(result), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      const status = message.includes("INVALID_STATE_TRANSITION") ? 409
        : message.includes("CANNOT_SNAPSHOT") ? 409
        : message.includes("SNAPSHOT_IN_PROGRESS") ? 409
        : message.includes("RESIZE_FAILED") ? 502
        : 500;
      return new Response(JSON.stringify({ error: message }), {
        status,
        headers: { "Content-Type": "application/json" },
      });
    }

    // ── Fallback: proxy to container (code-server, noVNC static, etc.)
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
