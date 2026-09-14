import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
    pool: "forks",
    minWorkers: 1,
    maxWorkers: 1,
  },
});
