import { describe, expect, it, vi } from "vitest";
import { runCycle, type Env } from "../src/index";

function memoryKv(): KVNamespace {
  const values = new Map<string, string>();
  return {
    async get(key: string): Promise<string | null> {
      return values.get(key) ?? null;
    },
    async put(key: string, value: string): Promise<void> {
      values.set(key, value);
    },
  } as unknown as KVNamespace;
}

describe("FABRIC_PROBES_ENABLED", () => {
  it.each([undefined, "", " ", "\t", "0", "malformed", "true", "yes", "2", "01", " 1 "]) (
    "fails closed for %j: zero fabricd fetches",
    async (flag) => {
      const fabricFetch = vi.fn(async (): Promise<Response> => {
        throw new Error("fabricd must not be called unless the flag is exactly 1");
      });
      const spawnFetch = vi.fn(async (): Promise<Response> =>
        new Response(JSON.stringify({ counters: { spawn_failed: 0 } }), { status: 200 }),
      );

      const env = {
        CANARY_KV: memoryKv(),
        FABRICD_SVC: { fetch: fabricFetch } as unknown as Fetcher,
        SPAWN_SVC: { fetch: spawnFetch } as unknown as Fetcher,
        FABRIC_OBSERVABILITY_KEY: "fabric-test-key",
        METRICS_OBSERVABILITY_KEY: "metrics-test-key",
        ...(flag === undefined ? {} : { FABRIC_PROBES_ENABLED: flag }),
      } satisfies Env;

      const summary = await runCycle(env, Date.UTC(2026, 8, 1, 16, 30));

      expect(fabricFetch).not.toHaveBeenCalled();
      expect(spawnFetch).toHaveBeenCalledOnce();
      expect(summary).toContain("health=SKIPPED");
    },
  );

  it("arms both legacy fabricd probes only for the exact string 1", async () => {
    const fabricFetch = vi.fn(async (input: RequestInfo | URL): Promise<Response> => {
      const url = String(input);
      if (url.endsWith("/v1/health")) return new Response("ok", { status: 200 });
      return new Response(JSON.stringify({ counters: {} }), { status: 200 });
    });
    const spawnFetch = vi.fn(async (): Promise<Response> =>
      new Response(JSON.stringify({ counters: { spawn_failed: 0 } }), { status: 200 }),
    );

    const env = {
      CANARY_KV: memoryKv(),
      FABRICD_SVC: { fetch: fabricFetch } as unknown as Fetcher,
      SPAWN_SVC: { fetch: spawnFetch } as unknown as Fetcher,
      FABRIC_PROBES_ENABLED: "1",
      FABRIC_OBSERVABILITY_KEY: "fabric-test-key",
      METRICS_OBSERVABILITY_KEY: "metrics-test-key",
    } satisfies Env;

    const summary = await runCycle(env, Date.UTC(2026, 8, 1, 16, 30));

    expect(fabricFetch).toHaveBeenCalledTimes(2);
    expect(spawnFetch).toHaveBeenCalledOnce();
    expect(summary).toContain("health=200");
  });
});
