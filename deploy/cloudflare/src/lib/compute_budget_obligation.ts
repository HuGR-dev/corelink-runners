import type { ComputeReceipt } from "./compute_budget_client";

/** Trusted control-plane binding, never supplied by container/user HTTP data. */
export interface ComputeBinding {
  token: string;
  reservationId: string;
  tenantId: string;
  workloadKind: "spawn_worker_runner" | "devenv";
  workloadId: string;
  vcpuCount: number;
  maximumWallMs: number;
}

export interface ComputeObligationStorage {
  get<T>(key: string): Promise<T | undefined>;
  put(key: string, value: unknown): Promise<void>;
  list<T>(options: { prefix: string; limit: number; startAfter?: string }): Promise<Map<string, T>>;
}

export interface ComputeTransport {
  reserve(token: string, reservationId: string): Promise<ComputeReceipt>;
  activate(token: string, reservationId: string): Promise<ComputeReceipt>;
  cancel(token: string, reservationId: string): Promise<ComputeReceipt>;
  settle(token: string, reservationId: string, actualVcpuMs: string, evidence: string): Promise<ComputeReceipt>;
}

/** Caller serializes operations with its Durable Object input gate. */
export class ComputeObligations {
  constructor(private readonly storage: ComputeObligationStorage, private readonly client: ComputeTransport) {}

  async prepare(_binding: ComputeBinding, _nowMs: number): Promise<void> {
    throw new Error("COMPUTE_OBLIGATION_UNAVAILABLE");
  }
  async claimProvider(_reservationId: string, _workloadId: string, _nowMs: number): Promise<void> {
    throw new Error("COMPUTE_OBLIGATION_UNAVAILABLE");
  }
  async abandonUnused(_reservationId: string): Promise<void> {
    throw new Error("COMPUTE_OBLIGATION_UNAVAILABLE");
  }
  async settleProven(_reservationId: string, _actualVcpuMs: string, _evidenceDigest: string): Promise<void> {
    throw new Error("COMPUTE_OBLIGATION_UNAVAILABLE");
  }
  async drainUnused(_nowMs: number, _cursor?: string): Promise<{ cursor?: string }> {
    throw new Error("COMPUTE_OBLIGATION_UNAVAILABLE");
  }
}
