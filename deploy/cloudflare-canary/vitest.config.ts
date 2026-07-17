import { defineConfig } from "vitest/config";

// Plain vitest (node). The alert-detection logic (rules.ts) and email formatting
// (notify.ts) are PURE — no Workers runtime needed. The tests never import
// index.ts (the only module that touches KV/fetch/`ScheduledController` types),
// so no workerd stub/alias is required.
export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
  },
});
