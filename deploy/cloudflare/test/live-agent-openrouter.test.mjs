import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Test for the OPENROUTER_API_KEY env var handling in live-agent-openrouter.mjs.
// We do NOT import the script directly (it has top-level side effects). Instead
// we test the env-var check pattern by mocking process.env and re-requiring the
// script via a fresh import after clearing module cache.

describe("live-agent-openrouter.mjs OPENROUTER_API_KEY handling", () => {
  const originalEnv = { ...process.env };

  beforeEach(() => {
    // Reset env and clear module cache so re-import reads the current env.
    delete process.env.OPENROUTER_API_KEY;
    vi.resetModules();
  });

  afterEach(() => {
    // Restore original env.
    process.env = { ...originalEnv };
    vi.resetModules();
  });

  it("fails fast with clear error if OPENROUTER_API_KEY is not set", async () => {
    process.env = { ...originalEnv };
    delete process.env.OPENROUTER_API_KEY;
    // Re-import the script — its top-level code will read env and exit(1).
    // We capture the exit by spawning a child process so we can assert the
    // behavior without actually exiting the test process.
    const { spawnSync } = await import("node:child_process");
    const scriptPath = new URL(
      "../scripts/live-agent-openrouter.mjs",
      import.meta.url,
    ).pathname;
    const result = spawnSync("node", [scriptPath], {
      env: { ...process.env, PATH: process.env.PATH },
      encoding: "utf8",
    });
    expect(result.status).not.toBe(0);
    const combined = `${result.stdout ?? ""}${result.stderr ?? ""}`;
    expect(combined).toMatch(/FATAL/);
    expect(combined).toMatch(/OPENROUTER_API_KEY/);
  });

  it("does not contain a hardcoded OpenRouter key in source", async () => {
    const fs = await import("node:fs/promises");
    const path = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const __dirname = path.dirname(fileURLToPath(import.meta.url));
    const scriptPath = path.join(__dirname, "../scripts/live-agent-openrouter.mjs");
    const src = await fs.readFile(scriptPath, "utf8");
    // The hardcoded key prefix is what we previously had. Assert it's gone.
    expect(src).not.toMatch(/sk-or-v1-[a-f0-9]{64}/);
    // The new env-var contract is present.
    expect(src).toMatch(/process\.env\.OPENROUTER_API_KEY/);
    expect(src).toMatch(/FATAL/);
  });
});
