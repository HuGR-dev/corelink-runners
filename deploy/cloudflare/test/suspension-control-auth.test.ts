import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import worker from "../src/index";

const env = {
  CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-key",
  CLOUDFLARE_EXEC_AUTH_TOKEN: "exec-key",
  CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: "lifecycle-key",
};

function request(token?: string) {
  return new Request("https://worker.test/internal/v1/tenant-suspension", {
    method: "POST",
    headers: token ? { authorization: `Bearer ${token}` } : {},
    // An authenticated request reaches validation, but no revocation is needed
    // to prove this boundary. Missing authority makes an accidental drive fail.
    body: "{}",
  });
}

describe("tenant suspension lifecycle control boundary", () => {
  afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

  it.each([undefined, "spawn-key", "exec-key"])("rejects %s before touching credentials", async token => {
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    const response = await worker.fetch(request(token), env as never, {} as never);
    expect(response.status).toBe(401);
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("accepts only the current lifecycle key and invalidates its predecessor", async () => {
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    expect((await worker.fetch(request("lifecycle-key"), env as never, {} as never)).status).toBe(400);
    const rotated = { ...env, CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: "new-lifecycle-key" };
    expect((await worker.fetch(request("lifecycle-key"), rotated as never, {} as never)).status).toBe(401);
    expect((await worker.fetch(request("new-lifecycle-key"), rotated as never, {} as never)).status).toBe(400);
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("rejects missing or reused lifecycle configuration", async () => {
    for (const token of [undefined, "", "spawn-key", "exec-key"]) {
      const invalid = { ...env, CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: token };
      expect((await worker.fetch(request(token), invalid as never, {} as never)).status).toBe(401);
    }
  });
});
