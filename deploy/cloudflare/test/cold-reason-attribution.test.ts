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
// ⚠️ THE CONTRACT PINNED HERE is that the three stay distinguishable, and that the
// DEFAULT for an unarmed key stays a cold spawn rather than a refusal. A3.17 asks
// for a refusal; it is available under REQUIRE_MINT_KEY=1 and is proven below, but
// defaulting to it would trade silent misattribution for a fleet-wide CI stop on a
// config slip — the exact shape that cost twelve days on 2026-08-31. If a future
// change makes the default refuse, cell 1 goes red.

import { describe, it, expect } from "vitest";
import { buildContainerEnv } from "../src/lib";

const REPO = "acme/api";

describe("buildContainerEnv — cold attribution", () => {
  it("cell 1 — an unarmed mint key spawns COLD by DEFAULT (never a silent refusal)", async () => {
    const r = await buildContainerEnv({}, { jobId: "1", repoFullName: REPO, installationId: "555" });
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({});
    expect(r.coldReason).toBe("mint_key_unarmed");
  });

  it("cell 2 — REQUIRE_MINT_KEY=1 turns the misconfiguration into a HARD DENY", async () => {
    const r = await buildContainerEnv(
      { REQUIRE_MINT_KEY: "1" },
      { jobId: "1", repoFullName: REPO, installationId: "555" },
    );
    expect(r.authz).toBe("forbidden");
    expect(r.coldReason).toBe("mint_key_unarmed");
  });

  it("cell 3 — only the exact '1' arms the refusal; anything else stays cold", async () => {
    for (const v of ["0", "true", "yes", "", " 1"]) {
      const r = await buildContainerEnv(
        { REQUIRE_MINT_KEY: v },
        { jobId: "1", repoFullName: REPO, installationId: "555" },
      );
      expect(r.authz, `REQUIRE_MINT_KEY=${JSON.stringify(v)}`).toBe("ok");
    }
  });

  it("cell 4 — the ORDINARY cold spawn names itself, and is NOT the misconfiguration", async () => {
    const r = await buildContainerEnv(
      { CORELINK_RUNNER_MINT_AUTH_KEY: "k" },
      { jobId: "1", repoFullName: REPO },
    );
    expect(r.authz).toBe("ok");
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
    expect(bad.authz).toBe(ordinary.authz); // same behaviour, different diagnosis
  });
});
