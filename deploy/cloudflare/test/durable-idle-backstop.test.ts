// The durable idle backstop (2026-08-23 incident).
//
// THE DEFECT THIS PINS. `@cloudflare/containers` 0.3.7 keeps its idle deadline in
// `sleepAfterMs`, a bare in-memory field (container.js:1024), and the `Container`
// constructor calls `renewActivityTimeout()` UNCONDITIONALLY inside
// `blockConcurrencyWhile` (container.js:348-360). The alarm TIME is durable
// (`ctx.storage.setAlarm`); the deadline is not. So any re-instantiation of the
// Durable Object — an eviction, a Worker redeploy, or merely a `/v1/status` poll
// landing on a cold stub — silently rearms the full 15-minute window with no real
// activity, and `onActivityExpired()`/`stop()` is never reached at all.
//
// On top of that the SDK's `alarm()` calls `renewActivityTimeout()` immediately
// after firing `onActivityExpired()` (container.js:1566-1569, "so we don't spam
// calls here"), so the idle alarm is a self-perpetuating loop that cannot conclude
// anything. Three boxes reached 10.5 h against a 15-minute window while `stop()`
// was called roughly forty times with no effect.
//
// WHY THE EXISTING TESTS DID NOT CATCH IT. `test/keepalive-verified-busy.test.ts`
// asserts that an idle runner STOPS BEING RENEWED. That is a statement about our
// bookkeeping, not about the box, and it passed green through the whole incident.
// It also mocks `@cloudflare/containers` as `class {}`, so the SDK's deadline and
// alarm machinery are never exercised.
//
// THE CELL THAT MATTERS is "survives a DO re-instantiation": it builds a second
// instance over the SAME storage, exactly as the runtime does after an eviction,
// and asserts the backstop still fires. Every other cell here is a guard rail
// around that one.
//
// THE CONSTRAINT THAT SHAPES THE FIX. This code can destroy a customer's running
// job. So every uncertain branch must leave the box alone: no stamp, an unreadable
// state, or a container that is not running must all be no-ops. Cells 1, 7 and 8
// exist solely to pin that direction, and a future "tidy-up" that turns any of them
// into a reclaim must go red.
import { describe, it, expect, vi, beforeEach } from "vitest";

// Spies shared by the fake base class. The real SDK is not exercised here — what
// is under test is OUR decision layer sitting above it.
const baseSpies = {
  stop: vi.fn(async () => {}),
  destroy: vi.fn(async () => {}),
  getState: vi.fn(async () => ({ status: "running" })),
  renewActivityTimeout: vi.fn(() => {}),
  alarm: vi.fn(async () => {}),
};

vi.mock("@cloudflare/containers", () => ({
  Container: class {
    ctx: { storage: FakeStorage };
    env: unknown;
    constructor(ctx: { storage: FakeStorage }, env: unknown) {
      this.ctx = ctx;
      this.env = env;
      // Mirror the real constructor's unconditional rearm. Nothing in our code may
      // depend on this NOT happening.
      baseSpies.renewActivityTimeout();
    }
    stop = baseSpies.stop;
    destroy = baseSpies.destroy;
    getState = baseSpies.getState;
    renewActivityTimeout = baseSpies.renewActivityTimeout;
    alarm = baseSpies.alarm;
    start = vi.fn(async () => {});
  },
  getContainer: vi.fn(),
}));

import { RunnerContainer } from "../src/index";

// ── fixtures ────────────────────────────────────────────────────────────────

/** DO storage that OUTLIVES the object, which is the entire point of the
 *  re-instantiation cell: the runtime keeps storage and throws the instance away. */
class FakeStorage {
  map = new Map<string, unknown>();
  async get<T>(k: string): Promise<T | undefined> {
    return this.map.get(k) as T | undefined;
  }
  async put(k: string, v: unknown): Promise<void> {
    this.map.set(k, v);
  }
  async delete(k: string): Promise<void> {
    this.map.delete(k);
  }
}

const MINUTE = 60 * 1000;
/** Comfortably past the 45-minute backstop, and far past the 15-minute SDK window. */
const LONG_AGO_MS = 90 * MINUTE;

function make(storage = new FakeStorage()) {
  const c = new RunnerContainer({ storage } as never, {} as never);
  return { c, storage };
}

/** Put the durable clock `ms` in the past without going through keepAlive(). */
function idleFor(storage: FakeStorage, ms: number) {
  storage.map.set("corelink:lastActivityAt", Date.now() - ms);
}

beforeEach(() => {
  for (const s of Object.values(baseSpies)) s.mockClear();
  baseSpies.getState.mockImplementation(async () => ({ status: "running" }));
});

describe("durable idle backstop", () => {
  // ── Cell 1 — no stamp yet: stamp, and do nothing else ─────────────────────
  // A box spawned before this shipped has no durable clock. Judging it from an
  // assumed start time would make the very first alarm a potential killer.
  it("stamps and takes no action when there is no durable clock yet", async () => {
    const { c, storage } = make();
    await c.enforceDurableIdleBackstop();
    expect(baseSpies.stop).not.toHaveBeenCalled();
    expect(baseSpies.destroy).not.toHaveBeenCalled();
    expect(typeof storage.map.get("corelink:lastActivityAt")).toBe("number");
  });

  // ── Cell 2 — recent activity: nothing happens ─────────────────────────────
  it("does nothing while activity is recent", async () => {
    const { c, storage } = make();
    idleFor(storage, 5 * MINUTE);
    await c.enforceDurableIdleBackstop();
    expect(baseSpies.stop).not.toHaveBeenCalled();
    expect(baseSpies.destroy).not.toHaveBeenCalled();
  });

  // ── Cell 3 — past the window on a running box: soft stop first ────────────
  it("asks the container to stop the first time the window elapses", async () => {
    const { c, storage } = make();
    idleFor(storage, LONG_AGO_MS);
    await c.enforceDurableIdleBackstop();
    expect(baseSpies.stop).toHaveBeenCalledTimes(1);
    expect(baseSpies.destroy).not.toHaveBeenCalled();
    expect(storage.map.get("corelink:softStopCount")).toBe(1);
  });

  // ── Cell 4 — escalation: destroy() once stop() has demonstrably failed ────
  // This is the ceiling the incident lacked. `stop()` is SIGTERM-only and never
  // escalates on its own, so without this a stop()-defeating bug has no cost bound.
  it("escalates to destroy() after the soft stops have not worked", async () => {
    const { c, storage } = make();
    idleFor(storage, LONG_AGO_MS);
    await c.enforceDurableIdleBackstop(); // soft stop 1
    idleFor(storage, LONG_AGO_MS);
    await c.enforceDurableIdleBackstop(); // soft stop 2
    expect(baseSpies.destroy).not.toHaveBeenCalled();

    idleFor(storage, LONG_AGO_MS);
    await c.enforceDurableIdleBackstop(); // ceiling reached
    expect(baseSpies.destroy).toHaveBeenCalledTimes(1);
    expect(baseSpies.stop).toHaveBeenCalledTimes(2);
  });

  // ── Cell 5 — THE ONE THAT MATTERS: survives a DO re-instantiation ─────────
  // The runtime discards the instance and keeps the storage. The SDK's in-memory
  // deadline is rearmed by the constructor (asserted here, so this cell fails if
  // that stops being modelled) — and the backstop must be unmoved by it.
  it("still fires after the DO is re-instantiated and the SDK deadline is rearmed", async () => {
    const { storage } = make();
    idleFor(storage, LONG_AGO_MS);

    baseSpies.renewActivityTimeout.mockClear();
    const second = new RunnerContainer({ storage } as never, {} as never);
    expect(baseSpies.renewActivityTimeout).toHaveBeenCalled(); // the rearm really happened

    await second.enforceDurableIdleBackstop();
    expect(baseSpies.stop).toHaveBeenCalledTimes(1);
  });

  // ── Cell 6 — real work clears the escalation counter ─────────────────────
  // A box that is working again has not been ignoring anything. Letting a stale
  // count carry over would eventually destroy a healthy container.
  it("clears the soft-stop counter when the box is observed working", async () => {
    const { c, storage } = make();
    idleFor(storage, LONG_AGO_MS);
    await c.enforceDurableIdleBackstop();
    expect(storage.map.get("corelink:softStopCount")).toBe(1);

    await c.noteActivity();
    expect(storage.map.get("corelink:softStopCount")).toBeUndefined();

    idleFor(storage, LONG_AGO_MS);
    await c.enforceDurableIdleBackstop();
    expect(baseSpies.destroy).not.toHaveBeenCalled();
  });

  // ── Cell 7 — a container that is not running is left alone ───────────────
  it("takes no action when the container is not running", async () => {
    baseSpies.getState.mockImplementation(async () => ({ status: "stopped" }));
    const { c, storage } = make();
    idleFor(storage, LONG_AGO_MS);
    await c.enforceDurableIdleBackstop();
    expect(baseSpies.stop).not.toHaveBeenCalled();
    expect(baseSpies.destroy).not.toHaveBeenCalled();
  });

  // ── Cell 8 — an unreadable state is never grounds to kill ────────────────
  it("takes no action when the container state cannot be read", async () => {
    baseSpies.getState.mockImplementation(async () => {
      throw new Error("control plane unreachable");
    });
    const { c, storage } = make();
    idleFor(storage, LONG_AGO_MS);
    await c.enforceDurableIdleBackstop();
    expect(baseSpies.stop).not.toHaveBeenCalled();
    expect(baseSpies.destroy).not.toHaveBeenCalled();
  });

  // ── Cell 9 — keepAlive() records activity durably ────────────────────────
  // `renewActivityTimeout()` alone writes only the in-memory field, which is what
  // made the original deadline unreliable. The two must move together.
  it("keepAlive records the activity durably, not only in memory", async () => {
    const { c, storage } = make();
    c.keepAlive();
    await new Promise((r) => setTimeout(r, 0)); // the durable write is fire-and-forget
    expect(baseSpies.renewActivityTimeout).toHaveBeenCalled();
    expect(typeof storage.map.get("corelink:lastActivityAt")).toBe("number");
  });

  // ── Cell 10 — a bug in the backstop must not break the SDK alarm ─────────
  // Container lifecycle management runs through this alarm. Our addition failing
  // closed over it would be a far worse outage than the leak it prevents.
  it("still runs the SDK alarm when the backstop throws", async () => {
    const { c } = make();
    vi.spyOn(c, "enforceDurableIdleBackstop").mockRejectedValue(new Error("boom"));
    await c.alarm(undefined as never);
    expect(baseSpies.alarm).toHaveBeenCalledTimes(1);
  });
});
