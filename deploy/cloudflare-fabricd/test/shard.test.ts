// Golden conformance test for the FROZEN cross-language shard contract.
//
// conformance/lease_shard.json is the drift tripwire shared with the Rust
// `crate::shard` (crates/corelink-fabric-server/src/shard.rs). Both sides run
// these SAME vectors; any divergence in the FNV-1a hash, the modulo, or the byte
// encoding breaks a golden test on one side. All 36 cases (6 hashes + 30 shard
// cases) MUST pass.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { fnv1a32, shardOf } from "../src/shard";

interface HashCase {
  lease_id: string;
  fnv1a_32: number;
}
interface ShardCase {
  lease_id: string;
  num_shards: number;
  shard: number;
}
interface Vectors {
  hashes: HashCase[];
  cases: ShardCase[];
}

const vectors: Vectors = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("../../../conformance/lease_shard.json", import.meta.url)),
    "utf8",
  ),
);

describe("fnv1a32 — frozen hash vectors", () => {
  for (const c of vectors.hashes) {
    it(`fnv1a32(${JSON.stringify(c.lease_id)}) === ${c.fnv1a_32}`, () => {
      expect(fnv1a32(c.lease_id)).toBe(c.fnv1a_32);
    });
  }
});

describe("shardOf — frozen routing vectors", () => {
  for (const c of vectors.cases) {
    it(`shardOf(${JSON.stringify(c.lease_id)}, ${c.num_shards}) === ${c.shard}`, () => {
      expect(shardOf(c.lease_id, c.num_shards)).toBe(c.shard);
    });
  }
});
