import { describe, expect, it, vi } from "vitest";
import { runCycle, type Env } from "../src/index";

function memoryKv(): { kv: KVNamespace; values: Map<string, string> } {
  const values = new Map<string, string>();
  const kv = {
    async get(key: string): Promise<string | null> {
      return values.get(key) ?? null;
    },
    async put(key: string, value: string): Promise<void> {
      values.set(key, value);
    },
  } as unknown as KVNamespace;
  return { kv, values };
}

function jsonMemoryKv(): { kv: KVNamespace; values: Map<string, string> } {
  const values = new Map<string, string>();
  const kv = {
    async get(key: string, type?: string): Promise<unknown> {
      const raw = values.get(key) ?? null;
      return raw === null || type !== "json" ? raw : JSON.parse(raw);
    },
    async put(key: string, value: string): Promise<void> {
      values.set(key, value);
    },
  } as unknown as KVNamespace;
  return { kv, values };
}

describe("fabricd probe containment", () => {
  it("performs zero fabricd fetches, keeps spawn monitoring, and records SKIPPED", async () => {
    const { kv, values } = memoryKv();
    const fabricFetch = vi.fn(async (): Promise<Response> => {
      throw new Error("fabricd must not be called while probes are disabled");
    });
    const spawnFetch = vi.fn(async (): Promise<Response> =>
      new Response(JSON.stringify({ counters: { spawn_failed: 0 } }), { status: 200 }),
    );

    const env = {
      CANARY_KV: kv,
      FABRICD_SVC: { fetch: fabricFetch } as unknown as Fetcher,
      SPAWN_SVC: { fetch: spawnFetch } as unknown as Fetcher,
      FABRIC_PROBES_ENABLED: "0",
      METRICS_OBSERVABILITY_KEY: "test-only",
    } satisfies Env;

    const summary = await runCycle(env, Date.UTC(2026, 8, 1, 16, 30));

    expect(fabricFetch).not.toHaveBeenCalled();
    expect(spawnFetch).toHaveBeenCalledOnce();
    expect(summary).toContain("fabric=404 health=SKIPPED spawn=200");

    const snapshot = JSON.parse(values.get("snapshot:last") ?? "null") as {
      fabricHealth?: { reachable?: boolean; status?: number; skipped?: boolean };
    };
    expect(snapshot.fabricHealth).toEqual({ reachable: true, status: 0, skipped: true });
  });
});

describe("surface and state failure visibility", () => {
  it.each(["", "not-json", "null", "[]", "{}", '{"counters":{}}'])(
    "alerts on a malformed, empty, or non-object 200 body (%j)",
    async (body) => {
      const { kv, values } = jsonMemoryKv();
      const spawnFetch = vi.fn(async (): Promise<Response> => new Response(body, { status: 200 }));
      const env = {
        CANARY_KV: kv,
        SPAWN_SVC: { fetch: spawnFetch } as unknown as Fetcher,
        METRICS_OBSERVABILITY_KEY: "armed",
        FABRIC_PROBES_ENABLED: "0",
      } satisfies Env;

      const summary = await runCycle(env, Date.UTC(2026, 8, 1, 16, 30));

      expect(summary).toContain("triggered=1");
      expect(spawnFetch).toHaveBeenCalledOnce();
      expect(JSON.parse(values.get("snapshot:last") ?? "null").spawn.failure).toEqual({
        code: "invalid_body",
        detail: expect.any(String),
      });
    },
  );

  it("alerts when an armed metrics surface returns 404", async () => {
    const { kv } = jsonMemoryKv();
    const env = {
      CANARY_KV: kv,
      SPAWN_SVC: { fetch: vi.fn(async (): Promise<Response> => new Response("gone", { status: 404 })) } as unknown as Fetcher,
      METRICS_OBSERVABILITY_KEY: "armed",
      FABRIC_PROBES_ENABLED: "0",
    } satisfies Env;

    const summary = await runCycle(env, Date.UTC(2026, 8, 1, 16, 30));

    expect(summary).toContain("triggered=1");
  });

  it("does not call or alert an explicitly unarmed metrics surface", async () => {
    const { kv } = jsonMemoryKv();
    const spawnFetch = vi.fn(async (): Promise<Response> => new Response("unexpected", { status: 404 }));
    const env = {
      CANARY_KV: kv,
      SPAWN_SVC: { fetch: spawnFetch } as unknown as Fetcher,
      FABRIC_PROBES_ENABLED: "0",
    } satisfies Env;

    const summary = await runCycle(env, Date.UTC(2026, 8, 1, 16, 30));

    expect(spawnFetch).not.toHaveBeenCalled();
    expect(summary).toContain("triggered=0");
  });

  it("surfaces KV read failures and preserves the prior state by skipping writes", async () => {
    const puts: string[] = [];
    const kv = {
      async get(): Promise<null> {
        throw new Error("KV unavailable");
      },
      async put(key: string): Promise<void> {
        puts.push(key);
      },
    } as unknown as KVNamespace;
    const env = {
      CANARY_KV: kv,
      FABRIC_PROBES_ENABLED: "0",
    } satisfies Env;

    const summary = await runCycle(env, Date.UTC(2026, 8, 1, 16, 30));

    expect(summary).toContain("triggered=2");
    expect(puts).toEqual([]);
  });

  it("surfaces KV write failures instead of claiming state was persisted", async () => {
    const kv = {
      async get(): Promise<null> {
        return null;
      },
      async put(): Promise<void> {
        throw new Error("KV read-only");
      },
    } as unknown as KVNamespace;
    const env = {
      CANARY_KV: kv,
      FABRIC_PROBES_ENABLED: "0",
    } satisfies Env;

    const summary = await runCycle(env, Date.UTC(2026, 8, 1, 16, 30));

    expect(summary).toContain("triggered=2");
    expect(summary).toContain("alert(s)");
  });
});
