import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * T8-W4b's boundary contract is exercised by the host-shell fixture because
 * the behavior under test is the two real container entrypoints: they create
 * the short-lived 0400 auth file, re-exec without the provider bearer, and
 * remove the file on every exit path.  Keep this test in the Worker suite so
 * the DAG-required acceptance surface cannot disappear from collection.
 */
describe("T8-W4b auth secret bridge boundary", () => {
  it("runs the check-host and DevEnv bridge fixture without exposing the raw secret", () => {
    const script = fileURLToPath(
      new URL("../../check-host/test/auth-secret-bridge.sh", import.meta.url),
    );
    const result = spawnSync("sh", [script], {
      encoding: "utf8",
      timeout: 120_000,
      env: {
        PATH: process.env.PATH,
        TMPDIR: process.env.TMPDIR,
        // The real DevEnv entrypoint requires the ticket path and endpoint
        // before it reaches supervisord; these are inert fixture values.
        CLW_CRED_TICKET: "fixture-ticket",
        CLW_LEASE_ID: "fixture-lease",
        CLW_FABRIC_ENDPOINT: "https://fixture.invalid",
      },
    });

    expect(result.error).toBeUndefined();
    expect(result.status).toBe(0);
    expect(`${result.stdout}\n${result.stderr}`).toContain(
      "auth-secret-bridge: PASS",
    );

    // The fixture uses these sentinels for the check-host, marked, TERM,
    // failure, hydration, and DevEnv paths.  A boundary failure must not echo
    // any of them through stdout/stderr or an argv diagnostic.
    for (const secret of [
      "bridge-secret",
      "marked-secret",
      "term-secret",
      "fail-secret",
      "hydrate-fail",
      "cloud-secret",
      "marked-init-secret",
      "cloud-term",
      "cloud-fail",
      "cloud-hydrate-fail",
    ]) {
      expect(`${result.stdout}\n${result.stderr}`).not.toContain(secret);
    }
  });
});
