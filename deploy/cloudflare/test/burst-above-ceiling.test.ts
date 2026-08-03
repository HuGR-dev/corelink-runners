// BEHAVIOURAL regression for the burst that loses jobs above the concurrency
// ceiling — a replay of the measured incident, not a unit test of one function.
//
// ── The incident being replayed ───────────────────────────────────────────────
// corelink-server run 30826164339, 2026-08-03T15:09:34Z, a deliberate 24-job
// fan-out at a GLOBAL ceiling of 20. Measured from the Worker's own logs:
//
//   • 24 spawns driven; 8 refused at the ceiling (`spawn_at_ceiling`,
//     over_key_cap); 3 hard start failures (`container start failed after 3
//     attempts: start timed out after 8000ms`).
//   • The dead-letter machinery handled ALL of those: 9 `orphan_recorded`, 9
//     `orphan_retry_recovered`, and ZERO `orphan_retry_giveup` / ZERO
//     `orphan_refusal_giveup`. The 3-strike budget was never even approached.
//   • And yet 13 fan-out jobs ran and 11 sat `queued` with no runner from 15:12:59
//     until GitHub cancelled them at 15:29:56 — 17 minutes in which the fleet was
//     idle and the 1-minute cron ticked 17 times without placing one of them.
//
// The 11 were lost in the one state nothing watched: the Worker had started a
// container for each, logged `runner_spawned`, and moved on. A box that starts and
// then never comes online to claim its job is, from the spawn path, identical to
// success — so no dead-letter was ever written and no recovery path could see them.
//
// ── What this test drives ────────────────────────────────────────────────────
// The REAL production reconciler (`retryOrphanedSpawns`) and the REAL record
// writers (`recordOrphan`, `recordPlacement`) plus the real pure decisions, over an
// in-memory KV, tick by tick on the 1-minute cron. Only the two things that are
// genuinely external are simulated, at the same seams the production code injects
// for exactly this purpose:
//
//   • the FLEET   — a slot counter enforcing the ceiling, and a per-job outcome
//     (comes online / starts but never registers), driven through the `drive` seam.
//   • GITHUB      — a job table the confirmation reads, through the `verify` seam.
//
// So the multi-tick state machine under test is production code; the edges are not.
// A live 24-box burst against prod would cost real fleet capacity and, per the
// repo's deploy posture, cannot run against unreleased code at all.
import { describe, it, expect, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));

import {
  retryOrphanedSpawns,
  recordOrphan,
  recordPlacement,
  type Env,
} from "../src/index";
import { SpawnRefusedError, MAX_ORPHAN_ATTEMPTS } from "../src/lib";

const CTX = {} as unknown as ExecutionContext;
const REPO = "HuGR-Labs/corelink-server";
const INSTALLATION = "150584374";
const LABELS = ["corelink"];

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

/**
 * The simulated world: a capacity-capped fleet plus GitHub's view of each job.
 *
 * `neverComesOnline` is the measured failure: the container starts (the spawn path
 * returns success) but no runner ever registers, so GitHub keeps the job `queued`
 * with `runner_id: 0`. Each entry is consumed once, so a re-drive of the same job
 * succeeds — which is the whole behaviour under test: the second attempt is the one
 * that has to happen at all.
 */
function makeWorld(opts: { capacity: number; jobDurationTicks: number }) {
  const gh = new Map<string, { status: string; runner_id: number }>();
  const running: { jobId: string; freeAtTick: number }[] = [];
  const neverComesOnline = new Set<string>();
  let tick = 0;
  let spawnsDriven = 0;
  let runnerId = 1;

  const world = {
    gh,
    neverComesOnline,
    get spawnsDriven() {
      return spawnsDriven;
    },
    queue(jobId: string) {
      gh.set(jobId, { status: "queued", runner_id: 0 });
    },
    /** Advance the cron by one minute, freeing every job whose 45 s run is over. */
    advance() {
      tick++;
      for (let i = running.length - 1; i >= 0; i--) {
        if (running[i].freeAtTick <= tick) {
          const done = running.splice(i, 1)[0];
          gh.set(done.jobId, { status: "completed", runner_id: gh.get(done.jobId)!.runner_id });
        }
      }
    },
    inFlight: () => running.length,

    /** Stands in for `driveSpawn` — same signature, same throw contract. */
    drive: async (env: Env, o: { jobId: string; repo: string; installationId: string; labels: string[] }) => {
      spawnsDriven++;
      // At the ceiling this is BACKPRESSURE, not failure — the production path
      // throws a typed refusal so it never consumes the 3-strike error budget.
      if (running.length >= opts.capacity) throw new SpawnRefusedError("over_key_cap");
      if (neverComesOnline.has(o.jobId)) {
        // The measured loss: the container starts, so the spawn path RETURNS
        // SUCCESS and writes the provisional placement — but GitHub never sees a
        // runner, so the job stays queued. Consume the entry: a re-drive works.
        neverComesOnline.delete(o.jobId);
        await recordPlacement(env, o);
        return;
      }
      running.push({ jobId: o.jobId, freeAtTick: tick + opts.jobDurationTicks });
      gh.set(o.jobId, { status: "in_progress", runner_id: runnerId++ });
      await recordPlacement(env, o);
    },

    /** Stands in for `fetchJobPlacement` — GitHub's authoritative view of one job. */
    verify: async (_env: Env, _repo: string, jobId: string) => gh.get(jobId) ?? null,
  };
  return world;
}

const placed = (world: ReturnType<typeof makeWorld>) =>
  [...world.gh.values()].filter((j) => j.status !== "queued").length;

describe("burst above the ceiling — jobs must queue and drain, never vanish", () => {
  it("replays the 24-job / cap-20 burst: every job is eventually placed", async () => {
    const kv = fakeKv();
    const env = { RUNNER_JOB_PATS: kv } as unknown as Env;
    // Capacity 20; a 45 s job clears inside one 60 s cron tick.
    const world = makeWorld({ capacity: 20, jobDurationTicks: 1 });

    const jobs = Array.from({ length: 24 }, (_, i) => `job-${i + 1}`);
    // The measured mix: 11 of the 24 start a box that never comes online. These are
    // precisely the 11 that were lost in the incident.
    for (const id of jobs.slice(13)) world.neverComesOnline.add(id);

    // ── t0: the webhook wave. GitHub delivers `workflow_job.queued` ONCE per job.
    for (const jobId of jobs) {
      world.queue(jobId);
      try {
        await world.drive(env, { jobId, repo: REPO, installationId: INSTALLATION, labels: LABELS });
      } catch (e) {
        // `driveSpawnGuarded` dead-letters a refusal AND a failure — both are
        // recoverable, and only a genuine error is counted as an attempt.
        expect(e).toBeInstanceOf(SpawnRefusedError);
        await recordOrphan(env, { jobId, repo: REPO, installationId: INSTALLATION, labels: LABELS });
      }
    }

    // Not everything is placed at t0 — that is expected and fine. The ceiling is a
    // billing control and it is doing its job; the requirement is that the excess
    // WAITS rather than disappearing.
    expect(placed(world)).toBeLessThan(jobs.length);

    // ── the 1-minute cron, for the 17 minutes the real incident had available.
    let now = Date.now();
    for (let t = 0; t < 17; t++) {
      now += 60_000;
      world.advance();
      await retryOrphanedSpawns(env, CTX, now, world.drive, world.verify);
      // The live path releases the spawn claim on a refusal/failure; the 60 s gap
      // between real ticks is what lets the next tick re-claim.
      for (const jobId of jobs) kv.store.delete(`spawn:${jobId}`);
    }

    // THE REQUIREMENT: at the ceiling, jobs queue and drain. None vanish.
    const stranded = jobs.filter((id) => world.gh.get(id)!.status === "queued");
    expect(stranded).toEqual([]);
    expect(placed(world)).toBe(24);
  });

  it("a box that NEVER comes online still dead-letters fast — it does not respawn forever", async () => {
    // Requirement 3: recovery must not become an infinite retry. A permanently
    // broken job (a bad image, a registration that can never succeed) has to stop,
    // loudly and quickly, instead of burning a slot and real COGS every minute.
    const kv = fakeKv();
    const env = { RUNNER_JOB_PATS: kv } as unknown as Env;
    const world = makeWorld({ capacity: 20, jobDurationTicks: 1 });
    const jobId = "job-broken";
    world.queue(jobId);

    // Unlike the burst above, this box NEVER registers — re-arm it every tick.
    const drive = async (e: Env, o: Parameters<typeof world.drive>[1]) => {
      world.neverComesOnline.add(o.jobId);
      return world.drive(e, o);
    };
    await drive(env, { jobId, repo: REPO, installationId: INSTALLATION, labels: LABELS });

    let now = Date.now();
    let redrives = 0;
    for (let t = 0; t < 20; t++) {
      now += 60_000;
      world.advance();
      const before = world.spawnsDriven;
      await retryOrphanedSpawns(env, CTX, now, drive, world.verify);
      redrives += world.spawnsDriven - before;
      kv.store.delete(`spawn:${jobId}`);
    }

    // Bounded by the 3-strike budget, NOT by the 20 ticks we gave it.
    expect(redrives).toBeLessThanOrEqual(MAX_ORPHAN_ATTEMPTS);
    // And it ended: the dead-letter is gone (given up + logged loud), not looping.
    expect(kv.store.has(`orphan:${jobId}`)).toBe(false);
  });

  it("a healthy in-flight job is never re-driven (no duplicate spawn, no wasted slot)", async () => {
    // The confirmation must not become a spawn amplifier. A job that IS running has
    // to be left completely alone — re-driving it would burn a concurrency slot and
    // real money on a job that was never in trouble.
    const kv = fakeKv();
    const env = { RUNNER_JOB_PATS: kv } as unknown as Env;
    // A long job: still running well past the confirmation grace window.
    const world = makeWorld({ capacity: 20, jobDurationTicks: 30 });
    const jobId = "job-healthy";
    world.queue(jobId);
    await world.drive(env, { jobId, repo: REPO, installationId: INSTALLATION, labels: LABELS });
    const afterSpawn = world.spawnsDriven;

    let now = Date.now();
    for (let t = 0; t < 15; t++) {
      now += 60_000;
      world.advance();
      await retryOrphanedSpawns(env, CTX, now, world.drive, world.verify);
      kv.store.delete(`spawn:${jobId}`);
    }

    expect(world.spawnsDriven).toBe(afterSpawn); // never re-driven
    expect(world.gh.get(jobId)!.status).toBe("in_progress");
    // Confirmed placed ⇒ the provisional record was dropped, so it stops costing a
    // GitHub read on every subsequent tick.
    expect(kv.store.has(`orphan:${jobId}`)).toBe(false);
  });
});
