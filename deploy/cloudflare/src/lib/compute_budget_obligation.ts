import { ComputeBudgetClientError, type ComputeReceipt } from "./compute_budget_client";

export interface ComputeBinding {
  token: string; reservationId: string; tenantId: string;
  workloadKind: "spawn_worker_runner" | "devenv"; workloadId: string;
  vcpuCount: number; maximumWallMs: number;
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
type Phase = "preparing" | "active" | "dispatched" | "abandoning" | "settling" | "terminal";
interface StoredObligation {
  binding: ComputeBinding; phase: Phase; deadlineMs: number;
  priorPhase?: "preparing" | "active"; actualVcpuMs?: string; evidenceDigest?: string;
  terminalKind?: "cancelled" | "settled";
}

export class ComputeObligations {
  constructor(private readonly storage: ComputeObligationStorage, private readonly client: ComputeTransport) {}
  private key(id: string): string { return `compute:obligation:${id}`; }

  private async read(id: string): Promise<StoredObligation | undefined> {
    const raw = await this.storage.get<unknown>(this.key(id));
    if (raw === undefined) return undefined;
    const row = this.validateStored(raw);
    if (row.binding.reservationId !== id) throw new Error("compute storage identity conflict");
    return row;
  }

  private validateStored(raw: unknown): StoredObligation {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) throw new Error("corrupt compute obligation");
    const row = raw as Record<string, unknown>;
    if (!row.binding || typeof row.phase !== "string" || !PHASES.has(row.phase as Phase) || !Number.isSafeInteger(row.deadlineMs)) throw new Error("corrupt compute obligation");
    const binding = row.binding as Partial<ComputeBinding>;
    if (typeof binding.token !== "string" || typeof binding.reservationId !== "string" || typeof binding.tenantId !== "string" || typeof binding.workloadKind !== "string" || typeof binding.workloadId !== "string" || typeof binding.vcpuCount !== "number" || typeof binding.maximumWallMs !== "number") throw new Error("corrupt compute obligation");
    const payload = this.parseToken(binding as ComputeBinding);
    if (row.deadlineMs !== payload.expires_at_ms) throw new Error("corrupt compute obligation");
    if (row.phase === "abandoning" && row.priorPhase !== "preparing" && row.priorPhase !== "active") throw new Error("corrupt compute obligation");
    if ((row.phase === "settling" || row.actualVcpuMs !== undefined || row.evidenceDigest !== undefined) && (typeof row.actualVcpuMs !== "string" || typeof row.evidenceDigest !== "string")) throw new Error("corrupt compute obligation");
    if (row.phase === "terminal" && row.terminalKind !== "cancelled" && row.terminalKind !== "settled") throw new Error("corrupt compute obligation");
    if (row.terminalKind === "settled" || row.actualVcpuMs !== undefined) {
      if (typeof row.actualVcpuMs !== "string" || !/^(0|[1-9][0-9]{0,18})$/.test(row.actualVcpuMs) ||
          BigInt(row.actualVcpuMs) > I64_MAX || typeof row.evidenceDigest !== "string" || !/^[0-9a-f]{64}$/i.test(row.evidenceDigest)) throw new Error("corrupt compute obligation");
    }
    return raw as StoredObligation;
  }

  private parseToken(binding: ComputeBinding): Record<string, unknown> {
    if (typeof binding.token !== "string" || new TextEncoder().encode(binding.token).length > 8192) throw new Error("invalid compute grant");
    const parts = binding.token.split(".");
    if (parts.length !== 2 || !/^[A-Za-z0-9_-]+$/.test(parts[0]) || !/^[A-Za-z0-9_-]+$/.test(parts[1])) throw new Error("invalid compute grant");
    let value: unknown;
    try { value = JSON.parse(new TextDecoder().decode(this.decode(parts[0]))); } catch { throw new Error("invalid compute grant"); }
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid compute grant");
    const payload = value as Record<string, unknown>;
    const fields = ["v", "key_id", "tenant_id", "workload_kind", "workload_id", "reservation_id", "period_key", "ceiling_vcpu_ms", "vcpu_count", "maximum_wall_ms", "issued_at_ms", "expires_at_ms"];
    if (Object.keys(payload).length !== fields.length || fields.some(field => !(field in payload))) throw new Error("invalid compute grant");
    if (payload.v !== 1 || payload.tenant_id !== binding.tenantId || payload.workload_kind !== binding.workloadKind || payload.workload_id !== binding.workloadId || payload.reservation_id !== binding.reservationId || payload.vcpu_count !== binding.vcpuCount || payload.maximum_wall_ms !== binding.maximumWallMs) throw new Error("compute binding mismatch");
    if (!UUID.test(binding.reservationId) || !UUID.test(binding.tenantId) || !/^(spawn_worker_runner|devenv)$/.test(binding.workloadKind) || !WORKLOAD_ID.test(binding.workloadId) || binding.workloadId.length > 256 || !Number.isSafeInteger(binding.vcpuCount) || binding.vcpuCount < 1 || binding.vcpuCount > 16 || !Number.isSafeInteger(binding.maximumWallMs) || binding.maximumWallMs < 1 || binding.maximumWallMs > 28_800_000) throw new Error("invalid compute binding");
    if (typeof payload.key_id !== "string" || !KEY_ID.test(payload.key_id) || payload.key_id.length > 64 || typeof payload.tenant_id !== "string" || typeof payload.period_key !== "number" || !Number.isSafeInteger(payload.period_key) || !validPeriod(payload.period_key) || typeof payload.ceiling_vcpu_ms !== "string" || !/^[1-9][0-9]{0,18}$/.test(payload.ceiling_vcpu_ms) || BigInt(payload.ceiling_vcpu_ms) > I64_MAX) throw new Error("invalid compute grant");
    if (typeof payload.issued_at_ms !== "number" || typeof payload.expires_at_ms !== "number" || !Number.isSafeInteger(payload.issued_at_ms) || !Number.isSafeInteger(payload.expires_at_ms) || payload.issued_at_ms < 0 || payload.expires_at_ms <= payload.issued_at_ms || payload.expires_at_ms - payload.issued_at_ms > 90_000) throw new Error("invalid compute grant");
    return payload;
  }

  private decode(value: string): Uint8Array {
    const padded = value.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - value.length % 4) % 4);
    const binary = atob(padded); return Uint8Array.from(binary, c => c.charCodeAt(0));
  }

  /** Persist ownership before a caller publishes its pointer or recovery alarm. */
  async stage(binding: ComputeBinding, nowMs: number): Promise<void> {
    const payload = this.parseToken(binding);
    const issuedAt = payload.issued_at_ms as number; const expiresAt = payload.expires_at_ms as number;
    if (!Number.isSafeInteger(nowMs) || nowMs < issuedAt || nowMs >= expiresAt) throw new Error("compute grant expired");
    const key = this.key(binding.reservationId);
    const existing = await this.read(binding.reservationId);
    if (existing) {
      if (!sameBinding(existing.binding, binding) || existing.deadlineMs !== expiresAt) throw new Error("compute obligation conflict");
      if (existing.phase === "active") return;
      if (existing.phase !== "preparing") throw new Error("compute obligation transition refused");
    } else await this.storage.put(key, { binding, phase: "preparing", deadlineMs: expiresAt });
  }

  async prepare(binding: ComputeBinding, nowMs: number): Promise<void> {
    await this.stage(binding, nowMs);
    const staged = await this.read(binding.reservationId);
    if (!staged) throw new Error("compute obligation missing after staging");
    if (staged.phase === "active") return;
    const reserved = await this.client.reserve(binding.token, binding.reservationId);
    if (reserved.reservation_id !== binding.reservationId || (reserved.state !== "prepared" && reserved.state !== "active")) throw new Error("invalid compute reserve receipt");
    const activated = reserved.state === "active" ? reserved : await this.client.activate(binding.token, binding.reservationId);
    if (activated.reservation_id !== binding.reservationId || activated.state !== "active") throw new Error("invalid compute activate receipt");
    await this.storage.put(this.key(binding.reservationId), { binding, phase: "active", deadlineMs: staged.deadlineMs });
  }

  async claimProvider(reservationId: string, workloadId: string, nowMs: number): Promise<void> {
    const row = await this.read(reservationId);
    if (!row || row.phase !== "active" || !Number.isSafeInteger(nowMs) || row.binding.workloadId !== workloadId) throw new Error("compute obligation claim refused");
    const payload = this.parseToken(row.binding);
    if (nowMs < (payload.issued_at_ms as number) || nowMs >= (payload.expires_at_ms as number) || row.deadlineMs <= nowMs) throw new Error("compute obligation claim refused");
    await this.storage.put(this.key(reservationId), { ...row, phase: "dispatched" });
  }

  async abandonUnused(reservationId: string): Promise<void> {
    const row = await this.read(reservationId);
    if (!row || row.phase === "dispatched" || row.phase === "settling") throw new Error("compute obligation abandonment refused");
    if (row.phase === "terminal") return;
    const prior = row.phase === "abandoning" ? row.priorPhase : row.phase;
    if (prior !== "preparing" && prior !== "active") throw new Error("compute obligation abandonment refused");
    if (row.phase !== "abandoning") await this.storage.put(this.key(reservationId), { ...row, phase: "abandoning", priorPhase: prior });
    let receipt: ComputeReceipt;
    let proof = row.actualVcpuMs && row.evidenceDigest ? { actualVcpuMs: row.actualVcpuMs, evidenceDigest: row.evidenceDigest } : undefined;
    try {
      if (proof) throw new ComputeBudgetClientError("conflict", "retry settlement");
      receipt = await this.client.cancel(row.binding.token, reservationId);
    } catch (error) {
      if (!(error instanceof ComputeBudgetClientError) || error.code !== "conflict") throw error;
      proof ??= { actualVcpuMs: "0", evidenceDigest: await this.neverDispatchedDigest(reservationId) };
      await this.storage.put(this.key(reservationId), { ...row, phase: "abandoning", priorPhase: prior, ...proof });
      receipt = await this.client.settle(row.binding.token, reservationId, proof.actualVcpuMs, proof.evidenceDigest);
    }
    if (receipt.reservation_id !== reservationId || (receipt.state !== "cancelled" && receipt.state !== "settled")) throw new Error("invalid compute cleanup receipt");
    await this.storage.put(this.key(reservationId), { ...row, phase: "terminal", terminalKind: receipt.state, priorPhase: prior, ...proof });
  }

  private async neverDispatchedDigest(reservationId: string): Promise<string> {
    const hash = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`corelink:compute:never-dispatched:${reservationId}`));
    return [...new Uint8Array(hash)].map(byte => byte.toString(16).padStart(2, "0")).join("");
  }

  async settleProven(reservationId: string, actualVcpuMs: string, evidenceDigest: string): Promise<void> {
    if (typeof actualVcpuMs !== "string" || actualVcpuMs.length > 19 || !/^(0|[1-9][0-9]*)$/.test(actualVcpuMs) || BigInt(actualVcpuMs) > I64_MAX || !/^[0-9a-f]{64}$/i.test(evidenceDigest)) throw new Error("invalid compute settlement");
    const row = await this.read(reservationId);
    if (!row) throw new Error("compute settlement refused");
    if (row.phase === "terminal") { if (row.actualVcpuMs === actualVcpuMs && row.evidenceDigest === evidenceDigest) return; throw new Error("compute settlement conflict"); }
    if (row.phase !== "dispatched" && row.phase !== "settling") throw new Error("compute settlement refused");
    if (row.phase === "settling" && (row.actualVcpuMs !== actualVcpuMs || row.evidenceDigest !== evidenceDigest)) throw new Error("compute settlement conflict");
    const next = { ...row, phase: "settling" as const, actualVcpuMs, evidenceDigest };
    if (row.phase !== "settling") await this.storage.put(this.key(reservationId), next);
    const receipt = await this.client.settle(row.binding.token, reservationId, actualVcpuMs, evidenceDigest);
    if (receipt.reservation_id !== reservationId || receipt.state !== "settled") throw new Error("invalid compute settlement receipt");
    await this.storage.put(this.key(reservationId), { ...next, phase: "terminal" as const, terminalKind: "settled" });
  }

  async drainUnused(nowMs: number, cursor?: string): Promise<{ cursor?: string; pending: boolean; retryRequired: boolean }> {
    if (!Number.isSafeInteger(nowMs) || nowMs < 0) throw new Error("invalid cleanup clock");
    const page = await this.storage.list<unknown>({ prefix: "compute:obligation:", limit: 25, ...(cursor ? { startAfter: cursor } : {}) });
    let processed = 0;
    let last: string | undefined;
    let pending = false;
    let remaining = false;
    for (const [key, raw] of page) {
      if (processed >= 2) { remaining = true; break; }
      last = key;
      let row: StoredObligation;
      try {
        row = this.validateStored(raw);
        if (key !== this.key(row.binding.reservationId)) throw new Error("compute storage identity conflict");
      } catch { pending = true; continue; }
      const eligible = row.phase === "preparing" || row.phase === "active" || row.phase === "abandoning";
      if (!eligible) continue;
      if (row.phase !== "abandoning" && row.deadlineMs > nowMs) { pending = true; continue; }
      processed++;
      try { await this.abandonUnused(row.binding.reservationId); } catch { pending = true; }
    }
    const continuation = (remaining || page.size === 25) && last ? last : undefined;
    return { ...(continuation ? { cursor: continuation } : {}), pending: pending || !!continuation, retryRequired: pending };
  }
}

function sameBinding(a: ComputeBinding, b: ComputeBinding): boolean {
  return a.token === b.token && a.reservationId === b.reservationId && a.tenantId === b.tenantId && a.workloadKind === b.workloadKind && a.workloadId === b.workloadId && a.vcpuCount === b.vcpuCount && a.maximumWallMs === b.maximumWallMs;
}
function validPeriod(period: number): boolean { const month = period % 100; return period >= 197001 && period <= 999912 && month >= 1 && month <= 12; }
const WORKLOAD_ID = /^[A-Za-z0-9:_./-]{1,256}$/;
const KEY_ID = /^[A-Za-z0-9_-]{1,64}$/;
const UUID = /^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const PHASES = new Set<Phase>(["preparing", "active", "dispatched", "abandoning", "settling", "terminal"]);
const I64_MAX = 9_223_372_036_854_775_807n;
