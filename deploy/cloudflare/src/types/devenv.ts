// deploy/cloudflare/src/types/devenv.ts

// ─── Constants ────────────────────────────────────────────────────────
export const DevenvStatus = {
  STOPPED: "stopped",
  STARTING: "starting",
  RUNNING: "running",
  STOPPING: "stopping",
  ERRORED: "errored",
} as const;

export type DevenvStatus = (typeof DevenvStatus)[keyof typeof DevenvStatus];

export const VALID_TRANSITIONS: Record<DevenvStatus, readonly DevenvStatus[]> = {
  stopped: ["starting"],
  starting: ["running", "errored", "stopped"],
  running: ["stopping", "errored"],
  stopping: ["stopped", "errored"],
  errored: ["starting", "stopped"],
};

export function validateStateTransition(current: DevenvStatus, next: DevenvStatus): void {
  const allowed = VALID_TRANSITIONS[current];
  if (!allowed || !allowed.includes(next)) {
    throw new Error(
      `INVALID_STATE_TRANSITION: cannot transition from '${current}' to '${next}' (allowed: ${allowed?.join(", ") ?? "none"})`
    );
  }
}

// ─── Hardware Tiers (Scale-to-Infinity) ──────────────────────────────
export const DEVENV_TIERS = {
  "standard-2": { vcpus: 2, memoryMb: 4096, label: "Standard (2 vCPU, 4 GB)" },
  "standard-4": { vcpus: 4, memoryMb: 8192, label: "Standard (4 vCPU, 8 GB)" }, // default
  "power-8":    { vcpus: 8, memoryMb: 16384, label: "Power (8 vCPU, 16 GB)" },
  "ultra-16":   { vcpus: 16, memoryMb: 32768, label: "Ultra (16 vCPU, 32 GB)" },
} as const;

export type DevenvTier = keyof typeof DEVENV_TIERS;

// ─── Lightweight Pure TS Validators (Zero Runtime Dependencies) ──────
export function validateWorkspaceName(name: string): string {
  if (typeof name !== "string" || name.length < 1 || name.length > 128) {
    throw new Error("workspace_name must be between 1 and 128 characters");
  }
  if (!/^[a-zA-Z0-9_-]+$/.test(name)) {
    throw new Error("workspace_name must be alphanumeric with _ or -");
  }
  return name;
}

export function validateProfileName(name: string): string {
  if (typeof name !== "string" || name.length < 1 || name.length > 128) {
    throw new Error("profile_name must be between 1 and 128 characters");
  }
  if (!/^[a-zA-Z0-9_-]+$/.test(name)) {
    throw new Error("profile_name must be alphanumeric with _ or -");
  }
  return name;
}

export function validateClwToken(token: string): string {
  if (typeof token !== "string" || !/^cl_[a-zA-Z0-9_]{16,}$/.test(token)) {
    throw new Error("clw_token must be a valid CoreLink PAT");
  }
  return token;
}

export function validateTenantId(id: string): string {
  if (typeof id !== "string" || !/^[a-z0-9-]{8,}$/.test(id)) {
    throw new Error("tenant_id must be 8+ lowercase alphanumeric chars");
  }
  return id;
}

// ─── Discriminated Union State (impossible states are unrepresentable) ───
export type DevenvState =
  | {
      readonly status: "stopped";
      readonly createdAt: number;
      readonly generationId?: number;
    }
  | {
      readonly status: "starting";
      readonly createdAt: number;
      readonly startedAt: number;
      readonly sessionUuid: string;
      readonly billingSeq: number;
      readonly generationId: number;
      readonly workspaceName: string;
      readonly profileName: string;
      readonly tier?: DevenvTier;
    }
  | {
      readonly status: "running";
      readonly createdAt: number;
      readonly startedAt: number;
      readonly sessionUuid: string;
      readonly billingSeq: number;
      readonly generationId: number;
      readonly workspaceName: string;
      readonly profileName: string;
      readonly tier?: DevenvTier;
      readonly containerHandle: string;
      readonly lastHealthCheckAt: number;
      readonly healthCheckFailures: number;
    }
  | {
      readonly status: "stopping";
      readonly createdAt: number;
      readonly startedAt: number;
      readonly sessionUuid: string;
      readonly billingSeq: number;
      readonly generationId: number;
      readonly workspaceName: string;
      readonly profileName: string;
      readonly tier?: DevenvTier;
    }
  | {
      readonly status: "errored";
      readonly createdAt: number;
      readonly lastError: string;
      readonly lastWorkspaceName: string;
      readonly generationId?: number;
      readonly tier?: DevenvTier;
    };

// ─── RPC Payloads ────────────────────────────────────────────────────
export interface StartPayload {
  readonly config: {
    readonly workspaceName: string;
    readonly profileName: string;
    readonly tier?: DevenvTier;
    readonly clwEndpoint: string;
    readonly clwTenant: string;
    readonly clwToken: string;
  };
}

export interface StatusResponse {
  readonly status: DevenvStatus;
  readonly workspaceName: string | null;
  readonly profileName: string | null;
  readonly tier: DevenvTier;
  readonly uptimeMs: number | null;
  readonly ports: readonly [6080, 7681, 8080];
  readonly containerHandle: string | null;
}

export interface SnapshotRequest {
  readonly force: boolean;
}

export interface SnapshotResponse {
  readonly ok: true;
  readonly profileSnapshot: { readonly root: string; readonly bytesTotal: number };
  readonly workspaceSnapshot: { readonly root: string; readonly bytesTotal: number };
}

export interface ResizeRequest {
  readonly width: number;
  readonly height: number;
}

export interface HealthCheckResponse {
  readonly status: "healthy" | "unhealthy";
  readonly ports: Array<{ readonly port: number; readonly healthy: boolean }>;
  readonly wsConnections: number;
  readonly healthCheckFailures: number;
  readonly lastCheckAt: number;
}

export type StopResponse = { readonly ok: true };
export type StartResponse = StatusResponse;

export interface HydrateMetadata {
  readonly root: string | null;
  readonly bytesTotal: number;
  readonly bytesFromCache: number;
  readonly bytesDownloaded: number;
  readonly timestamp: number;
}

export interface SnapshotMetadata {
  readonly name: string;
  readonly root: string;
  readonly bytesTotal: number;
  readonly files: number;
  readonly chunksTotal: number;
  readonly chunksUploaded: number;
  readonly unchanged: boolean;
  readonly timestamp: number;
}

