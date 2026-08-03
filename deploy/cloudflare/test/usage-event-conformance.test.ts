// TS-side conformance golden for the billing usage-event `UsageEvent` shape.
//
// The committed vector conformance/UsageEvent.json is the drift tripwire for the
// billing usage-event, which is transcribed on THREE sides (Rust
// `UsageEventData`, this TS `UsageEvent`, and corelink-server's ingest). Unlike
// RunnerLease/FenceManifest it had NO committed vector, so a TS field rename
// here would ship green while breaking the live wire (a 400 at the ingest).
// This binds the TS `UsageEvent` + `buildUsageEvent` to the SAME committed
// vector so the CLAUDE.md law holds symmetrically: "either side's golden tests
// break on any type divergence, so a difference is never silent."
//
// Two-directional tripwire:
//   • rename/remove a `UsageEvent` field ⇒ `buildUsageEvent` stops emitting the
//     vector's key-set ⇒ the key-set assertion breaks.
//   • add a key to the vector that TS doesn't model ⇒ the key-set assertion breaks.
//
// NEW FILE. Does NOT touch any existing test.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { buildUsageEvent, RUNNER_BOX_VCPU, type UsageEvent } from "../src/lib";

// The committed cross-repo vector (repo root: conformance/UsageEvent.json).
const VECTOR_PATH = fileURLToPath(
  new URL("../../../conformance/UsageEvent.json", import.meta.url),
);
const vector = JSON.parse(readFileSync(VECTOR_PATH, "utf8")) as Record<string, unknown>;

// The keys the TS `UsageEvent` interface models (src/lib.ts). A vector key
// outside this set means the vector drifted ahead of the TS type; a modeled key
// absent from the vector means the reverse.
const KNOWN_KEYS = [
  "tenant_id",
  "event_kind",
  "qty",
  "billing_period",
  "region",
  "source",
  "time_ms",
  "idem_key",
].sort();

describe("conformance: UsageEvent ↔ conformance/UsageEvent.json", () => {
  it("the committed vector's key-set is EXACTLY what the TS UsageEvent models", () => {
    expect(Object.keys(vector).sort()).toEqual(KNOWN_KEYS);
  });

  it("the vector parses into the UsageEvent shape with the right field types", () => {
    // Structural bind: a field rename on the TS side makes one of these `unknown`
    // reads land on `undefined`, tripping the type assertion below.
    const ev = vector as unknown as UsageEvent;
    expect(typeof ev.tenant_id).toBe("string");
    expect(typeof ev.event_kind).toBe("string");
    expect(typeof ev.qty).toBe("number");
    expect(typeof ev.billing_period).toBe("string");
    expect(typeof ev.region).toBe("string");
    expect(typeof ev.source).toBe("string");
    expect(typeof ev.time_ms).toBe("number");
    expect(typeof ev.idem_key).toBe("string");

    // Value pins on the canonical instance. The vector is FABRICD's golden and
    // fabricd still emits the capacity kind, so this stays `runner_slot_seconds`
    // here — the spawn-worker's BILLABLE kind is pinned separately below.
    expect(ev.event_kind).toBe("runner_slot_seconds");
    expect(ev.billing_period).toMatch(/^\d{4}-\d{2}$/);
    expect(ev.region).toHaveLength(3);
    expect(ev.idem_key).toMatch(/^[0-9a-f]{64}$/);
    // tenant_id must be a UUID: the corelink-server ingest validates it as a
    // `Uuid` (billing_ingest.rs), so a non-UUID example (e.g. "acme") is bytes
    // the server rejects (422). Pin the shape so our tripwire catches it too.
    expect(ev.tenant_id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/,
    );
  });

  it("buildUsageEvent EMITS exactly the vector's key-set (rename ⇒ this breaks)", async () => {
    // The real builder the webhook path uses. Its output keys must match the
    // vector's byte-for-byte in NAME — a renamed/removed `UsageEvent` field
    // changes this set and breaks the assertion. `source` differs by front door
    // (fabricd vs spawn-worker), so we bind the SHAPE, not that value.
    const ev = await buildUsageEvent({
      tenantId: "3fa85f64-5717-4562-b3fc-2c963f66afa6",
      jobId: "lease-0001-held",
      startedMs: 1_781_524_797_000, // 3s before completion
      completedMs: 1_781_524_800_000, // matches the vector's time_ms
      region: "iad",
    });
    expect(Object.keys(ev).sort()).toEqual(KNOWN_KEYS);

    // The builder reproduces the vector's canonical scalar values, proving the
    // emitted wire matches the vector. FOUR fields are intentionally NOT pinned
    // by equality, each for a stated reason:
    //   • `source`   — differs by front door (fabricd vs spawn-worker).
    //   • `idem_key` — digest differs by side (Rust BLAKE3 vs TS SHA-256); the
    //                  contract pins that it is SOME stable 64-hex.
    //   • `event_kind` and `qty` — added to this list 2026-08-02. The vector is
    //                  FABRICD's golden and fabricd emits the CAPACITY kind
    //                  (`runner_slot_seconds`, slot-seconds). The spawn-worker
    //                  now emits the BILLABLE kind (`runner_vcpu_seconds`,
    //                  slot-seconds × vCPU). They are different quantities in
    //                  different units by design — see RUNNER_VCPU_SECONDS_KIND.
    //
    // Moving those two off equality is NOT a weakening: the assertions below
    // replace a coincidental "== 3" with the billing ARITHMETIC itself, which is
    // the thing that actually must not drift. An equality against one frozen
    // example would have passed just as happily with the multiplier missing.
    expect(ev.tenant_id).toBe(vector.tenant_id);
    expect(ev.billing_period).toBe(vector.billing_period);
    expect(ev.region).toBe(vector.region);
    expect(ev.time_ms).toBe(vector.time_ms);
    expect(ev.idem_key).toMatch(/^[0-9a-f]{64}$/);

    // The billable kind + the exact billing math: 3 allocated seconds on a
    // 4-vCPU box is 12 vCPU-seconds. If the multiplier is ever dropped this
    // reads 3 (the vector's value) and fails LOUDLY — which is precisely the
    // 4×-under-bill this whole change exists to prevent.
    expect(ev.event_kind).toBe("runner_vcpu_seconds");
    const allocatedSeconds = (1_781_524_800_000 - 1_781_524_797_000) / 1000;
    expect(ev.qty).toBe(allocatedSeconds * RUNNER_BOX_VCPU);
    expect(ev.qty).toBe(12);
    // And it is a strict MULTIPLE of the vector's slot-second quantity, so the
    // two kinds stay reconcilable against each other.
    expect(ev.qty).toBe(vector.qty * RUNNER_BOX_VCPU);
  });

  it("an explicit vcpu overrides the fleet default — a mixed-size fleet bills each box correctly", async () => {
    // The guard against the failure mode the constant's comment warns about: a
    // bigger SKU must bill MORE, not silently bill as if it were standard-4.
    const big = await buildUsageEvent({
      tenantId: "3fa85f64-5717-4562-b3fc-2c963f66afa6",
      jobId: "lease-0002-big",
      startedMs: 1_781_524_797_000,
      completedMs: 1_781_524_800_000,
      region: "iad",
      vcpu: 16,
    });
    expect(big.qty).toBe(3 * 16);
    expect(big.qty).toBeGreaterThan(3 * RUNNER_BOX_VCPU);

    // A nonsense vCPU count must NOT zero the bill — a zeroed bill is
    // indistinguishable from a job that never ran, so it falls back to the
    // fleet default rather than silently billing nothing.
    for (const bad of [0, -4, Number.NaN, undefined]) {
      const ev = await buildUsageEvent({
        tenantId: "3fa85f64-5717-4562-b3fc-2c963f66afa6",
        jobId: `lease-bad-${String(bad)}`,
        startedMs: 1_781_524_797_000,
        completedMs: 1_781_524_800_000,
        region: "iad",
        vcpu: bad as number | undefined,
      });
      expect(ev.qty).toBe(3 * RUNNER_BOX_VCPU);
    }
  });
});
