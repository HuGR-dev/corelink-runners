// ─────────────────────────────────────────────────────────────────────────────
// STRANDED IN-FLIGHT JOBS — the loss nothing watched.
// ─────────────────────────────────────────────────────────────────────────────
//
// A runner box that dies MID-JOB is invisible to this Worker today:
//   • `jobPlacementVerdict` reads anything past `queued` as "placed" and the
//     reconciler DROPS the record;
//   • `listOrphanRunnerJobs` selects only `queued`;
//   • `recordOrphan` is written only from a SPAWN-time failure.
// So a box killed after a successful spawn enters no dead letter at all, and the
// first anyone hears of it is GitHub's own ~600 s timeout telling the CUSTOMER
// "the self-hosted runner lost communication with the server" — while our
// accounting (concurrency slot, per-job `cas:rw` PAT, KV bindings) leaks for hours
// because every release hangs off a `completed` webhook that never arrives.
//
// ⚠️ THE LOAD-BEARING CONTRACTS IN THIS FILE, in order of how much they cost when
// broken:
//
//   1. THE SWEEP TEARS NOTHING DOWN. On 2026-08-02 a teardown keyed on our own
//      bookkeeping SIGKILLed five live customer boxes. Cell 1 asserts that a
//      CONFIRMED strand — the case with the most apparent justification to reclaim
//      the box — issues zero `destroy()`/`stop()` calls.
//   2. IT CONCLUDES ONLY FROM GITHUB. A 404 on the runner is GitHub answering on
//      the wire; a timeout is `null`, a rate limit is 403/429, an outage is 5xx.
//      Cells 4a-4c assert none of those classify anything.
//   3. IT DOES NOT RE-DRIVE. A stranded record is TERMINAL; cell 6 asserts the
//      retry reconciler skips it. Re-driving in-flight work is a separate decision
//      that has not been made.
//
// If a future change makes an ambiguous answer actionable, cells 4a-4c go red.

import { describe, it, expect, vi, beforeEach } from "vitest";

// Every container-facing verb the SDK exposes, recorded. The point of the mock is
// not to make the sweep work — the sweep must never call any of these.
const containerCalls: string[] = [];
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((_ns: unknown, handle: string) => ({
    destroy: vi.fn(async () => {
      containerCalls.push(`destroy:${handle}`);
    }),
    stop: vi.fn(async () => {
      containerCalls.push(`stop:${handle}`);
    }),
    keepAlive: vi.fn(async () => {
      containerCalls.push(`keepAlive:${handle}`);
    }),
    startWithEnv: vi.fn(async () => {
      containerCalls.push(`start:${handle}`);
    }),
  })),
}));

import { ContainmentDO, detectStrandedInFlightJobs, retryOrphanedSpawns } from "../src/index";
import {
  runnerGoneVerdict,
  strandedJobVerdict,
  encodeRunnerBinding,
  parseRunnerBinding,
  type OrphanRecord,
} from "../src/lib";

const NOW = 1_800_000_000_000;
const RID = 4242;
const JOB = "99887766";
const REPO = "acme/widgets";
const INST = "5150";

const BINDING = encodeRunnerBinding({
  h: "do-handle-1",
  rid: RID,
  repo: REPO,
  inst: INST,
  jid: JOB,
  t: NOW - 300_000, // bound 5 minutes ago
});

// ── the fake world ───────────────────────────────────────────────────────────

function kvWith(raw: Record<string, string>) {
  const store = new Map<string, string>(Object.entries(raw));
  return {
    store,
    list: vi.fn(async ({ prefix }: { prefix: string }) => ({
      keys: [...store.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })),
    })),
    get: vi.fn(async (k: string) => store.get(k) ?? null),
    put: vi.fn(async (k: string, v: string) => {
      store.set(k, v);
    }),
    delete: vi.fn(async (k: string) => {
      store.delete(k);
    }),
  };
}

// Credential revocation is owned by ContainmentDO. The stranded sweep fixture
// must bind that authority so its four side-effect assertions exercise the same
// registration -> request -> revoke -> confirm contract as production.
class ContainmentFixtureStorage {
  private readonly map = new Map<string, unknown>();

  async get<T>(key: string): Promise<T | undefined> {
    return this.map.get(key) as T | undefined;
  }

  async put(key: string, value: unknown): Promise<void> {
    this.map.set(key, value);
  }

  async delete(key: string): Promise<void> {
    this.map.delete(key);
  }

  async list<T>(
    opts: { prefix?: string; startAfter?: string; limit?: number } = {},
  ): Promise<Map<string, T>> {
    const entries = [...this.map.entries()]
      .filter(([key]) => key.startsWith(opts.prefix ?? ""))
      .filter(([key]) => !opts.startAfter || key > opts.startAfter)
      .sort(([a], [b]) => a.localeCompare(b))
      .slice(0, opts.limit ?? Number.POSITIVE_INFINITY);
    return new Map(entries) as Map<string, T>;
  }

  async transaction<T>(fn: (storage: this) => Promise<T>): Promise<T> {
    return fn(this);
  }
}

function containmentFixture(kv: ReturnType<typeof kvWith>) {
  const storage = new ContainmentFixtureStorage();
  const instance = new ContainmentDO({ storage } as never, {} as never);
  const patId = kv.store.get(JOB);
  const tenant = kv.store.get(`jtenant:${JOB}`) ?? "fallback-tenant";
  const registered = patId
    ? instance.registerCredential({ jobId: JOB, tenant, patId })
    : Promise.resolve();
  const authority = {
    async closeJobCredentials(jobId: string) {
      await registered;
      return instance.closeJobCredentials(jobId);
    },
    async pendingCredentials(selection: { kind: "job"; jobId: string }, cursor?: string) {
      await registered;
      return instance.pendingCredentials(selection, cursor);
    },
    async requestCredentialRevocation(identity: { jobId: string; tenant: string; patId: string }) {
      await registered;
      return instance.requestCredentialRevocation(identity);
    },
    async confirmCredentialRevoked(identity: { jobId: string; tenant: string; patId: string }) {
      await registered;
      return instance.confirmCredentialRevoked(identity);
    },
  };
  return {
    idFromName: vi.fn(() => "global"),
    get: vi.fn(() => authority),
  };
}

const released: string[] = [];
const revokes: { url: string; body: unknown }[] = [];

function envWith(kv: ReturnType<typeof kvWith>) {
  return {
    RUNNER_JOB_PATS: kv,
    RUNNER_CONTAINER: {},
    CONTAINMENT: containmentFixture(kv),
    CRED_STASH: {
      idFromName: (_name: string) => "runner-credential",
      get: () => ({ wipe: async () => {} }),
    },
    // METRICS absent ⇒ bumpMetrics is a documented no-op.
    CONCURRENCY_SLOTS: {
      idFromName: (n: string) => n,
      get: () => ({
        release: async (jobId: string) => {
          released.push(jobId);
        },
      }),
    },
    // Both required for `revokeCompletedJob` to do anything at all.
    CORELINK_RUNNER_MINT_AUTH_KEY: "k",
    CORELINK_MINT_URL: "https://mint.test",
    CLW_TENANT: "fallback-tenant",
  } as never;
}

/** A live runner: GitHub still knows it, and it is executing a job. */
const runnerPresent = async () => ({ httpStatus: 200, runner: { status: "online", busy: true } });
/** GitHub has forgotten this runner — the box is gone. */
const runnerGone = async () => ({ httpStatus: 404, runner: null });

const jobInProgressOnOurRunner = async () => ({
  httpStatus: 200,
  job: { status: "in_progress", runner_id: RID, runner_name: "runner-1" },
});
const jobCompleted = async () => ({
  httpStatus: 200,
  job: { status: "completed", runner_id: RID, runner_name: "runner-1" },
});

beforeEach(() => {
  containerCalls.length = 0;
  released.length = 0;
  revokes.length = 0;
  // The only network the sweep itself performs is the D-9 revoke (both GitHub
  // verifiers are injected). Anything else reaching the wire is a bug.
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      revokes.push({ url: String(url), body: JSON.parse(String(init?.body ?? "null")) });
      return { ok: true, status: 200, text: async () => "", json: async () => ({}) } as never;
    }),
  );
});

// ── the pure verdicts (the authority argument, isolated) ─────────────────────

describe("runnerGoneVerdict — only a definitive 404 means the box is gone", () => {
  it("404 (GitHub answered: no such runner) ⇒ gone", () => {
    expect(runnerGoneVerdict({ httpStatus: 404, runner: null })).toBe("gone");
  });
  it("200 ⇒ present", () => {
    expect(runnerGoneVerdict({ httpStatus: 200, runner: { status: "online", busy: true } })).toBe(
      "present",
    );
  });
  it("a TRANSPORT failure is null, NEVER 404 ⇒ unknown", () => {
    expect(runnerGoneVerdict(null)).toBe("unknown");
  });
  it("rate limits and outages are 403/429/5xx, NEVER 404 ⇒ unknown", () => {
    for (const httpStatus of [401, 403, 429, 500, 502, 503]) {
      expect(runnerGoneVerdict({ httpStatus, runner: null })).toBe("unknown");
    }
  });
});

describe("strandedJobVerdict — GitHub must confirm the job→box link", () => {
  it("in_progress on OUR runner id ⇒ stranded", () => {
    expect(
      strandedJobVerdict({ httpStatus: 200, job: { status: "in_progress", runner_id: RID } }, RID),
    ).toBe("stranded");
  });
  it("in_progress on a DIFFERENT runner ⇒ unknown (the 2026-08-02 permutation)", () => {
    expect(
      strandedJobVerdict({ httpStatus: 200, job: { status: "in_progress", runner_id: 7 } }, RID),
    ).toBe("unknown");
  });
  it("completed / queued ⇒ not_stranded", () => {
    expect(strandedJobVerdict({ httpStatus: 200, job: { status: "completed" } }, RID)).toBe(
      "not_stranded",
    );
    expect(strandedJobVerdict({ httpStatus: 200, job: { status: "queued" } }, RID)).toBe(
      "not_stranded",
    );
  });
  it("an undocumented status is never read as stranded", () => {
    expect(
      strandedJobVerdict({ httpStatus: 200, job: { status: "hibernating", runner_id: RID } }, RID),
    ).toBe("unknown");
  });
  it("no answer / non-200 ⇒ unknown", () => {
    expect(strandedJobVerdict(null, RID)).toBe("unknown");
    expect(strandedJobVerdict({ httpStatus: 429, job: null }, RID)).toBe("unknown");
  });
});

describe("RunnerBinding carries the candidate job, and legacy shapes still parse", () => {
  it("round-trips jid + t", () => {
    const b = parseRunnerBinding(BINDING);
    expect(b?.jid).toBe(JOB);
    expect(b?.t).toBe(NOW - 300_000);
  });
  it("a legacy bare handle still parses (and has no jid ⇒ unaskable)", () => {
    expect(parseRunnerBinding("do-handle-legacy")).toEqual({ h: "do-handle-legacy" });
  });
});

// ── the sweep ────────────────────────────────────────────────────────────────

describe("detectStrandedInFlightJobs", () => {
  it("cell 1 — runner GONE + job in_progress ⇒ STRANDED: slot released, PAT revoked, dead-letter written, and NOTHING torn down", async () => {
    const kv = kvWith({
      "rhandle:runner-1": BINDING,
      [JOB]: "pat-abc", // the jobId→patId revoke key written at mint time
      [`jtenant:${JOB}`]: "tenant-real",
    });
    const n = await detectStrandedInFlightJobs(
      envWith(kv),
      NOW,
      runnerGone,
      jobInProgressOnOurRunner,
    );

    expect(n).toBe(1);
    // (a) THE INVARIANT: not a single container verb was issued.
    expect(containerCalls).toEqual([]);
    // (b) the concurrency slot came back, by jobId
    expect(released).toEqual([JOB]);
    // (c) the per-job cas:rw PAT was revoked through the `completed` path, by
    //     pat_id, against the DERIVED tenant. The legacy KV projection remains
    //     present; ContainmentDO is the revocation authority and confirmation,
    //     rather than deleting mutable KV, is the terminal proof.
    expect(revokes).toHaveLength(1);
    expect(revokes[0].url).toBe("https://mint.test/internal/v1/runner/revoke");
    expect(revokes[0].body).toEqual({ pat_id: "pat-abc", owner_tenant: "tenant-real" });
    expect(kv.store.has(JOB)).toBe(true);
    // (d) the dead-letter, in the ONE existing OrphanRecord format, marked terminal
    const rec = JSON.parse(kv.store.get(`orphan:${JOB}`)!) as OrphanRecord;
    expect(rec.repo).toBe(REPO);
    expect(rec.installationId).toBe(INST);
    expect(rec.stranded).toBe(NOW);
    expect(rec.strandedRunner).toBe("runner-1");
    expect(rec.placedMs).toBeUndefined();
    // (e) the binding itself is left to expire normally — the sweep does not
    //     reach into the box's own bookkeeping.
    expect(kv.store.has("rhandle:runner-1")).toBe(true);
  });

  it("cell 1b — a stranded job's LOUD console.error names job, repo, runner and age", async () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    const kv = kvWith({
      "rhandle:runner-1": BINDING,
      [JOB]: "pat-abc",
      [`jtenant:${JOB}`]: "tenant-real",
    });
    await detectStrandedInFlightJobs(envWith(kv), NOW, runnerGone, jobInProgressOnOurRunner);
    const line = err.mock.calls.map((c) => String(c[0])).find((l) => l.includes("job_stranded"));
    expect(line).toBeDefined();
    const parsed = JSON.parse(line!);
    expect(parsed.level).toBe("error");
    expect(parsed.jobId).toBe(JOB);
    expect(parsed.repo).toBe(REPO);
    expect(parsed.runnerName).toBe("runner-1");
    expect(parsed.boundAgoMs).toBe(300_000);
    err.mockRestore();
  });

  it("cell 2 — runner GONE + job COMPLETED ⇒ NOT stranded, zero side effects (this is the ordinary ephemeral de-registration)", async () => {
    const kv = kvWith({
      "rhandle:runner-1": BINDING,
      [JOB]: "pat-abc",
    });
    const n = await detectStrandedInFlightJobs(envWith(kv), NOW, runnerGone, jobCompleted);
    expect(n).toBe(0);
    expect(containerCalls).toEqual([]);
    expect(released).toEqual([]);
    expect(revokes).toEqual([]);
    expect(kv.store.has(`orphan:${JOB}`)).toBe(false);
    expect(kv.store.get(JOB)).toBe("pat-abc"); // PAT untouched
  });

  it("cell 3 — a runner still REGISTERED and busy is untouched, and its job is never even asked about", async () => {
    const kv = kvWith({ "rhandle:runner-1": BINDING, [JOB]: "pat-abc" });
    const askJob = vi.fn(jobInProgressOnOurRunner);
    const n = await detectStrandedInFlightJobs(envWith(kv), NOW, runnerPresent, askJob);
    expect(n).toBe(0);
    expect(askJob).not.toHaveBeenCalled(); // one API call per binding, not two
    expect(containerCalls).toEqual([]);
    expect(released).toEqual([]);
    expect(revokes).toEqual([]);
    expect(kv.store.has(`orphan:${JOB}`)).toBe(false);
  });

  it("cell 4a — GitHub UNREACHABLE (transport failure ⇒ null) classifies nothing and retries next tick", async () => {
    const kv = kvWith({ "rhandle:runner-1": BINDING, [JOB]: "pat-abc" });
    const env = envWith(kv);
    const n = await detectStrandedInFlightJobs(env, NOW, async () => null, jobInProgressOnOurRunner);
    expect(n).toBe(0);
    expect(containerCalls).toEqual([]);
    expect(released).toEqual([]);
    expect(revokes).toEqual([]);
    expect(kv.store.has(`orphan:${JOB}`)).toBe(false);
    // Nothing was consumed: the SAME binding is still there for the next tick,
    // and the next tick — with an answer — does classify it.
    expect(kv.store.has("rhandle:runner-1")).toBe(true);
    const again = await detectStrandedInFlightJobs(env, NOW, runnerGone, jobInProgressOnOurRunner);
    expect(again).toBe(1);
  });

  it("cell 4b — RATE-LIMITED (403/429 on the runner) classifies nothing", async () => {
    for (const httpStatus of [403, 429, 500]) {
      containerCalls.length = 0;
      released.length = 0;
      revokes.length = 0;
      const kv = kvWith({ "rhandle:runner-1": BINDING, [JOB]: "pat-abc" });
      const n = await detectStrandedInFlightJobs(
        envWith(kv),
        NOW,
        async () => ({ httpStatus, runner: null }),
        jobInProgressOnOurRunner,
      );
      expect(n).toBe(0);
      expect(released).toEqual([]);
      expect(revokes).toEqual([]);
      expect(kv.store.has(`orphan:${JOB}`)).toBe(false);
    }
  });

  it("cell 4c — the runner is gone but the JOB read is rate-limited/ambiguous ⇒ still no classification", async () => {
    const kv = kvWith({ "rhandle:runner-1": BINDING, [JOB]: "pat-abc" });
    const n = await detectStrandedInFlightJobs(envWith(kv), NOW, runnerGone, async () => ({
      httpStatus: 429,
      job: null,
    }));
    expect(n).toBe(0);
    expect(containerCalls).toEqual([]);
    expect(released).toEqual([]);
    expect(revokes).toEqual([]);
    expect(kv.store.has(`orphan:${JOB}`)).toBe(false);
  });

  it("cell 4d — a THROWING verifier does not abandon the sweep, and does not classify", async () => {
    const kv = kvWith({ "rhandle:runner-1": BINDING });
    const n = await detectStrandedInFlightJobs(
      envWith(kv),
      NOW,
      async () => {
        throw new Error("boom");
      },
      jobInProgressOnOurRunner,
    );
    expect(n).toBe(0);
    expect(released).toEqual([]);
    expect(kv.store.has(`orphan:${JOB}`)).toBe(false);
  });

  it("cell 5 — the per-tick verification cap (40) is honoured", async () => {
    const raw: Record<string, string> = {};
    for (let i = 0; i < 60; i++) {
      raw[`rhandle:runner-${i}`] = encodeRunnerBinding({
        h: `h-${i}`,
        rid: RID + i,
        repo: REPO,
        inst: INST,
        jid: `job-${i}`,
        t: NOW - 1000,
      });
    }
    const askRunner = vi.fn(runnerPresent);
    const n = await detectStrandedInFlightJobs(
      envWith(kvWith(raw)),
      NOW,
      askRunner,
      jobInProgressOnOurRunner,
    );
    expect(n).toBe(0);
    expect(askRunner).toHaveBeenCalledTimes(40);
  });

  it("cell 5b — a job already classified STRANDED is not re-classified (no alarm every minute for 2 h)", async () => {
    const kv = kvWith({ "rhandle:runner-1": BINDING, [JOB]: "pat-abc" });
    const env = envWith(kv);
    expect(await detectStrandedInFlightJobs(env, NOW, runnerGone, jobInProgressOnOurRunner)).toBe(1);
    const askRunner = vi.fn(runnerGone);
    expect(
      await detectStrandedInFlightJobs(env, NOW + 60_000, askRunner, jobInProgressOnOurRunner),
    ).toBe(0);
    expect(askRunner).not.toHaveBeenCalled();
    expect(released).toEqual([JOB]); // released ONCE
  });

  it("cell 5c — an UNASKABLE binding (legacy bare handle / no jid) costs zero API calls", async () => {
    const kv = kvWith({
      "rhandle:legacy": "bare-handle",
      "rhandle:nojid": encodeRunnerBinding({ h: "h", rid: RID, repo: REPO, inst: INST }),
    });
    const askRunner = vi.fn(runnerGone);
    expect(await detectStrandedInFlightJobs(envWith(kv), NOW, askRunner, jobInProgressOnOurRunner)).toBe(0);
    expect(askRunner).not.toHaveBeenCalled();
  });

  it("cell 5d — an UNBOUND credential logs its skip rather than reading as a quiet tick", async () => {
    const info = vi.spyOn(console, "log").mockImplementation(() => {});
    const n = await detectStrandedInFlightJobs({ RUNNER_CONTAINER: {} } as never, NOW);
    expect(n).toBe(0);
    const line = info.mock.calls
      .map((c) => String(c[0]))
      .find((l) => l.includes("strand_sweep_skipped_unbound"));
    expect(line).toBeDefined();
    info.mockRestore();
  });
});

// ── the terminal contract ────────────────────────────────────────────────────

describe("a stranded dead-letter is TERMINAL", () => {
  it("cell 6 — retryOrphanedSpawns never re-drives a record marked `stranded`", async () => {
    const rec: OrphanRecord = {
      repo: REPO,
      installationId: INST,
      labels: ["corelink"],
      attempts: 0,
      firstRecordedMs: NOW - 60_000,
      stranded: NOW - 30_000,
      strandedRunner: "runner-1",
    };
    const kv = kvWith({ [`orphan:${JOB}`]: JSON.stringify(rec) });
    const drive = vi.fn(async () => {});
    const verify = vi.fn(async () => null);
    await retryOrphanedSpawns(envWith(kv), {} as never, NOW, drive, verify);
    expect(drive).not.toHaveBeenCalled();
    expect(verify).not.toHaveBeenCalled();
    // …and the record is left in place for visibility until it TTLs.
    expect(kv.store.has(`orphan:${JOB}`)).toBe(true);
  });
});
