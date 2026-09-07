import { describe, expect, it, vi } from "vitest";
import { ComputeBudgetClient, ComputeBudgetClientError, type AuthenticatedTerminalReceipt } from "../src/lib/compute_budget_client";
import { ComputeObligations, type ComputeBinding, type ComputeObligationStorage, type ComputeTransport } from "../src/lib/compute_budget_obligation";

const tenantId = "22222222-2222-4222-8222-222222222222";
const firstId = "11111111-1111-4111-8111-111111111111";
const digest = "a".repeat(64);
function terminal(state: "cancelled" | "settled", id = firstId, actualVcpuMs = state === "settled" ? "12" : "0"): AuthenticatedTerminalReceipt {
  return { reservation_id: id, state, materialized: state === "settled", actual_vcpu_ms: actualVcpuMs, evidence_digest: digest, future_materialization_fence: "b".repeat(64), terminal_authority: "fabric_compute", authority_signature: "c".repeat(86) };
}
function clone<T>(value: T): T { return value === undefined ? value : structuredClone(value); }
class Storage implements ComputeObligationStorage {
  readonly values = new Map<string, unknown>();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.values.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.values.set(key, clone(value)); }
  async list<T>(o: { prefix: string; limit: number; startAfter?: string }): Promise<Map<string, T>> { return new Map([...this.values].filter(([key]) => key.startsWith(o.prefix) && (!o.startAfter || key > o.startAfter)).sort(([a], [b]) => a.localeCompare(b)).slice(0, o.limit).map(([key, value]) => [key, clone(value) as T])); }
}
function makeToken(id: string, workloadId: string, now = 1_000_000) {
  const payload = { v: 1, key_id: "key", tenant_id: tenantId, workload_kind: "spawn_worker_runner", workload_id: workloadId, reservation_id: id, period_key: 202609, ceiling_vcpu_ms: "1000000", vcpu_count: 4, maximum_wall_ms: 28_800_000, issued_at_ms: now - 1_000, expires_at_ms: now + 60_000 };
  return `${btoa(JSON.stringify(payload)).replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_")}.signature`;
}
function binding(id = firstId, workloadId = "job-1"): ComputeBinding { return { token: makeToken(id, workloadId), reservationId: id, tenantId, workloadKind: "spawn_worker_runner", workloadId, vcpuCount: 4, maximumWallMs: 28_800_000 }; }
function client(fetcher: typeof fetch) { return new ComputeBudgetClient("https://fabric.example", fetcher); }
function ok(state: string, id = firstId) { return new Response(JSON.stringify({ reservation_id: id, state }), { status: 200 }); }

describe("compute obligation recovery integration", () => {
  it("writes preparing and abandoning fences before remote effects", async () => {
    const storage = new Storage();
    const fetcher = vi.fn(async () => ok("prepared"));
    const transport = client(fetcher);
    const blocked = new ComputeObligations({ get: storage.get.bind(storage), list: storage.list.bind(storage), put: async () => { throw new Error("storage unavailable"); } }, transport);
    await expect(blocked.prepare(binding(), 1_000_500)).rejects.toThrow("storage unavailable");
    expect(fetcher).not.toHaveBeenCalled();
    await storage.put(`compute:obligation:${firstId}`, { binding: binding(), phase: "preparing", deadlineMs: 1_060_000 });
    const abandoning = new ComputeObligations({ get: storage.get.bind(storage), list: storage.list.bind(storage), put: async () => { throw new Error("storage unavailable"); } }, transport);
    await expect(abandoning.abandonUnused(firstId)).rejects.toThrow("storage unavailable");
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("keeps terminal cancellation idempotent and accepts only the same settlement proof", async () => {
    const storage = new Storage();
    let settleCalls = 0;
    const fetcher = vi.fn(async (url: string) => url.endsWith("/reserve") ? ok("prepared") : url.endsWith("/activate") ? ok("active") : url.endsWith("/cancel") ? new Response("conflict", { status: 409 }) : (settleCalls++, new Response(JSON.stringify(terminal("settled", "11111111-1111-4111-8111-111111111112")), { status: 200 })));
    const obligations = new ComputeObligations(storage, client(fetcher));
    await obligations.prepare(binding(), 1_000_500);
    await expect(obligations.abandonUnused(firstId)).rejects.toThrow("compute request rejected");
    await expect(obligations.abandonUnused(firstId)).rejects.toThrow("compute request rejected");
    expect(settleCalls).toBe(0);
    const dispatched = new ComputeObligations(storage, client(fetcher));
    const secondId = "11111111-1111-4111-8111-111111111112";
    await storage.put(`compute:obligation:${secondId}`, { binding: binding(secondId, "job-2"), phase: "dispatched", deadlineMs: 1_060_000 });
    await dispatched.settleProven(secondId, terminal("settled", secondId));
    await dispatched.settleProven(secondId, terminal("settled", secondId));
    await expect(dispatched.settleProven(secondId, terminal("settled", secondId, "13"))).rejects.toThrow("conflict");
  });

  it("drains pages fairly with a two-effect budget and skips corrupt records", async () => {
    const storage = new Storage();
    const attempted: string[] = [];
    let cursor: string | undefined;
    const failures = new Set(["11111111-1111-4111-8111-000000000001", "11111111-1111-4111-8111-000000000002"]);
    const transport: ComputeTransport = {
      reserve: async (_t, id) => ({ reservation_id: id, state: "prepared" }), activate: async (_t, id) => ({ reservation_id: id, state: "active" }),
      settle: async (_t, id, actual) => terminal("settled", id, actual),
      cancel: async (_t, id) => { attempted.push(id); if (failures.has(id)) throw new ComputeBudgetClientError("baseline_or_unavailable", "temporary"); return terminal("cancelled", id); },
    };
    await storage.put("compute:obligation:00000000-bad", { broken: true });
    for (let n = 1; n <= 28; n++) {
      const id = `11111111-1111-4111-8111-${String(n).padStart(12, "0")}`;
      await storage.put(`compute:obligation:${id}`, { binding: binding(id, `job-${n}`), phase: "preparing", deadlineMs: 1_060_000 });
    }
    const obligations = new ComputeObligations(storage, transport);
    let pending = true;
    for (let page = 0; page < 20 && pending; page++) {
      const before = attempted.length;
      const result = await obligations.drainUnused(2_000_000, cursor);
      expect(attempted.length - before).toBeLessThanOrEqual(2);
      cursor = result.cursor;
      pending = result.pending;
    }
    expect(attempted).toContain("11111111-1111-4111-8111-000000000003");
    expect(attempted).toContain("11111111-1111-4111-8111-000000000028");
    expect(attempted).toContain("11111111-1111-4111-8111-000000000001");
    expect(await storage.get("compute:obligation:11111111-1111-4111-8111-000000000001")).toMatchObject({ phase: "abandoning" });
    expect(await storage.get("compute:obligation:11111111-1111-4111-8111-000000000028")).toMatchObject({ phase: "terminal", terminalKind: "cancelled" });
  });
});
