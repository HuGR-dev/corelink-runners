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

/**
 * Mint a PAT in the REAL console — `/corelink/en/customer/keys` (server-TL
 * confirmed 2026-07-19; verified live by 00-discover: "Create token" button,
 * `keys-create-name` input, `keys-scope-*` checkboxes, `keys-new-token` reveal).
 * Returns the token or null (→ honest finding). Value never logged by the caller.
 */
interface MintResult {
  pat: string | null;
  createStatuses: number[];
  createBody: string;
}

async function mintPatInConsole(page: Page): Promise<MintResult> {
  const createStatuses: number[] = [];
  let createBody = "";
  page.on("response", async (r) => {
    if (r.url().includes("/v1/customer/keys") && r.request().method() === "POST") {
      createStatuses.push(r.status());
      if (r.status() >= 400 && !createBody) createBody = (await r.text().catch(() => "")).slice(0, 200);
    }
  });

  const resp = await page.goto("/corelink/en/customer/keys", { waitUntil: "domcontentloaded", timeout: 30_000 }).catch(() => null);
  if (!resp || page.url().includes("/sign-in")) return { pat: null, createStatuses, createBody };
  await page.waitForLoadState("networkidle").catch(() => {});

  // Name the token + grant at least one scope (cache:r) so it is usable.
  const name = page.locator('[data-testid="keys-create-name"], input[name="keys-create-name"], #keys-create-name').first();
  await name.fill(`e2e-undercover-${Date.now()}`).catch(() => {});
  const scope = page.locator('[data-testid="keys-scope-cache:r"], input[name="keys-scope-cache:r"]').first();
  if (await scope.isVisible().catch(() => false)) await scope.check().catch(() => {});

  const createBtn = page.getByRole("button", { name: /^create token$/i }).first();
  await createBtn.click().catch(() => {});
  await page.waitForTimeout(2500);

  // The plaintext token is revealed once, in `keys-new-token`.
  const reveal = await page
    .locator('[data-testid="keys-new-token"], [data-testid*="new-token"]')
    .first()
    .evaluate((el) => (el as HTMLInputElement).value || el.textContent || "")
    .catch(() => "");
  let m = reveal.match(CORELINK_PAT_RE);
  if (m) return { pat: m[0], createStatuses, createBody };

  // Fallbacks: any input value or the page text.
  const inputVal = await page
    .locator("input,textarea,code,pre")
    .evaluateAll((els) => els.map((e) => (e as HTMLInputElement).value || e.textContent || "").join("\n"))
    .catch(() => "");
  m = inputVal.match(CORELINK_PAT_RE) || (await page.content()).match(CORELINK_PAT_RE);
  return { pat: m ? m[0] : null, createStatuses, createBody };
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

  // Mint a runner PAT the way a real user would — in the REAL console
  // (/corelink/en/customer/keys — server-TL confirmed, 00-discover verified).
  const { pat, createStatuses, createBody } = await mintPatInConsole(page);
  if (!pat) {
    // The console + form are correct (00-discover proved "Create token" +
    // keys-create-name + keys-scope-*). The create POST failed — two modes seen,
    // HONESTLY classified (do not overclaim a server outage):
    //  - 503 CONTAINER_UNAVAILABLE (container_start_threw): unambiguously server-side.
    //  - 401 unauthorized: AMBIGUOUS — likely this harness mints the Clerk user via
    //    the Backend API and POSTs before the Clerk→signup-worker webhook has
    //    provisioned the tenant (a provisioning race), NOT necessarily a server bug.
    //    The server's own tests/e2e-browser/05-keys-console reportedly mints OK.
    note(`FINDING: could not mint a PAT — POST /v1/customer/keys statuses=${JSON.stringify(createStatuses)} body=${createBody}`);
    note("The console renders + the create form submits correctly; the create endpoint failed in-run.");
    note("Seen: 503 container_start_threw (server-side) AND 401 unauthorized (likely a fresh-user");
    note("tenant-provisioning race in THIS harness). Open q for server-TL: what provisions the tenant");
    note("for a Backend-API-minted user, so the undercover flow can wait for it? (See relay.)");
    test.info().annotations.push({
      type: "blocked",
      description: `PAT-mint create failed in-run (statuses ${createStatuses.join(",")}) — 503 server-side + 401 likely provisioning race`,
    });
    throw new Error(
      `BLOCKED: PAT-mint create failed (statuses ${createStatuses.join(",")}; ${createBody}) — 503 is server-side; 401 likely a fresh-user provisioning race (see relay).`,
    );
  }
  expect(pat).toMatch(CORELINK_PAT_RE);
  note(`minted a runner PAT in-console (len=${pat.length}, prefix=corelink_…, create=${createStatuses.join(",")}) — value never logged`);

  // 3) Hit the REAL fabric /v1 with the fresh PAT — public Bearer surface only.
  const u = await usage(pat);
  note(`GET /v1/usage → ${u.status} plan_cap=${u.cap ?? "null"} activeNow=${u.activeNow ?? "?"}`);
  expect(u.status, "the fabric must introspect a real fresh PAT (not 5xx)").toBeLessThan(500);

  // 4) Attempt a real lease — assert the ACTUAL entitlement behavior honestly.
  const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 15_000, tmpRoot: `/tmp/undercover-${Date.now()}` });
  note(`POST /v1/leases → ${a.status} state=${a.state ?? "-"} lease=${a.leaseId ?? "-"}`);

  if (a.status === 401 || a.status === 403) {
    // OUTCOME B — correct fail-closed gate. Per server-TL Q2, a fresh tenant with
    // no GitHub App install has an EMPTY repo_allowlist → C1 authz fail-closes.
    // The unblock is the App install ("Connect a tool"), not a code change.
    note("OUTCOME-B: acquire fail-closes (401/403) — the correct gate for an empty repo_allowlist.");
    note("PROVEN: organic signup + prod session + REAL in-console PAT mint all work; the last gate");
    note("to a running job is the GitHub App install (populates repo_allowlist) — server-TL Q2.");
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
