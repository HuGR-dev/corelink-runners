import { describe, expect, it } from "vitest";
import { ComputeObligations, type ComputeBinding, type ComputeObligationStorage, type ComputeTransport } from "../src/lib/compute_budget_obligation";
import { ComputeBudgetClientError } from "../src/lib/compute_budget_client";

const reservationId = "11111111-1111-4111-8111-111111111111";
function token(expires = 1_080_000) {
  const payload = { v: 1, key_id: "k", tenant_id: "tenant", workload_kind: "devenv", workload_id: "work", reservation_id: reservationId, period_key: "202609", ceiling_vcpu_ms: "1000", vcpu_count: 2, maximum_wall_ms: 1000, issued_at_ms: 1_000_000, expires_at_ms: expires };
  return `${btoa(JSON.stringify(payload)).replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_")}.sig`;
}
function binding(): ComputeBinding { return { token: token(), reservationId, tenantId: "tenant", workloadKind: "devenv", workloadId: "work", vcpuCount: 2, maximumWallMs: 1000 }; }
function setup() {
  const map = new Map<string, unknown>();
  const storage: ComputeObligationStorage = { get: async key => map.get(key), put: async (key, value) => map.set(key, value), list: async options => new Map([...map].filter(([key]) => key.startsWith(options.prefix) && (!options.startAfter || key > options.startAfter)).slice(0, options.limit)) };
  const calls: string[] = [];
  const client: ComputeTransport = { reserve: async (_token, id) => { calls.push("reserve"); return { reservation_id: id, state: "prepared" }; }, activate: async (_token, id) => { calls.push("activate"); return { reservation_id: id, state: "active" }; }, cancel: async (_token, id) => { calls.push("cancel"); return { reservation_id: id, state: "cancelled" }; }, settle: async (_token, id) => { calls.push("settle"); return { reservation_id: id, state: "settled" }; } };
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
    await s.obligations.settleProven(reservationId, "2000", "a".repeat(64));
    await s.obligations.settleProven(reservationId, "2000", "a".repeat(64));
  });

  it("cancels prepared work and retains abandoning state on remote failure", async () => {
    const s = setup(); const b = binding();
    await s.obligations.prepare(b, 1_050_000);
    const failing: ComputeTransport = { ...s.client, cancel: async () => { throw new ComputeBudgetClientError("baseline_or_unavailable", "down"); } };
    const retry = new ComputeObligations(s.storage, failing);
    await expect(retry.abandonUnused(reservationId)).rejects.toThrow();
    expect((s.map.get(`compute:obligation:${reservationId}`) as { phase: string }).phase).toBe("abandoning");
  });

  it("settles an active cancel conflict with a durable zero-use proof", async () => {
    const s = setup(); const b = binding();
    await s.obligations.prepare(b, 1_050_000);
    const conflict: ComputeTransport = {
      ...s.client,
      cancel: async () => { throw new ComputeBudgetClientError("conflict", "already active"); },
    };
    const cleanup = new ComputeObligations(s.storage, conflict);
    await cleanup.abandonUnused(reservationId);
    const row = s.map.get(`compute:obligation:${reservationId}`) as { phase: string; actualVcpuMs: string; evidenceDigest: string };
    expect(row.phase).toBe("terminal");
    expect(row.actualVcpuMs).toBe("0");
    expect(row.evidenceDigest).toMatch(/^[0-9a-f]{64}$/);
    expect(s.calls).toEqual(["reserve", "activate", "settle"]);
  });
});
