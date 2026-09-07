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
import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { buildUsageEvent, pushUsageEvent, type UsageEvent } from "../src/lib";

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
    //   • `event_kind` and `qty` — the Worker uses the frozen canonical
    //                  per-job slot-seconds event, byte-compatible with the vector.
    //
    // These assertions bind the canonical per-job quantity directly.
    expect(ev.tenant_id).toBe(vector.tenant_id);
    expect(ev.billing_period).toBe(vector.billing_period);
    expect(ev.region).toBe(vector.region);
    expect(ev.time_ms).toBe(vector.time_ms);
    expect(ev.idem_key).toMatch(/^[0-9a-f]{64}$/);

    // The canonical kind + exact billing math: 3 allocated seconds per job.
    expect(ev.event_kind).toBe("runner_slot_seconds");
    const allocatedSeconds = (1_781_524_800_000 - 1_781_524_797_000) / 1000;
    expect(ev.qty).toBe(allocatedSeconds);
    expect(ev.qty).toBe(3);
    expect(ev.qty).toBe(vector.qty);
  });

  it("an explicit vcpu hint cannot change the canonical slot-second quantity", async () => {
    const big = await buildUsageEvent({
      tenantId: "3fa85f64-5717-4562-b3fc-2c963f66afa6",
      jobId: "lease-0002-big",
      startedMs: 1_781_524_797_000,
      completedMs: 1_781_524_800_000,
      region: "iad",
      vcpu: 16,
    });
    expect(big.qty).toBe(3);

    // Legacy callers may still pass a vCPU hint; it cannot alter the wire unit.
    for (const bad of [0, -4, Number.NaN, undefined]) {
      const ev = await buildUsageEvent({
        tenantId: "3fa85f64-5717-4562-b3fc-2c963f66afa6",
        jobId: `lease-bad-${String(bad)}`,
        startedMs: 1_781_524_797_000,
        completedMs: 1_781_524_800_000,
        region: "iad",
        vcpu: bad as number | undefined,
      });
      expect(ev.qty).toBe(3);
    }
  });

  it("pushUsageEvent emits the committed spawn-worker billing wire byte-for-byte", async () => {
    const fixture = readFileSync(
      fileURLToPath(new URL("../../../conformance/spawn-worker-billing-wire.json", import.meta.url)),
      "utf8",
    );
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ accepted: 1 }), { status: 202 }));
    vi.stubGlobal("fetch", fetchMock);
    try {
      const event = await buildUsageEvent({
        tenantId: "3560e213-1e23-4fd0-8871-7033c6052ebd",
        jobId: "82597479935",
        startedMs: Date.parse("2026-06-23T11:00:00Z"),
        completedMs: Date.parse("2026-06-23T11:00:03Z"),
        region: "iad",
      });
      await pushUsageEvent({
        BILLING_INGEST_URL: "https://billing.example/internal/v1/usage",
        BILLING_INGEST_AUTH_KEY: "billing-test-key",
      }, event);
      const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
      expect(init.body).toBe(fixture);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
