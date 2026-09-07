import { build } from "esbuild";
import { Miniflare } from "miniflare";
import { describe, expect, it } from "vitest";

describe("scheduled tick Durable Object adapter", () => {
  it("invokes the bundled adapter with an actual Miniflare DO binding", async () => {
    const bundled = await build({
      entryPoints: ["src/index.ts"],
      bundle: true,
      format: "esm",
      platform: "neutral",
      write: false,
    });
    const script = bundled.outputFiles[0]?.text;
    if (!script) throw new Error("esbuild produced no Worker module");

    const mf = new Miniflare({
      modules: true,
      script,
      bindings: { FABRIC_PROBES_ENABLED: "0" },
      durableObjects: { CANARY_TICK_OUTBOX: "CanaryTickOutboxAdapter" },
    });
    try {
      const namespace =
        await mf.getDurableObjectNamespace("CANARY_TICK_OUTBOX");
      const stub = namespace.get(namespace.idFromName("scheduled-tick"));

      const missing = await stub.fetch("https://canary.invalid/", {
        method: "POST",
        body: JSON.stringify({ command: "scheduled-tick", scheduled_for: 1_000 }),
      });
      expect(missing.status).toBe(200);
      expect(await missing.text()).toContain("tick config unavailable");

      const malformed = await stub.fetch("https://canary.invalid/", {
        method: "POST",
        body: "not-json",
      });
      expect(malformed.status).toBe(400);

      const missingScheduledFor = await stub.fetch("https://canary.invalid/", {
        method: "POST",
        body: JSON.stringify({ command: "scheduled-tick" }),
      });
      expect(missingScheduledFor.status).toBe(400);

      const wrongMethod = await stub.fetch("https://canary.invalid/", {
        method: "GET",
      });
      expect(wrongMethod.status).toBe(405);
    } finally {
      await mf.dispose();
    }
  }, 30_000); // Cold workerd startup is separate from the producer's 60s deadline.
});
