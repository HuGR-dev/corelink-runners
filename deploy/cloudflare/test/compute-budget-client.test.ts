import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { ComputeBudgetClient, ComputeBudgetClientError } from "../src/lib/compute_budget_client";

const ID = "123e4567-e89b-12d3-a456-426614174000";
const TOKEN = "grant.example";
const DIGEST = "a".repeat(64);

let terminalKeys: CryptoKeyPair;
let terminalPublicKey = "";
const terminalConfig = { terminalAuthority: "fabric_compute", terminalPublicKey: "", receiptVersion: "t9-w1-terminal-v2", terminalKeyId: "terminal-key" };
function base64url(bytes: ArrayBuffer): string { return btoa(String.fromCharCode(...new Uint8Array(bytes))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""); }
beforeAll(async () => {
  terminalKeys = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  terminalPublicKey = base64url(await crypto.subtle.exportKey("raw", terminalKeys.publicKey));
  terminalConfig.terminalPublicKey = terminalPublicKey;
});
function client(fetcher: typeof fetch, configured = false) { return new ComputeBudgetClient("https://fabric.example", fetcher, configured ? terminalConfig : undefined); }
async function ok(state: string, id = ID) {
  const terminal = state === "cancelled" || state === "settled";
  if (!terminal) return new Response(JSON.stringify({ reservation_id: id, state }), { status: 200 });
  const unsigned = { receipt_version: "t9-w1-terminal-v2", reservation_id: id, tenant_id: "223e4567-e89b-12d3-a456-426614174000", grant_digest: DIGEST, generation: "g-test", state, materialized: state === "settled", actual_vcpu_ms: state === "settled" ? "1" : "0", evidence_digest: DIGEST, future_materialization_fence: "b".repeat(64), authority: "fabric_compute", key_id: "terminal-key", alg: "Ed25519", signed_at_ms: 1, expires_at_ms: 2 };
  const signature = await crypto.subtle.sign("Ed25519", terminalKeys.privateKey, new TextEncoder().encode(JSON.stringify(unsigned)));
  return new Response(JSON.stringify({ ...unsigned, signature: base64url(signature) }), { status: 200 });
}
function calls(fetcher: ReturnType<typeof vi.fn>) { return fetcher.mock.calls[0]![1] as RequestInit; }

describe("ComputeBudgetClient", () => {
  afterEach(() => vi.useRealTimers());

  it("requires an HTTPS origin and rejects endpoint injection", async () => {
    expect(() => new ComputeBudgetClient("http://fabric.example")).toThrow(ComputeBudgetClientError);
    expect(() => new ComputeBudgetClient("https://fabric.example/path")).toThrow(ComputeBudgetClientError);
    expect(() => new ComputeBudgetClient("https://user:pass@fabric.example")).toThrow(ComputeBudgetClientError);
    expect(() => new ComputeBudgetClient("https://fabric.example?x=1")).toThrow(ComputeBudgetClientError);
    const fetcher = vi.fn(async () => ok("prepared"));
    await new ComputeBudgetClient("https://fabric.example", fetcher).reserve(TOKEN, ID);
    expect((fetcher.mock.calls[0]![1] as RequestInit).redirect).toBe("error");
  });

  it("uses fixed paths, exact grant auth, and exact settle fields", async () => {
    const fetcher = vi.fn(async () => ok("prepared"));
    const c = client(fetcher, true);
    await expect(c.reserve(TOKEN, ID)).resolves.toEqual({ reservation_id: ID, state: "prepared" });
    expect(fetcher.mock.calls[0]![0]).toBe("https://fabric.example/internal/v1/compute/reserve");
    expect(calls(fetcher).body).toBe("{}");
    expect((calls(fetcher).headers as Record<string, string>).Authorization).toBe(`ComputeGrant ${TOKEN}`);

    fetcher.mockResolvedValueOnce(await ok("settled"));
    await expect(c.settle(TOKEN, ID, "1", DIGEST)).resolves.toMatchObject({ reservation_id: ID, state: "settled", materialized: true, actual_vcpu_ms: "1" });
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
    await expect(client(vi.fn(async () => ok("cancelled", "123e4567-e89b-12d3-a456-426614174001"))).cancel(TOKEN, ID)).rejects.toMatchObject({ code: "ambiguous" });
    await expect(client(vi.fn(async () => new Response(JSON.stringify({ reservation_id: ID, state: "active", extra: 1 })))).reserve(TOKEN, ID)).rejects.toMatchObject({ code: "ambiguous" });
  });

  it.each([
    ["state-only", { reservation_id: ID, state: "cancelled" }],
    ["missing evidence", { reservation_id: ID, state: "cancelled", materialized: false, actual_vcpu_ms: "0", future_materialization_fence: "b".repeat(64), terminal_authority: "fabric_compute", authority_signature: "c".repeat(86) }],
    ["wrong identity", { reservation_id: "123e4567-e89b-12d3-a456-426614174001", state: "cancelled", materialized: false, actual_vcpu_ms: "0", evidence_digest: DIGEST, future_materialization_fence: "b".repeat(64), terminal_authority: "fabric_compute", authority_signature: "c".repeat(86) }],
    ["future fence missing", { reservation_id: ID, state: "cancelled", materialized: false, actual_vcpu_ms: "0", evidence_digest: DIGEST, terminal_authority: "fabric_compute", authority_signature: "c".repeat(86) }],
  ] as const)("rejects unauthenticated terminal receipt: %s", async (_name, body) => {
    await expect(client(vi.fn(async () => new Response(JSON.stringify(body), { status: 200 }))).cancel(TOKEN, ID)).rejects.toMatchObject({ code: "ambiguous" });
  });

  it.each(["signature", "payload", "authority", "fence"] as const)("rejects a validly shaped receipt when %s is mutated", async kind => {
    const response = await ok("cancelled");
    const body = await response.json() as Record<string, unknown>;
    if (kind === "signature") body.signature = `${String(body.signature).slice(0, -1)}A`;
    if (kind === "payload") body.actual_vcpu_ms = "1";
    if (kind === "authority") body.authority = "other-authority";
    if (kind === "fence") body.future_materialization_fence = "c".repeat(64);
    const fetcher = vi.fn(async () => new Response(JSON.stringify(body), { status: 200 }));
    await expect(client(fetcher, true).cancel(TOKEN, ID)).rejects.toMatchObject({ code: "ambiguous" });
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
    const stream = new ReadableStream<Uint8Array>({ start(controller) { controller.enqueue(new Uint8Array(8192)); controller.enqueue(new Uint8Array(1)); }, cancel() { return new Promise(() => {}); } });
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
