import { EXEC_SERVER_AUTH_TOKEN_FILE } from "./clw.js";
import { randomTicket, revokeCasPatById } from "../lib.js";
import { DEVENV_TIERS, validateProfileName, validateWorkspaceName, type AuthorizedDevenvStart, type AuthorizedDevenvAck, type DevenvState } from "../types/devenv.js";

export const DEVENV_CREDENTIAL_KEY = "devenv:credential-cleanup";
export const MAX_DEVENV_SESSION_MS = 8 * 3600 * 1000;
const CLEANUP_TIMEOUT_MS = 5000;
const CLEANUP_RETRY_BASE_MS = 60_000;
const CLEANUP_RETRY_MAX_MS = 15 * 60_000;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export interface DevenvCredentialHandle {
  tenantId: string;
  sessionUuid: string;
  patId: string;
  expiresAtMs: number;
  leaseId: string;
  providerMayExist: boolean;
  stashWiped: boolean;
  revoked: boolean;
  cleanupPending?: boolean;
  cleanupRetryAttempt?: number;
  cleanupRetryAtMs?: number;
}

function validUuid(value: unknown): value is string {
  return typeof value === "string" && UUID.test(value) && value !== "00000000-0000-0000-0000-000000000000";
}

function trustedHttpsBase(value: unknown): string {
  if (typeof value !== "string") throw new Error("DEVENV_CREDENTIAL_CONFIGURATION_REQUIRED");
  const url = new URL(value);
  if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash || url.pathname !== "/") {
    throw new Error("DEVENV_CREDENTIAL_CONFIGURATION_INVALID");
  }
  return url.origin;
}

/** Only the trusted DO binding may supply this grant; never accept it over HTTP. */
export function validateAuthorizedDevenvStart(payload: AuthorizedDevenvStart, env: any, now: number): void {
  if (!payload?.config || !payload?.grant) throw new Error("DEVENV_AUTHORIZED_GRANT_REQUIRED");
  validateWorkspaceName(payload.config.workspaceName);
  validateProfileName(payload.config.profileName);
  if (!Object.hasOwn(DEVENV_TIERS, payload.config.tier ?? "standard-4")) throw new Error("DEVENV_INVALID_TIER");
  const grant = payload.grant;
  if (![grant.tenantId, grant.sessionUuid, grant.patId].every(validUuid)) throw new Error("DEVENV_INVALID_GRANT_IDENTITY");
  if (!Number.isSafeInteger(grant.expiresAtMs) || grant.expiresAtMs <= now || grant.expiresAtMs - now > MAX_DEVENV_SESSION_MS) {
    throw new Error("DEVENV_INVALID_GRANT_EXPIRY");
  }
  if (typeof grant.casPat !== "string" || grant.casPat.length < 1 || grant.casPat.length > 4096 || /[\s\x00-\x1f\x7f]/.test(grant.casPat)) {
    throw new Error("DEVENV_INVALID_GRANT_TOKEN");
  }
  if (!env.CRED_STASH || typeof env.CRED_STASH.get !== "function" || typeof env.CRED_STASH.idFromName !== "function" ||
      typeof env.CORELINK_RUNNER_MINT_AUTH_KEY !== "string" || !env.CORELINK_RUNNER_MINT_AUTH_KEY.trim()) {
    throw new Error("DEVENV_CREDENTIAL_CONFIGURATION_REQUIRED");
  }
  trustedHttpsBase(env.SPAWN_WORKER_PUBLIC_URL);
  trustedHttpsBase(env.CORELINK_MINT_URL ?? "https://corelink-api.humangr.com");
  if (env.BILLING_INGEST_URL && (typeof env.BILLING_REGION !== "string" || !/^[a-z]{3}$/.test(env.BILLING_REGION))) {
    throw new Error("DEVENV_BILLING_REGION_REQUIRED");
  }
}

async function bounded<T>(operation: Promise<T>): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([operation, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error("DEVENV_CREDENTIAL_CLEANUP_TIMEOUT")), CLEANUP_TIMEOUT_MS);
    })]);
  } finally { if (timer !== undefined) clearTimeout(timer); }
}

/** Durable cleanup ownership contains identifiers only; the PAT lives in CRED_STASH. */
export class DevenvCredentials {
  private serial: Promise<unknown> = Promise.resolve();
  private loaded = false;
  private handle: DevenvCredentialHandle | undefined;
  constructor(private readonly storage: any, private readonly env: any) {}

  private run<T>(work: () => Promise<T>): Promise<T> {
    const result = this.serial.then(work);
    this.serial = result.catch(() => undefined);
    return result;
  }
  private async read(): Promise<DevenvCredentialHandle | undefined> {
    if (!this.loaded) {
      this.handle = await this.storage.get(DEVENV_CREDENTIAL_KEY);
      this.loaded = true;
    }
    return this.handle;
  }
  current(): Promise<DevenvCredentialHandle | undefined> {
    return this.run(async () => { const handle = await this.read(); return handle ? { ...handle } : undefined; });
  }

  reserve(payload: AuthorizedDevenvStart): Promise<void> {
    return this.run(async () => {
      if (await this.read()) throw new Error("DEVENV_CREDENTIAL_CLEANUP_PENDING");
      const { grant } = payload;
      const leaseId = `devenv:${grant.sessionUuid}`;
      this.handle = { tenantId: grant.tenantId, sessionUuid: grant.sessionUuid, patId: grant.patId,
        expiresAtMs: grant.expiresAtMs, leaseId, providerMayExist: false, stashWiped: false, revoked: false };
      await this.storage.put(DEVENV_CREDENTIAL_KEY, { ...this.handle });
    });
  }

  issue(payload: AuthorizedDevenvStart): Promise<Record<string, string>> {
    return this.run(async () => {
      const { grant } = payload;
      const handle = await this.read();
      if (!handle || handle.sessionUuid !== grant.sessionUuid || handle.stashWiped) throw new Error("DEVENV_CREDENTIAL_CLEANUP_PENDING");
      const ttlMs = Math.min(grant.expiresAtMs - Date.now(), MAX_DEVENV_SESSION_MS);
      if (ttlMs <= 0) throw new Error("DEVENV_INVALID_GRANT_EXPIRY");
      const ticket = await bounded(this.env.CRED_STASH.get(this.env.CRED_STASH.idFromName(handle.leaseId)).stash(randomTicket(), {
        token: grant.casPat, tenant: grant.tenantId, endpoint: "https://corelink-api.humangr.com",
      }, ttlMs, grant.expiresAtMs));
      if (typeof ticket !== "string" || !/^[0-9a-f]{64}$/.test(ticket)) throw new Error("DEVENV_INVALID_STASH_TICKET");
      return { CLW_CRED_TICKET: ticket, CLW_LEASE_ID: handle.leaseId,
        CLW_FABRIC_ENDPOINT: trustedHttpsBase(this.env.SPAWN_WORKER_PUBLIC_URL) };
    });
  }

  markProviderPossible(): Promise<void> {
    return this.run(async () => {
      const handle = await this.read();
      if (!handle || handle.expiresAtMs <= Date.now()) throw new Error("DEVENV_INVALID_GRANT_EXPIRY");
      handle.providerMayExist = true;
      await this.storage.put(DEVENV_CREDENTIAL_KEY, { ...handle });
    });
  }

  async recoverTerminal(destroy: () => Promise<void>, completeStopped: () => Promise<void>): Promise<void> {
    const handle = await this.current();
    if (handle?.providerMayExist) {
      try { await destroy(); } catch {
        await this.cleanup(false);
        throw new Error("DEVENV_CREDENTIAL_CLEANUP_PENDING");
      }
      let cleaned = false;
      try { await completeStopped(); }
      finally { cleaned = await this.cleanup(true); }
      if (!cleaned) throw new Error("DEVENV_CREDENTIAL_CLEANUP_PENDING");
    } else if (!await this.cleanup(true)) throw new Error("DEVENV_CREDENTIAL_CLEANUP_PENDING");
  }

  async expire(sessionUuid: string, destroy: () => Promise<void>, completeStopped: () => Promise<void>): Promise<boolean | undefined> {
    const handle = await this.current();
    if (!handle || handle.sessionUuid !== sessionUuid) return true;
    let stopped = false;
    let cleaned = false;
    try {
      if (handle.providerMayExist) {
        await destroy(); stopped = true;
        await completeStopped();
      } else {
        stopped = true;
      }
    } finally { cleaned = await this.cleanup(stopped); }
    return cleaned;
  }

  /** Persist a bounded-backoff recovery deadline before asking the SDK to schedule it. */
  planCleanupRetry(sessionUuid: string, now = Date.now()): Promise<number | undefined> {
    return this.run(async () => {
      const handle = await this.read();
      if (!handle || handle.sessionUuid !== sessionUuid) return undefined;
      if (!handle.cleanupPending && !handle.providerMayExist) handle.cleanupPending = true;
      if (!handle.cleanupPending) return undefined;
      if (Number.isSafeInteger(handle.cleanupRetryAtMs) && (handle.cleanupRetryAtMs as number) > now) {
        return handle.cleanupRetryAtMs;
      }
      const attempt = Math.min((handle.cleanupRetryAttempt ?? 0) + 1, 10);
      const delayMs = Math.min(CLEANUP_RETRY_BASE_MS * 2 ** (attempt - 1), CLEANUP_RETRY_MAX_MS);
      handle.cleanupRetryAttempt = attempt;
      handle.cleanupRetryAtMs = now + delayMs;
      await this.storage.put(DEVENV_CREDENTIAL_KEY, { ...handle });
      return handle.cleanupRetryAtMs;
    });
  }

  cleanup(providerStopped: boolean): Promise<boolean> {
    return this.run(async () => {
      const handle = await this.read();
      if (!handle) return true;
      if (providerStopped) handle.providerMayExist = false;
      // Independent effects remain retryable if either fails. Never log upstream errors.
      await Promise.all([
        (async () => {
          if (!handle.stashWiped) {
            try {
              await bounded(this.env.CRED_STASH.get(this.env.CRED_STASH.idFromName(handle.leaseId)).wipe(handle.expiresAtMs));
              handle.stashWiped = true;
            } catch { /* retain cleanup ownership */ }
          }
        })(),
        (async () => {
          if (!handle.revoked) {
            try {
              await bounded(revokeCasPatById(this.env, handle.patId, handle.tenantId, AbortSignal.timeout(CLEANUP_TIMEOUT_MS)));
              handle.revoked = true;
            } catch { /* retain cleanup ownership */ }
          }
        })(),
      ]);
      if (handle.providerMayExist || !handle.stashWiped || !handle.revoked) {
        handle.cleanupPending = !handle.providerMayExist;
        await this.storage.put(DEVENV_CREDENTIAL_KEY, { ...handle });
        return false;
      }
      // Keep a retryable durable marker until deletion itself is confirmed.
      handle.cleanupPending = true;
      await this.storage.put(DEVENV_CREDENTIAL_KEY, { ...handle });
      await this.storage.delete(DEVENV_CREDENTIAL_KEY);
      this.handle = undefined;
      return true;
    }).catch(() => false);
  }
}


interface DevenvLaunchHost {
  env: any;
  credentials: DevenvCredentials;
  execToken: string;
  getState(): DevenvState;
  transition(state: DevenvState): Promise<void>;
  settle(): Promise<{ outcome: string }>;
  completeStopped(): Promise<void>;
  start(envVars: Record<string, string>): Promise<void>;
  destroy(): Promise<void>;
  schedule(when: Date, callback: string, payload: { sessionUuid: string }): Promise<unknown>;
  scheduleCleanupRetry(sessionUuid: string): Promise<void>;
  noteActivity(): void;
}

/** Invoked under the DevEnv DO's input gate; the HTTP router never reaches this function. */
export async function launchAuthorizedDevenv(host: DevenvLaunchHost, payload: AuthorizedDevenvStart): Promise<AuthorizedDevenvAck> {
  validateAuthorizedDevenvStart(payload, host.env, Date.now());
  let state = host.getState();
  if (state.status !== "stopped" && state.status !== "errored") throw new Error("DEVENV_START_REQUIRES_TERMINAL_STATE");
  await host.credentials.recoverTerminal(host.destroy, host.completeStopped);
  const settlement = await host.settle();
  if (settlement.outcome === "pending" || settlement.outcome === "invalid") throw new Error("DEVENV_BILLING_PENDING");
  const { config, grant } = payload;
  state = host.getState();
  if ((state.status === "stopped" || state.status === "errored") && state.terminalUsage?.sessionId === grant.sessionUuid) {
    throw new Error("DEVENV_SESSION_REPLAY");
  }
  const generationId = (state.generationId ?? 0) + 1;
  try {
    await host.credentials.reserve(payload);
    // Persist the deadline before even sending the raw PAT to the stash RPC.
    await host.schedule(new Date(grant.expiresAtMs), "expireAuthorizedSession", { sessionUuid: grant.sessionUuid });
    const brokerEnv = await host.credentials.issue(payload);
    const envVars = {
      CLW_REF_DOMAIN: "runner", CLW_ENDPOINT: "https://corelink-api.humangr.com", ...brokerEnv,
      CLW_TENANT: grant.tenantId, WORKSPACE_NAME: config.workspaceName, PROFILE_NAME: config.profileName,
      EXEC_SERVER_AUTH_TOKEN_FILE, EXEC_SERVER_AUTH_TOKEN: host.execToken,
      SESSION_UUID: grant.sessionUuid, BILLING_TENANT_UUID: grant.tenantId,
      DEVENV_GENERATION_ID: String(generationId),
    };
    await host.credentials.markProviderPossible();
    await host.transition({
      status: "starting", createdAt: state.createdAt, startedAt: Date.now(),
      sessionUuid: grant.sessionUuid, tenantId: grant.tenantId, billingSeq: 0, generationId,
      workspaceName: config.workspaceName, profileName: config.profileName, tier: config.tier ?? "standard-4",
    });
    if (grant.expiresAtMs <= Date.now()) throw new Error("DEVENV_INVALID_GRANT_EXPIRY");
    await host.start(envVars);
    state = host.getState();
    if ((state.status !== "starting" && state.status !== "running") || state.sessionUuid !== grant.sessionUuid) {
      throw new Error("DEVENV_START_NOT_ACKNOWLEDGED");
    }
    host.noteActivity();
    return { sessionUuid: grant.sessionUuid, status: state.status };
  } catch {
    const handle = await host.credentials.current();
    let stopped = !handle?.providerMayExist;
    if (!stopped) {
      try { await host.destroy(); stopped = true; } catch { /* retain provider ownership */ }
    }
    const failedState = host.getState();
    try {
      if (stopped) await host.completeStopped();
      else if (failedState.status === "starting" || failedState.status === "running") {
        await host.transition({ ...failedState, status: "stopping" });
      }
    } finally {
      if (!await host.credentials.cleanup(stopped)) await host.scheduleCleanupRetry(grant.sessionUuid);
    }
    throw new Error("DEVENV_AUTHORIZED_START_FAILED");
  }
}
