// SJ-5 — CONCURRENCY-SLOT ADMISSION exhausted to the atom ("250% coverage" depth run).
//
// Concurrency is the BILLING SKU, so the ceiling MUST be real: not best-effort, not
// fail-open at capacity, and enforced on BOTH warm mints (per-tenant entitlement,
// clamped to the physical FLEET) AND cold spawns (per-repo COLD_REPO_CAP — the old
// KV path left cold spawns UNCAPPED). This file drives every layer of that admission:
//
//   • the PURE decision core   — decideSlotAcquire / releaseSlotByJob (src/lib.ts),
//     every cap combination, ordering, expiry-prune, idempotency, and a seeded
//     random-interleaving property that the live count NEVER exceeds either cap.
//   • the ATOMIC DO wrapper     — the REAL ConcurrencySlotsDO.acquire/release over a
//     Map-backed strongly-consistent storage stub (persists decideSlotAcquire.slots).
//   • the SELECTION + FAIL-OPEN — the internal acquireConcurrencySlot, exercised
//     end-to-end through the real `worker.fetch` /webhook drive: warm-vs-cold key/cap
//     selection, a THROWN DO error ⇒ fail-open ADMIT (never block a legit job), and a
//     clean {admitted:false} ⇒ HONORED refusal (not fail-open).
//
// NEW FILE. Read-only on src; touches no other test file. `@cloudflare/containers`
// pulls `cloudflare:workers`, so we vi.mock it (mirrors webhook-route.test.ts) purely
// to make src/index.ts importable under node vitest + to observe container spawns.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// ── Test double for @cloudflare/containers (mirrors webhook-route.test.ts) ─────
interface FakeContainer {
  ns: unknown;
  handle: string;
  startWithEnv: ReturnType<typeof vi.fn>;
  teardown: ReturnType<typeof vi.fn>;
}
let containers: FakeContainer[] = [];
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((ns: unknown, handle: string): FakeContainer => {
    const c: FakeContainer = {
      ns,
      handle,
      startWithEnv: vi.fn(async () => {}),
      teardown: vi.fn(async () => {}),
    };
    containers.push(c);
    return c;
  }),
}));

// Import AFTER the mock is registered.
import worker, { ConcurrencySlotsDO, type Env } from "../src/index";
import { getContainer } from "@cloudflare/containers";
import {
  decideSlotAcquire,
  releaseSlotByJob,
  SLOT_TTL_S,
  FLEET_MAX_CONCURRENCY,
  COLD_REPO_CAP,
  type SlotRecord,
} from "../src/lib";
import { makeWorkerAuthorities } from "./helpers/worker-authorities";

const ns = <T>(value: T) => ({ idFromName: vi.fn(() => "global"), get: vi.fn(() => value) });

const NOW = 1_800_000_000_000;
const TTL = SLOT_TTL_S * 1000;
// A fleet cap large enough to never bind, isolating the PER-KEY cap under test.
const BIG_FLEET = 1_000;

// A live slot expiring `ttl` ms after `at` (default NOW + TTL).
function slot(key: string, jobId: string, expiresMs = NOW + TTL): SlotRecord {
  return { key, jobId, expiresMs };
}
// Count live (unexpired) slots overall / per key, at `now`.
const liveCount = (s: SlotRecord[], now = NOW) => s.filter((x) => x.expiresMs > now).length;
const liveKeyCount = (s: SlotRecord[], key: string, now = NOW) =>
  s.filter((x) => x.expiresMs > now && x.key === key).length;

// ════════════════════════════════════════════════════════════════════════════
// PART A — the PURE decideSlotAcquire, exhausted (cells 1-3,5-10,14 + property).
// ════════════════════════════════════════════════════════════════════════════
describe("SJ-5 · decideSlotAcquire — admission decision core", () => {
  // ── Cell 1 — admit when perKey < cap AND fleet < fleetCap ──────────────────
  it("cell1: ADMITS when perKey count < cap AND fleet < fleetCap, appending the slot", () => {
    const d = decideSlotAcquire([slot("t", "j1")], "t", "j2", 2, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.reason).toBeUndefined();
    expect(d.slots).toEqual([slot("t", "j1"), slot("t", "j2")]);
  });

  it("cell1b: ADMITS the very first slot from an empty ledger", () => {
    const d = decideSlotAcquire([], "t", "j1", 1, 1, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.slots).toEqual([slot("t", "j1")]);
  });

  // ── Cell 2 — reject over_key_cap AT the per-key cap (fleet has room) ────────
  it("cell2: REJECTS over_key_cap exactly AT the per-key cap while the fleet has room", () => {
    const slots = [slot("t", "j1"), slot("t", "j2")];
    const d = decideSlotAcquire(slots, "t", "j3", 2, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_key_cap");
    expect(d.slots).toEqual(slots); // unchanged — nothing appended
  });

  it("cell2b: the per-key cap binds one slot BELOW where the fleet would (cap<fleet)", () => {
    // perKey=1 is full at cap=1; fleet has 19 free. The KEY cap must fire first.
    const d = decideSlotAcquire([slot("t", "j1")], "t", "j2", 1, 20, NOW, TTL);
    expect(d.reason).toBe("over_key_cap");
  });

  // ── Cell 3 — reject over_fleet_cap: under per-key but fleet FULL ────────────
  it("cell3: REJECTS over_fleet_cap when this key is UNDER its cap but the fleet is FULL", () => {
    // 2 slots under DIFFERENT keys, fleetCap=2. The new key's own count is 0 (< 5),
    // yet the global fleet is full ⇒ the FLEET cap fires.
    const slots = [slot("a", "j1"), slot("b", "j2")];
    const d = decideSlotAcquire(slots, "c", "j3", 5, 2, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_fleet_cap");
    expect(d.slots).toEqual(slots);
  });

  it("cell3b: per-key cap is checked BEFORE fleet cap (both full ⇒ over_key_cap wins)", () => {
    // Same key at cap=2 AND fleet at cap=2 — the ordering guarantee: key first.
    const slots = [slot("t", "j1"), slot("t", "j2")];
    const d = decideSlotAcquire(slots, "t", "j3", 2, 2, NOW, TTL);
    expect(d.reason).toBe("over_key_cap");
  });

  // ── Cell 5 — expired-slot pruning on acquire frees capacity ────────────────
  it("cell5: PRUNES an expired slot on acquire, freeing capacity that was 'full'", () => {
    // Key at cap=1 but its one slot is EXPIRED ⇒ prune ⇒ admit (capacity freed).
    const slots = [slot("t", "expired", NOW - 1)];
    const d = decideSlotAcquire(slots, "t", "fresh", 1, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.slots.map((s) => s.jobId)).toEqual(["fresh"]); // stale dropped, fresh added
  });

  it("cell5b: an expired slot does NOT count toward the FLEET cap either", () => {
    // fleetCap=1, one expired slot on another key ⇒ prune ⇒ admit.
    const d = decideSlotAcquire([slot("a", "old", NOW - 1)], "b", "new", 5, 1, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.slots.map((s) => s.jobId)).toEqual(["new"]);
  });

  it("cell5c: the returned slot list is always the PRUNED live set on a refusal too", () => {
    const slots = [slot("t", "live"), slot("t", "dead", NOW - 1)];
    const d = decideSlotAcquire(slots, "t", "j3", 1, BIG_FLEET, NOW, TTL);
    // perKey live = 1 = cap ⇒ refuse; but the dead slot is still pruned in `slots`.
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_key_cap");
    expect(d.slots).toEqual([slot("t", "live")]);
  });

  // ── Cell 6 (math) — warm clamp: cap = min(entitlement, FLEET) ──────────────
  it("cell6-math: the warm clamp min(entitlement, FLEET) BINDS when entitlement > FLEET", () => {
    // Prove the clamp value the wrapper computes: an entitlement ABOVE the fleet cap
    // clamps down to FLEET. (Uses 500 so the scenario holds after the fleet raise to
    // 250 — the value only needs to exceed FLEET_MAX_CONCURRENCY.)
    const entitlement = FLEET_MAX_CONCURRENCY + 250;
    const perKeyCap = Math.min(entitlement, FLEET_MAX_CONCURRENCY);
    expect(perKeyCap).toBe(FLEET_MAX_CONCURRENCY);
    // And fill to exactly that clamp ⇒ the (FLEET+1)-th on the SAME key refuses.
    const slots = Array.from({ length: perKeyCap }, (_, i) => slot("tenant", `j${i}`));
    const d = decideSlotAcquire(slots, "tenant", "over", perKeyCap, FLEET_MAX_CONCURRENCY, NOW, TTL);
    expect(d.admitted).toBe(false);
    // At the clamp, key and fleet are BOTH full; key is checked first.
    expect(d.reason).toBe("over_key_cap");
  });

  it("cell6-math-b: when entitlement < FLEET the ENTITLEMENT binds (customer's bought N)", () => {
    const entitlement = 3;
    const perKeyCap = Math.min(entitlement, FLEET_MAX_CONCURRENCY);
    expect(perKeyCap).toBe(3);
    const slots = [slot("t", "a"), slot("t", "b"), slot("t", "c")];
    const d = decideSlotAcquire(slots, "t", "d", perKeyCap, FLEET_MAX_CONCURRENCY, NOW, TTL);
    expect(d.reason).toBe("over_key_cap"); // bought 3, 4th refused though fleet has room
  });

  // ── Cell 7 (math) — cold spawn IS capped at COLD_REPO_CAP (the old bug) ─────
  it("cell7-math: a COLD repo key is now CAPPED at COLD_REPO_CAP (was unlimited)", () => {
    const key = "repo:owner/name";
    const slots = Array.from({ length: COLD_REPO_CAP }, (_, i) => slot(key, `c${i}`));
    expect(liveKeyCount(slots, key)).toBe(COLD_REPO_CAP);
    const d = decideSlotAcquire(slots, key, "cN", COLD_REPO_CAP, FLEET_MAX_CONCURRENCY, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_key_cap"); // the cold repo can no longer spawn unbounded
  });

  it("cell7-math-b: a cold repo UNDER COLD_REPO_CAP still admits", () => {
    const key = "repo:owner/name";
    const slots = Array.from({ length: COLD_REPO_CAP - 1 }, (_, i) => slot(key, `c${i}`));
    const d = decideSlotAcquire(slots, key, "last", COLD_REPO_CAP, FLEET_MAX_CONCURRENCY, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(liveKeyCount(d.slots, key)).toBe(COLD_REPO_CAP); // exactly at the cap now
  });

  // ── Cell 8 — cross-key isolation, but BOTH count toward the fleet ──────────
  it("cell8: tenant A AT its cap does NOT block tenant B (per-key isolation)", () => {
    // A holds 2 (its cap); B is empty. fleet is big ⇒ B admits.
    const slots = [slot("A", "a1"), slot("A", "a2")];
    const d = decideSlotAcquire(slots, "B", "b1", 2, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.reason).toBeUndefined();
  });

  it("cell8b: A and B slots BOTH count toward the global fleet cap", () => {
    // fleetCap=3: A holds 2, B holds 1 ⇒ fleet full ⇒ B's next (under its OWN cap) refuses.
    const slots = [slot("A", "a1"), slot("A", "a2"), slot("B", "b1")];
    const d = decideSlotAcquire(slots, "B", "b2", 5, 3, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_fleet_cap"); // B under its per-key cap, blocked by the fleet
  });

  // ── Cell 9 — ordering/interleaving: acquire A, acquire B (same key cap1) ────
  it("cell9: acquire→refuse→release→re-acquire on a cap-1 key (full sequential replay)", () => {
    let s: SlotRecord[] = [];
    // acquire A
    let d = decideSlotAcquire(s, "k", "A", 1, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(true);
    s = d.slots;
    // acquire B (same key, cap 1) ⇒ REJECTED
    d = decideSlotAcquire(s, "k", "B", 1, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_key_cap");
    s = d.slots; // still just A
    expect(s.map((x) => x.jobId)).toEqual(["A"]);
    // release A
    s = releaseSlotByJob(s, "A", NOW);
    expect(s).toEqual([]);
    // re-acquire (B now fits) ⇒ ADMITTED
    d = decideSlotAcquire(s, "k", "B", 1, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.slots.map((x) => x.jobId)).toEqual(["B"]);
  });

  // ── Cell 10 — fill the fleet across MANY keys, next any-key rejects ─────────
  it("cell10: filling the fleet across MANY distinct keys ⇒ a fresh empty key still hits over_fleet_cap", () => {
    const fleet = FLEET_MAX_CONCURRENCY;
    // One slot each on `fleet` distinct keys ⇒ fleet is full, every per-key count = 1.
    const slots = Array.from({ length: fleet }, (_, i) => slot(`key-${i}`, `j-${i}`));
    expect(liveCount(slots)).toBe(fleet);
    // A brand-new key with a generous per-key cap: its own count is 0, yet refused.
    const d = decideSlotAcquire(slots, "brand-new-key", "jX", 10, fleet, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_fleet_cap");
  });

  // ── Cell 14 — boundaries: cap=0, fleetCap=0, huge N ────────────────────────
  it("cell14: perKeyCap=0 REJECTS every acquire (over_key_cap), even on an empty ledger", () => {
    const d = decideSlotAcquire([], "t", "j", 0, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_key_cap");
    expect(d.slots).toEqual([]);
  });

  it("cell14b: fleetCap=0 REJECTS every acquire (over_fleet_cap) when per-key allows it", () => {
    const d = decideSlotAcquire([], "t", "j", 5, 0, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_fleet_cap");
  });

  it("cell14c: huge N — admits up to a large cap and refuses the (N+1)-th, no overflow", () => {
    const N = 5000;
    const slots = Array.from({ length: N }, (_, i) => slot("t", `j${i}`));
    const at = decideSlotAcquire(slots, "t", "next", N, N + 10, NOW, TTL);
    expect(at.reason).toBe("over_key_cap"); // exactly at N
    const under = decideSlotAcquire(slots.slice(0, N - 1), "t", "next", N, N + 10, NOW, TTL);
    expect(under.admitted).toBe(true);
  });

  it("cell14d: a huge already-EXPIRED backlog prunes to empty and admits (no leak, no overflow)", () => {
    const dead = Array.from({ length: 4000 }, (_, i) => slot("t", `d${i}`, NOW - 1));
    const d = decideSlotAcquire(dead, "t", "fresh", 1, 1, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.slots).toEqual([slot("t", "fresh")]);
  });

  // ── Property — random interleavings NEVER exceed either cap ────────────────
  it("property: seeded random acquire/release interleavings never exceed per-key OR fleet caps", () => {
    // Deterministic PRNG (mulberry32) so a failure is reproducible.
    let seedState = 0x9e3779b9;
    const rnd = () => {
      seedState |= 0;
      seedState = (seedState + 0x6d2b79f5) | 0;
      let t = Math.imul(seedState ^ (seedState >>> 15), 1 | seedState);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    const KEYS = ["ta", "tb", "tc", "repo:x/y"];
    const PER_KEY = 3;
    const FLEET = 7;
    let s: SlotRecord[] = [];
    const held: { key: string; jobId: string }[] = [];
    let clock = NOW;
    let seq = 0;
    for (let step = 0; step < 4000; step++) {
      clock += Math.floor(rnd() * 5); // occasionally the same instant (races), sometimes ticks
      // Bias slightly toward acquires so the caps get pressured.
      const doAcquire = held.length === 0 || rnd() < 0.62;
      if (doAcquire) {
        const key = KEYS[Math.floor(rnd() * KEYS.length)];
        const jobId = `job-${seq++}`;
        const d = decideSlotAcquire(s, key, jobId, PER_KEY, FLEET, clock, TTL);
        s = d.slots;
        if (d.admitted) held.push({ key, jobId });
      } else {
        const idx = Math.floor(rnd() * held.length);
        const { jobId } = held.splice(idx, 1)[0];
        s = releaseSlotByJob(s, jobId, clock);
      }
      // INVARIANTS at every step: no cap is ever exceeded among LIVE slots.
      expect(liveCount(s, clock)).toBeLessThanOrEqual(FLEET);
      for (const key of KEYS) {
        expect(liveKeyCount(s, key, clock)).toBeLessThanOrEqual(PER_KEY);
      }
      // No duplicate jobId ever holds two live slots (idempotency safety).
      const liveIds = s.filter((x) => x.expiresMs > clock).map((x) => x.jobId);
      expect(new Set(liveIds).size).toBe(liveIds.length);
    }
  });
});

// ════════════════════════════════════════════════════════════════════════════
// PART B — the PURE releaseSlotByJob, exhausted (cell 11 + expiry-prune cell 5).
// ════════════════════════════════════════════════════════════════════════════
describe("SJ-5 · releaseSlotByJob — release by jobId + self-heal prune", () => {
  // ── Cell 11 — removes exactly the jobId's slot (any key), rest untouched ────
  it("cell11: removes EXACTLY the jobId's slot regardless of key, leaving the rest", () => {
    const slots = [slot("a", "j1"), slot("b", "j2"), slot("a", "j3")];
    expect(releaseSlotByJob(slots, "j2", NOW)).toEqual([slot("a", "j1"), slot("a", "j3")]);
  });

  it("cell11b: releasing by jobId works WITHOUT knowing the key (globally-unique id)", () => {
    const slots = [slot("some-weird-key", "jX")];
    expect(releaseSlotByJob(slots, "jX", NOW)).toEqual([]);
  });

  it("cell11c: releasing an UNKNOWN jobId is a safe no-op (returns the live set)", () => {
    const slots = [slot("t", "j1")];
    expect(releaseSlotByJob(slots, "never-existed", NOW)).toEqual(slots);
  });

  it("cell11d: releasing an ALREADY-released jobId twice is a safe no-op the second time", () => {
    let s: SlotRecord[] = [slot("t", "j1"), slot("t", "j2")];
    s = releaseSlotByJob(s, "j1", NOW);
    expect(s.map((x) => x.jobId)).toEqual(["j2"]);
    s = releaseSlotByJob(s, "j1", NOW); // again — no-op
    expect(s.map((x) => x.jobId)).toEqual(["j2"]);
  });

  it("cell5-release: release ALSO prunes expired slots (self-heal on any release)", () => {
    const slots = [slot("t", "live"), slot("t", "dead", NOW - 1), slot("t", "gone")];
    // Release "gone" AND the expired "dead" self-heals ⇒ only "live" remains.
    expect(releaseSlotByJob(slots, "gone", NOW)).toEqual([slot("t", "live")]);
  });

  it("cell5-release-b: releasing an unknown id STILL prunes an expired backlog", () => {
    const slots = [slot("t", "keep"), slot("t", "d1", NOW - 1), slot("t", "d2", NOW - 5)];
    expect(releaseSlotByJob(slots, "unknown", NOW)).toEqual([slot("t", "keep")]);
  });
});

// ════════════════════════════════════════════════════════════════════════════
// PART C — the REAL ConcurrencySlotsDO wrapper over Map-backed storage
//          (cells 4, 5, 13 — atomic persist round-trip).
// ════════════════════════════════════════════════════════════════════════════
// A strongly-consistent DO storage stub (mirrors cred-stash-do.test.ts).
function makeStorage() {
  const map = new Map<string, unknown>();
  let tail = Promise.resolve();
  return {
    map,
    async get<T>(key: string): Promise<T | undefined> {
      return map.get(key) as T | undefined;
    },
    async put(key: string, value: unknown): Promise<void> {
      // Deep-clone on put so a caller mutating its array can't retro-alter storage
      // (workerd serializes; the Map would otherwise alias the live reference).
      map.set(key, JSON.parse(JSON.stringify(value)));
    },
    async delete(key: string): Promise<void> { map.delete(key); },
    async transaction<T>(fn: (tx: ReturnType<typeof makeStorage>) => Promise<T>): Promise<T> {
      const run = tail.then(async () => {
        const snapshot = new Map(map);
        const tx = makeStorage();
        tx.map.clear(); for (const [key, value] of snapshot) tx.map.set(key, value);
        const result = await fn(tx);
        map.clear(); for (const [key, value] of tx.map) map.set(key, value);
        return result;
      });
      tail = run.then(() => undefined, () => undefined);
      return run;
    },
  };
}
function makeDO() {
  const storage = makeStorage();
  const doInst = new ConcurrencySlotsDO({ storage } as never, {} as never);
  return { doInst, storage };
}
const readSlots = (storage: ReturnType<typeof makeStorage>): SlotRecord[] =>
  (storage.map.get("slots") as SlotRecord[] | undefined) ?? [];

describe("SJ-5 · ConcurrencySlotsDO — atomic acquire/release over real storage", () => {
  // ── Cell 13 — acquire/release round-trip persists d.slots ──────────────────
  it("cell13: acquire PERSISTS the appended slot list to storage under 'slots'", async () => {
    const { doInst, storage } = makeDO();
    const r = await doInst.acquire("t", "j1", 2, FLEET_MAX_CONCURRENCY, TTL);
    expect(r.admitted).toBe(true);
    const persisted = readSlots(storage);
    expect(persisted).toHaveLength(1);
    expect(persisted[0]).toMatchObject({ key: "t", jobId: "j1" });
    expect(persisted[0].expiresMs).toBeGreaterThan(Date.now()); // real Date.now()-based TTL
  });

  it("cell13b: release PERSISTS the slot list with exactly that jobId removed", async () => {
    const { doInst, storage } = makeDO();
    await doInst.acquire("t", "j1", 5, FLEET_MAX_CONCURRENCY, TTL);
    await doInst.acquire("t", "j2", 5, FLEET_MAX_CONCURRENCY, TTL);
    expect(readSlots(storage)).toHaveLength(2);
    await doInst.release("j1");
    const persisted = readSlots(storage);
    expect(persisted.map((s) => s.jobId)).toEqual(["j2"]);
  });

  it("cell13c: a refused acquire persists the (pruned) list but does NOT append", async () => {
    const { doInst, storage } = makeDO();
    await doInst.acquire("t", "j1", 1, FLEET_MAX_CONCURRENCY, TTL); // fills cap 1
    const r = await doInst.acquire("t", "j2", 1, FLEET_MAX_CONCURRENCY, TTL); // over_key_cap
    expect(r.admitted).toBe(false);
    expect(r.reason).toBe("over_key_cap");
    expect(readSlots(storage).map((s) => s.jobId)).toEqual(["j1"]); // j2 not added
  });

  // ── Cell 4 — idempotent re-admit through the DO (no 2nd slot) ───────────────
  it("cell4: a repeat acquire for the SAME jobId re-admits WITHOUT adding a 2nd slot", async () => {
    const { doInst, storage } = makeDO();
    const r1 = await doInst.acquire("t", "dup", 5, FLEET_MAX_CONCURRENCY, TTL);
    const r2 = await doInst.acquire("t", "dup", 5, FLEET_MAX_CONCURRENCY, TTL);
    expect(r1.admitted).toBe(true);
    expect(r2.admitted).toBe(true); // re-admitted (a redelivery/retry)
    const persisted = readSlots(storage);
    expect(persisted).toHaveLength(1); // NOT double-counted
    expect(persisted.filter((s) => s.jobId === "dup")).toHaveLength(1);
  });

  it("cell4b: idempotent re-admit even when the key is AT its cap (retry never self-blocks)", async () => {
    const { doInst } = makeDO();
    await doInst.acquire("t", "j1", 1, FLEET_MAX_CONCURRENCY, TTL); // key now full (cap 1)
    // j1 retries: it already holds the only slot ⇒ re-admitted, not refused as over_key_cap.
    const r = await doInst.acquire("t", "j1", 1, FLEET_MAX_CONCURRENCY, TTL);
    expect(r.admitted).toBe(true);
    expect(r.reason).toBeUndefined();
  });

  // ── Cell 5 — expired-slot pruning through the DO wrapper ────────────────────
  it("cell5-do: the DO prunes an expired slot on acquire (freeing capacity) and persists it", async () => {
    const { doInst, storage } = makeDO();
    // Seed a slot already expired relative to real Date.now().
    storage.map.set("slots", [{ key: "t", jobId: "stale", expiresMs: Date.now() - 1 }]);
    const r = await doInst.acquire("t", "fresh", 1, FLEET_MAX_CONCURRENCY, TTL); // cap 1 — only fits post-prune
    expect(r.admitted).toBe(true);
    expect(readSlots(storage).map((s) => s.jobId)).toEqual(["fresh"]); // stale pruned
  });

  it("cell5-do-release: the DO prunes an expired slot on release too", async () => {
    const { doInst, storage } = makeDO();
    storage.map.set("slots", [
      { key: "t", jobId: "keep", expiresMs: Date.now() + TTL },
      { key: "t", jobId: "stale", expiresMs: Date.now() - 1 },
    ]);
    await doInst.release("some-unrelated-job"); // no-op on jobId, but still prunes
    expect(readSlots(storage).map((s) => s.jobId)).toEqual(["keep"]);
  });

  it("cell13d: fleet cap is honored through the DO — the (fleet+1)-th across keys refuses", async () => {
    const { doInst, storage } = makeDO();
    for (let i = 0; i < 3; i++) {
      await doInst.acquire(`k${i}`, `j${i}`, 10, 3 /* fleetCap */, TTL);
    }
    const r = await doInst.acquire("k-new", "j-new", 10, 3, TTL);
    expect(r.admitted).toBe(false);
    expect(r.reason).toBe("over_fleet_cap");
    expect(readSlots(storage)).toHaveLength(3);
  });
});

// ════════════════════════════════════════════════════════════════════════════
// PART D — acquireConcurrencySlot via the REAL worker.fetch /webhook drive
//          (cell 6 warm key/cap, cell 7 cold key/cap, cell 12 refusal/honored).
// ════════════════════════════════════════════════════════════════════════════
// A CONCURRENCY_SLOTS DO double that RECORDS the (key, jobId, perKeyCap, fleetCap,
// ttlMs) the real acquireConcurrencySlot computes, and is configurable to admit,
// refuse, or THROW — so we can prove selection + authority-failure refusal.
function fakeSlots(mode: "admit" | "refuse" | "throw") {
  let currentMode = mode;
  const acquire = vi.fn(async (..._args: unknown[]) => {
    if (currentMode === "throw") throw new Error("DO infra reset (simulated)");
    return currentMode === "admit"
      ? { admitted: true }
      : { admitted: false, reason: "over_key_cap" };
  });
  const release = vi.fn(async () => {});
  const claims = new Map<string, { generation: number; ownerToken: string }>();
  const acquireSpawnClaim = vi.fn(async (jobId: string) => {
    if (claims.has(jobId)) return { status: "held", generation: claims.get(jobId)!.generation };
    const claim = { generation: 1, ownerToken: `owner-${jobId}` };
    claims.set(jobId, claim);
    return { status: "acquired", ...claim };
  });
  const markSpawnClaimActive = vi.fn(async (jobId: string, generation: number, ownerToken: string) =>
    claims.get(jobId)?.generation === generation && claims.get(jobId)?.ownerToken === ownerToken);
  const bindSpawnClaimProvider = vi.fn(async () => true);
  const releaseSpawnClaim = vi.fn(async (jobId: string, generation: number, ownerToken: string) => {
    const claim = claims.get(jobId);
    if (!claim) return "missing";
    if (claim.generation !== generation || claim.ownerToken !== ownerToken) return "stale";
    claims.delete(jobId); return "released";
  });
  const recordRetry = vi.fn(async () => ({ attempts: 1, recorded: true }));
  const readRetry = vi.fn(async () => 1);
  const stub = { acquire, release, acquireSpawnClaim, markSpawnClaimActive, bindSpawnClaimProvider, releaseSpawnClaim, recordRetry, readRetry };
  return { get: vi.fn(() => stub), idFromName: vi.fn((n: string) => n), _stub: stub, setMode: (next: typeof mode) => { currentMode = next; } };
}

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

// ── The real GitHub HMAC + a queued webhook (mirrors webhook-route.test.ts) ───
async function ghSign(secret: string, body: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  const hex = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `sha256=${hex}`;
}
const SECRET = "whsec-sj5";
const MINT_KEY = "mint-internal-key-sj5";

function makeCtx() {
  const tasks: Promise<unknown>[] = [];
  return {
    tasks,
    waitUntil(p: Promise<unknown>) {
      tasks.push(Promise.resolve(p));
    },
    passThroughOnException() {},
  };
}
async function drain(ctx: { tasks: Promise<unknown>[] }): Promise<void> {
  for (let i = 0; i < 6 && ctx.tasks.length > 0; i++) {
    const batch = ctx.tasks.splice(0, ctx.tasks.length);
    await Promise.all(batch);
  }
}
async function queuedWebhook(
  env: Env,
  ctx: unknown,
  opts: { jobId: string; repo: string; installationId?: number },
): Promise<Response> {
  const body = JSON.stringify({
    action: "queued",
    workflow_job: { id: Number(opts.jobId), labels: ["corelink-dogfood"] },
    repository: { full_name: opts.repo },
    ...(opts.installationId !== undefined ? { installation: { id: opts.installationId } } : {}),
  });
  return worker.fetch(
    new Request("https://w/webhook", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-github-event": "workflow_job",
        "x-hub-signature-256": await ghSign(SECRET, body),
      },
      body,
    }),
    env,
    ctx as never,
  );
}

// A fetch router for the two external calls the warm drive makes. `mintConcurrency`
// flips the entitlement the mint returns (to prove the clamp).
let fetchCalls: string[] = [];
let mintConcurrency = 5;
let issuedOperationId = "";
function installFetchRouter() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url = typeof input === "string" ? input : (input as Request).url ?? String(input);
      fetchCalls.push(url);
      if (url.includes("generate-jitconfig")) {
        return new Response(JSON.stringify({ encoded_jit_config: "jit-encoded-sj5" }), {
          status: 200,
        });
      }
      if (url.endsWith("/internal/v1/runner/authorize")) {
        return new Response(JSON.stringify({ tenant: "acme", max_concurrency: mintConcurrency }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/mint")) {
        issuedOperationId = String((JSON.parse(String(init?.body ?? "{}")) as { operation_id?: string }).operation_id ?? "");
        return new Response(
          JSON.stringify({
            operation_id: issuedOperationId,
            token_plaintext: "cas-pat-plaintext",
            pat_id: "pat-sj5",
            tenant: "acme",
            lifecycle_generation: "1",
            max_concurrency: mintConcurrency,
          }),
          { status: 200 },
        );
      }
      if (url.endsWith("/internal/v1/runner/adopt")) {
        expect(JSON.parse(String(init?.body))).toMatchObject({ operation_id: issuedOperationId, pat_id: "pat-sj5" });
        return new Response(null, { status: 204 });
      }
      throw new Error(`unexpected fetch: ${url}`);
    }),
  );
}
function baseEnv(over: Partial<Env> = {}): Env {
  const env = {
    RUNNER_CONTAINER: { _ns: "runner" } as never,
    CHECK_HOST_CONTAINER: { _ns: "check" } as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-secret",
    GITHUB_WEBHOOK_SECRET: SECRET,
    GITHUB_MINT_TOKEN: "ghp-mint",
    PINNED_IMAGE_DIGEST: "",
    ...over,
  } as Env;
  const authorities = makeWorkerAuthorities(env.RUNNER_JOB_PATS);
  if (!over.CONTAINMENT) env.CONTAINMENT = authorities.CONTAINMENT as never;
  return env;
}

describe("SJ-5 · acquireConcurrencySlot — selection + fail-open (real worker.fetch)", () => {
  beforeEach(() => {
    containers = [];
    fetchCalls = [];
    mintConcurrency = 5;
    vi.mocked(getContainer).mockClear();
    installFetchRouter();
  });
  afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

  // ── Current contract: no identity means refusal before any execution. ──────
  it("cell7-select: a COLD spawn refuses before slot, mint, JIT, or container", async () => {
    const slots = fakeSlots("admit");
    // No CORELINK_RUNNER_MINT_AUTH_KEY ⇒ cold overlay (no derived tenant).
    const env = baseEnv({
      RUNNER_JOB_PATS: fakeKv() as never,
      METRICS: fakeMetrics() as never,
      CONCURRENCY_SLOTS: slots as never,
    });
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "7001", repo: "acme/api" });
    await drain(ctx);
    expect(slots._stub.acquire).not.toHaveBeenCalled();
    expect(fetchCalls.some((u) => u.includes("/internal/v1/runner/mint") || u.includes("generate-jitconfig"))).toBe(false);
    expect(containers).toHaveLength(0);
  });

  // ── Cell 6 (selection) — WARM spawn ⇒ key=tenant, cap=min(entitlement,FLEET) ─
  it("cell6-select: a WARM spawn selects key=tenant and perKeyCap=min(entitlement, FLEET) — clamp BINDS", async () => {
    mintConcurrency = FLEET_MAX_CONCURRENCY + 250; // entitlement over FLEET ⇒ clamp binds to FLEET
    const slots = fakeSlots("admit");
    const env = baseEnv({
      RUNNER_JOB_PATS: fakeKv() as never,
      METRICS: fakeMetrics() as never,
      CONCURRENCY_SLOTS: slots as never,
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
      CRED_STASH: ns({ stash: vi.fn(async () => "ticket"), wipe: vi.fn(async () => {}) }),
      REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "42" }),
    });
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "6001", repo: "acme/api", installationId: 42 });
    await drain(ctx);
    expect(slots._stub.acquire).toHaveBeenCalledTimes(1);
    const [key, , perKeyCap] = slots._stub.acquire.mock.calls[0] as unknown[];
    expect(key).toBe("acme"); // the SERVER-DERIVED tenant, not repo:*
    expect(perKeyCap).toBe(FLEET_MAX_CONCURRENCY); // min(entitlement, FLEET) clamps to FLEET
  });

  it("cell6-select-b: a WARM spawn with entitlement < FLEET uses the ENTITLEMENT as the cap", async () => {
    mintConcurrency = 5; // under FLEET ⇒ the entitlement binds
    const slots = fakeSlots("admit");
    const env = baseEnv({
      RUNNER_JOB_PATS: fakeKv() as never,
      METRICS: fakeMetrics() as never,
      CONCURRENCY_SLOTS: slots as never,
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
      CRED_STASH: ns({ stash: vi.fn(async () => "ticket"), wipe: vi.fn(async () => {}) }),
      REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "42" }),
    });
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "6002", repo: "acme/api", installationId: 42 });
    await drain(ctx);
    const [key, , perKeyCap] = slots._stub.acquire.mock.calls[0] as unknown[];
    expect(key).toBe("acme");
    expect(perKeyCap).toBe(5); // min(5, 20) = 5
  });

  // ── Cell 12 — a THROWN DO error is a closed admission refusal ─────────────
  it("cell12-authority-failure: a THROWN DO acquire error refuses before mint or provider", async () => {
    const slots = fakeSlots("throw"); // simulate a DO/infra reset
    const metrics = fakeMetrics();
    const env = baseEnv({
      RUNNER_JOB_PATS: fakeKv() as never,
      METRICS: metrics as never,
      CONCURRENCY_SLOTS: slots as never,
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
      CRED_STASH: ns({ stash: vi.fn(async () => "ticket"), wipe: vi.fn(async () => {}) }),
      REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "42" }),
    });
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "1201", repo: "acme/api", installationId: 42 });
    await drain(ctx);
    // The acquire threw, so no emergency slot or provider path is available.
    expect(slots._stub.acquire).toHaveBeenCalledTimes(1);
    expect(fetchCalls.some((u) => u.includes("/internal/v1/runner/mint") || u.includes("generate-jitconfig"))).toBe(false);
    expect(containers).toHaveLength(0);
    expect(metrics.counts.runner_spawned).toBeUndefined();
  });

  // ── Cell 12 — a clean {admitted:false} ⇒ HONORED refusal (NOT fail-open) ────
  it("cell12-honored: a clean {admitted:false} ⇒ NO spawn, claim released, spawn_at_ceiling bumped", async () => {
    const slots = fakeSlots("refuse"); // a REAL at-capacity decision
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      METRICS: metrics as never,
      CONCURRENCY_SLOTS: slots as never,
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
      CRED_STASH: ns({ stash: vi.fn(async () => "ticket"), wipe: vi.fn(async () => {}) }),
      REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "42" }),
    });
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "1202", repo: "acme/api", installationId: 42 });
    await drain(ctx);
    // The refusal is HONORED: no JIT, no container — the ceiling is REAL, not fail-open.
    expect(fetchCalls.some((u) => u.includes("generate-jitconfig"))).toBe(false);
    expect(containers).toHaveLength(0);
    // The claim was released (so a later re-drive isn't orphaned) + the golden signal moved.
    expect(kv.store.has("spawn:1202")).toBe(false);
    expect(metrics.counts.spawn_at_ceiling).toBe(1);
    expect(metrics.counts.runner_spawned).toBeUndefined();
  });

  // ── The 2026-08-02 incident pin ────────────────────────────────────────────
  // Honoring the ceiling is correct. LOSING the job to it is not — and that is
  // what happened: ~24 jobs pushed at once, 12 ran, 12 sat `queued` forever with
  // nothing reported as failed. `driveSpawn` RETURNED at the ceiling, and
  // `driveSpawnGuarded` writes the dead-letter only from its CATCH, so a refused
  // spawn left no record for `retryOrphanedSpawns` to find. GitHub sends
  // `workflow_job.queued` exactly once and never redelivers it, so no record
  // meant no recovery, ever.
  //
  // This drives the REAL webhook path. It has to: the injected-drive seam in
  // orphan-retry.test.ts cannot observe a return-vs-throw difference, so a test
  // written there would pass against the bug (verified — it did).
  it("cell12-deadletter: a capacity refusal retains the verified webhook and recovers once", async () => {
    const slots = fakeSlots("refuse");
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const authorities = makeWorkerAuthorities(kv);
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never, METRICS: metrics as never, CONCURRENCY_SLOTS: slots as never,
      CONTAINMENT: authorities.CONTAINMENT as never, CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
      CRED_STASH: ns({ stash: vi.fn(async () => "ticket"), wipe: vi.fn(async () => {}) }),
      REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "42" }),
    });
    vi.spyOn(Date, "now").mockReturnValue(1_000_000);
    const ctx = makeCtx();
    const accepted = await queuedWebhook(env, ctx, { jobId: "1204", repo: "acme/api", installationId: 4242 });
    expect(accepted.status).toBe(202);
    await drain(ctx);
    const { runNormalIntakeDrain } = await import("../src/index");
    const stored = [...authorities.containmentStorage.values.entries()].find(([key]) => key.startsWith("normal-inbox:v1:event:"))?.[1] as { state: string; next_attempt_ms: number };
    expect(stored).toMatchObject({ job_id: "1204", repo: "acme/api", installation_id: "4242", state: "pending", next_attempt_ms: 1_060_000 });
    expect(kv.store.has("spawn:1204")).toBe(false);
    expect(fetchCalls.some((u) => u.includes("generate-jitconfig"))).toBe(false);
    expect(containers).toHaveLength(0);

    vi.mocked(Date.now).mockReturnValue(1_060_000);
    slots.setMode("admit");
    await runNormalIntakeDrain(env as never);
    const settled = [...authorities.containmentStorage.values.entries()].find(([key]) => key.startsWith("normal-inbox:v1:event:"))?.[1] as { state: string };
    expect(settled.state).toBe("complete");
    await runNormalIntakeDrain(env as never);
    expect(metrics.counts.spawn_failed).toBeUndefined();
    expect(fetchCalls.filter((u) => u.includes("generate-jitconfig"))).toHaveLength(1);
    expect(containers).toHaveLength(1);
    expect(metrics.counts.spawn_at_ceiling).toBe(1);
  });

  it("cell12-deadletter-cold: a COLD refusal records NOTHING (not warm-recoverable — known, bounded)", async () => {
    // No installation id ⇒ nothing to re-drive WARM with, so recordOrphan
    // deliberately skips it (a cold re-drive would bypass per-job authz/mint).
    // Pinned so the gap stays a DECISION rather than drifting into a surprise:
    // cold spawns remain covered only by the first-party GitHub scan.
    const slots = fakeSlots("refuse");
    const kv = fakeKv();
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      METRICS: fakeMetrics() as never,
      CONCURRENCY_SLOTS: slots as never,
    });
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "1205", repo: "acme/api" });
    await drain(ctx);
    expect(containers).toHaveLength(0);
    expect(kv.store.has("orphan:1205")).toBe(false);
  });

  it("cell12-cold-cap-live: a COLD request refuses before the slot decision", async () => {
    const slots = fakeSlots("refuse");
    const env = baseEnv({
      RUNNER_JOB_PATS: fakeKv() as never,
      METRICS: fakeMetrics() as never,
      CONCURRENCY_SLOTS: slots as never,
    });
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "1203", repo: "acme/api" });
    await drain(ctx);
    expect(slots._stub.acquire).not.toHaveBeenCalled();
    expect(fetchCalls.some((u) => u.includes("/internal/v1/runner/mint") || u.includes("generate-jitconfig"))).toBe(false);
    expect(containers).toHaveLength(0);
  });
});
