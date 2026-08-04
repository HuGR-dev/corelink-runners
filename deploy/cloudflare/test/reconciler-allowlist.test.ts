/**
 * Config-drift guard for the re-drive reconciler allowlist.
 *
 * # Why this test exists
 *
 * `redriveOrphanedJobs` is the ONLY recovery path for a lost
 * `workflow_job.queued`. GitHub fires that event exactly once, so when it is
 * dropped the job sits queued+labeled+runnerless forever — there is no second
 * webhook and no client-side retry. The reconciler runs every minute and fixes
 * precisely this, but it is OPT-IN: `parseReconcilerRepos` returns `[]` for an
 * absent value and `redriveOrphanedJobs` returns immediately, so a repo missing
 * from the allowlist gets NO recovery and nothing anywhere goes red about it.
 *
 * That silence is the hazard. On 2026-08-04 a `HuGR-Labs/corelink-server` job
 * sat QUEUED for 25 minutes with every `corelink` runner offline; a cancel+rerun
 * got a box instantly, proving the spawn had been LOST rather than refused for
 * capacity. The safety net was healthy and running the whole time — it just was
 * not scanning that repo. corelink-server had moved its Rust PR-gate and docs-ci
 * lanes onto `runs-on: corelink` the day before, which made it the fabric's
 * heaviest consumer while leaving it the one first-party repo the recovery path
 * did not cover.
 *
 * A stale allowlist therefore fails SILENTLY and only shows up as "CI is stuck
 * again". This test makes that failure loud at build time instead.
 *
 * It asserts the shipped `wrangler.jsonc` value, not a hand-written literal,
 * because the value that matters is the one that gets deployed.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { parseReconcilerRepos } from "../src/lib.js";

/** Every first-party repo that dispatches jobs to the `corelink` fabric. */
const FABRIC_CONSUMERS = ["HuGR-Labs/corelink-runners", "HuGR-Labs/corelink-server"] as const;

/**
 * Read `RECONCILER_REPOS` out of the deployed config.
 *
 * `wrangler.jsonc` is JSONC — it carries `//` comments that `JSON.parse`
 * rejects — and the file is heavily commented by design (the reasoning for each
 * var lives next to it). Rather than ship a fragile comment-stripper, pull the
 * one key with a targeted match: the value is a plain double-quoted string, so
 * this cannot be confused by a comment that merely mentions the key, because the
 * match requires the `"KEY": "value"` shape.
 */
function reconcilerReposFromWrangler(): string {
  const path = fileURLToPath(new URL("../wrangler.jsonc", import.meta.url));
  const raw = readFileSync(path, "utf8");
  const match = /^\s*"RECONCILER_REPOS"\s*:\s*"([^"]*)"/m.exec(raw);
  if (!match) {
    throw new Error("RECONCILER_REPOS not found in wrangler.jsonc — did the key move or get renamed?");
  }
  return match[1];
}

describe("re-drive reconciler allowlist (config-drift guard)", () => {
  it("covers every first-party repo that runs jobs on the fabric", () => {
    const repos = parseReconcilerRepos(reconcilerReposFromWrangler());
    for (const consumer of FABRIC_CONSUMERS) {
      expect(
        repos,
        `${consumer} dispatches jobs to the corelink fabric but is NOT in RECONCILER_REPOS. ` +
          "A lost workflow_job.queued for it would strand the job with no recovery: " +
          "GitHub fires that event once, and redriveOrphanedJobs only scans this allowlist.",
      ).toContain(consumer);
    }
  });

  it("parses to well-formed owner/repo entries only", () => {
    const raw = reconcilerReposFromWrangler();
    const repos = parseReconcilerRepos(raw);

    // `parseReconcilerRepos` silently DROPS anything without a "/" — a typo like
    // "HuGR-Labs corelink-server" would halve the allowlist with no error. Assert
    // nothing was dropped, so a malformed entry fails here instead of at 3am.
    const tokens = raw.split(/[,\s]+/).filter((s) => s.length > 0);
    expect(
      repos.length,
      `RECONCILER_REPOS has ${tokens.length} token(s) but only ${repos.length} parsed as owner/repo — ` +
        "an entry was silently dropped (parseReconcilerRepos filters on a literal '/').",
    ).toBe(tokens.length);

    for (const repo of repos) {
      expect(repo, `"${repo}" is not owner/repo shaped`).toMatch(/^[\w.-]+\/[\w.-]+$/);
    }
  });

  it("stays first-party — a cold re-drive skips per-job authz", () => {
    // The allowlist grants an authz-skipping cold re-spawn, so a third-party repo
    // landing here would be a privilege hole, not a convenience. Pin the owner.
    for (const repo of parseReconcilerRepos(reconcilerReposFromWrangler())) {
      expect(
        repo.split("/")[0],
        `${repo} is not under HuGR-Labs. RECONCILER_REPOS grants an authz-skipping ` +
          "cold re-spawn; only first-party repos belong here.",
      ).toBe("HuGR-Labs");
    }
  });
});
