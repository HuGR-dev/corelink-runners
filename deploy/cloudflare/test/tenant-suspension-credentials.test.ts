import { afterEach, describe, expect, it, vi } from "vitest";
import {
  consumeTenantSuspensionCredentials,
  type TenantSuspensionConsumerDependencies,
  type TenantSuspensionInput,
} from "../src/lib/tenant_suspension_credentials";
import type { CredentialIdentity } from "../src/lib/credential_authority_contract";

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
    isLegacyCoverageComplete: vi.fn(async () => true),
    ...overrides,
  };
}
function response(status: 200 | 202, input = INPUT, complete = status === 200, extra: Record<string, unknown> = {}) {
  return new Response(JSON.stringify({ ...input, complete, ...extra }), { status, headers: { "content-type": "application/json" } });
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

  it("replay-completes only after begin and legacy coverage validate", async () => {
    const begin = vi.fn(async () => ({ complete: true, cursor: "done" }));
    const coverage = vi.fn(async () => true);
    const d = deps({ authority: { ...deps().authority, beginTenantSuspension: begin } as never, isLegacyCoverageComplete: coverage });
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("must not call producer after receipt completion"); }));
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: true });
    expect(begin).toHaveBeenCalledWith(INPUT); expect(coverage).toHaveBeenCalledWith(INPUT.tenant_id, "7");
  });

  it("retains the receipt cursor when a credential revoke fails", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200)));
    const d = deps({
      authority: { ...deps().authority, beginTenantSuspension: vi.fn(async () => ({ complete: false, cursor: "c1" })), pendingCredentials: vi.fn(async () => ({ records: [identity("job-1")], complete: true })) } as never,
      revokeCredential: vi.fn(async () => { throw new Error("revoke unavailable"); }),
    });
    await expect(consumeTenantSuspensionCredentials(ENV, INPUT, d)).rejects.toThrow("revoke unavailable");
    expect(d.authority.checkpointTenantSuspension).not.toHaveBeenCalled();
  });

  it("advances one bounded page only after every identity is confirmed", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200)));
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
    vi.stubGlobal("fetch", vi.fn(async () => response(200)));
    const checkpoint = vi.fn(async () => true);
    const d = deps({ authority: { ...deps().authority, checkpointTenantSuspension: checkpoint } as never, isLegacyCoverageComplete: vi.fn(async () => false) });
    expect(await consumeTenantSuspensionCredentials(ENV, INPUT, d)).toEqual({ complete: false });
    expect(checkpoint).not.toHaveBeenCalled();
  });

  it("rejects strict response mismatches, auth fallback, redirects, and invalid input", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => response(200, INPUT, false)));
    await expect(consumeTenantSuspensionCredentials(ENV, INPUT, deps())).rejects.toThrow("completion mismatch");
    await expect(consumeTenantSuspensionCredentials({ ...ENV, CORELINK_RUNNER_MINT_AUTH_KEY: undefined }, INPUT, deps())).rejects.toThrow("auth key");
    await expect(consumeTenantSuspensionCredentials(ENV, { ...INPUT, lifecycle_generation: "01" }, deps())).rejects.toThrow("invalid tenant suspension input");
    await expect(consumeTenantSuspensionCredentials(ENV, { ...INPUT, tenant_id: "00000000-0000-0000-0000-000000000000" }, deps())).rejects.toThrow("invalid tenant suspension input");
  });
});
