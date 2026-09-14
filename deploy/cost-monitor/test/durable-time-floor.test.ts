import { describe, expect, it } from "vitest";
import { MemoryStateStore, type MonitorStateStore } from "../src/state.js";
import { DurableTimeFloor, DurableTimeFloorError } from "../src/durable_time_floor.js";

const minimum = 1_700_000_000_000;

describe("DurableTimeFloor", () => {
  it("initializes exactly once under 100 concurrent loaders and preserves it across instances", async () => {
    const store = new MemoryStateStore();
    const floors = await Promise.all(Array.from({ length: 100 }, () => new DurableTimeFloor({ store, key: "tsa/floor", minimumTimeMs: minimum }).load()));
    expect(new Set(floors)).toEqual(new Set([minimum]));
    expect(await new DurableTimeFloor({ store, key: "tsa/floor", minimumTimeMs: minimum }).commit(minimum, minimum + 10)).toBe(true);
    expect(await new DurableTimeFloor({ store, key: "tsa/floor", minimumTimeMs: minimum }).load()).toBe(minimum + 10);
  });

  it("uses both value and version fences for competing commits", async () => {
    const store = new MemoryStateStore();
    const floor = new DurableTimeFloor({ store, key: "tsa/floor", minimumTimeMs: minimum });
    await floor.load();
    const results = await Promise.all(Array.from({ length: 100 }, (_, i) => floor.commit(minimum, minimum + i + 1)));
    expect(results.filter(Boolean)).toHaveLength(1);
    await expect(floor.commit(minimum - 1, minimum)).rejects.toBeInstanceOf(DurableTimeFloorError);
  });

  it("refuses malformed or regressed persisted state and never resets it", async () => {
    const store = new MemoryStateStore();
    await store.transact([{ key: "bad", expectedVersion: null, value: { version: "1", timeMs: minimum - 1 } }]);
    const floor = new DurableTimeFloor({ store, key: "bad", minimumTimeMs: minimum });
    await expect(floor.load()).rejects.toMatchObject({ code: "corrupt" });
    expect(await store.get("bad")).toMatchObject({ value: { timeMs: minimum - 1 } });
  });

  it("surfaces backend ambiguity and does not report a false commit", async () => {
    const failing: MonitorStateStore = {
      async get() { throw new Error("timeout"); },
      async transact() { throw new Error("ambiguous"); },
      async scan() { return { items: [], nextCursor: null }; },
    };
    const floor = new DurableTimeFloor({ store: failing, key: "tsa/floor", minimumTimeMs: minimum });
    await expect(floor.load()).rejects.toMatchObject({ code: "backend" });
    await expect(floor.commit(minimum, minimum + 1)).rejects.toMatchObject({ code: "backend" });
  });
});
