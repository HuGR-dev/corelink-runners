// ─────────────────────────────────────────────────────────────────────────────
// WHY A SPAWN WENT COLD — ★A3.17 / union-01
// ─────────────────────────────────────────────────────────────────────────────
//
// `buildContainerEnv` used to collapse three unrelated facts into ONE silent
// `{ authz: "ok", containerEnv: {} }`:
//
//   • the mint key is unarmed — OUR deployment is misconfigured and EVERY job on
//     the fleet spawns tenantless and unattributed;
//   • the request carries no repo;
//   • there is no installation id and no acquiring PAT — the ORDINARY cold spawn
//     for any repo outside REPO_INSTALLATION_MAP.
//
// The first is a fleet-wide money-attribution outage. The third is Tuesday. Being
// indistinguishable is the defect: nothing could alert on the one that matters.
//
// A3.17 pins all required authorization failures to a hard deny. The reason is
// retained for operators, but it can never authorize a cold spawn.

import { describe, it, expect } from "vitest";
import { buildContainerEnv } from "../src/lib";

const REPO = "acme/api";

describe("buildContainerEnv — cold attribution", () => {
  it("cell 1 — an unarmed mint key is a hard deny", async () => {
    const r = await buildContainerEnv({}, { jobId: "1", repoFullName: REPO, installationId: "555" });
    expect(r.authz).toBe("forbidden");
    expect(r.containerEnv).toEqual({});
    expect(r.coldReason).toBe("mint_key_unarmed");
  });

  it("cell 2 — REQUIRE_MINT_KEY cannot permit the misconfiguration", async () => {
    const r = await buildContainerEnv(
      { REQUIRE_MINT_KEY: "1" },
      { jobId: "1", repoFullName: REPO, installationId: "555" },
    );
    expect(r.authz).toBe("forbidden");
    expect(r.coldReason).toBe("mint_key_unarmed");
  });

  it("cell 3 — every REQUIRE_MINT_KEY value remains fail-closed", async () => {
    for (const v of ["0", "true", "yes", "", " 1"]) {
      const r = await buildContainerEnv(
        { REQUIRE_MINT_KEY: v },
        { jobId: "1", repoFullName: REPO, installationId: "555" },
      );
      expect(r.authz, `REQUIRE_MINT_KEY=${JSON.stringify(v)}`).toBe("forbidden");
    }
  });

  it("cell 4 — the ORDINARY cold spawn names itself, and is NOT the misconfiguration", async () => {
    const r = await buildContainerEnv(
      { CORELINK_RUNNER_MINT_AUTH_KEY: "k" },
      { jobId: "1", repoFullName: REPO },
    );
    expect(r.authz).toBe("forbidden");
    expect(r.coldReason).toBe("no_installation_or_pat");
    expect(r.coldReason).not.toBe("mint_key_unarmed");
  });

  it("cell 5 — a missing repo is its own reason, not folded into the others", async () => {
    const r = await buildContainerEnv(
      { CORELINK_RUNNER_MINT_AUTH_KEY: "k" },
      { jobId: "1", repoFullName: "", installationId: "555" },
    );
    expect(r.coldReason).toBe("no_repo");
  });

  // The whole point: an alert on the misconfiguration must not fire on Tuesday's
  // ordinary cold spawn. If these two ever return the same reason, alerting on the
  // dangerous one is impossible again.
  it("cell 6 — the misconfiguration and the ordinary cold spawn are DISTINGUISHABLE", async () => {
    const bad = await buildContainerEnv({}, { jobId: "1", repoFullName: REPO, installationId: "5" });
    const ordinary = await buildContainerEnv(
      { CORELINK_RUNNER_MINT_AUTH_KEY: "k" },
      { jobId: "1", repoFullName: REPO },
    );
    expect(bad.coldReason).not.toBe(ordinary.coldReason);
    expect(bad.authz).toBe("forbidden");
    expect(ordinary.authz).toBe("forbidden");
  });
});
