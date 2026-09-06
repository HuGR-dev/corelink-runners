import { describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: () => ({}) }));
import { classifyMintForbidden } from "../src/lib";
import { COUNTER_NAMES } from "../src/metrics";
import { reapStaleSpawnClaims } from "../src/index";

function fakeKv(entries: Record<string, string>) {
  const store = new Map(Object.entries(entries));
  return {
    store,
    async get(key: string) { return store.get(key) ?? null; },
    async put(key: string, value: string) { store.set(key, value); },
    async delete(key: string) { store.delete(key); },
    async list({ prefix }: { prefix: string }) {
      return { keys: [...store.keys()].filter((key) => key.startsWith(prefix)).map((name) => ({ name })) };
    },
  };
}

describe("T3-W2 lifecycle contracts", () => {
  it("keeps edge proxy 403 distinct from application authz 403", () => {
    expect(classifyMintForbidden("<html><title>Cloudflare Access denied</title>")).toBe("edge_proxy");
    expect(classifyMintForbidden('{"error":"runner mint unauthorized"}')).toBe("authz");
  });

  it("registers every finite lifecycle counter", () => {
    expect(COUNTER_NAMES).toEqual(expect.arrayContaining([
      "job_stranded",
      "stale_spawn_claim_reaped",
      "spawn_cold_mint_key_unarmed",
      "spawn_cold_no_repo",
      "spawn_cold_no_installation_or_pat",
    ]));
  });

  it("reaps an old claim only when no durable handle exists", async () => {
    const now = 1_800_000_000_000;
    const kv = fakeKv({ "spawn:stale": String(now - 7_200_001), "spawn:live": String(now - 7_200_001), "jhandle:live": "h-live" });
    const n = await reapStaleSpawnClaims({ RUNNER_JOB_PATS: kv } as never, now);
    expect(n).toBe(1);
    expect(kv.store.has("spawn:stale")).toBe(false);
    expect(kv.store.has("spawn:live")).toBe(true);
  });
});
