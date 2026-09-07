import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { ContainmentDO } from "../src/index";
import { cancelledEvidence, terminalConfig, terminalResponse } from "./compute-terminal-test-helpers";

const tenantId = "22222222-2222-4222-8222-222222222222";
const reservationId = "11111111-1111-4111-8111-111111111111";

function clone<T>(value: T): T { return value === undefined ? value : structuredClone(value); }

class Storage {
  readonly values = new Map<string, unknown>();
  async get<T>(key: string): Promise<T | undefined> { return clone(this.values.get(key) as T | undefined); }
  async put(key: string, value: unknown): Promise<void> { this.values.set(key, clone(value)); }
  async delete(key: string): Promise<void> { this.values.delete(key); }
  async list<T>(options: { prefix: string; limit: number; startAfter?: string }): Promise<Map<string, T>> {
    return new Map([...this.values].filter(([key]) => key.startsWith(options.prefix) && (!options.startAfter || key > options.startAfter)).sort(([a], [b]) => a.localeCompare(b)).slice(0, options.limit).map(([key, value]) => [key, clone(value) as T]));
  }
  async transaction<T>(fn: (storage: this) => Promise<T>): Promise<T> { return fn(this); }
}

function gate() {
  let tail = Promise.resolve();
  return async <T>(fn: () => Promise<T>): Promise<T> => {
    let release!: () => void;
    const wait = tail;
    tail = new Promise<void>(resolve => { release = resolve; });
    await wait;
    try { return await fn(); } finally { release(); }
  };
}

function token(now = Date.now(), id = reservationId, workloadId = "job-1") {
  const payload = { v: 1, key_id: "key", tenant_id: tenantId, workload_kind: "spawn_worker_runner", workload_id: workloadId, reservation_id: id, period_key: 202609, ceiling_vcpu_ms: "864000000", vcpu_count: 4, maximum_wall_ms: 28_800_000, issued_at_ms: now - 1_000, expires_at_ms: now + 60_000 };
  const encoded = btoa(JSON.stringify(payload)).replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_");
  return `${encoded}.signature`;
}

function binding(id = reservationId, workloadId = "job-1") {
  return { token: token(Date.now(), id, workloadId), reservationId: id, tenantId, workloadKind: "spawn_worker_runner" as const, workloadId, vcpuCount: 4, maximumWallMs: 28_800_000 };
}
const computeEnv = { FABRIC_COMPUTE_URL: "https://fabric.example", FABRIC_COMPUTE_TERMINAL_AUTHORITY: terminalConfig.terminalAuthority, FABRIC_COMPUTE_TERMINAL_PUBLIC_KEY: terminalConfig.terminalPublicKey, FABRIC_COMPUTE_TERMINAL_RECEIPT_VERSION: terminalConfig.receiptVersion, FABRIC_COMPUTE_TERMINAL_KEY_ID: terminalConfig.terminalKeyId };

function fixture(fetcher: typeof fetch) {
  const storage = new Storage();
  const containment = new ContainmentDO({ storage, blockConcurrencyWhile: gate() } as never, computeEnv as never);
  vi.stubGlobal("fetch", fetcher);
  return { storage, containment };
}

async function receipt(state: string, id = reservationId) {
  const terminal = state === "cancelled" || state === "settled";
  return terminal ? terminalResponse(state as "cancelled" | "settled", id) : new Response(JSON.stringify({ reservation_id: id, state }), { status: 200 });
}
async function receiptForRequest(state: string, init?: RequestInit) {
  const authorization = new Headers(init?.headers).get("authorization") ?? "";
  const encoded = authorization.slice("ComputeGrant ".length).split(".")[0] ?? "";
  const payload = JSON.parse(atob(encoded.replace(/-/g, "+").replace(/_/g, "/"))) as { reservation_id: string };
  return receipt(state, payload.reservation_id);
}

beforeEach(() => vi.spyOn(Date, "now").mockReturnValue(Date.parse("2026-09-06T12:00:00Z")));
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("compute budget admission integration", () => {
  it("prepares through reserve/activate and fences a provider claim", async () => {
    const fetcher = vi.fn(async (url: string) => receipt(url.endsWith("/reserve") ? "prepared" : "active"));
    const { containment } = fixture(fetcher);
    await containment.prepareCompute(binding());
    expect(fetcher).toHaveBeenCalledTimes(2);
    await containment.claimComputeProvider(reservationId, "job-1");
    await expect(containment.claimComputeProvider(reservationId, "job-1")).rejects.toThrow("claim refused");
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it.each([
    ["over compute", 429, undefined],
    ["missing baseline", 503, undefined],
    ["bad receipt", 200, "not-a-valid-state"],
  ])("refuses claim after %s", async (_name, status, state) => {
    const fetcher = vi.fn(async () => state ? receipt(state) : new Response("busy", { status }));
    const { containment, storage } = fixture(fetcher);
    await expect(containment.prepareCompute(binding())).rejects.toThrow();
    await expect(containment.claimComputeProvider(reservationId, "job-1")).rejects.toThrow("claim refused");
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect((await storage.get<{ phase: string }>(`compute:obligation:${reservationId}`))?.phase).toBe("preparing");
  });

  it("restarts against the same storage without duplicating reservation calls", async () => {
    const fetcher = vi.fn(async (url: string) => receipt(url.endsWith("/reserve") ? "prepared" : "active"));
    const first = fixture(fetcher);
    const b = binding();
    await first.containment.prepareCompute(b);
    await first.containment.claimComputeProvider(reservationId, "job-1");
    const restarted = new ContainmentDO({ storage: first.storage, blockConcurrencyWhile: gate() } as never, computeEnv as never);
    await expect(restarted.prepareCompute(b)).rejects.toThrow();
    expect(fetcher).toHaveBeenCalledTimes(2);
    await expect(restarted.claimComputeProvider(reservationId, "job-1")).rejects.toThrow("claim refused");
  });

  it("retains an activation with a lost HTTP reply until terminal provider proof", async () => {
    let remotelyActive = false;
    const fetcher = vi.fn(async (url: string) => {
      if (url.endsWith("/activate")) { remotelyActive = true; throw new Error("lost reply"); }
      if (url.endsWith("/cancel")) {
        expect(remotelyActive).toBe(true);
        return new Response("conflict", { status: 409 });
      }
      return receipt("prepared");
    });
    const first = fixture(fetcher);
    await expect(first.containment.prepareCompute(binding())).rejects.toThrow("ambiguous");
    const restarted = new ContainmentDO({ storage: first.storage, blockConcurrencyWhile: gate() } as never, computeEnv as never);
    await expect(restarted.abandonUnusedCompute(reservationId)).rejects.toThrow("compute request rejected");
    await expect(restarted.claimComputeProvider(reservationId, "job-1")).rejects.toThrow("claim refused");
    expect(await first.storage.get(`compute:obligation:${reservationId}`)).toMatchObject({ phase: "abandoning" });
  });

  it("keeps a lost activation acknowledgement recoverable without fabricating usage", async () => {
    let puts = 0;
    const fetcher = vi.fn(async (url: string) => url.endsWith("/cancel") ? new Response("conflict", { status: 409 }) : receipt(url.endsWith("/reserve") ? "prepared" : "active"));
    const first = fixture(fetcher);
    const originalPut = first.storage.put.bind(first.storage);
    first.storage.put = async (key, value) => { puts++; if (puts === 2) throw new Error("activation acknowledgement lost"); return originalPut(key, value); };
    await expect(first.containment.prepareCompute(binding())).rejects.toThrow("activation acknowledgement lost");
    expect((await first.storage.get<{ phase: string }>(`compute:obligation:${reservationId}`))?.phase).toBe("preparing");
    first.storage.put = originalPut;
    await expect(first.containment.abandonUnusedCompute(reservationId)).rejects.toThrow("compute request rejected");
    expect(fetcher.mock.calls.map(call => String(call[0])).filter(url => url.endsWith("/settle"))).toHaveLength(0);
    expect((await first.storage.get<{ phase: string }>(`compute:obligation:${reservationId}`))?.phase).toBe("abandoning");
  });

  it("retains a failed final-page cleanup retry across the scheduled sweep", async () => {
    let unavailable = true;
    const fetcher = vi.fn(async (url: string) => unavailable
      ? new Response("unavailable", { status: 503 })
      : receipt("cancelled"));
    const { containment, storage } = fixture(fetcher);
    const expiredBinding = { ...binding(), token: token(Date.now() - 120_000) };
    await storage.put(`compute:obligation:${reservationId}`, {
      binding: expiredBinding, phase: "preparing", deadlineMs: Date.now() - 60_000,
    });

    await containment.drainUnusedCompute();
    expect(await storage.get(`compute:obligation:${reservationId}`)).toMatchObject({ phase: "abandoning" });
    expect(storage.values.get("compute:drain-retry")).toBe(true);
    expect(storage.values.has("compute:drain-cursor")).toBe(false);

    unavailable = false;
    await containment.drainUnusedCompute();
    // The first failed attempt leaves an abandoning record; the next sweep
    // must finish it before retiring the retry marker.
    expect(await storage.get(`compute:obligation:${reservationId}`)).toMatchObject({ phase: "terminal" });
    expect(storage.values.has("compute:drain-retry")).toBe(false);
    expect(storage.values.has("compute:drain-state")).toBe(false);
    expect(await storage.get(`compute:obligation:${reservationId}`)).toMatchObject({
      phase: "terminal", terminalKind: "cancelled",
    });
  });

  it("recovers a cursor after a crash between durable state and mirror writes", async () => {
    const fetcher = vi.fn(async (url: string, init?: RequestInit) => receiptForRequest(url.endsWith("/cancel") ? "cancelled" : "active", init));
    const first = fixture(fetcher);
    for (let n = 1; n <= 26; n++) {
      const id = `11111111-1111-4111-8111-${String(n).padStart(12, "0")}`;
      await first.storage.put(`compute:obligation:${id}`, n === 26 ? {
        binding: { ...binding(id, id), token: token(Date.now() - 120_000, id, id) }, phase: "preparing", deadlineMs: Date.now() - 60_000,
      } : { binding: binding(id, id), phase: "terminal", deadlineMs: Date.now() + 60_000, ...(await cancelledEvidence(id)) });
    }
    const originalPut = first.storage.put.bind(first.storage);
    let failMirror = true;
    first.storage.put = async (key, value) => {
      if (key === "compute:drain-cursor" && failMirror) { failMirror = false; throw new Error("cursor mirror crash"); }
      return originalPut(key, value);
    };
    await expect(first.containment.drainUnusedCompute()).rejects.toThrow("cursor mirror crash");
    expect(first.storage.values.get("compute:drain-state")).toMatchObject({ retryRequired: false, cursor: expect.stringContaining("compute:obligation:") });
    first.storage.put = originalPut;

    const restarted = new ContainmentDO({ storage: first.storage, blockConcurrencyWhile: gate() } as never, computeEnv as never);
    await restarted.drainUnusedCompute();
    expect(await first.storage.get(`compute:obligation:11111111-1111-4111-8111-000000000026`)).toMatchObject({ phase: "terminal", terminalKind: "cancelled" });
    expect(first.storage.values.has("compute:drain-state")).toBe(false);
  });

  it("retains an earlier-page 503 through the tail page and retries it after restart", async () => {
    let unavailable = true;
    const failedId = "11111111-1111-4111-8111-000000000001";
    const fetcher = vi.fn(async (url: string, init?: RequestInit) => unavailable && url.endsWith("/cancel")
      ? new Response("unavailable", { status: 503 }) : receiptForRequest("cancelled", init));
    const first = fixture(fetcher);
    for (let n = 1; n <= 26; n++) {
      const id = `11111111-1111-4111-8111-${String(n).padStart(12, "0")}`;
      await first.storage.put(`compute:obligation:${id}`, n === 1 || n === 26 ? {
        binding: { ...binding(id, id), token: token(Date.now() - 120_000, id, id) }, phase: "preparing", deadlineMs: Date.now() - 60_000,
      } : { binding: binding(id, id), phase: "terminal", deadlineMs: Date.now() + 60_000, ...(await cancelledEvidence(id)) });
    }
    await first.containment.drainUnusedCompute();
    expect(first.storage.values.get("compute:drain-state")).toMatchObject({ retryRequired: true, cursor: expect.stringContaining("000000000025") });
    unavailable = false;
    const restarted = new ContainmentDO({ storage: first.storage, blockConcurrencyWhile: gate() } as never, computeEnv as never);
    await restarted.drainUnusedCompute();
    expect(await first.storage.get(`compute:obligation:11111111-1111-4111-8111-000000000026`)).toMatchObject({ phase: "terminal" });
    expect(first.storage.values.get("compute:drain-retry")).toBe(true);
    await restarted.drainUnusedCompute();
    expect(await first.storage.get(`compute:obligation:${failedId}`)).toMatchObject({ phase: "terminal", terminalKind: "cancelled" });
    await restarted.drainUnusedCompute();
    expect(first.storage.values.has("compute:drain-state")).toBe(false);
  });
});
