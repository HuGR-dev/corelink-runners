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

import { buildUsageEvent, type UsageEvent } from "../src/lib";

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

    // Value pins on the canonical instance.
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
    // emitted wire matches the vector. Two fields are intentionally NOT pinned
    // by equality: `source` differs by front door (fabricd vs spawn-worker), and
    // `idem_key`'s digest differs by side (Rust BLAKE3 vs TS SHA-256) — the
    // contract pins that it is SOME stable 64-hex, not which algorithm.
    expect(ev.tenant_id).toBe(vector.tenant_id);
    expect(ev.event_kind).toBe(vector.event_kind);
    expect(ev.qty).toBe(vector.qty);
    expect(ev.billing_period).toBe(vector.billing_period);
    expect(ev.region).toBe(vector.region);
    expect(ev.time_ms).toBe(vector.time_ms);
    expect(ev.idem_key).toMatch(/^[0-9a-f]{64}$/);
  });
});
