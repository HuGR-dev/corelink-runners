// ─────────────────────────────────────────────────────────────────────────────
// CONTAINER IMAGE PINS — every `image` in wrangler.jsonc must be an @sha256 digest.
// ─────────────────────────────────────────────────────────────────────────────
//
// WHY THIS EXISTS. The `RunnerDevEnvDO` entry pinned
// `corelink-runner-devenv:latest`, and the Containers API refuses that outright —
// "VALIDATE_INPUT image Invalid image. Latest tags are not allowed on images." One
// bad entry therefore failed EVERY `wrangler deploy` of this Worker, so no fix to
// the spawn path, the reaper, or anything else could reach production. Nothing in
// the suite read the `containers` block, so nothing caught it: the failure surfaced
// only at deploy time, on the operator.
//
// It can come back the same way it arrived. PR #516 (`feat(devenv)`, open) touches
// this same file and owns `RunnerDevEnvDO`; if it rebases after this change it will
// most likely re-introduce a `:latest` entry, and today nothing would stop it. This
// cell is the stop.
//
// ⚠️ ANTI-VACUITY. A scan that finds nothing must FAIL, not pass. Two guards: the
// scan must find at least one image, and the number of pins it extracted must equal
// the number of `"image"` keys in the file — so a re-formatted or oddly-quoted entry
// cannot slip past the extractor and be silently counted as "no violations".

import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const WRANGLER = fileURLToPath(new URL("../wrangler.jsonc", import.meta.url));

/** An image reference is acceptable ONLY if it is pinned by immutable digest. */
export function isDigestPinned(image: string): boolean {
  return /@sha256:[0-9a-f]{64}$/.test(image);
}

/** Every `"image": "…"` value in a wrangler.jsonc, in file order. */
function extractImagePins(src: string): string[] {
  return [...src.matchAll(/"image"\s*:\s*"([^"]*)"/g)].map((m) => m[1]!);
}

describe("wrangler.jsonc container image pins", () => {
  const src = readFileSync(WRANGLER, "utf8");
  const pins = extractImagePins(src);

  it("the scan is not vacuous — it finds every `image` key in the file", () => {
    // Count only real JSON keys, not the word appearing in prose comments.
    const keyCount = (src.match(/"image"\s*:/g) ?? []).length;
    expect(keyCount).toBeGreaterThan(0);
    expect(pins).toHaveLength(keyCount);
  });

  it("REFUSES any image that is not pinned by @sha256", () => {
    const unpinned = pins.filter((p) => !isDigestPinned(p));
    expect(unpinned).toEqual([]);
  });

  // ── Q-7: the gate's own teeth, both sides ────────────────────────────────
  it("teeth — a tag pin (`:latest` and any other tag) is REJECTED", () => {
    const bad = [
      "registry.cloudflare.com/acct/corelink-runner-devenv:latest",
      "registry.cloudflare.com/acct/corelink-runner-devenv:v1.2.3",
      "registry.cloudflare.com/acct/corelink-runner-devenv",
      // A digest that is not a full 64-hex sha256 is not a digest.
      "registry.cloudflare.com/acct/x@sha256:deadbeef",
      // A digest in the middle of the string is not the pin.
      "registry.cloudflare.com/acct/x@sha256:" + "a".repeat(64) + ":latest",
    ];
    for (const image of bad) expect(isDigestPinned(image)).toBe(false);
  });

  it("teeth — a real @sha256 digest pin is ACCEPTED", () => {
    expect(
      isDigestPinned(
        "registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-spawn-worker-runnercontainer@sha256:2f3dd8a166e890888d026669757c996750b42230a04710fa08f13db56b0cf029",
      ),
    ).toBe(true);
  });

  it("teeth — the extractor sees an entry the file does not currently contain", () => {
    // Proves the extractor is not simply returning the two pins it happens to know:
    // feed it a synthetic file and it must report the synthetic entry, unpinned.
    const synthetic = `{ "containers": [ { "image": "foo/bar:latest" } ] }`;
    expect(extractImagePins(synthetic)).toEqual(["foo/bar:latest"]);
    expect(extractImagePins(synthetic).filter(isDigestPinned)).toEqual([]);
  });
});
