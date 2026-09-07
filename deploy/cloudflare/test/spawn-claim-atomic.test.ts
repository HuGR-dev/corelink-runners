import { describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { ConcurrencySlotsDO } from "../src/index";

function storage() {
  const map = new Map<string, unknown>();
  let tail = Promise.resolve();
  return {
    map,
    async get<T>(key: string): Promise<T | undefined> { return map.get(key) as T | undefined; },
    async put(key: string, value: unknown): Promise<void> { map.set(key, JSON.parse(JSON.stringify(value))); },
    async delete(key: string): Promise<void> { map.delete(key); },
    async transaction<T>(fn: (tx: ReturnType<typeof storage>) => Promise<T>): Promise<T> {
      const run = tail.then(async () => fn(this));
      tail = run.then(() => undefined, () => undefined);
      return run;
    },
  };
}

function authority() {
  const s = storage();
  return { s, doInst: new ConcurrencySlotsDO({ storage: s } as never, {} as never) };
}

describe("spawn claim authority", () => {
  it("has one winner across 100 concurrent deliveries", async () => {
    const { doInst } = authority();
    const results = await Promise.all(Array.from({ length: 100 }, () => doInst.acquireSpawnClaim("job-1", 60_000)));
    expect(results.filter(result => result.status === "acquired")).toHaveLength(1);
    expect(results.filter(result => result.status === "held")).toHaveLength(99);
  });

  it("requires the matching owner token and generation to release", async () => {
    const { doInst, s } = authority();
    const claim = await doInst.acquireSpawnClaim("job-2", 60_000);
    expect(claim.status).toBe("acquired");
    if (claim.status !== "acquired") return;
    expect(await doInst.releaseSpawnClaim("job-2", claim.generation + 1, claim.ownerToken)).toBe("stale");
    expect(await doInst.releaseSpawnClaim("job-2", claim.generation, "wrong-owner")).toBe("stale");
    expect(await doInst.releaseSpawnClaim("job-2", claim.generation, claim.ownerToken)).toBe("released");
    expect((await doInst.acquireSpawnClaim("job-2", 60_000)).status).toBe("acquired");
  });

  it("keeps active claims renewable and supports conditional active transition", async () => {
    const { doInst, s } = authority();
    const claim = await doInst.acquireSpawnClaim("job-3", 60_000);
    expect(claim.status).toBe("acquired");
    if (claim.status !== "acquired") return;
    expect(await doInst.markSpawnClaimActive("job-3", claim.generation, claim.ownerToken)).toBe(true);
    expect((s.map.get("spawn-claim:job-3") as { phase: string; expiresAtMs: number }).phase).toBe("active");
    expect((s.map.get("spawn-claim:job-3") as { expiresAtMs: number }).expiresAtMs).toBe(Number.MAX_SAFE_INTEGER);
    expect(await doInst.renewSpawnClaim("job-3", claim.generation, claim.ownerToken, 60_000)).toBe(true);
    expect(await doInst.markSpawnClaimActive("job-3", claim.generation + 1, claim.ownerToken)).toBe(false);
  });

  it("does not let stale completion A release replacement B", async () => {
    const { doInst } = authority();
    const a = await doInst.acquireSpawnClaim("job-4", 60_000);
    expect(a.status).toBe("acquired");
    if (a.status !== "acquired") return;
    expect(await doInst.markSpawnClaimActive("job-4", a.generation, a.ownerToken)).toBe(true);
    expect(await doInst.bindSpawnClaimProvider("job-4", a.generation, a.ownerToken, "runner-a")).toBe(true);
    expect(await doInst.releaseSpawnClaim("job-4", a.generation, a.ownerToken)).toBe("released");

    const b = await doInst.acquireSpawnClaim("job-4", 60_000);
    expect(b.status).toBe("acquired");
    if (b.status !== "acquired") return;
    expect(b.generation).toBe(a.generation + 1);
    expect(await doInst.markSpawnClaimActive("job-4", b.generation, b.ownerToken)).toBe(true);
    expect(await doInst.bindSpawnClaimProvider("job-4", b.generation, b.ownerToken, "runner-b")).toBe(true);

    expect(await doInst.releaseSpawnClaimForCompletion("job-4", "runner-a")).toBe("stale");
    expect((await doInst.readSpawnClaim("job-4"))?.ownerToken).toBe(b.ownerToken);
    expect(await doInst.releaseSpawnClaimForCompletion("job-4", "runner-b")).toBe("released");
  });
});
