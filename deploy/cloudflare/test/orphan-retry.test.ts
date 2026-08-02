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
  orphanRefusalStep,
  SpawnRefusedError,
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
      // Stamped at FIRST record and preserved by every later write-back — it is
      // what bounds a ceiling-refusal wait to an ABSOLUTE window instead of a TTL
      // that would reset on each re-put. Asserted as a real, current epoch-ms
      // rather than pinned to a literal (which would just re-encode Date.now()).
      firstRecordedMs: expect.any(Number),
    });
    const stamped = JSON.parse(raw!).firstRecordedMs as number;
    expect(stamped).toBeGreaterThan(Date.now() - 60_000);
    expect(stamped).toBeLessThanOrEqual(Date.now());
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

// ── 3. Ceiling REFUSAL — backpressure must not be mistaken for success OR failure
//
// The 2026-08-02 incident: ~24 jobs pushed at once, 12 ran, 12 sat `queued`
// forever. Two defects compounded, and each is pinned below by a test that FAILS
// against the pre-fix code:
//
//   (a) `driveSpawn` RETURNED at the ceiling instead of throwing, so
//       `driveSpawnGuarded` (which records the dead-letter only from its catch)
//       never recorded one — and GitHub never redelivers `workflow_job.queued`.
//   (b) `retryOrphanedSpawns` reads a normal return as recovery and DELETES the
//       record. So even once a dead-letter existed, the first refused retry tick
//       would have destroyed it. `refusal must NOT delete` below is exactly that
//       regression.
//
// The third property is the one that makes the fix actually work under a real
// burst: a refusal must not spend the 3-strike budget, or a burst lasting longer
// than MAX_ORPHAN_ATTEMPTS ticks still loses every job behind the ceiling.

describe("orphanRefusalStep (PURE — the absolute-window bound on a refusal wait)", () => {
  const T0 = 1_700_000_000_000;

  it("waits while the window is open, carrying the REMAINING ttl (never extends the deadline)", () => {
    const rec = REC({ firstRecordedMs: T0 });
    const step = orphanRefusalStep(rec, T0 + 600_000, ORPHAN_TTL_S); // 10 min in
    expect(step.action).toBe("wait");
    expect(step.waitedS).toBe(600);
    // 30-min window minus the 10 already waited — NOT a fresh ORPHAN_TTL_S.
    expect(step.ttlS).toBe(ORPHAN_TTL_S - 600);
    expect(step.ttlS).toBeLessThan(ORPHAN_TTL_S);
  });

  it("the deadline is ABSOLUTE across repeated refusals — re-putting never resets it", () => {
    const rec = REC({ firstRecordedMs: T0 });
    // Ten consecutive refused ticks, one minute apart (the real cron cadence).
    const ttls = Array.from({ length: 10 }, (_, i) =>
      orphanRefusalStep(rec, T0 + (i + 1) * 60_000, ORPHAN_TTL_S).ttlS);
    // Strictly decreasing ⇒ the window really is closing, not being renewed.
    for (let i = 1; i < ttls.length; i++) expect(ttls[i]).toBeLessThan(ttls[i - 1]);
    expect(ttls.at(-1)).toBe(ORPHAN_TTL_S - 600);
  });

  it("gives up once the window is exhausted (a job GitHub will no longer place)", () => {
    const rec = REC({ firstRecordedMs: T0 });
    expect(orphanRefusalStep(rec, T0 + ORPHAN_TTL_S * 1000, ORPHAN_TTL_S).action).toBe("giveup");
    expect(orphanRefusalStep(rec, T0 + ORPHAN_TTL_S * 1000 + 1, ORPHAN_TTL_S).action).toBe("giveup");
    // One second BEFORE the boundary is still a wait — the bound is not off-by-one.
    expect(orphanRefusalStep(rec, T0 + (ORPHAN_TTL_S - 1) * 1000, ORPHAN_TTL_S).action).toBe("wait");
  });

  it("clamps to Cloudflare KV's 60 s minimum TTL near the deadline", () => {
    const rec = REC({ firstRecordedMs: T0 });
    const step = orphanRefusalStep(rec, T0 + (ORPHAN_TTL_S - 5) * 1000, ORPHAN_TTL_S);
    expect(step.action).toBe("wait");
    expect(step.ttlS).toBe(60); // true remainder is 5 s; KV would reject that
  });

  it("a legacy record with no firstRecordedMs is treated as FRESH, never as expired", () => {
    // Records written before the field existed must not be given up instantly —
    // they stay bounded by their own KV TTL and by the attempt count.
    const step = orphanRefusalStep(REC(), T0, ORPHAN_TTL_S);
    expect(step.action).toBe("wait");
    expect(step.waitedS).toBe(0);
  });
});

describe("retryOrphanedSpawns — a REFUSED retry is backpressure, not a failed attempt", () => {
  const refuse = () => {
    throw new SpawnRefusedError("over_fleet_cap");
  };

  it("a refusal LEAVES the dead-letter in place for the next tick", async () => {
    const kv = fakeKv({ "orphan:job-1": JSON.stringify(REC({ attempts: 1, firstRecordedMs: Date.now() })) });
    await retryOrphanedSpawns(envWith(kv), CTX, Date.now(), vi.fn(refuse));
    expect(kv.store.has("orphan:job-1")).toBe(true);
  });
  // NOTE — deliberately NOT called a regression pin. This seam injects `drive`,
  // so it cannot distinguish the pre-fix ceiling RETURN from a throw; written
  // against the buggy code it passes (verified). The honest pin for that defect
  // drives the real webhook path: see `cell12-deadletter` in
  // test/journey-sj5-concurrency-slot.test.ts. What the two tests BELOW pin is
  // the second defect — refusals spending the attempt budget — and those do go
  // red against the pre-fix code.

  it("does NOT spend the attempt budget — 5 refused ticks leave attempts at 1", async () => {
    const now = Date.now();
    const kv = fakeKv({ "orphan:job-1": JSON.stringify(REC({ attempts: 1, firstRecordedMs: now })) });
    for (let i = 0; i < 5; i++) {
      await retryOrphanedSpawns(envWith(kv), CTX, now + i * 60_000, vi.fn(refuse));
      // The live path releases the claim on refusal; clear it so the next tick can
      // re-claim, exactly as the 60 s gap between real cron ticks does.
      kv.store.delete("spawn:job-1");
    }
    // MAX_ORPHAN_ATTEMPTS is 3 — under the old accounting this job would have been
    // given up on tick 3 and never placed, purely because the fleet was busy.
    expect(kv.store.has("orphan:job-1")).toBe(true);
    expect(JSON.parse(kv.store.get("orphan:job-1")!).attempts).toBe(1);
    expect(MAX_ORPHAN_ATTEMPTS).toBeLessThan(5); // the bound this test outlives
  });

  it("recovers on a later tick once capacity frees up", async () => {
    const now = Date.now();
    const kv = fakeKv({ "orphan:job-1": JSON.stringify(REC({ attempts: 1, firstRecordedMs: now })) });
    await retryOrphanedSpawns(envWith(kv), CTX, now, vi.fn(refuse));
    kv.store.delete("spawn:job-1");
    expect(kv.store.has("orphan:job-1")).toBe(true);

    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(envWith(kv), CTX, now + 60_000, drive);
    expect(drive).toHaveBeenCalledTimes(1);
    expect(kv.store.has("orphan:job-1")).toBe(false); // placed ⇒ dead-letter cleared
  });

  it("gives up LOUDLY once the absolute window closes (a capacity fault, not routine)", async () => {
    const now = Date.now();
    const kv = fakeKv({
      "orphan:job-1": JSON.stringify(REC({ attempts: 1, firstRecordedMs: now - ORPHAN_TTL_S * 1000 })),
    });
    await retryOrphanedSpawns(envWith(kv), CTX, now, vi.fn(refuse));
    expect(kv.store.has("orphan:job-1")).toBe(false);
  });

  it("a GENUINE failure still bumps the attempt count (the 3-strike bound is intact)", async () => {
    const now = Date.now();
    const kv = fakeKv({ "orphan:job-1": JSON.stringify(REC({ attempts: 1, firstRecordedMs: now })) });
    await retryOrphanedSpawns(envWith(kv), CTX, now, vi.fn(async () => {
      throw new Error("JIT mint 500");
    }));
    // Refusals are free; real errors are not. Both bounds must coexist.
    expect(JSON.parse(kv.store.get("orphan:job-1")!).attempts).toBe(2);
  });
});
