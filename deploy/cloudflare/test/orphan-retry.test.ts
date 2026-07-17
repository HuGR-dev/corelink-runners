// Unit tests for the dead-letter orphan retry (W7/F8) — external-repo orphan
// recovery via a WARM re-drive.
//
// Two layers:
//   1. `orphanRetryStep` — the PURE decision (missing / retry-under-max / giveup),
//      unit-tested exhaustively in plain vitest (node).
//   2. `retryOrphanedSpawns` + `recordOrphan` — the KV/driveSpawn I/O in index.ts,
//      driven against an in-memory KvLike mock + an INJECTED mock drive (so the
//      real driveSpawn's fetch/DO I/O is never touched). `driveSpawn` is injected
//      as the last (defaulted) arg of `retryOrphanedSpawns`, which is the only
//      seam through which the reconciler's bump→claim→drive→delete/leave branches
//      are observable without the Workers runtime.
//
// `@cloudflare/containers` imports the Workers-only `cloudflare:workers`, so we
// vi.mock it (as the sibling DO tests do) purely to make src/index.ts importable
// under node vitest.
import { describe, it, expect, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));

// Import AFTER the mock is registered.
import { retryOrphanedSpawns, recordOrphan, type Env } from "../src/index";
import {
  orphanRetryStep,
  ORPHAN_TTL_S,
  MAX_ORPHAN_ATTEMPTS,
  type OrphanRecord,
} from "../src/lib";

// ── An in-memory KV mock mirroring the KVNamespace subset the reconciler uses
// (get/put/delete/list). Prefix-aware list, like the real binding.
function fakeKv(seed: Record<string, string> = {}) {
  const store = new Map<string, string>(Object.entries(seed));
  return {
    store,
    get: vi.fn(async (k: string) => store.get(k) ?? null),
    put: vi.fn(async (k: string, v: string) => {
      store.set(k, v);
    }),
    delete: vi.fn(async (k: string) => {
      store.delete(k);
    }),
    list: vi.fn(async ({ prefix }: { prefix: string }) => ({
      keys: [...store.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })),
    })),
  };
}

// Build an Env carrying only the RUNNER_JOB_PATS the reconciler reads (METRICS
// absent ⇒ bumpMetrics is a no-op).
function envWith(kv: ReturnType<typeof fakeKv> | undefined): Env {
  return { RUNNER_JOB_PATS: kv } as unknown as Env;
}
const CTX = {} as unknown as ExecutionContext;

const REC = (over: Partial<OrphanRecord> = {}): OrphanRecord => ({
  repo: "octo/external-repo",
  installationId: "44556677",
  labels: ["corelink"],
  attempts: 1,
  ...over,
});

// ── 1. orphanRetryStep (the PURE decision) ────────────────────────────────────

describe("orphanRetryStep (pure dead-letter decision)", () => {
  it("null record ⇒ missing (the key TTL-expired between list and get)", () => {
    expect(orphanRetryStep(null, MAX_ORPHAN_ATTEMPTS)).toEqual({
      action: "missing",
      nextAttempts: 0,
    });
  });

  it("attempts < max ⇒ retry, incrementing the attempt count", () => {
    expect(orphanRetryStep(REC({ attempts: 1 }), 3)).toEqual({ action: "retry", nextAttempts: 2 });
    expect(orphanRetryStep(REC({ attempts: 2 }), 3)).toEqual({ action: "retry", nextAttempts: 3 });
  });

  it("attempts == max ⇒ giveup (bounded — never retries forever)", () => {
    expect(orphanRetryStep(REC({ attempts: 3 }), 3)).toEqual({ action: "giveup", nextAttempts: 3 });
  });

  it("attempts > max ⇒ giveup (defensive: a stale over-count still stops)", () => {
    expect(orphanRetryStep(REC({ attempts: 5 }), 3)).toEqual({ action: "giveup", nextAttempts: 5 });
  });

  it("respects the max boundary exactly (max-1 retries, max gives up)", () => {
    expect(orphanRetryStep(REC({ attempts: MAX_ORPHAN_ATTEMPTS - 1 }), MAX_ORPHAN_ATTEMPTS).action).toBe(
      "retry",
    );
    expect(orphanRetryStep(REC({ attempts: MAX_ORPHAN_ATTEMPTS }), MAX_ORPHAN_ATTEMPTS).action).toBe(
      "giveup",
    );
  });
});

// ── 2a. recordOrphan (part 1: record the FIRST warm-recoverable failure) ──────

describe("recordOrphan (dead-letter record on spawn failure)", () => {
  const OPTS = {
    jobId: "job-1",
    repo: "octo/external-repo",
    installationId: "44556677",
    labels: ["corelink"],
  };

  it("records {repo, installationId, labels, attempts:1} under the orphan: prefix", async () => {
    const kv = fakeKv();
    await recordOrphan(envWith(kv), OPTS);
    const raw = kv.store.get("orphan:job-1");
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw!)).toEqual({
      repo: "octo/external-repo",
      installationId: "44556677",
      labels: ["corelink"],
      attempts: 1,
    });
    // The FIRST-failure record uses the self-healing 30-min TTL.
    expect(kv.put).toHaveBeenCalledWith("orphan:job-1", expect.any(String), {
      expirationTtl: ORPHAN_TTL_S,
    });
  });

  it("does NOT record a COLD spawn (no installation_id ⇒ not warm-recoverable)", async () => {
    const kv = fakeKv();
    await recordOrphan(envWith(kv), { ...OPTS, installationId: "" });
    expect(kv.store.has("orphan:job-1")).toBe(false);
    expect(kv.put).not.toHaveBeenCalled();
  });

  it("IDEMPOTENT: an existing dead-letter is NOT overwritten (attempts not bumped)", async () => {
    // Simulate the reconciler having already bumped attempts to 2.
    const kv = fakeKv({ "orphan:job-1": JSON.stringify(REC({ attempts: 2 })) });
    await recordOrphan(envWith(kv), OPTS);
    expect(JSON.parse(kv.store.get("orphan:job-1")!).attempts).toBe(2); // untouched
    expect(kv.put).not.toHaveBeenCalled(); // no clobber
  });

  it("no-op (no throw) when no KV is bound", async () => {
    await expect(recordOrphan(envWith(undefined), OPTS)).resolves.toBeUndefined();
  });

  it("best-effort: swallows a KV put error (never breaks the spawn path)", async () => {
    const kv = fakeKv();
    kv.put.mockRejectedValueOnce(new Error("KV down"));
    await expect(recordOrphan(envWith(kv), OPTS)).resolves.toBeUndefined();
  });
});

// ── 2b. retryOrphanedSpawns (part 2: WARM re-drive of the dead-letter) ─────────

describe("retryOrphanedSpawns (scheduled WARM re-drive)", () => {
  it("bump → claim → drive → DELETE-on-success (recovered), WARM", async () => {
    const kv = fakeKv({ "orphan:job-1": JSON.stringify(REC({ attempts: 1 })) });
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(envWith(kv), CTX, Date.now(), drive);

    // Drove WARM with the recorded installation_id + labels + repo.
    expect(drive).toHaveBeenCalledTimes(1);
    expect(drive).toHaveBeenCalledWith(
      expect.anything(),
      { jobId: "job-1", repo: "octo/external-repo", installationId: "44556677", labels: ["corelink"] },
    );
    // Claimed the spawn (dedup vs the live path).
    expect(kv.store.get("spawn:job-1")).toBe("1");
    // Recovered ⇒ the dead-letter is deleted.
    expect(kv.store.has("orphan:job-1")).toBe(false);
  });

  it("bumps the attempt count BEFORE driving (so a killed tick still advances)", async () => {
    const kv = fakeKv({ "orphan:job-1": JSON.stringify(REC({ attempts: 1 })) });
    // Capture the persisted record at the moment of the put, before the delete.
    let bumpedAtPut: OrphanRecord | undefined;
    kv.put.mockImplementation(async (k: string, v: string) => {
      if (k === "orphan:job-1") bumpedAtPut = JSON.parse(v) as OrphanRecord;
      kv.store.set(k, v);
    });
    await retryOrphanedSpawns(envWith(kv), CTX, Date.now(), vi.fn(async () => {}));
    expect(bumpedAtPut?.attempts).toBe(2); // 1 → 2 before the drive
  });

  it("LEAVE-on-fail: a drive throw releases the claim + leaves the (bumped) record", async () => {
    const kv = fakeKv({ "orphan:job-1": JSON.stringify(REC({ attempts: 1 })) });
    const drive = vi.fn(async () => {
      throw new Error("spawn still failing");
    });
    await retryOrphanedSpawns(envWith(kv), CTX, Date.now(), drive);

    expect(drive).toHaveBeenCalledTimes(1);
    // The record survives for the next tick, with attempts bumped to 2.
    const raw = kv.store.get("orphan:job-1");
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw!).attempts).toBe(2);
    // The claim was RELEASED so a later tick / the live path can re-drive.
    expect(kv.store.has("spawn:job-1")).toBe(false);
  });

  it("GIVEUP at max: deletes the dead-letter and does NOT drive", async () => {
    const kv = fakeKv({
      "orphan:job-1": JSON.stringify(REC({ attempts: MAX_ORPHAN_ATTEMPTS })),
    });
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(envWith(kv), CTX, Date.now(), drive);

    expect(drive).not.toHaveBeenCalled();
    expect(kv.store.has("orphan:job-1")).toBe(false); // given up ⇒ deleted
    expect(kv.store.has("spawn:job-1")).toBe(false); // never claimed
  });

  it("already-claimed (live path won it): bumps but SKIPS the drive, LEAVES the record", async () => {
    const kv = fakeKv({
      "orphan:job-1": JSON.stringify(REC({ attempts: 1 })),
      "spawn:job-1": "1", // a live path / another tick already holds the claim
    });
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(envWith(kv), CTX, Date.now(), drive);

    expect(drive).not.toHaveBeenCalled(); // claimSpawn returned false ⇒ skip
    // Attempt was bumped (1 → 2) but the record is LEFT for a later tick.
    expect(JSON.parse(kv.store.get("orphan:job-1")!).attempts).toBe(2);
  });

  it("missing record (key listed but value gone by get) ⇒ skip, no drive, no throw", async () => {
    // list() returns a phantom key that get() resolves to null (the TTL raced
    // between the list and the get).
    const kv = {
      get: vi.fn(async () => null),
      put: vi.fn(async () => {}),
      delete: vi.fn(async () => {}),
      list: vi.fn(async () => ({ keys: [{ name: "orphan:ghost" }] })),
    };
    const drive = vi.fn(async () => {});
    await expect(
      retryOrphanedSpawns({ RUNNER_JOB_PATS: kv } as unknown as Env, CTX, Date.now(), drive),
    ).resolves.toBeUndefined();
    expect(drive).not.toHaveBeenCalled();
    expect(kv.delete).not.toHaveBeenCalled(); // missing ≠ giveup: nothing to delete
  });

  it("no-op (no throw) when no KV is bound", async () => {
    const drive = vi.fn(async () => {});
    await expect(
      retryOrphanedSpawns(envWith(undefined), CTX, Date.now(), drive),
    ).resolves.toBeUndefined();
    expect(drive).not.toHaveBeenCalled();
  });

  it("only scans the orphan: prefix (never touches spawn:/done:/bare-jobId keys)", async () => {
    const kv = fakeKv({
      "orphan:job-1": JSON.stringify(REC({ attempts: 1 })),
      "spawn:other": "1",
      "done:other": "1",
      "job-1": "pat-id",
    });
    await retryOrphanedSpawns(envWith(kv), CTX, Date.now(), vi.fn(async () => {}));
    expect(kv.list).toHaveBeenCalledWith({ prefix: "orphan:" });
    // The unrelated namespaces are untouched.
    expect(kv.store.get("spawn:other")).toBe("1");
    expect(kv.store.get("done:other")).toBe("1");
    expect(kv.store.get("job-1")).toBe("pat-id");
  });

  it("processes MULTIPLE dead-letters in one tick", async () => {
    const kv = fakeKv({
      "orphan:job-1": JSON.stringify(REC({ attempts: 1 })),
      "orphan:job-2": JSON.stringify(REC({ attempts: 1, repo: "octo/repo-2" })),
    });
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(envWith(kv), CTX, Date.now(), drive);
    expect(drive).toHaveBeenCalledTimes(2);
    expect(kv.store.has("orphan:job-1")).toBe(false);
    expect(kv.store.has("orphan:job-2")).toBe(false);
  });
});
