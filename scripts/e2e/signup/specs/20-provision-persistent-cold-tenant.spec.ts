import { test as base, expect } from "@playwright/test";
import { signInFreshUser } from "../fixtures/auth.js";
import { mintPatInConsole } from "../fixtures/mint.js";
import fs from "node:fs";

/**
 * PROVISION a PERSISTENT cold-organic tenant (does NOT DSR-delete — unlike the
 * `test` fixture in auth.ts). Purpose: create a real stranger tenant that SURVIVES
 * so the server-TL can grant it runner concurrency (and a Stripe 100%-off checkout
 * can entitle it), then prove OUTCOME-ADMIT (a real box) against the SAME tenant.
 *
 * Outputs (identifiers the server-TL relay needs to resolve + grant the tenant):
 *   - email, userId (Clerk), any org/tenant hints
 *   - the minted PAT: written to the OOB path in COLD_PAT_OUT (NEVER logged/committed)
 *   - pat length + prefix printed for the record
 *
 * Run manually (never in blocking CI): real signup + a real PAT that persists.
 */
base("provision a persistent cold-organic tenant + mint a runner PAT (survives for the grant)", async ({
  page,
}) => {
  const note = (s: string) => console.log(`[persistent-cold] ${s}`);

  const user = await signInFreshUser(page);
  note(`SIGNED IN (persistent, NOT deleted): email=${user.email} userId=${user.userId}`);

  // Tenant/org hints the server-TL can use to resolve the tenant row.
  const clerkHints = await page
    .evaluate(() => {
      const c = (window as any).Clerk;
      return {
        userId: c?.user?.id ?? null,
        orgId: c?.organization?.id ?? null,
        orgMemberships: (c?.user?.organizationMemberships ?? []).map((m: any) => m?.organization?.id),
        primaryEmail: c?.user?.primaryEmailAddress?.emailAddress ?? null,
      };
    })
    .catch(() => ({}));
  note(`CLERK HINTS: ${JSON.stringify(clerkHints)}`);

  // Readiness: tenant row provisions ~3s after user.created (server-TL measured).
  await page.waitForTimeout(4000);

  const { pat, createStatuses } = await mintPatInConsole(page);
  expect(pat, "must mint a real 96-char PAT (server PR #867)").toBeTruthy();
  expect(pat!.length, "full token, not a truncated capture").toBeGreaterThan(60);
  note(`MINTED PAT len=${pat!.length} prefix=${pat!.slice(0, 9)}… create=${createStatuses.join(",")} — value NOT logged`);

  // Persist the PAT to the OOB path (for the later /v1 OUTCOME-ADMIT proof). Never
  // to the repo, never to the console.
  const out = process.env["COLD_PAT_OUT"];
  if (out) {
    fs.writeFileSync(
      out,
      JSON.stringify({ email: user.email, userId: user.userId, pat, clerkHints }, null, 2) + "\n",
    );
    note(`wrote {email,userId,pat,hints} to OOB COLD_PAT_OUT`);
  }

  // Sanity: the fabric introspects this fresh PAT (proves the PAT is valid + the
  // tenant exists), and acquire is correctly capped at 0 concurrency PRE-grant.
  const usageRes = await page.request
    .get("https://corelink-api.humangr.com/v1/usage", { headers: { authorization: `Bearer ${pat}` } })
    .catch(() => null);
  note(`GET /v1/usage → ${usageRes ? usageRes.status() : "ERR"} (200 ⇒ valid PAT; cap is 0 pre-grant)`);
});
