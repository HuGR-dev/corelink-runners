import { ComputeBudgetClientError, type ComputeReceipt } from "./compute_budget_client";

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

  private key(id: string): string { return `compute:obligation:${id}`; }
  private async read(id: string): Promise<Record<string, unknown> | undefined> { return this.storage.get(this.key(id)); }
  private token(binding: ComputeBinding): Record<string, unknown> {
    if (typeof binding.token !== "string" || new TextEncoder().encode(binding.token).length > 8192) throw new Error("invalid compute binding");
    const parts = binding.token.split(".");
    if (parts.length !== 2) throw new Error("invalid compute grant");
    let payload: unknown;
    try { payload = JSON.parse(new TextDecoder().decode(this.decode(parts[0]))); } catch { throw new Error("invalid compute grant"); }
    if (!payload || typeof payload !== "object" || Array.isArray(payload)) throw new Error("invalid compute grant");
    const value = payload as Record<string, unknown>;
    const expected = ["v", "key_id", "tenant_id", "workload_kind", "workload_id", "reservation_id", "period_key", "ceiling_vcpu_ms", "vcpu_count", "maximum_wall_ms", "issued_at_ms", "expires_at_ms"];
    if (Object.keys(value).length !== expected.length || expected.some(key => !(key in value))) throw new Error("invalid compute grant");
    if (value.v !== 1 || value.tenant_id !== binding.tenantId || value.workload_kind !== binding.workloadKind || value.workload_id !== binding.workloadId || value.reservation_id !== binding.reservationId || value.vcpu_count !== binding.vcpuCount || value.maximum_wall_ms !== binding.maximumWallMs) throw new Error("compute binding mismatch");
    if (!UUID.test(binding.reservationId) || (binding.workloadKind !== "spawn_worker_runner" && binding.workloadKind !== "devenv") || !Number.isSafeInteger(binding.vcpuCount) || binding.vcpuCount < 1 || binding.vcpuCount > 16 || !Number.isSafeInteger(binding.maximumWallMs) || binding.maximumWallMs < 1 || binding.maximumWallMs > 28_800_000) throw new Error("invalid compute binding");
    if (typeof value.key_id !== "string" || !value.key_id || typeof value.workload_id !== "string" || !value.workload_id || typeof value.period_key !== "string" || !/^\d{6}$/.test(value.period_key) || typeof value.ceiling_vcpu_ms !== "string" || !/^(0|[1-9][0-9]*)$/.test(value.ceiling_vcpu_ms)) throw new Error("invalid compute grant");
    if (typeof value.expires_at_ms !== "number" || typeof value.issued_at_ms !== "number" || !Number.isSafeInteger(value.expires_at_ms) || !Number.isSafeInteger(value.issued_at_ms) || value.issued_at_ms < 0 || value.expires_at_ms <= value.issued_at_ms || value.expires_at_ms - value.issued_at_ms > 90_000) throw new Error("invalid compute grant");
    if (typeof value.tenant_id !== "string" || value.tenant_id.trim() !== value.tenant_id || !value.tenant_id) throw new Error("invalid compute grant");
    return value;
  }
  private decode(value: string): Uint8Array { if (!/^[A-Za-z0-9_-]+$/.test(value)) throw new Error("invalid compute grant"); const padded = value.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - value.length % 4) % 4); const binary = atob(padded); return Uint8Array.from(binary, c => c.charCodeAt(0)); }

  async prepare(binding: ComputeBinding, nowMs: number): Promise<void> {
    const payload = this.token(binding);
    if (!Number.isSafeInteger(nowMs) || nowMs < 0 || nowMs >= (payload.expires_at_ms as number)) throw new Error("compute grant expired");
    const key = this.key(binding.reservationId);
    const existing = await this.storage.get<Record<string, unknown>>(key);
    if (existing) {
      if (!existing.binding || JSON.stringify(existing.binding) !== JSON.stringify(binding) || existing.deadlineMs !== payload.expires_at_ms) throw new Error("compute obligation conflict");
      if (existing.phase === "active") return;
      if (existing.phase !== "preparing") throw new Error("compute obligation transition refused");
    } else {
      await this.storage.put(key, { binding, phase: "preparing", deadlineMs: payload.expires_at_ms });
    }
    const reserved = await this.client.reserve(binding.token, binding.reservationId);
    if (reserved.reservation_id !== binding.reservationId || (reserved.state !== "prepared" && reserved.state !== "active")) throw new Error("invalid compute reserve receipt");
    const activated = reserved.state === "active" ? reserved : await this.client.activate(binding.token, binding.reservationId);
    if (activated.reservation_id !== binding.reservationId || activated.state !== "active") throw new Error("invalid compute activate receipt");
    await this.storage.put(key, { binding, phase: "active", deadlineMs: payload.expires_at_ms });
  }

  async claimProvider(reservationId: string, workloadId: string, nowMs: number): Promise<void> {
    const row = await this.read(reservationId);
    if (!row || row.phase !== "active" || typeof row.deadlineMs !== "number" || row.deadlineMs <= nowMs || !row.binding || (row.binding as ComputeBinding).workloadId !== workloadId) throw new Error("compute obligation claim refused");
    await this.storage.put(this.key(reservationId), { ...row, phase: "dispatched" });
  }

  async abandonUnused(reservationId: string): Promise<void> {
    const row = await this.read(reservationId);
    if (!row || row.phase === "dispatched" || row.phase === "settling") throw new Error("compute obligation abandonment refused");
    if (row.phase === "terminal") return;
    const prior = row.phase === "abandoning" ? row.priorPhase : row.phase;
    if (prior !== "preparing" && prior !== "active") throw new Error("compute obligation abandonment refused");
    if (row.phase !== "abandoning") await this.storage.put(this.key(reservationId), { ...row, phase: "abandoning", priorPhase: prior });
    const binding = row.binding as ComputeBinding;
    let receipt: ComputeReceipt;
    let settlement: { actualVcpuMs: string; evidenceDigest: string } | undefined;
    try { receipt = await this.client.cancel(binding.token, reservationId); }
    catch (error) {
      if (prior !== "active" || !(error instanceof ComputeBudgetClientError) || error.code !== "conflict") throw error;
      const evidenceDigest = await this.neverDispatchedDigest(reservationId);
      receipt = await this.client.settle(binding.token, reservationId, "0", evidenceDigest);
      settlement = { actualVcpuMs: "0", evidenceDigest };
    }
    if (receipt.reservation_id !== reservationId || (receipt.state !== "cancelled" && receipt.state !== "settled")) throw new Error("invalid compute cleanup receipt");
    await this.storage.put(this.key(reservationId), { ...row, phase: "terminal", priorPhase: prior, ...settlement });
  }

  private async neverDispatchedDigest(reservationId: string): Promise<string> {
    const data = new TextEncoder().encode(`corelink:compute:never-dispatched:${reservationId}`);
    const hash = await crypto.subtle.digest("SHA-256", data);
    return [...new Uint8Array(hash)].map(byte => byte.toString(16).padStart(2, "0")).join("");
  }

  async settleProven(reservationId: string, actualVcpuMs: string, evidenceDigest: string): Promise<void> {
    if (!/^(0|[1-9][0-9]*)$/.test(actualVcpuMs) || !/^[0-9a-f]{64}$/i.test(evidenceDigest)) throw new Error("invalid compute settlement");
    const row = await this.read(reservationId);
    if (!row) throw new Error("compute settlement refused");
    if (row.phase === "terminal") {
      if (row.actualVcpuMs === actualVcpuMs && row.evidenceDigest === evidenceDigest) return;
      throw new Error("compute settlement conflict");
    }
    if (row.phase !== "dispatched" && row.phase !== "settling") throw new Error("compute settlement refused");
    if (row.phase === "settling" && (row.actualVcpuMs !== actualVcpuMs || row.evidenceDigest !== evidenceDigest)) throw new Error("compute settlement conflict");
    const next = { ...row, phase: "settling", actualVcpuMs, evidenceDigest };
    if (row.phase !== "settling") await this.storage.put(this.key(reservationId), next);
    const receipt = await this.client.settle((row.binding as ComputeBinding).token, reservationId, actualVcpuMs, evidenceDigest);
    if (receipt.reservation_id !== reservationId || receipt.state !== "settled") throw new Error("invalid compute settlement receipt");
    await this.storage.put(this.key(reservationId), { ...next, phase: "terminal" });
  }

  async drainUnused(nowMs: number, cursor?: string): Promise<{ cursor?: string }> {
    const page = await this.storage.list<Record<string, unknown>>({ prefix: "compute:obligation:", limit: 25, ...(cursor ? { startAfter: cursor } : {}) });
    for (const [key, row] of page) {
      if ((row.phase === "preparing" || row.phase === "active" || row.phase === "abandoning") && typeof row.deadlineMs === "number" && row.deadlineMs <= nowMs) {
        try { await this.abandonUnused(key.slice("compute:obligation:".length)); } catch { /* retain for next bounded drain */ }
      }
    }
    return page.size === 25 ? { cursor: [...page.keys()].at(-1) } : {};
  }
}

const UUID = /^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
