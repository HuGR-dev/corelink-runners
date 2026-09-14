import { ComputeBudgetClient } from "../lib/compute_budget_client";
import { ComputeObligations, type ComputeBinding } from "../lib/compute_budget_obligation";
import { Container } from "@cloudflare/containers";
import {
  DevenvState,
  DevenvTier,
  AuthorizedDevenvStart,
  AuthorizedDevenvAck,
  AuthorizedDevenvStop,
  AuthorizedDevenvStopResponse,
  StatusResponse,
  SnapshotRequest,
  SnapshotResponse,
  ResizeRequest,
  validateStateTransition,
} from "../types/devenv.js";
import { DevenvCredentials, launchAuthorizedDevenv } from "../lib/devenv_credentials.js";
import { pushUsageEvent } from "../lib.js";
import { buildDevenvUsageEvent, type DevenvUsageInput } from "../lib/devenv_usage.js";
import {
  DEVENV_USAGE_PENDING_KEY,
  DEVENV_USAGE_SETTLED_KEY,
  freezeDevenvUsage,
  nextDevenvUsageAttempt,
  type DevenvUsagePending,
} from "../lib/devenv_usage_outbox.js";

/** State machine storage key */
const STATE_KEY = "state";
/** Last activity tracking key */
const ACTIVITY_KEY = "lastActivityAt";
/** Exec-server loopback auth token key */
const EXEC_TOKEN_KEY = "execServerToken";
/** Exec server internal port */
const EXEC_SERVER_PORT = 9090;
/** RPC Timeout ms */
const EXEC_RPC_TIMEOUT_MS = 30_000;
/** Max bytes queued per WS (backpressure: 1 MiB) */
const MAX_WS_BUFFERED_BYTES = 1 << 20;
const AUTHORIZED_STOP_KEY_PREFIX = "devenv:authorized-stop:";
const AUTHORIZED_STOP_INDEX_KEY = "devenv:authorized-stop-index";
const AUTHORIZED_STOP_TTL_MS = 8 * 3600 * 1000 + 3600 * 1000;
const MAX_AUTHORIZED_STOP_TOMBSTONES = 64;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function authorizedStopKey(tenantId: unknown, sessionUuid: unknown): string | undefined {
  if (typeof tenantId !== "string" || typeof sessionUuid !== "string" ||
      !UUID.test(tenantId) || !UUID.test(sessionUuid) ||
      tenantId === "00000000-0000-0000-0000-000000000000" || sessionUuid === "00000000-0000-0000-0000-000000000000") return undefined;
  return `${AUTHORIZED_STOP_KEY_PREFIX}${encodeURIComponent(tenantId)}:${encodeURIComponent(sessionUuid)}`;
}

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
  
  private devenvState: DevenvState = { status: "stopped", createdAt: Date.now() };
  override envVars: Record<string, string> = {};
  private execToken: string = "default-token";
  private wsPairs: Map<string, WsPair> = new Map();
  private readonly credentials: DevenvCredentials;
  private settlementPromise: Promise<DevenvUsageOutcome> | null = null;

  constructor(ctx: any, env: any) {
    super(ctx, env);
    this.credentials = new DevenvCredentials(ctx.storage, env);
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

  /** Legacy raw-PAT ingress is intentionally closed, including direct RPC calls. */
  async startDevenv(_payload: unknown): Promise<never> {
    throw new Error("DEVENV_AUTHORIZED_RPC_REQUIRED");
  }

  private computeObligations(): ComputeObligations {
    const terminalConfig = {
      terminalAuthority: this.env.FABRIC_COMPUTE_TERMINAL_AUTHORITY ?? "",
      terminalPublicKey: this.env.FABRIC_COMPUTE_TERMINAL_PUBLIC_KEY ?? "",
      receiptVersion: this.env.FABRIC_COMPUTE_TERMINAL_RECEIPT_VERSION ?? "",
      terminalKeyId: this.env.FABRIC_COMPUTE_TERMINAL_KEY_ID ?? "",
    };
    return new ComputeObligations(this.ctx.storage, new ComputeBudgetClient(this.env.FABRIC_COMPUTE_URL ?? "", fetch, terminalConfig), terminalConfig);
  }

  async prepareAuthorizedCompute(binding: ComputeBinding): Promise<void> {
    return this.ctx.blockConcurrencyWhile(async () => {
      if (binding.workloadKind !== "devenv" || binding.reservationId !== binding.workloadId || binding.vcpuCount !== 4 || binding.maximumWallMs !== 28_800_000) {
        throw new Error("DEVENV_COMPUTE_BINDING_INVALID");
      }
      if (this.devenvState.status !== "stopped" && this.devenvState.status !== "errored") throw new Error("DEVENV_COMPUTE_SESSION_ACTIVE");
      const previous = await this.ctx.storage.get<string>("compute:devenv-session");
      if (previous && previous !== binding.reservationId) await this.computeObligations().abandonUnused(previous);
      await this.computeObligations().stage(binding, Date.now());
      await this.ctx.storage.put("compute:devenv-session", binding.reservationId);
      // Schedule recovery before RPCs; an uncertain reserve still has an owner.
      await this.schedule(new Date(Date.now() + 90_000), "retryUnusedCompute", { reservationId: binding.reservationId });
      await this.computeObligations().prepare(binding, Date.now());
    });
  }

  async abandonAuthorizedCompute(reservationId: string): Promise<void> {
    return this.ctx.blockConcurrencyWhile(() => this.computeObligations().abandonUnused(reservationId));
  }

  async retryUnusedCompute(payload: { reservationId: string }): Promise<void> {
    let pending = true;
    try {
      pending = await this.ctx.blockConcurrencyWhile(async () => {
        const cursor = await this.ctx.storage.get<string>("compute:drain-cursor");
        const result = await this.computeObligations().drainUnused(Date.now(), cursor);
        const retryRequired = result.retryRequired ||
          await this.ctx.storage.get<boolean>("compute:drain-retry") === true;
        if (result.cursor) {
          // Keep failures from earlier pages until this complete pass finishes.
          await this.ctx.storage.put("compute:drain-retry", retryRequired);
          await this.ctx.storage.put("compute:drain-cursor", result.cursor);
        } else {
          await this.ctx.storage.delete("compute:drain-cursor");
          await this.ctx.storage.delete("compute:drain-retry");
        }
        return !!result.cursor || retryRequired;
      });
    } catch { /* retain the independent retry alarm */ }
    if (pending) await this.schedule(new Date(Date.now() + 60_000), "retryUnusedCompute", payload);
  }

  async startAuthorizedDevenv(payload: AuthorizedDevenvStart): Promise<AuthorizedDevenvAck> {
    return this.ctx.blockConcurrencyWhile(async () => {
      if (authorizedStopKey(payload?.grant?.tenantId, payload?.grant?.sessionUuid) &&
          await this.ctx.storage.get(authorizedStopKey(payload.grant.tenantId, payload.grant.sessionUuid)!)) {
        throw new Error("DEVENV_AUTHORIZED_START_CANCELED");
      }
      const reservationId = payload?.grant?.computeReservationId;
      if (reservationId && (reservationId !== payload.grant.sessionUuid ||
          await this.ctx.storage.get<string>("compute:devenv-session") !== reservationId)) throw new Error("DEVENV_COMPUTE_BINDING_INVALID");
      try {
        return await launchAuthorizedDevenv({
          env: this.env, credentials: this.credentials, execToken: this.execToken,
          getState: () => this.devenvState, transition: (state) => this.transitionState(state),
          settle: () => this.recordUsage(), completeStopped: () => this.completeStoppedSession(),
          start: async (envVars) => {
            if (reservationId) await this.computeObligations().claimProvider(reservationId, payload.grant.sessionUuid, Date.now());
            this.envVars = envVars;
            await this.start({ envVars, enableInternet: true }, { portToCheck: this.defaultPort, signal: AbortSignal.timeout(Math.max(1, Math.min(8000, payload.grant.expiresAtMs - Date.now()))) });
          },
          destroy: () => this.destroy(), schedule: (when, callback, value) => this.schedule(when, callback, value),
          noteActivity: () => this.noteActivity(),
        }, payload);
      } catch (error) {
        if (reservationId) {
          try { await this.computeObligations().abandonUnused(reservationId); }
          catch { /* The independently scheduled compute obligation remains. */ }
        }
        throw error;
      }
    });
  }

  /** Trusted server compensation for a start ACK that was not adopted. */
  async stopAuthorizedDevenv(payload: AuthorizedDevenvStop): Promise<AuthorizedDevenvStopResponse> {
    return this.ctx.blockConcurrencyWhile(async () => {
      const requestedSession = payload?.sessionUuid;
      const requestedTenant = payload?.tenantId;
      const cancellationKey = authorizedStopKey(requestedTenant, requestedSession);
      if (cancellationKey) {
        // This tombstone also covers a stop that races ahead of a delayed start RPC.
        const now = Date.now();
        const index = await this.readStopTombstoneIndex(now);
        const existing = index.find((entry) => entry.key === cancellationKey);
        if (index.length >= MAX_AUTHORIZED_STOP_TOMBSTONES && !existing) {
          throw new Error("DEVENV_AUTHORIZED_STOP_CAPACITY");
        }
        const entry = { key: cancellationKey, canceledAt: now, expiresAt: now + AUTHORIZED_STOP_TTL_MS };
        await this.ctx.storage.put(cancellationKey, { tenantId: requestedTenant, sessionUuid: requestedSession, canceledAt: now, expiresAt: entry.expiresAt });
        await this.ctx.storage.put(AUTHORIZED_STOP_INDEX_KEY, [...index.filter((item) => item.key !== cancellationKey), entry]);
        await this.schedule(new Date(entry.expiresAt), "expireAuthorizedStop", { tenantId: requestedTenant, sessionUuid: requestedSession, canceledAt: now });
      }
      const state = this.devenvState;
      const terminalUsage = state.status === "stopped" || state.status === "errored"
        ? state.terminalUsage
        : undefined;
      const stateMatches = state.status === "starting" || state.status === "running" || state.status === "stopping"
        ? state.sessionUuid === requestedSession && state.tenantId === requestedTenant
        : terminalUsage?.sessionId === requestedSession && terminalUsage.tenantId === requestedTenant;
      if (!stateMatches) return { sessionUuid: requestedSession, status: "not_current" };

      const credentialHandle = await this.credentials.current();
      const credentialsMatch = credentialHandle
        ? credentialHandle.sessionUuid === requestedSession && credentialHandle.tenantId === requestedTenant
        : false;
      if (credentialHandle && !credentialsMatch) {
        return { sessionUuid: requestedSession, status: "not_current" };
      }
      if ((state.status === "stopped" || state.status === "errored") && !credentialHandle) {
        return { sessionUuid: requestedSession, status: "already_stopped" };
      }

      if (state.status === "stopped" || state.status === "errored") {
        await this.recoverTerminalCredentials();
        return { sessionUuid: requestedSession, status: "already_stopped" };
      }

      if (state.status !== "stopping") {
        await this.transitionState({
          status: "stopping", createdAt: state.createdAt, startedAt: state.startedAt,
          sessionUuid: state.sessionUuid, tenantId: state.tenantId, billingSeq: state.billingSeq,
          generationId: state.generationId, workspaceName: state.workspaceName,
          profileName: state.profileName, tier: state.tier,
        });
      }

      let stopped = false;
      try {
        await this.destroy();
        stopped = true;
        await this.completeStoppedSession();
      } catch (error) {
        if (!stopped) throw new Error("DEVENV_PROVIDER_STOP_FAILED");
        throw error;
      } finally {
        await this.credentials.cleanup(stopped);
      }
      return { sessionUuid: requestedSession, status: "stopped" };
    });
  }

  async expireAuthorizedStop(payload: { tenantId: string; sessionUuid: string; canceledAt: number }): Promise<void> {
    return this.ctx.blockConcurrencyWhile(async () => {
      const key = authorizedStopKey(payload?.tenantId, payload?.sessionUuid);
      if (!key || !Number.isSafeInteger(payload?.canceledAt)) return;
      const current = await this.ctx.storage.get<{ canceledAt: number }>(key);
      if (!current || current.canceledAt !== payload.canceledAt) return;
      await this.ctx.storage.delete(key);
      const index = await this.ctx.storage.get<Array<{ key: string; canceledAt: number; expiresAt: number }>>(AUTHORIZED_STOP_INDEX_KEY) ?? [];
      const next = index.filter((entry) => entry.key !== key);
      if (next.length) await this.ctx.storage.put(AUTHORIZED_STOP_INDEX_KEY, next);
      else await this.ctx.storage.delete(AUTHORIZED_STOP_INDEX_KEY);
    });
  }

  private async readStopTombstoneIndex(now: number): Promise<Array<{ key: string; canceledAt: number; expiresAt: number }>> {
    const raw = await this.ctx.storage.get<Array<{ key: string; canceledAt: number; expiresAt: number }>>(AUTHORIZED_STOP_INDEX_KEY);
    const index = Array.isArray(raw) ? raw : [];
    const live = index.filter((entry) => entry && typeof entry.key === "string" && entry.expiresAt > now);
    for (const expired of index) {
      if (!live.includes(expired)) await this.ctx.storage.delete(expired.key);
    }
    if (live.length !== index.length) {
      if (live.length) await this.ctx.storage.put(AUTHORIZED_STOP_INDEX_KEY, live);
      else await this.ctx.storage.delete(AUTHORIZED_STOP_INDEX_KEY);
    }
    return live;
  }

  private recoverTerminalCredentials(): Promise<void> {
    return this.credentials.recoverTerminal(() => this.destroy(), () => this.completeStoppedSession());
  }

  /** Session binding makes callbacks from an older SDK schedule harmless. */
  async expireAuthorizedSession(payload: { sessionUuid: string }): Promise<void> {
    await this.ctx.blockConcurrencyWhile(() => this.credentials.expire(
      payload?.sessionUuid, () => this.destroy(), () => this.completeStoppedSession(),
    )).catch(() => { console.error(JSON.stringify({ event: "devenv_expiry_cleanup_pending" })); });
  }

  async requestStop(): Promise<{ readonly ok: true }> {
    return await this.ctx.blockConcurrencyWhile(async () => {
      if (this.devenvState.status === "stopped" || this.devenvState.status === "errored") {
        // A terminal container stays stoppable even while billing is unavailable.
        // Preserve its identity; authorized start requires successful settlement.
        if (this.devenvState.status === "errored" && !await this.credentials.current()) await this.destroy();
        await this.recoverTerminalCredentials();
        await this.recordUsage();
        return { ok: true };
      }
      if (this.devenvState.status === "stopping") {
        // A repeated stop awaits provider force-stop, not an empty local-state proof.
        let stopped = false;
        try {
          await this.destroy(); stopped = true;
          await this.completeStoppedSession();
        } finally { await this.credentials.cleanup(stopped); }
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
      
      try { await this.stop(); } catch {
        await this.credentials.cleanup(false);
        throw new Error("DEVENV_PROVIDER_STOP_FAILED");
      }
      // Let the SIGTERM snapshot finish; onStop wipes credentials on confirmed exit.
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
    try { await this.completeStoppedSession(); }
    finally { await this.credentials.cleanup(true); }
  }

  private async completeStoppedSession(): Promise<void> {
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

  override async onError(_error: unknown): Promise<void> {
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
        lastError: "DEVENV_CONTAINER_ERROR",
        lastWorkspaceName: state.workspaceName,
        generationId: state.generationId,
        tier: state.tier,
        terminalUsage,
      };
    }
    try { await this.recordUsage(); }
    finally { await this.credentials.cleanup(false); }
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
  // RUNNER_DEVENV_DO binding using RPC for authorized start and fetch for controls. This override
  // Path matrix (all under /v1/customer/devenv):
  //   GET  /                     → list / getStatus
  //   POST /                     → denied; trusted start uses typed RPC
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
      // Grant-shaped JSON or forged tenant headers never authorize a start.
      if (method === "POST" && (normSub === "" || normSub === "/")) {
        return Response.json({ error: "DEVENV_AUTHORIZED_RPC_REQUIRED" }, { status: 403 });
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
