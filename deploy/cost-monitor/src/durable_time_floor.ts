import type { MonitorStateStore } from "./state.js";

export interface DurableTimeFloorOptions {
  store: MonitorStateStore;
  key: string;
  minimumTimeMs: number;
}

export class DurableTimeFloorError extends Error {
  readonly code: "corrupt" | "backend";
  constructor(code: "corrupt" | "backend", message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "DurableTimeFloorError";
    this.code = code;
  }
}

interface FloorValue { version: "1"; timeMs: number }
const MAX_INITIALIZE_ATTEMPTS = 8;

function validTime(value: unknown, minimum: number): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= minimum;
}

function parseFloor(value: unknown, minimum: number): FloorValue {
  if (!value || typeof value !== "object") throw new DurableTimeFloorError("corrupt", "trusted time floor is malformed");
  const record = value as Record<string, unknown>;
  if (record.version !== "1" || !validTime(record.timeMs, minimum) || Object.keys(record).length !== 2) {
    throw new DurableTimeFloorError("corrupt", "trusted time floor is malformed");
  }
  return { version: "1", timeMs: record.timeMs };
}

export class DurableTimeFloor {
  private readonly options: DurableTimeFloorOptions;

  constructor(options: DurableTimeFloorOptions) {
    if (!options.store || typeof options.key !== "string" || options.key.length === 0 || !validTime(options.minimumTimeMs, 1)) {
      throw new DurableTimeFloorError("corrupt", "invalid trusted time floor configuration");
    }
    this.options = options;
  }

  async load(): Promise<number> {
    for (let attempt = 0; attempt < MAX_INITIALIZE_ATTEMPTS; attempt += 1) {
      let stored;
      try { stored = await this.options.store.get<FloorValue>(this.options.key); }
      catch (cause) { throw new DurableTimeFloorError("backend", "trusted time floor read failed", { cause }); }
      if (stored) return parseFloor(stored.value, this.options.minimumTimeMs).timeMs;
      let result: "committed" | "conflict";
      try {
        result = await this.options.store.transact([{ key: this.options.key, expectedVersion: null, value: { version: "1", timeMs: this.options.minimumTimeMs } }]);
      } catch (cause) { throw new DurableTimeFloorError("backend", "trusted time floor initialization failed", { cause }); }
      if (result === "committed") return this.options.minimumTimeMs;
    }
    throw new DurableTimeFloorError("backend", "trusted time floor initialization remained contested");
  }

  async commit(expected: number, next: number): Promise<boolean> {
    if (!validTime(expected, this.options.minimumTimeMs) || !validTime(next, expected)) {
      throw new DurableTimeFloorError("corrupt", "trusted time floor update is invalid");
    }
    let stored;
    try { stored = await this.options.store.get<FloorValue>(this.options.key); }
    catch (cause) { throw new DurableTimeFloorError("backend", "trusted time floor read failed", { cause }); }
    if (!stored) return false;
    const current = parseFloor(stored.value, this.options.minimumTimeMs);
    if (current.timeMs !== expected) return false;
    try {
      return (await this.options.store.transact([{ key: this.options.key, expectedVersion: stored.version, value: { version: "1", timeMs: next } }])) === "committed";
    } catch (cause) { throw new DurableTimeFloorError("backend", "trusted time floor update failed", { cause }); }
  }
}
