import { describe, expect, it } from "vitest";
import { ComputeObligations, type ComputeBinding, type ComputeObligationStorage, type ComputeTransport } from "../src/lib/compute_budget_obligation";
import { ComputeBudgetClientError, type AuthenticatedTerminalReceipt, type ComputeReceipt } from "../src/lib/compute_budget_client";

const reservationId = "11111111-1111-4111-8111-111111111111";
const tenantId = "22222222-2222-4222-8222-222222222222";
function token(expires = 1_080_000) {
  const payload = { v: 1, key_id: "k", tenant_id: tenantId, workload_kind: "devenv", workload_id: "work", reservation_id: reservationId, period_key: 202609, ceiling_vcpu_ms: "1000", vcpu_count: 2, maximum_wall_ms: 1000, issued_at_ms: 1_000_000, expires_at_ms: expires };
  return `${btoa(JSON.stringify(payload)).replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_")}.sig`;
}
function binding(): ComputeBinding { return { token: token(), reservationId, tenantId, workloadKind: "devenv", workloadId: "work", vcpuCount: 2, maximumWallMs: 1000 }; }
function terminal(state: "cancelled" | "settled", id = reservationId, actualVcpuMs = state === "settled" ? "2000" : "0"): AuthenticatedTerminalReceipt {
  return { reservation_id: id, state, materialized: state === "settled", actual_vcpu_ms: actualVcpuMs, evidence_digest: "a".repeat(64), future_materialization_fence: "b".repeat(64), terminal_authority: "fabric_compute", authority_signature: "c".repeat(86) };
}
function setup() {
  const map = new Map<string, unknown>();
  const storage: ComputeObligationStorage = { get: async key => map.get(key), put: async (key, value) => map.set(key, value), list: async options => new Map([...map].filter(([key]) => key.startsWith(options.prefix) && (!options.startAfter || key > options.startAfter)).slice(0, options.limit)) };
  const calls: string[] = [];
  const client: ComputeTransport = { reserve: async (_token, id) => { calls.push("reserve"); return { reservation_id: id, state: "prepared" }; }, activate: async (_token, id) => { calls.push("activate"); return { reservation_id: id, state: "active" }; }, cancel: async (_token, id) => { calls.push("cancel"); return terminal("cancelled", id); }, settle: async (_token, id, actual) => { calls.push("settle"); return terminal("settled", id, actual); } };
  return { map, calls, storage, client, obligations: new ComputeObligations(storage, client) };
}

describe("durable compute runtime obligations", () => {
  it("persists before reserve and survives replay without fresh IDs", async () => {
    const s = setup(); const b = binding();
    await s.obligations.prepare(b, 1_050_000);
    expect(s.calls).toEqual(["reserve", "activate"]);
    const restarted = new ComputeObligations(s.storage, s.client);
    await restarted.prepare(b, 1_050_001);
    expect(s.calls).toEqual(["reserve", "activate"]);
  });

  it("fences a late provider claim and settles only dispatched work", async () => {
    const s = setup(); const b = binding();
    await s.obligations.prepare(b, 1_050_000);
    await s.obligations.claimProvider(reservationId, "work", 1_050_001);
    await expect(s.obligations.claimProvider(reservationId, "work", 1_050_002)).rejects.toThrow();
    await s.obligations.settleProven(reservationId, terminal("settled"));
    await s.obligations.settleProven(reservationId, terminal("settled"));
  });

  it("cancels prepared work and retains abandoning state on remote failure", async () => {
    const s = setup(); const b = binding();
    await s.obligations.prepare(b, 1_050_000);
    const failing: ComputeTransport = { ...s.client, cancel: async () => { throw new ComputeBudgetClientError("baseline_or_unavailable", "down"); } };
    const retry = new ComputeObligations(s.storage, failing);
    await expect(retry.abandonUnused(reservationId)).rejects.toThrow();
    expect((s.map.get(`compute:obligation:${reservationId}`) as { phase: string }).phase).toBe("abandoning");
  });

  it("retains the obligation when cancel has no authenticated terminal fence", async () => {
    const s = setup(); const b = binding();
    await s.obligations.prepare(b, 1_050_000);
    const conflict: ComputeTransport = {
      ...s.client,
      cancel: async () => { throw new ComputeBudgetClientError("conflict", "already active"); },
    };
    const cleanup = new ComputeObligations(s.storage, conflict);
    await expect(cleanup.abandonUnused(reservationId)).rejects.toThrow("already active");
    const row = s.map.get(`compute:obligation:${reservationId}`) as { phase: string; providerReceipt?: unknown };
    expect(row.phase).toBe("abandoning");
    expect(row.providerReceipt).toBeUndefined();
    expect(s.calls).toEqual(["reserve", "activate"]);
  });

  it.each([
    ["state-only", { reservation_id: reservationId, state: "cancelled" }],
    ["wrong identity", { ...terminal("cancelled"), reservation_id: "11111111-1111-4111-8111-222222222222" }],
    ["missing evidence", (() => { const r = terminal("cancelled") as Record<string, unknown>; delete r.evidence_digest; return r; })()],
    ["missing future fence", (() => { const r = terminal("cancelled") as Record<string, unknown>; delete r.future_materialization_fence; return r; })()],
  ] as const)("keeps obligation pending for %s provider receipt", async (_name, invalidReceipt) => {
    const s = setup();
    const bad: ComputeTransport = { ...s.client, cancel: async () => invalidReceipt as ComputeReceipt };
    await new ComputeObligations(s.storage, bad).prepare(binding(), 1_050_000);
    await expect(new ComputeObligations(s.storage, bad).abandonUnused(reservationId)).rejects.toThrow();
    expect((s.map.get(`compute:obligation:${reservationId}`) as { phase: string; providerReceipt?: unknown }).phase).toBe("abandoning");
    expect((s.map.get(`compute:obligation:${reservationId}`) as { providerReceipt?: unknown }).providerReceipt).toBeUndefined();
  });

  it("does not call the remote when the durable preparing fence cannot be written", async () => {
    const s = setup(); const b = binding();
    const storage: ComputeObligationStorage = { ...s.storage, put: async () => { throw new Error("storage down"); } };
    await expect(new ComputeObligations(storage, s.client).prepare(b, 1_050_000)).rejects.toThrow("storage down");
    expect(s.calls).toEqual([]);
  });

  it("recovers a lost activation acknowledgement through conflict settlement", async () => {
    const s = setup(); const b = binding(); let puts = 0;
    const flaky: ComputeObligationStorage = { ...s.storage, put: async (key, value) => { puts++; if (puts === 2) throw new Error("lost write"); await s.storage.put(key, value); } };
    await expect(new ComputeObligations(flaky, s.client).prepare(b, 1_050_000)).rejects.toThrow("lost write");
    const conflict: ComputeTransport = { ...s.client, cancel: async () => { throw new ComputeBudgetClientError("conflict", "active"); } };
    await expect(new ComputeObligations(s.storage, conflict).abandonUnused(reservationId)).rejects.toThrow("active");
    expect((s.map.get(`compute:obligation:${reservationId}`) as { phase: string }).phase).toBe("abandoning");
    expect(s.calls).toEqual(["reserve", "activate"]);
  });
});
