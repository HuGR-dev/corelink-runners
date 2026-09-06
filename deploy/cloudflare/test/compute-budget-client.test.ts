import { afterEach, describe, expect, it, vi } from "vitest";
import { ComputeBudgetClient, ComputeBudgetClientError } from "../src/lib/compute_budget_client";

const ID = "123e4567-e89b-12d3-a456-426614174000";
const TOKEN = "grant.example";
const DIGEST = "a".repeat(64);

function client(fetcher: typeof fetch) { return new ComputeBudgetClient("https://fabric.example", fetcher); }
function ok(state: string, id = ID) { return new Response(JSON.stringify({ reservation_id: id, state }), { status: 200 }); }
function calls(fetcher: ReturnType<typeof vi.fn>) { return fetcher.mock.calls[0]![1] as RequestInit; }

describe("ComputeBudgetClient", () => {
  afterEach(() => vi.useRealTimers());

  it("requires an HTTPS origin and rejects endpoint injection", () => {
    expect(() => new ComputeBudgetClient("http://fabric.example")).toThrow(ComputeBudgetClientError);
    expect(() => new ComputeBudgetClient("https://fabric.example/path")).toThrow(ComputeBudgetClientError);
    expect(() => new ComputeBudgetClient("https://user:pass@fabric.example")).toThrow(ComputeBudgetClientError);
    expect(() => new ComputeBudgetClient("https://fabric.example?x=1")).toThrow(ComputeBudgetClientError);
    const fetcher = vi.fn(async () => ok("prepared"));
    void new ComputeBudgetClient("https://fabric.example", fetcher).reserve(TOKEN, ID);
    expect((fetcher.mock.calls[0]![1] as RequestInit).redirect).toBe("error");
  });

  it("uses fixed paths, exact grant auth, and exact settle fields", async () => {
    const fetcher = vi.fn(async () => ok("prepared"));
    const c = client(fetcher);
    await expect(c.reserve(TOKEN, ID)).resolves.toEqual({ reservation_id: ID, state: "prepared" });
    expect(fetcher.mock.calls[0]![0]).toBe("https://fabric.example/internal/v1/compute/reserve");
    expect(calls(fetcher).body).toBe("{}");
    expect((calls(fetcher).headers as Record<string, string>).Authorization).toBe(`ComputeGrant ${TOKEN}`);

    fetcher.mockResolvedValueOnce(ok("settled"));
    await c.settle(TOKEN, ID, "1", DIGEST);
    expect(fetcher.mock.calls[1]![0]).toBe("https://fabric.example/internal/v1/compute/settle");
    expect((fetcher.mock.calls[1]![1] as RequestInit).body).toBe(JSON.stringify({ actual_vcpu_ms: "1", terminal_evidence_digest: DIGEST }));
  });

  it("reuses the caller's exact reservation id without generating or changing it", async () => {
    const fetcher = vi.fn(async () => ok("prepared"));
    const c = client(fetcher);
    await c.reserve(TOKEN, ID);
    await c.reserve(TOKEN, ID);
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(fetcher.mock.calls[0]![0]).toBe(fetcher.mock.calls[1]![0]);
    const first = { ...(fetcher.mock.calls[0]![1] as RequestInit), signal: undefined };
    const second = { ...(fetcher.mock.calls[1]![1] as RequestInit), signal: undefined };
    expect(first).toEqual(second);
  });

  it("allows reserve prepared or active, but enforces operation states and identity", async () => {
    const fetcher = vi.fn(async () => ok("active"));
    await expect(client(fetcher).reserve(TOKEN, ID)).resolves.toEqual({ reservation_id: ID, state: "active" });
    await expect(client(vi.fn(async () => ok("prepared"))).activate(TOKEN, ID)).rejects.toMatchObject({ code: "ambiguous" });
    await expect(client(vi.fn(async () => ok("cancelled", "123e4567-e89b-12d3-a456-426614174001"))).cancel(TOKEN, ID)).rejects.toMatchObject({ code: "invalid" });
    await expect(client(vi.fn(async () => new Response(JSON.stringify({ reservation_id: ID, state: "active", extra: 1 })))).reserve(TOKEN, ID)).rejects.toMatchObject({ code: "ambiguous" });
  });

  it("maps HTTP outcomes without reflecting response secrets", async () => {
    for (const [status, code] of [[429, "over_compute"], [503, "baseline_or_unavailable"], [401, "unauthorized"], [409, "conflict"], [400, "invalid"]] as const) {
      const fetcher = vi.fn(async () => new Response("secret server detail", { status }));
      await expect(client(fetcher).reserve(TOKEN, ID)).rejects.toMatchObject({ code, message: "compute request rejected" });
    }
    for (const status of [201, 202]) {
      const fetcher = vi.fn(async () => new Response(JSON.stringify({ reservation_id: ID, state: "prepared" }), { status }));
      await expect(client(fetcher).reserve(TOKEN, ID)).rejects.toMatchObject({ code: "invalid" });
    }
  });

  it("rejects malformed inputs and never invents an id or retries", async () => {
    const fetcher = vi.fn(async () => ok("prepared"));
    const c = client(fetcher);
    await expect(c.reserve("bad\nsecret", ID)).rejects.toMatchObject({ code: "invalid" });
    await expect(c.reserve(TOKEN, "not-a-uuid")).rejects.toMatchObject({ code: "invalid" });
    await expect(c.settle(TOKEN, ID, "-1", DIGEST)).rejects.toMatchObject({ code: "invalid" });
    await expect(c.settle(TOKEN, ID, "0", "bad")).rejects.toMatchObject({ code: "invalid" });
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("bounds an oversized streamed response", async () => {
    const stream = new ReadableStream<Uint8Array>({ start(controller) { controller.enqueue(new Uint8Array(8192)); controller.enqueue(new Uint8Array(1)); } });
    await expect(client(vi.fn(async () => new Response(stream, { status: 200 }))).reserve(TOKEN, ID)).rejects.toMatchObject({ code: "ambiguous" });
  });

  it("times out a response body read, including after headers", async () => {
    vi.useFakeTimers();
    const stream = new ReadableStream<Uint8Array>({ pull() { return new Promise(() => {}); } });
    const fetcher = vi.fn(async () => new Response(stream, { status: 200 }));
    const promise = client(fetcher).reserve(TOKEN, ID);
    const assertion = expect(promise).rejects.toMatchObject({ code: "ambiguous" });
    await vi.advanceTimersByTimeAsync(5001);
    await assertion;
  });

  it("times out a fetcher that ignores AbortSignal", async () => {
    vi.useFakeTimers();
    const fetcher = vi.fn(() => new Promise<Response>(() => {}));
    const promise = client(fetcher).reserve(TOKEN, ID);
    const assertion = expect(promise).rejects.toMatchObject({ code: "ambiguous" });
    await vi.advanceTimersByTimeAsync(5001);
    await assertion;
  });
});
