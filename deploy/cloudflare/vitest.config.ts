import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

// Plain vitest (node) — the spawn-Worker's security-critical logic is pure and
// unit-tested without the Workers runtime. The ONLY workerd-virtual import is
// `cloudflare:workers` (the `DurableObject` base that index.ts's CredStashDO
// extends); alias it to a node stub so index.ts loads under node. `@cloudflare/
// containers` resolves natively from node_modules (no alias needed).
export default defineConfig({
  test: {
    alias: {
      "cloudflare:workers": fileURLToPath(
        new URL("./test/stubs/cloudflare-workers.ts", import.meta.url),
      ),
    },
    // ── Coverage floor (WP-3, 2026-08-29) ────────────────────────────────
    // Enforced by the `spawn-worker-ci.yml` PR gate, which runs
    // `npm run test:coverage`. vitest only evaluates `thresholds` when the
    // coverage provider is enabled, so the script's `--coverage` flag and this
    // block are a PAIR — remove either and the floor silently stops existing.
    //
    // `include` is set EXPLICITLY. By default v8 coverage reports only files a
    // test happened to import, so a brand-new untested src file would not move
    // the percentage at all and the floor would be blind to it. Measured here:
    // src/lib/clw.ts is imported by no test and shows 0% — it is counted only
    // because of this `include`, and it is part of the 80.97% below.
    //
    // Measured 2026-08-29 (provider v8, include src/**/*.ts, 552 tests green):
    //   L 80.97  S 80.97  F 91.46  B 83.44
    // Floors sit ~5 points below measured. Ratchet UP as coverage improves;
    // never write an aspirational number here — a floor above reality is a
    // permanently-red check that trains everyone to ignore CI.
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts"],
      exclude: ["src/**/*.d.ts"],
      reporter: ["text"],
      thresholds: {
        lines: 75,
        functions: 86,
        branches: 78,
        statements: 75,
      },
    },
  },
});
