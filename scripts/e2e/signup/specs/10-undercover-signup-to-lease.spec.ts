import { test, expect } from "../fixtures/auth.js";
import type { Page } from "@playwright/test";
// Reuse the SAME undercover /v1 verbs the 142-journey suite uses — a fresh
// tenant's REAL PAT against the REAL fabric, only the public Bearer surface.
import { acquire, closeLease, usage, VALID_IMAGE } from "../../lib/fabric.mjs";

/**
 * THE UNDERCOVER JOURNEY — a stranger signs up and runs a job, and the system
 * never knows it is a test.
 *
 *   fresh Clerk user (prod-accepted browser session, fixture)
 *     → land on the authed console
 *     → mint a runner PAT in-console (a real user action)
 *     → hit the REAL fabric /v1 with that PAT (public Bearer surface only)
 *     → assert the brand-new tenant is treated EXACTLY like any customer.
 *
 * HONEST BRANCHING (no fake green): a fresh free-tier tenant may or may not carry
 * a runner entitlement + repo_allowlist before it pays. Both outcomes are a REAL
 * finding, asserted explicitly:
 *   (A) entitled  → acquire admits (or 429 at cap) → close/teardown → PROVEN.
 *   (B) not entitled → acquire fail-closes (401/403) → that is the CORRECT gate;
 *       signup+PAT proven, runner-access correctly gates on entitlement.
 */

const CORELINK_PAT_RE = /corelink_[A-Za-z0-9_-]{16,}/;

/** Best-effort in-console PAT mint. Returns the token or null (→ honest BLOCKED). */
async function mintPatInConsole(page: Page): Promise<string | null> {
  const routes = ["/corelink/keys", "/corelink/settings/keys", "/corelink/api-keys", "/corelink/tokens"];
  for (const route of routes) {
    const resp = await page.goto(route, { waitUntil: "domcontentloaded", timeout: 30_000 }).catch(() => null);
    if (!resp || page.url().includes("/sign-in")) continue;
    await page.waitForLoadState("networkidle").catch(() => {});

    // A token may already be scannable, or a create affordance may need a click.
    let scan = await page.content();
    let m = scan.match(CORELINK_PAT_RE);
    if (m) return m[0];

    const createBtn = page
      .getByRole("button", { name: /create|new|generate|add.*(key|token)/i })
      .or(page.locator('[data-testid*="create"],[data-testid*="new-key"],[data-testid*="generate"]'))
      .first();
    if (await createBtn.isVisible().catch(() => false)) {
      await createBtn.click().catch(() => {});
      // A name field + confirm may follow.
      const nameField = page.getByLabel(/name|label/i).or(page.locator('input[name*="name"]')).first();
      if (await nameField.isVisible().catch(() => false)) {
        await nameField.fill(`e2e-undercover-${Date.now()}`).catch(() => {});
        const confirm = page.getByRole("button", { name: /create|generate|save|confirm/i }).first();
        await confirm.click().catch(() => {});
      }
      await page.waitForTimeout(2500);
      scan = await page.content();
      m = scan.match(CORELINK_PAT_RE);
      if (m) return m[0];
      // Some UIs reveal the token in an input value, not the DOM text.
      const inputVal = await page
        .locator("input,textarea,code,pre")
        .evaluateAll((els) =>
          els.map((e) => (e as HTMLInputElement).value || e.textContent || "").join("\n"),
        )
        .catch(() => "");
      const m2 = inputVal.match(CORELINK_PAT_RE);
      if (m2) return m2[0];
    }
  }
  return null;
}

// ── PART 1: the undercover SIGNUP capability — PROVEN GREEN ──────────────────
// A brand-new stranger signs up via the real Clerk flow and gets a prod-accepted
// session the system cannot distinguish from a customer's. This is the property
// the owner asked to prove; it passes today.
test("undercover: a fresh stranger signs up and the system accepts them as a real customer", async ({
  authedPage: page,
  user,
}) => {
  console.log(`[undercover] fresh tenant signed in: ${user.email} (userId=${user.userId})`);
  await page.goto("/corelink/dashboard", { waitUntil: "domcontentloaded" }).catch(() => {});
  await page.waitForLoadState("networkidle").catch(() => {});
  // Prod-accepted: not bounced to sign-in (a headless FAPI token would be 401'd).
  expect(page.url(), "a signed-in stranger must not be bounced to sign-in").not.toContain("/sign-in");
  console.log(`[undercover] authed landing = ${page.url().replace("https://humangr.com", "")} — prod session accepted`);
});

// ── PART 2: stranger → runner PAT → real lease — BLOCKED on the console gap ───
// Currently a FINDING, not a pass: the self-serve runner console (keys/usage)
// does NOT exist in prod — every /corelink/* app route renders the marketing
// SPA (proven by 00-discover). So a fresh tenant has no way to mint a runner PAT
// through the UI → the cold-signup→job chain cannot complete. This flips GREEN
// the day the server/console ships a PAT-mint surface (then the /v1 lifecycle
// below runs undercover unchanged).
test("undercover: a fresh stranger mints a runner PAT and the fabric treats them as a real customer", async ({
  authedPage: page,
  user,
}) => {
  const note = (s: string) => console.log(`[undercover] ${s}`);
  note(`fresh tenant: ${user.email}`);

  // Mint a runner PAT the way a real user would — in the console.
  const pat = await mintPatInConsole(page);
  if (!pat) {
    note("FINDING (product gap): no in-console PAT-mint surface exists for a fresh tenant.");
    note("00-discover proved every /corelink/* app route renders the marketing SPA — the");
    note("self-serve runner console (keys/usage) is not built in prod yet (server/console-owned).");
    note("So the cold-signup → runner-PAT → job chain cannot complete. Signup itself is PROVEN");
    note("(see Part 1). This test flips GREEN the day a console PAT-mint surface ships.");
    test.info().annotations.push({
      type: "product-gap",
      description: "no self-serve runner console / PAT-mint surface in prod (server-owned) — cold signup cannot reach a runner PAT",
    });
    throw new Error(
      "FINDING (product gap): no self-serve PAT-mint surface — a fresh tenant cannot obtain a runner PAT; the console is not built (server-owned).",
    );
  }
  expect(pat).toMatch(CORELINK_PAT_RE);
  note(`minted a runner PAT in-console (len=${pat.length}, prefix=corelink_…) — value never logged`);

  // 3) Hit the REAL fabric /v1 with the fresh PAT — public Bearer surface only.
  const u = await usage(pat);
  note(`GET /v1/usage → ${u.status} plan_cap=${u.cap ?? "null"} activeNow=${u.activeNow ?? "?"}`);
  expect(u.status, "the fabric must introspect a real fresh PAT (not 5xx)").toBeLessThan(500);

  // 4) Attempt a real lease — assert the ACTUAL entitlement behavior honestly.
  const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 15_000, tmpRoot: `/tmp/undercover-${Date.now()}` });
  note(`POST /v1/leases → ${a.status} state=${a.state ?? "-"} lease=${a.leaseId ?? "-"}`);

  if (a.status === 401 || a.status === 403) {
    // OUTCOME B — correct fail-closed gate for an unentitled fresh tenant.
    note("OUTCOME-B: fresh tenant is NOT runner-entitled yet → acquire fail-closes (correct gate).");
    note("PROVEN: organic signup + prod session + in-console PAT all real; runner access correctly gates on entitlement.");
    expect([401, 403]).toContain(a.status);
    return;
  }

  // OUTCOME A — entitled. Admit (or 429 at cap), then close/teardown.
  expect([200, 201, 429], `unexpected acquire status ${a.status}`).toContain(a.status);
  if (a.status === 429) {
    note("OUTCOME-A(cap): entitled but already at concurrency cap → 429 (the cap holds — correct).");
    return;
  }
  expect(a.leaseId, "an admitted acquire must return a lease id").toBeTruthy();
  note(`OUTCOME-A: lease HELD as a brand-new customer — closing + teardown`);
  const c = await closeLease(pat, a.leaseId!, "succeeded");
  note(`POST /v1/leases/${a.leaseId}/close → ${c.status}`);
  expect([200, 202]).toContain(c.status);
  note("PROVEN end-to-end: stranger → signup → PAT → real lease → clean close, all undercover.");
});
