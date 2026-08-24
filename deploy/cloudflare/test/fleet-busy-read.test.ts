// GET /internal/v1/fleet/busy — the pre-roll deploy gate's authority (2026-08-24).
//
// WHAT THIS PINS, AND WHY IT IS NOT A DUPLICATE OF keepalive-verified-busy.
// That file pins a sweep whose ignorance resolves to "keep renewing" — leaking a
// container slot. THIS endpoint's ignorance decides whether a `wrangler deploy`
// rolls the fleet, i.e. whether live customer jobs are SIGTERMed and SIGKILLed
// 15 minutes later. The fail-safe direction is therefore INVERTED, and the cells
// below exist to keep it inverted: an unverifiable runner must land in
// `unverifiable`, never be quietly absent from `busy`.
//
// The gate is fail-closed on the CALLER side too (busy === 0 && unverifiable === 0),
// so a regression that turned an unknown box into a silent zero would read as a
// green "fleet idle" and roll over live work — the exact defect the endpoint
// exists to prevent, and the reason each unverifiable shape gets its own cell.
import { describe, it, expect, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));

import worker, { fleetBusySnapshot, type Env } from "../src/index";
import { encodeRunnerBinding, type RunnerObservation } from "../src/lib";

const KEY = "fleet-busy-read-key-001";
const ctx = {
  waitUntil: () => {},
  passThroughOnException: () => {},
} as unknown as ExecutionContext;

// A KV stub over the `rhandle:` prefix the fleet enumeration reads.
function fakeKv(seed: Record<string, string> = {}) {
  const store = new Map<string, string>(Object.entries(seed));
  return {
    store,
    get: vi.fn(async (k: string) => store.get(k) ?? null),
    put: vi.fn(async () => {}),
    delete: vi.fn(async () => {}),
    list: vi.fn(async ({ prefix }: { prefix: string }) => ({
      keys: [...store.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })),
      list_complete: true,
    })),
  };
}

// A full, verifiable binding: runner id + repo + installation.
function binding(rid: number, repo = "HuGR-Labs/corelink-runners") {
  return encodeRunnerBinding({ h: `h-${rid}`, rid, repo, inst: "150584374", t: Date.now() });
}

function env(overrides: Partial<Env> = {}): Env {
  return { CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-secret-001", ...overrides } as Env;
}

function req(headers: Record<string, string> = {}) {
  return new Request("https://w/internal/v1/fleet/busy", { headers });
}

// Canned GitHub answers, keyed by runner id.
function verifier(byId: Record<number, RunnerObservation | null>) {
  return vi.fn(async (_e: Env, _r: string, rid: number) => byId[rid] ?? null);
}

const BUSY: RunnerObservation = { httpStatus: 200, runner: { status: "online", busy: true } };
const IDLE: RunnerObservation = { httpStatus: 200, runner: { status: "online", busy: false } };

// ── the gate ────────────────────────────────────────────────────────────────

describe("GET /internal/v1/fleet/busy — dedicated key, default-off, fail-closed", () => {
  it("404 when FLEET_BUSY_READ_KEY is unset (route invisible)", async () => {
    const resp = await worker.fetch(req({ "x-corelink-internal-auth": KEY }), env(), ctx);
    expect(resp.status).toBe(404);
  });

  it("401 with a wrong key", async () => {
    const resp = await worker.fetch(
      req({ "x-corelink-internal-auth": "wrong" }),
      env({ FLEET_BUSY_READ_KEY: KEY }),
      ctx,
    );
    expect(resp.status).toBe(401);
  });

  it("401 with no header at all", async () => {
    const resp = await worker.fetch(req(), env({ FLEET_BUSY_READ_KEY: KEY }), ctx);
    expect(resp.status).toBe(401);
  });

  it("does NOT accept the spawn-CONTROL bearer (separate credential domain)", async () => {
    const resp = await worker.fetch(
      req({ authorization: "Bearer spawn-secret-001" }),
      env({ FLEET_BUSY_READ_KEY: KEY }),
      ctx,
    );
    expect(resp.status).toBe(401);
  });

  it("does NOT accept the metrics observability key (separate credential domain)", async () => {
    const resp = await worker.fetch(
      req({ "x-corelink-internal-auth": "metrics-obs-key-001" }),
      env({ FLEET_BUSY_READ_KEY: KEY, METRICS_OBSERVABILITY_KEY: "metrics-obs-key-001" }),
      ctx,
    );
    expect(resp.status).toBe(401);
  });

  it("200 with the right key, and the documented body shape", async () => {
    const resp = await worker.fetch(
      req({ "x-corelink-internal-auth": KEY }),
      env({ FLEET_BUSY_READ_KEY: KEY, RUNNER_JOB_PATS: fakeKv() as never }),
      ctx,
    );
    expect(resp.status).toBe(200);
    const body = (await resp.json()) as Record<string, unknown>;
    // EXACTLY the four documented fields — no extra field can leak in unnoticed.
    expect(Object.keys(body).sort()).toEqual(["busy", "checked", "runners", "unverifiable"]);
    expect(body).toEqual({ busy: 0, runners: [], checked: 0, unverifiable: 0 });
  });

  it("no KV binding ⇒ unverifiable, NOT an idle fleet", async () => {
    // The caller's rule (busy === 0 && unverifiable === 0) must refuse here.
    const resp = await worker.fetch(
      req({ "x-corelink-internal-auth": KEY }),
      env({ FLEET_BUSY_READ_KEY: KEY }), // RUNNER_JOB_PATS absent
      ctx,
    );
    expect(resp.status).toBe(200);
    const body = (await resp.json()) as { busy: number; unverifiable: number };
    expect(body.busy).toBe(0);
    expect(body.unverifiable).toBeGreaterThan(0);
  });
});

// ── the snapshot itself ─────────────────────────────────────────────────────

describe("fleetBusySnapshot — counting and naming", () => {
  it("counts and NAMES a busy runner (repo + name, nothing else)", async () => {
    const kv = fakeKv({ "rhandle:cl-abc123": binding(11) });
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      verifier({ 11: BUSY }),
    );
    expect(snap.busy).toBe(1);
    expect(snap.runners).toEqual([{ name: "cl-abc123", repo: "HuGR-Labs/corelink-runners" }]);
    expect(snap.checked).toBe(1);
    expect(snap.unverifiable).toBe(0);
  });

  it("an idle runner is counted in `checked` but not in `busy`", async () => {
    const kv = fakeKv({ "rhandle:cl-idle": binding(12) });
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      verifier({ 12: IDLE }),
    );
    expect(snap).toEqual({ busy: 0, runners: [], checked: 1, unverifiable: 0 });
  });

  it("a mixed fleet names only the busy boxes", async () => {
    const kv = fakeKv({
      "rhandle:cl-a": binding(1),
      "rhandle:cl-b": binding(2),
      "rhandle:cl-c": binding(3),
    });
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      verifier({ 1: IDLE, 2: BUSY, 3: IDLE }),
    );
    expect(snap.busy).toBe(1);
    expect(snap.runners.map((r) => r.name)).toEqual(["cl-b"]);
    expect(snap.checked).toBe(3);
    expect(snap.unverifiable).toBe(0);
  });
});

// ── the inverted fail-safe: ignorance must never read as idle ───────────────

describe("fleetBusySnapshot — an unverifiable runner is NOT idle", () => {
  const cases: Array<[string, string, RunnerObservation | null]> = [
    // No answer at all (no credential / network throw / unparseable body).
    ["GitHub produced no answer", binding(21), null],
    // A non-404 error — 403 (rate limited / permission), 429, 5xx.
    ["GitHub returned 403", binding(22), { httpStatus: 403, runner: null }],
    ["GitHub returned 429", binding(23), { httpStatus: 429, runner: null }],
    ["GitHub returned 500", binding(24), { httpStatus: 500, runner: null }],
    // A 200 whose body we do not recognise.
    ["the body has a non-boolean busy", binding(25), { httpStatus: 200, runner: {} }],
    // An undocumented status string must never be READ as idle.
    [
      "GitHub reports an undocumented status",
      binding(26),
      { httpStatus: 200, runner: { status: "provisioning", busy: false } },
    ],
  ];

  for (const [what, value, obs] of cases) {
    it(`${what} ⇒ unverifiable, and busy stays 0 (not "idle")`, async () => {
      const kv = fakeKv({ "rhandle:cl-unknown": value });
      const snap = await fleetBusySnapshot(
        { RUNNER_JOB_PATS: kv } as unknown as Env,
        vi.fn(async () => obs),
      );
      expect(snap.busy).toBe(0);
      expect(snap.runners).toEqual([]);
      expect(snap.checked).toBe(1);
      // THE ASSERTION. Absent this, the caller reads {busy:0, unverifiable:0} and
      // rolls the fleet on a runner nobody could ask about.
      expect(snap.unverifiable).toBe(1);
    });
  }

  it("a legacy bare-handle binding (no runner id) is unverifiable", async () => {
    const kv = fakeKv({ "rhandle:cl-legacy": "just-a-do-handle" });
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      vi.fn(async () => BUSY), // never consulted — nothing to ask about
    );
    expect(snap).toEqual({ busy: 0, runners: [], checked: 1, unverifiable: 1 });
  });

  it("a cold spawn (no installation) is unverifiable", async () => {
    const kv = fakeKv({
      "rhandle:cl-cold": encodeRunnerBinding({ h: "h-cold", rid: 31, repo: "o/r" }),
    });
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      vi.fn(async () => BUSY),
    );
    expect(snap.unverifiable).toBe(1);
    expect(snap.busy).toBe(0);
  });

  it("an unparseable KV value is unverifiable, never skipped", async () => {
    const kv = fakeKv({ "rhandle:cl-torn": "{not json" });
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      vi.fn(async () => BUSY),
    );
    expect(snap).toEqual({ busy: 0, runners: [], checked: 1, unverifiable: 1 });
  });

  it("a verifier that THROWS resolves to unverifiable and does not abandon the list", async () => {
    const kv = fakeKv({ "rhandle:cl-x": binding(41), "rhandle:cl-y": binding(42) });
    const verify = vi.fn(async (_e: Env, _r: string, rid: number) => {
      if (rid === 41) throw new Error("boom");
      return BUSY;
    });
    const snap = await fleetBusySnapshot({ RUNNER_JOB_PATS: kv } as unknown as Env, verify);
    expect(snap.unverifiable).toBe(1);
    // The second box was still examined — a throw mid-loop must not shrink `busy`.
    expect(snap.busy).toBe(1);
    expect(snap.runners.map((r) => r.name)).toEqual(["cl-y"]);
    expect(snap.checked).toBe(2);
  });

  it("a failing kv.list is unverifiable, never an idle fleet", async () => {
    const kv = { ...fakeKv(), list: vi.fn(async () => { throw new Error("kv down"); }) };
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      vi.fn(async () => IDLE),
    );
    expect(snap.busy).toBe(0);
    expect(snap.unverifiable).toBeGreaterThan(0);
  });

  it("bindings past the per-tick verification cap (40) are unverifiable, not idle", async () => {
    // The cap is KEEPALIVE_MAX_VERIFY_PER_TICK, shared with the sweep — this
    // deliberately does NOT add a second, contradicting ceiling. `checked`
    // surfaces it: 45 examined, 5 of them never asked about.
    const seed: Record<string, string> = {};
    for (let i = 0; i < 45; i++) seed[`rhandle:cl-${String(i).padStart(3, "0")}`] = binding(100 + i);
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: fakeKv(seed) } as unknown as Env,
      vi.fn(async () => IDLE),
    );
    expect(snap.checked).toBe(45);
    expect(snap.busy).toBe(0);
    expect(snap.unverifiable).toBe(5);
  });

  it("a TRUNCATED key list is unverifiable — under-counting must not read as idle", async () => {
    const kv = {
      ...fakeKv(),
      list: vi.fn(async () => ({ keys: [], list_complete: false })), // no cursor
    };
    const snap = await fleetBusySnapshot(
      { RUNNER_JOB_PATS: kv } as unknown as Env,
      vi.fn(async () => IDLE),
    );
    expect(snap.unverifiable).toBeGreaterThan(0);
  });
});

// ── disclosure ──────────────────────────────────────────────────────────────

describe("fleetBusySnapshot — discloses no credential material", () => {
  it("the response carries only runner name + repo — no handle, token, job or tenant", async () => {
    const kv = fakeKv({
      "rhandle:cl-secretbox": encodeRunnerBinding({
        h: "do-handle-must-not-leak",
        rid: 51,
        repo: "HuGR-Labs/corelink-runners",
        inst: "150584374",
        jid: "job-9999",
        t: 1,
      }),
    });
    const resp = await worker.fetch(
      req({ "x-corelink-internal-auth": KEY }),
      env({ FLEET_BUSY_READ_KEY: KEY, RUNNER_JOB_PATS: kv as never }) as Env,
      ctx,
    );
    const raw = await resp.text();
    // The gating key itself, the DO handle, the installation id and the job id are
    // all in scope of this code path; none may appear on the wire.
    for (const secret of ["do-handle-must-not-leak", "150584374", "job-9999", KEY, "spawn-secret-001"]) {
      expect(raw).not.toContain(secret);
    }
    const body = JSON.parse(raw) as { runners: Record<string, unknown>[] };
    for (const r of body.runners) expect(Object.keys(r).sort()).toEqual(["name", "repo"]);
  });
});
