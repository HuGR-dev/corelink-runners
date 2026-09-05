import { describe, expect, it, vi } from "vitest";
import { runCycle, type Env } from "../src/index";

function kv(): KVNamespace {
  return { get: vi.fn(async () => null), put: vi.fn(async () => undefined) } as unknown as KVNamespace;
}

describe("invalid fabric probe configuration", () => {
  it.each([undefined, "", " ", "00", "true", " 1 "])("is fail-visible and does no fabric fetch for %j", async (flag) => {
    const fabricFetch = vi.fn();
    const env = {
      CANARY_KV: kv(), CANARY_TICK_OUTBOX: {} as DurableObjectNamespace,
      FABRICD_SVC: { fetch: fabricFetch } as unknown as Fetcher,
      ...(flag === undefined ? {} : { FABRIC_PROBES_ENABLED: flag }),
    } satisfies Env;
    const result = await runCycle(env, Date.UTC(2026, 8, 1));
    expect(fabricFetch).not.toHaveBeenCalled();
    expect(result).toContain("config=invalid");
  });

  it("recognizes only exact zero as contained and visible-valid", async () => {
    const env = { CANARY_KV: kv(), CANARY_TICK_OUTBOX: {} as DurableObjectNamespace, FABRIC_PROBES_ENABLED: "0" } satisfies Env;
    expect(await runCycle(env, Date.UTC(2026, 8, 1))).toContain("config=valid");
  });
});
