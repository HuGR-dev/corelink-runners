// The keep-alive sweep must renew only boxes GitHub says are WORKING (2026-08-03).
//
// THE DEFECT THESE PIN. The sweep renewed the idle timeout of every box that still
// had an `rhandle:` binding, on the stated theory that such a box "has a job on it"
// and that "a stuck box is still reclaimed". Both were false in one specific,
// common case: the binding is written at SPAWN with a 2-hour TTL, so a box that
// boots and NEVER REGISTERS with GitHub produces no completion event naming it,
// nothing ever drops its binding, and the 1-minute cron renewed it ~120 times —
// defeating `RunnerContainer.sleepAfter = "15m"` entirely and holding a standard-4
// out of `max_instances: 20` for two hours.
//
// THE CONSTRAINT THAT SHAPES THE FIX. A box may NOT be reclaimed because its JOB
// looks queued. `generate-jitconfig` binds a runner to a repo + label set and to
// nothing else, so GitHub assigns queued jobs to idle runners by label match and
// the job→box mapping is a permutation. Teardown keyed on the spawn's jobId
// SIGKILLed five live customer jobs on 2026-08-02. So the sweep asks GitHub about
// one specific RUNNER ID and acts only on that runner's own reported state.
//
// WHICH CELLS ACTUALLY PIN THE DEFECT — MEASURED, not assumed, by reverting
// src/index.ts to origin/main (leaving lib.ts/metrics.ts, which are pure
// additions) and re-running this file: 8 failed, 17 passed.
//
// RED pre-fix, for the RIGHT reason — the sweep renewed a box GitHub says is not
// working:
//   • "an IDLE runner stops being renewed"                        (renewed 1, want 0)
//   • "THE DEFECT: … never registered stops being renewed"        (renewed 1, want 0)
//   • "a runner GitHub has forgotten (404) …"                     (renewed 1, want 0)
//   • "a mixed fleet renews … only the busy box"                  (renewed 4, want 1)
//   • "past the per-tick verification cap …"                      (renewed 45, want 5)
//   • "one box's verification failure does not stop the others"   (renewed 3, want 2)
//   • "counts renewed-busy, stopped-idle and kept-unverifiable"   (no counter at all)
//
// RED pre-fix for a WEAKER reason, and worth saying so: the BUSY cell also fails
// against origin/main, but only because the pre-fix sweep read the whole binding
// record as if it were a DO handle — it still renewed a container, just the wrong
// one. It is red on plumbing, not on judgement. Its real job is as the 2026-08-02
// REGRESSION GUARD, and it is the most important assertion in this file: a runner
// GitHub itself reports as executing a job must never stop being renewed.
//
// GREEN both before and after, and therefore proving nothing about the fix: every
// other FAIL-SAFE cell (API error, 403/429, unreachable, ambiguous body, legacy
// bare handle, cold spawn). Pre-fix the sweep renewed unconditionally, so of course
// it renewed those. They exist to stop a future "tidy-up" from turning an
// inconclusive check into a reclaim.
//
// The `runnerActivityVerdict` / `parseRunnerBinding` tables exercise functions that
// did not exist before this change, so "would they have caught it" does not apply.
// They pin the decision's contract.
import { describe, it, expect, vi, beforeEach } from "vitest";

interface FakeContainer {
  handle: string;
  keepAlive: ReturnType<typeof vi.fn>;
}
let containers: FakeContainer[] = [];

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((_ns: unknown, handle: string): FakeContainer => {
    const existing = containers.find((c) => c.handle === handle);
    if (existing) return existing;
    const c: FakeContainer = { handle, keepAlive: vi.fn(async () => ({ ok: true })) };
    containers.push(c);
    return c;
  }),
}));

import { keepAliveLiveRunners, type Env } from "../src/index";
import { runnerActivityVerdict, parseRunnerBinding, type RunnerObservation } from "../src/lib";

// ── fixtures ────────────────────────────────────────────────────────────────

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

function fakeMetrics() {
  const counts: Record<string, number> = {};
  const stub = {
    bump: vi.fn(async (names: string[]) => {
      for (const n of names) counts[n] = (counts[n] ?? 0) + 1;
    }),
    snapshot: vi.fn(async () => ({ ...counts })),
  };
  return { counts, get: vi.fn(() => stub), idFromName: vi.fn((n: string) => n) };
}

/** A `rhandle:` value in the shape the spawn writes it — handle + the identifiers
 *  the sweep needs to ask GitHub about THIS box's runner. Written as a literal so
 *  this file pins the on-the-wire shape, not just the encoder's round trip. */
function binding(handle: string, rid: number): string {
  return JSON.stringify({ h: handle, rid, repo: "acme/api", inst: "555" });
}

function envWith(kv: ReturnType<typeof fakeKv>, metrics?: ReturnType<typeof fakeMetrics>): Env {
  return {
    RUNNER_CONTAINER: { _ns: "runner" },
    GITHUB_MINT_TOKEN: "ghp-mint",
    RUNNER_JOB_PATS: kv,
    ...(metrics ? { METRICS: metrics } : {}),
  } as unknown as Env;
}

/** GitHub's answer for each runner id, injected in place of the real REST call. */
function verifier(byId: Record<number, RunnerObservation | null>) {
  return vi.fn(async (_e: Env, _repo: string, runnerId: number) => byId[runnerId] ?? null);
}

const BUSY: RunnerObservation = { httpStatus: 200, runner: { status: "online", busy: true } };
const IDLE: RunnerObservation = { httpStatus: 200, runner: { status: "online", busy: false } };
const NEVER_REGISTERED: RunnerObservation = {
  httpStatus: 200,
  runner: { status: "offline", busy: false },
};
const GONE: RunnerObservation = { httpStatus: 404, runner: null };

// ── the sweep ───────────────────────────────────────────────────────────────

describe("keepAliveLiveRunners renews only VERIFIED-BUSY boxes", () => {
  beforeEach(() => {
    containers = [];
  });

  // ⛔ THE MOST IMPORTANT CELL IN THIS FILE. On 2026-08-02, reclaiming a box on a
  // job-keyed correlation killed five live customer jobs mid-step. A runner GitHub
  // itself reports as executing a job must be renewed, forever, no matter what any
  // job record says. If this cell ever goes red, the incident is back.
  it("a BUSY runner is renewed — never reclaimed (2026-08-02 regression guard)", async () => {
    const kv = fakeKv({ "rhandle:cf-runner-busy": binding("h-busy", 901) });
    const verify = verifier({ 901: BUSY });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(1);
    expect(containers.find((c) => c.handle === "h-busy")!.keepAlive).toHaveBeenCalledTimes(1);
  });

  it("a busy runner is renewed even while its own job still reads as queued", async () => {
    // The permutation case, stated directly: GitHub gave this box somebody else's
    // job, so the job we spawned it for is still queued. The sweep must not care —
    // it never looks at a job at all.
    const kv = fakeKv({
      "rhandle:cf-runner-perm": binding("h-perm", 902),
      // The spawn-side record for the job that is STILL waiting. Present exactly so
      // this test would fail if the sweep ever started keying on it.
      "orphan:5150": JSON.stringify({ repo: "acme/api", placedMs: 1 }),
      "jhandle:5150": "h-perm",
    });
    const verify = verifier({ 902: BUSY });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(1);
  });

  it("an IDLE runner stops being renewed (it falls through to sleepAfter)", async () => {
    const kv = fakeKv({ "rhandle:cf-runner-idle": binding("h-idle", 903) });
    const verify = verifier({ 903: IDLE });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(0);
    expect(containers).toHaveLength(0); // never even resolved — nothing renewed
  });

  it("THE DEFECT: a box that booted and never registered stops being renewed", async () => {
    // `generate-jitconfig` creates the runner entity immediately and it reads
    // `offline` until the agent connects. Before this change nothing ever dropped
    // this box's binding, so the sweep renewed it every minute for the binding's
    // full 2-hour TTL — the slot sink this whole change exists to close.
    const kv = fakeKv({ "rhandle:cf-runner-stuck": binding("h-stuck", 904) });
    const verify = verifier({ 904: NEVER_REGISTERED });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(0);
  });

  it("a runner GitHub has forgotten (404) stops being renewed", async () => {
    // An ephemeral runner is de-registered by GitHub once it has processed its one
    // job, so a 404 means the work is over — the lost-completion-webhook case.
    const kv = fakeKv({ "rhandle:cf-runner-gone": binding("h-gone", 905) });
    const verify = verifier({ 905: GONE });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(0);
  });

  it("a mixed fleet renews the busy box and only the busy box", async () => {
    const kv = fakeKv({
      "rhandle:cf-runner-1": binding("h-1", 911), // busy
      "rhandle:cf-runner-2": binding("h-2", 912), // never registered
      "rhandle:cf-runner-3": binding("h-3", 913), // idle
      "rhandle:cf-runner-4": binding("h-4", 914), // gone
    });
    const verify = verifier({ 911: BUSY, 912: NEVER_REGISTERED, 913: IDLE, 914: GONE });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(1);
    expect(containers.map((c) => c.handle)).toEqual(["h-1"]);
  });

  // ── fail SAFE, not clean ──────────────────────────────────────────────────
  // Leaking a slot is recoverable; killing a running job is not. Every one of
  // these must KEEP the box renewed.

  it("FAIL-SAFE: an API error keeps the box renewed", async () => {
    const kv = fakeKv({ "rhandle:cf-runner-err": binding("h-err", 921) });
    const verify = verifier({ 921: { httpStatus: 500, runner: null } });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(1);
  });

  it("FAIL-SAFE: a rate-limited check (403/429) keeps the box renewed", async () => {
    const kv = fakeKv({
      "rhandle:cf-runner-403": binding("h-403", 922),
      "rhandle:cf-runner-429": binding("h-429", 923),
    });
    const verify = verifier({
      922: { httpStatus: 403, runner: null },
      923: { httpStatus: 429, runner: null },
    });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(2);
  });

  it("FAIL-SAFE: an unreachable GitHub (null observation) keeps the box renewed", async () => {
    const kv = fakeKv({ "rhandle:cf-runner-net": binding("h-net", 924) });
    const verify = verifier({}); // → null
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(1);
  });

  it("FAIL-SAFE: an ambiguous body (no `busy`) keeps the box renewed", async () => {
    const kv = fakeKv({ "rhandle:cf-runner-amb": binding("h-amb", 925) });
    const verify = verifier({ 925: { httpStatus: 200, runner: { status: "online" } } });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(1);
  });

  it("FAIL-SAFE: a verifier that THROWS keeps the box renewed, and the sweep survives", async () => {
    const kv = fakeKv({ "rhandle:cf-runner-throw": binding("h-throw", 926) });
    const verify = vi.fn(async () => {
      throw new Error("boom");
    });
    await expect(keepAliveLiveRunners(envWith(kv), verify as never)).resolves.toBe(1);
  });

  it("FAIL-SAFE: a legacy bare-handle binding (pre-upgrade box) keeps being renewed", async () => {
    // Bindings already in KV when this deploys carry no runner id. A deploy must
    // not start reclaiming the in-flight boxes it knows least about.
    const kv = fakeKv({ "rhandle:cf-runner-legacy": "h-legacy" });
    const verify = verifier({});
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(1);
    expect(verify).not.toHaveBeenCalled(); // nothing to ask about
    expect(containers.find((c) => c.handle === "h-legacy")!.keepAlive).toHaveBeenCalled();
  });

  it("FAIL-SAFE: a cold spawn (no installation id) keeps being renewed, unasked", async () => {
    const kv = fakeKv({
      "rhandle:cf-runner-cold": JSON.stringify({ h: "h-cold", rid: 931, repo: "acme/api" }),
    });
    const verify = verifier({ 931: IDLE });
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(1);
    expect(verify).not.toHaveBeenCalled();
  });

  it("FAIL-SAFE: past the per-tick verification cap, boxes are renewed unasked", async () => {
    // The cap bounds the sweep's REST spend so it can never be the thing that
    // exhausts the installation's budget. Running out of budget must not start
    // reclaiming boxes we can no longer ask about.
    const seed: Record<string, string> = {};
    for (let i = 0; i < 45; i++) seed[`rhandle:cf-runner-${i}`] = binding(`h-${i}`, 1000 + i);
    const kv = fakeKv(seed);
    const answers: Record<number, RunnerObservation> = {};
    for (let i = 0; i < 45; i++) answers[1000 + i] = IDLE; // all idle
    const verify = verifier(answers);
    // 40 verified idle ⇒ dropped; the 5 over the cap are unverifiable ⇒ renewed.
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(5);
    expect(verify).toHaveBeenCalledTimes(40);
  });

  it("one box's verification failure does not stop the others being judged", async () => {
    const kv = fakeKv({
      "rhandle:cf-runner-a": binding("h-a", 941), // no answer → unknown → renewed
      "rhandle:cf-runner-b": binding("h-b", 942), // busy
      "rhandle:cf-runner-c": binding("h-c", 943), // idle
    });
    const verify = verifier({ 942: BUSY, 943: IDLE }); // 941 → null → unknown
    expect(await keepAliveLiveRunners(envWith(kv), verify)).toBe(2);
    expect(containers.map((c) => c.handle).sort()).toEqual(["h-a", "h-b"]);
  });

  // ── observability ─────────────────────────────────────────────────────────

  it("counts renewed-busy, stopped-idle and kept-unverifiable separately", async () => {
    const kv = fakeKv({
      "rhandle:cf-runner-b1": binding("h-b1", 951),
      "rhandle:cf-runner-b2": binding("h-b2", 952),
      "rhandle:cf-runner-i1": binding("h-i1", 953),
      "rhandle:cf-runner-u1": binding("h-u1", 954),
    });
    const metrics = fakeMetrics();
    const verify = verifier({
      951: BUSY,
      952: BUSY,
      953: IDLE,
      954: { httpStatus: 500, runner: null },
    });
    await keepAliveLiveRunners(envWith(kv, metrics), verify);
    expect(metrics.counts.keepalive_renewed_busy).toBe(2);
    expect(metrics.counts.keepalive_stopped_idle).toBe(1);
    expect(metrics.counts.keepalive_renewed_unverifiable).toBe(1);
  });

  it("every new counter is REGISTERED, so a snapshot 0-fills it before it ever fires", async () => {
    // `placement_unconfirmed` was the warning here: a counter bumped at its seam but
    // absent from the fixed set reads as "no such signal" on a dashboard rather than
    // as zero. (It turned out to BE registered; four others were not — see
    // COUNTER_NAMES. This asserts ours are.)
    const { COUNTER_NAMES } = await import("../src/metrics");
    for (const n of [
      "keepalive_renewed_busy",
      "keepalive_stopped_idle",
      "keepalive_renewed_unverifiable",
    ]) {
      expect(COUNTER_NAMES as readonly string[]).toContain(n);
    }
  });

  it("no METRICS binding ⇒ the sweep still works (counters are default-off)", async () => {
    const kv = fakeKv({ "rhandle:cf-runner-nm": binding("h-nm", 961) });
    const verify = verifier({ 961: BUSY });
    await expect(keepAliveLiveRunners(envWith(kv), verify)).resolves.toBe(1);
  });
});

// ── the pure decision ───────────────────────────────────────────────────────

describe("runnerActivityVerdict (the pure decision)", () => {
  it("busy: true ⇒ busy, whatever the status says", () => {
    // A runner whose agent momentarily lost its connection reads `offline` while
    // GitHub still has a job assigned to it. `busy` is checked FIRST for that reason.
    expect(runnerActivityVerdict({ httpStatus: 200, runner: { status: "online", busy: true } }))
      .toBe("busy");
    expect(runnerActivityVerdict({ httpStatus: 200, runner: { status: "offline", busy: true } }))
      .toBe("busy");
    expect(runnerActivityVerdict({ httpStatus: 200, runner: { busy: true } })).toBe("busy");
  });

  it("online + not busy ⇒ idle; offline ⇒ idle; 404 ⇒ idle", () => {
    expect(runnerActivityVerdict({ httpStatus: 200, runner: { status: "online", busy: false } }))
      .toBe("idle");
    expect(runnerActivityVerdict({ httpStatus: 200, runner: { status: "offline", busy: false } }))
      .toBe("idle");
    expect(runnerActivityVerdict({ httpStatus: 404, runner: null })).toBe("idle");
  });

  it("everything inconclusive ⇒ unknown (the fail-safe direction)", () => {
    expect(runnerActivityVerdict(null)).toBe("unknown");
    expect(runnerActivityVerdict({ httpStatus: 500, runner: null })).toBe("unknown");
    expect(runnerActivityVerdict({ httpStatus: 403, runner: null })).toBe("unknown");
    expect(runnerActivityVerdict({ httpStatus: 429, runner: null })).toBe("unknown");
    expect(runnerActivityVerdict({ httpStatus: 200, runner: null })).toBe("unknown");
    expect(runnerActivityVerdict({ httpStatus: 200, runner: {} })).toBe("unknown");
    expect(runnerActivityVerdict({ httpStatus: 200, runner: { status: "online" } }))
      .toBe("unknown");
    // An UNDOCUMENTED status must never be read as idle. If GitHub adds one this
    // leaks slots (visible on a counter) instead of killing jobs.
    expect(runnerActivityVerdict({ httpStatus: 200, runner: { status: "draining", busy: false } }))
      .toBe("unknown");
  });
});

describe("parseRunnerBinding (both wire shapes)", () => {
  it("reads a record", () => {
    expect(parseRunnerBinding('{"h":"h-1","rid":7,"repo":"a/b","inst":"9"}')).toEqual({
      h: "h-1",
      rid: 7,
      repo: "a/b",
      inst: "9",
    });
  });

  it("reads a LEGACY bare handle as an unverifiable binding", () => {
    expect(parseRunnerBinding("h-legacy")).toEqual({
      h: "h-legacy",
      rid: undefined,
      repo: undefined,
      inst: undefined,
    });
  });

  it("rejects absent / malformed / handle-less values", () => {
    expect(parseRunnerBinding(null)).toBeNull();
    expect(parseRunnerBinding("")).toBeNull();
    expect(parseRunnerBinding("{not json")).toBeNull();
    expect(parseRunnerBinding('{"rid":7}')).toBeNull();
    expect(parseRunnerBinding('{"h":""}')).toBeNull();
  });

  it("drops wrong-typed fields rather than trusting them", () => {
    // A `rid` that is not a number would build a nonsense URL; better unverifiable.
    expect(parseRunnerBinding('{"h":"h-1","rid":"seven","repo":5}')).toEqual({
      h: "h-1",
      rid: undefined,
      repo: undefined,
      inst: undefined,
    });
  });
});
