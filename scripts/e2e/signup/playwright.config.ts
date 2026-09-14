import { defineConfig, devices } from "@playwright/test";

/**
 * Undercover organic-signup journey vs PROD humangr.com/corelink → then the
 * RUNNER product (`/v1` fabric). No webServer — we drive the DEPLOYED app with a
 * REAL Clerk session (via @clerk/testing), the only path that produces a session
 * the prod worker's Clerk verification accepts (headless FAPI mint is 401'd; a
 * browser session is not). So the tenant we create is undercover — the system
 * cannot tell it is a test.
 *
 * PROVENANCE: config shape adapted from corelink-server tests/e2e-browser.
 */
export default defineConfig({
  testDir: "./specs",
  globalSetup: "./global.setup.ts",
  timeout: 180_000,
  expect: { timeout: 20_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  use: {
    baseURL: process.env["CORELINK_APP_URL"] ?? "https://humangr.com",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
