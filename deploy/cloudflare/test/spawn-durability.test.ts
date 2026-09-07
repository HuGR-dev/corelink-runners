import { describe, expect, it } from "vitest";
import { vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { ConcurrencySlotsDO } from "../src/index";

function authority() {
  const map = new Map<string, unknown>(); let tail = Promise.resolve();
  const storage = {
    map,
    async get<T>(key: string): Promise<T | undefined> { return structuredClone(map.get(key) as T | undefined); },
    async put(key: string, value: unknown): Promise<void> { map.set(key, structuredClone(value)); },
    async delete(key: string): Promise<void> { map.delete(key); },
    async list<T>(opts: { prefix?: string; limit?: number } = {}): Promise<Map<string, T>> {
      return new Map([...map].filter(([key]) => key.startsWith(opts.prefix ?? "")).slice(0, opts.limit).map(([key, value]) => [key, structuredClone(value) as T]));
    },
    async transaction<T>(fn: (tx: typeof storage) => Promise<T>): Promise<T> {
      const run = tail.then(() => fn(storage)); tail = run.then(() => undefined, () => undefined); return run;
    },
  };
  return { storage, slots: new ConcurrencySlotsDO({ storage } as never, {} as never) };
}

async function active(slots: ConcurrencySlotsDO, job = "job", runner = "runner-a", handle = "handle-a") {
  const claim = await slots.acquireSpawnClaim(job, 1); expect(claim.status).toBe("acquired");
  if (claim.status !== "acquired") throw new Error("claim");
  expect(await slots.markSpawnClaimActive(job, claim.generation, claim.ownerToken)).toBe(true);
  expect(await slots.bindSpawnClaimProvider(job, claim.generation, claim.ownerToken, runner)).toBe(true);
  expect(await slots.persistActiveAttempt(job, handle, runner, 7, 1, "acme/repo", "42", "prep-a")).toBe(true);
  return claim;
}

describe("AU4.15 post-start durability", () => {
  it("retains exact generation, handle and JIT identity through 100 TTL-era replays", async () => {
    const { slots } = authority(); const claim = await active(slots);
    const replay = await Promise.all(Array.from({ length: 100 }, () => slots.acquireSpawnClaim("job", 1)));
    expect(replay.every(value => value.status === "held")).toBe(true);
    expect(await slots.readActiveAttempt("job")).toMatchObject({ generation: claim.generation, handle: "handle-a", runnerName: "runner-a", runnerId: 7, jitAttempt: 1, teardownIntent: true });
  });

  it("does not release an active attempt until its exact handle is confirmed", async () => {
    const { slots } = authority(); const claim = await active(slots);
    expect(await slots.releaseSpawnClaim("job", claim.generation, claim.ownerToken)).toBe("stale");
    expect(await slots.confirmAttemptTeardown("job", claim.generation, claim.ownerToken, "wrong")).toBeNull();
    expect(await slots.confirmAttemptTeardown("job", claim.generation, claim.ownerToken, "handle-a")).toMatchObject({ runnerName: "runner-a" });
    expect(await slots.readSpawnClaim("job")).toBeNull();
  });

  it("rejects stale generation cleanup and late completion after replacement", async () => {
    const { slots } = authority(); const a = await active(slots);
    await slots.confirmAttemptTeardown("job", a.generation, a.ownerToken, "handle-a");
    const b = await active(slots, "job", "runner-b", "handle-b"); // generation is monotonic even after terminalization
    expect(b.generation).toBe(a.generation + 1);
    expect(await slots.confirmAttemptTeardown("job", a.generation, a.ownerToken, "handle-a")).toBeNull();
    expect(await slots.releaseSpawnClaimForCompletion("job", "runner-a")).toBe("stale");
    expect((await slots.readActiveAttempt("job"))?.handle).toBe("handle-b");
  });
});
