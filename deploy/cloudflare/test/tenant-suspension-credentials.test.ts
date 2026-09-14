import { afterEach, describe, expect, it, vi } from "vitest";
import {
  consumeTenantSuspensionCredentials,
  type TenantSuspensionConsumerDependencies,
  type TenantSuspensionInput,
} from "../src/lib/tenant_suspension_credentials";
import type { CredentialIdentity } from "../src/lib/credential_authority_contract";
import { CredentialObligationAuthority } from "../src/lib/credential_obligation_authority";
import { TenantSuspensionAuthority } from "../src/lib/tenant_suspension_authority";

const INPUT: TenantSuspensionInput = {
  event_id: "suspend-event-1",
  tenant_id: "11111111-1111-4111-8111-111111111111",
  lifecycle_generation: "7",
};
const ENV = { CORELINK_MINT_URL: "https://server.example", CORELINK_RUNNER_MINT_AUTH_KEY: "runner-key" };

function identity(jobId: string, generation = "7"): CredentialIdentity {
  return { jobId, tenant: INPUT.tenant_id, patId: `pat-${jobId}`, lifecycleGeneration: generation };
}
function deps(overrides: Partial<TenantSuspensionConsumerDependencies> = {}): TenantSuspensionConsumerDependencies {
  return {
    authority: {
      beginTenantSuspension: vi.fn(async () => ({ complete: false, cursor: undefined })),
      checkpointTenantSuspension: vi.fn(async () => true),
      pendingCredentials: vi.fn(async () => ({ records: [], complete: true })),
    } as never,
    revokeCredential: vi.fn(async () => {}),
    ...overrides,
  };
}
function response(status: 200 | 202, input = INPUT, complete = status === 200, extra: Record<string, unknown> = {}, coverage: string | null = status === 200 ? "verified" : null) {
  const headers = new Headers({ "content-type": "application/json" }); if (coverage !== null) headers.set("x-corelink-legacy-coverage", coverage);
  return new Response(JSON.stringify({ ...input, complete, ...extra }), { status, headers });
}
function durableStorage() {
  const map = new Map<string, unknown>();
  const copy = <T>(value: T): T => value === undefined ? value : structuredClone(value);
  const base = {
    get: async <T>(key: string) => copy(map.get(key)) as T | undefined,
    put: async (key: string, value: unknown) => { map.set(key, copy(value)); },
    delete: async (key: string) => { map.delete(key); },
    list: async <T>(options: { prefix?: string; startAfter?: string; limit?: number }) => new Map(
      [...map.keys()].filter(key => key.startsWith(options.prefix ?? "") && (!options.startAfter || key > options.startAfter)).sort().slice(0, options.limit ?? Infinity).map(key => [key, copy(map.get(key)) as T]),
    ),
  };
  return {
    ...base,
    map,
    transaction: async <T>(fn: (s: typeof base) => Promise<T>) => {
      const snapshot = new Map([...map].map(([key, value]) => [key, copy(value)]));
      const tx = { ...base, get: async <V>(key: string) => copy(snapshot.get(key)) as V | undefined, put: async (key: string, value: unknown) => { snapshot.set(key, copy(value)); }, delete: async (key: string) => { snapshot.delete(key); }, list: async <V>(options: { prefix?: string; startAfter?: string; limit?: number }) => new Map([...snapshot.keys()].filter(key => key.startsWith(options.prefix ?? "") && (!options.startAfter || key > options.startAfter)).sort().slice(0, options.limit ?? Infinity).map(key => [key, copy(snapshot.get(key)) as V])) };
      const result = await fn(tx); map.clear(); for (const [key, value] of snapshot) map.set(key, value); return result;
    },
  } as never;
}

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("tenant suspension credential consumer", () => {
  it("sends the exact generation triple with the runner key and closes a 200 page", async () => {
    const fetchSpy = vi.fn(async (url: URL | string, init?: RequestInit) => {
      expect(String(url)).toBe("https://server.example/internal/v1/runner/credentials/close-generation");
      expect(init?.redirect).toBe("error");
      expect(new Headers(init?.headers).get("x-corelink-internal-auth")).toBe("runner-key");
      expect(JSON.parse(String(init?.body))).toEqual(INPUT);
      return response(200);
    });
    vi.stubGlobal("fetch", fetchSpy);
    const d = deps();
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: true });
    expect(d.authority.pendingCredentials).toHaveBeenCalledWith({ kind: "tenant", tenant: INPUT.tenant_id, throughGeneration: "7" }, undefined);
  });

  it("keeps the producer pending on 202 and performs no remote revoke", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(202, INPUT, false)));
    const d = deps({ revokeCredential: vi.fn(async () => { throw new Error("must not run"); }) });
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: false });
    expect(d.authority.pendingCredentials).not.toHaveBeenCalled();
    expect(d.authority.checkpointTenantSuspension).not.toHaveBeenCalled();
  });

  it("replay-completes only after begin and fresh verified server coverage", async () => {
    const begin = vi.fn(async () => ({ complete: true, cursor: "done" }));
    const d = deps({ authority: { ...deps().authority, beginTenantSuspension: begin } as never });
    const fetchSpy = vi.fn(async () => response(200)); vi.stubGlobal("fetch", fetchSpy);
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: true });
    expect(begin).toHaveBeenCalledWith(INPUT); expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  it("retains the receipt cursor when a credential revoke fails", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200, INPUT, true, {}, null)));
    const d = deps({
      authority: { ...deps().authority, beginTenantSuspension: vi.fn(async () => ({ complete: false, cursor: "c1" })), pendingCredentials: vi.fn(async () => ({ records: [identity("job-1")], complete: true })) } as never,
      revokeCredential: vi.fn(async () => { throw new Error("revoke unavailable"); }),
    });
    await expect(consumeTenantSuspensionCredentials(ENV, INPUT, d)).rejects.toThrow("revoke unavailable");
    expect(d.authority.checkpointTenantSuspension).not.toHaveBeenCalled();
  });

  it("advances one bounded page only after every identity is confirmed", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200, INPUT, true, {}, null)));
    const revoke = vi.fn(async () => {});
    const pending = vi.fn(async () => ({ records: [identity("old")], cursor: "c2", complete: false }));
    const checkpoint = vi.fn(async () => true);
    const d = deps({ authority: { ...deps().authority, pendingCredentials: pending, checkpointTenantSuspension: checkpoint } as never, revokeCredential: revoke });
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: false });
    expect(revoke).toHaveBeenCalledWith(identity("old")); expect(checkpoint).toHaveBeenCalledWith(INPUT, undefined, "c2", false);
  });

  it("isolates generations and never selects a newer credential", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200)));
    const newer = identity("new", "8");
    const d = deps({ authority: { ...deps().authority, pendingCredentials: vi.fn(async () => ({ records: [newer], complete: true })) } as never });
    await expect(consumeTenantSuspensionCredentials(ENV, INPUT, d)).rejects.toThrow("generation mismatch");
    expect(d.revokeCredential).not.toHaveBeenCalled();
  });

  it("refuses unknown legacy coverage before marking the final page complete", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200, INPUT, true, {}, null)));
    const checkpoint = vi.fn(async () => true);
    const d = deps({ authority: { ...deps().authority, checkpointTenantSuspension: checkpoint } as never });
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: false });
    expect(checkpoint).not.toHaveBeenCalled();
  });

  it("revokes known credentials even when the server coverage header is missing", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200, INPUT, true, {}, null)));
    const revoke = vi.fn(async () => {});
    const d = deps({ authority: { ...deps().authority, pendingCredentials: vi.fn(async () => ({ records: [identity("known")], complete: true })) } as never, revokeCredential: revoke });
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: false });
    expect(revoke).toHaveBeenCalledWith(identity("known"));
  });

  it("finishes on the next request when unknown coverage becomes verified", async () => {
    const begin = vi.fn().mockResolvedValueOnce({ complete: false, cursor: undefined }).mockResolvedValueOnce({ complete: false, cursor: undefined });
    const fetchSpy = vi.fn().mockResolvedValueOnce(response(200, INPUT, true, {}, "unknown")).mockResolvedValueOnce(response(200));
    vi.stubGlobal("fetch", fetchSpy);
    const d = deps({ authority: { ...deps().authority, beginTenantSuspension: begin } as never });
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: false });
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: true });
  });

  it("does not acknowledge a completed local receipt when the fresh server proof is unavailable", async () => {
    const d = deps({ authority: { ...deps().authority, beginTenantSuspension: vi.fn(async () => ({ complete: true })) } as never });
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("server unavailable"); }));
    await expect(consumeTenantSuspensionCredentials(ENV, INPUT, d)).rejects.toThrow("server unavailable");
  });

  it("rejects strict response mismatches, auth fallback, redirects, and invalid input", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200, INPUT, false)));
    await expect(consumeTenantSuspensionCredentials(ENV, INPUT, deps())).rejects.toThrow("completion mismatch");
    await expect(consumeTenantSuspensionCredentials({ ...ENV, CORELINK_RUNNER_MINT_AUTH_KEY: undefined }, INPUT, deps())).rejects.toThrow("auth key");
    await expect(consumeTenantSuspensionCredentials(ENV, { ...INPUT, lifecycle_generation: "01" }, deps())).rejects.toThrow("invalid tenant suspension input");
    await expect(consumeTenantSuspensionCredentials(ENV, { ...INPUT, tenant_id: "00000000-0000-0000-0000-000000000000" }, deps())).rejects.toThrow("invalid tenant suspension input");
  });

  it("matches the paired close-generation validator for canonical identity", async () => {
    const fetchSpy = vi.fn(); vi.stubGlobal("fetch", fetchSpy);
    const begin = vi.fn(async () => ({ complete: false, cursor: undefined }));
    const authority = { ...deps().authority, beginTenantSuspension: begin } as never;
    const invalid = [
      { ...INPUT, tenant_id: INPUT.tenant_id.toUpperCase() },
      { ...INPUT, event_id: ` ${INPUT.event_id}` },
      { ...INPUT, event_id: "x".repeat(257) },
      { ...INPUT, event_id: "bad\u0001event" },
      // 128 astral characters are 512 UTF-8 bytes despite only 256 UTF-16 units.
      { ...INPUT, event_id: "💥".repeat(128) },
    ];
    for (const input of invalid) await expect(consumeTenantSuspensionCredentials(ENV, input, { authority, revokeCredential: vi.fn() })).rejects.toThrow("invalid tenant suspension input");
    expect(begin).not.toHaveBeenCalled();
    expect(fetchSpy).not.toHaveBeenCalled();

    const accepted = "x".repeat(256);
    vi.stubGlobal("fetch", vi.fn(async () => response(200, { ...INPUT, event_id: accepted })));
    await expect(consumeTenantSuspensionCredentials(ENV, { ...INPUT, event_id: accepted }, { authority, revokeCredential: vi.fn() })).resolves.toEqual({ complete: true });
    expect(begin).toHaveBeenCalledTimes(1);

    const acceptedAstral = "💥".repeat(64); // exactly 256 UTF-8 bytes
    vi.stubGlobal("fetch", vi.fn(async () => response(200, { ...INPUT, event_id: acceptedAstral })));
    await expect(consumeTenantSuspensionCredentials(ENV, { ...INPUT, event_id: acceptedAstral }, { authority, revokeCredential: vi.fn() })).resolves.toEqual({ complete: true });
    expect(begin).toHaveBeenCalledTimes(2);
  });

  it("bounds a streaming body without content-length and cancels the reader", async () => {
    let canceled = false;
    const stream = new ReadableStream<Uint8Array>({
      start(controller) { controller.enqueue(new Uint8Array(4096)); },
      pull(controller) { controller.enqueue(new Uint8Array(1)); },
      cancel() { canceled = true; },
    });
    vi.stubGlobal("fetch", vi.fn(async () => new Response(stream, { status: 200 })));
    await expect(consumeTenantSuspensionCredentials(ENV, INPUT, deps())).rejects.toThrow("too large");
    expect(canceled).toBe(true);
  });

  it("times out a hanging response body and keeps the receipt retryable", async () => {
    vi.useFakeTimers();
    const stream = new ReadableStream<Uint8Array>({ pull() { return new Promise<void>(() => {}); } });
    vi.stubGlobal("fetch", vi.fn(async () => new Response(stream, { status: 200 })));
    const run = consumeTenantSuspensionCredentials(ENV, INPUT, deps()).then(() => null, error => error);
    await vi.advanceTimersByTimeAsync(5_001);
    await expect(run).resolves.toMatchObject({ message: expect.stringMatching(/aborted|abort/i) });
    vi.useRealTimers();
  });

  it("rejects path, query, credentials, and non-HTTPS origins before fetch", async () => {
    const fetchSpy = vi.fn(); vi.stubGlobal("fetch", fetchSpy);
    for (const origin of ["https://server.example/path", "https://server.example/?x=1", "https://user:pass@server.example", "http://server.example"]) {
      await expect(consumeTenantSuspensionCredentials({ ...ENV, CORELINK_MINT_URL: origin }, INPUT, deps())).rejects.toThrow("bare HTTPS origin");
    }
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("uses the durable floor and catches a late old-generation registration", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200)));
    const storage = durableStorage();
    const eventAuthority = new TenantSuspensionAuthority(storage);
    const credentials = new CredentialObligationAuthority(storage);
    const old = identity("old");
    await credentials.registerCredential(old);
    let injected = false;
    const authority = {
      beginTenantSuspension: eventAuthority.begin.bind(eventAuthority),
      checkpointTenantSuspension: eventAuthority.checkpoint.bind(eventAuthority),
      pendingCredentials: async (...args: Parameters<typeof credentials.pendingCredentials>) => {
        if (!injected) { injected = true; try { await credentials.registerCredential(identity("late")); } catch {} }
        return credentials.pendingCredentials(...args);
      },
    } as never;
    const revoked: string[] = [];
    const result = await consumeTenantSuspensionCredentials(ENV, INPUT, {
      authority,
      revokeCredential: async credential => { await credentials.requestCredentialRevocation(credential); await credentials.confirmCredentialRevoked(credential); revoked.push(credential.jobId); },
    });
    expect(result).toEqual({ complete: true });
    expect(revoked.sort()).toEqual(["late", "old"]);
    expect(storage.map.get(`credential-tenant-floor:${INPUT.tenant_id}`)).toMatchObject({ revokedThrough: "7" });
    expect((await credentials.pendingCredentials({ kind: "tenant", tenant: INPUT.tenant_id, throughGeneration: "7" })).records).toHaveLength(0);
  });
});
