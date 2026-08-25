// ─────────────────────────────────────────────────────────────────────────────
// STALE-BOX REAPER — the second layer of container termination.
// ─────────────────────────────────────────────────────────────────────────────
//
// Measured in prod 2026-08-23: three `standard-4` RunnerContainers were in state
// `running` for 10.2 h against a declared `sleepAfter = "15m"`, with every
// `keepalive_*` counter flat over a 180 s window. Two defences had failed at once:
//
//   1. `rhandle:` — the keep-alive binding — TTLs out with the job PAT (2 h), so
//      after 2 h the sweep cannot SEE the box at all: not to renew it, and not to
//      stop it.
//   2. The DO's own `sleepAfter` alarm, the sole remaining terminator, did not
//      fire. (Root cause of THAT is tracked separately; this file does not claim
//      to fix it.)
//
// `reapStaleBoxes` is the belt for #1: a durable `sbox:` record outlives the
// keep-alive binding, so an over-age box stays findable and can be actively
// destroyed rather than waited on.
//
// ⚠️ THE LOAD-BEARING CONTRACT IN THIS FILE is the fail-safe DIRECTION, which is
// deliberately the OPPOSITE of `keepAliveLiveRunners`. That sweep renews when it
// cannot verify (renewing on ignorance only wastes money). This one DESTROYS, so
// it must never act on ignorance — destroying a box that is really running a
// customer's job costs them the job. Reap ONLY on a definite "not busy".
//
// If a future change makes an unverifiable box reapable, cells 2-4 must go red.

import { describe, it, expect, vi, beforeEach } from "vitest";

const destroyed: string[] = [];
// Mutable container behaviour, so a cell can drive the two states cells 1-7
// never reach: a `destroy()` that THROWS, and what the box says afterwards.
// Same shape as `destroyed` above (a module-scope binding the mock factory
// closes over), so hoisting behaves identically.
const ctl = { destroyThrows: false, alive: false as boolean | "throw" };
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((_ns: unknown, handle: string) => ({
    destroy: vi.fn(async () => {
      if (ctl.destroyThrows) throw new Error("DO RPC transient");
      destroyed.push(handle);
    }),
    isAlive: vi.fn(async () => {
      if (ctl.alive === "throw") throw new Error("unreachable DO");
      return ctl.alive;
    }),
  })),
}));

import { reapStaleBoxes } from "../src/index";

const NOW = 1_800_000_000_000;
const TWO_H_MS = 7200 * 1000;

function kvWith(records: Record<string, unknown>) {
  const store = new Map<string, string>();
  for (const [k, v] of Object.entries(records)) store.set(k, JSON.stringify(v));
  return {
    store,
    list: vi.fn(async () => ({ keys: [...store.keys()].map((name) => ({ name })) })),
    get: vi.fn(async (k: string) => store.get(k) ?? null),
    delete: vi.fn(async (k: string) => {
      store.delete(k);
    }),
    put: vi.fn(async () => {}),
  };
}

function envWith(kv: ReturnType<typeof kvWith>) {
  // METRICS absent ⇒ bumpMetrics is a documented no-op, so the reaper is
  // exercised without a Durable Object stub.
  return { RUNNER_JOB_PATS: kv, RUNNER_CONTAINER: {} } as never;
}

const OLD = { h: "rh-old", rid: 7, repo: "o/r", inst: "42", t: NOW - TWO_H_MS - 60_000 };

beforeEach(() => {
  destroyed.length = 0;
  ctl.destroyThrows = false;
  ctl.alive = false;
});

describe("reapStaleBoxes", () => {
  it("cell 1 — reaps an over-age box GitHub reports IDLE, and clears its record", async () => {
    const kv = kvWith({ "sbox:runner-1": OLD });
    const n = await reapStaleBoxes(envWith(kv), NOW, async () => ({ httpStatus: 200, runner: { status: "online", busy: false } }));
    expect(n).toBe(1);
    expect(destroyed).toEqual(["rh-old"]);
    expect(kv.store.has("sbox:runner-1")).toBe(false);
  });

  it("cell 2 — NEVER reaps when GitHub says the runner is BUSY (it has a job)", async () => {
    const kv = kvWith({ "sbox:runner-1": OLD });
    const n = await reapStaleBoxes(envWith(kv), NOW, async () => ({ httpStatus: 200, runner: { status: "online", busy: true } }));
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
    expect(kv.store.has("sbox:runner-1")).toBe(true);
  });

  it("cell 3 — NEVER reaps an UNVERIFIABLE box (no runner id ⇒ no definite answer)", async () => {
    const kv = kvWith({ "sbox:runner-1": { ...OLD, rid: undefined } });
    const verify = vi.fn();
    const n = await reapStaleBoxes(envWith(kv), NOW, verify as never);
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
    expect(verify).not.toHaveBeenCalled();
  });

  it("cell 4 — NEVER reaps when the verifier THROWS (ignorance is not idleness)", async () => {
    const kv = kvWith({ "sbox:runner-1": OLD });
    const n = await reapStaleBoxes(envWith(kv), NOW, async () => {
      throw new Error("github 500");
    });
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
    expect(kv.store.has("sbox:runner-1")).toBe(true);
  });

  it("cell 5 — NEVER reaps a box younger than JOB_PAT_TTL_S, even when idle", async () => {
    const kv = kvWith({ "sbox:runner-1": { ...OLD, t: NOW - 60_000 } });
    const verify = vi.fn();
    const n = await reapStaleBoxes(envWith(kv), NOW, verify as never);
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
    expect(verify).not.toHaveBeenCalled();
  });

  it("cell 6 — an UNKNOWN verdict (null observation) is NOT idle and is left alone", async () => {
    const kv = kvWith({ "sbox:runner-1": OLD });
    const n = await reapStaleBoxes(envWith(kv), NOW, async () => null);
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
  });

  // ── Cells 8-10: a THROW from destroy() is not proof the box is down. ────────
  // This sweep runs only past the `rhandle:` TTL, so `sbox:` is the LAST
  // cron-visible handle for the box. Deleting it on a transient DO error strands
  // a RUNNING container forever — the exact 10.2 h shape this file was written
  // over, reintroduced by the code meant to fix it. Before 2026-08-25 the catch
  // deleted the record unconditionally; cell 8 goes RED against that version.
  it("cell 8 — destroy() THREW and the box is STILL ALIVE ⇒ keep the record and retry", async () => {
    ctl.destroyThrows = true;
    ctl.alive = true;
    const kv = kvWith({ "sbox:runner-1": OLD });
    const n = await reapStaleBoxes(envWith(kv), NOW, async () => ({ httpStatus: 200, runner: { status: "online", busy: false } }));
    expect(n).toBe(0);
    expect(destroyed).toEqual([]);
    expect(kv.store.has("sbox:runner-1")).toBe(true);
  });

  it("cell 9 — destroy() threw but the box is CONFIRMED DOWN ⇒ the record is cleared", async () => {
    ctl.destroyThrows = true;
    ctl.alive = false;
    const kv = kvWith({ "sbox:runner-1": OLD });
    const n = await reapStaleBoxes(envWith(kv), NOW, async () => ({ httpStatus: 200, runner: { status: "online", busy: false } }));
    expect(n).toBe(0);
    expect(kv.store.has("sbox:runner-1")).toBe(false);
  });

  it("cell 10 — an UNREACHABLE DO (isAlive itself throws) counts as down, not as alive", async () => {
    ctl.destroyThrows = true;
    ctl.alive = "throw";
    const kv = kvWith({ "sbox:runner-1": OLD });
    await reapStaleBoxes(envWith(kv), NOW, async () => ({ httpStatus: 200, runner: { status: "online", busy: false } }));
    expect(kv.store.has("sbox:runner-1")).toBe(false);
  });

  it("cell 7 — no KV binding ⇒ no-op, never throws", async () => {
    expect(await reapStaleBoxes({} as never, NOW)).toBe(0);
  });
});
