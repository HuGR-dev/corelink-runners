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
  },
});
