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
  // AUTHORITATIVE token source: the create 201 RESPONSE BODY carries the FULL
  // plaintext `corelink_…` token (server-TL). The DOM `keys-new-token` reveal is a
  // MASKED preview (~29 chars) — capturing that yields a truncated, INVALID token
  // (it 401s at introspect). So we read the token from the network response.
  let bodyToken: string | null = null;
  page.on("response", async (r) => {
    if (r.url().includes("/v1/customer/keys") && r.request().method() === "POST") {
      createStatuses.push(r.status());
      const text = await r.text().catch(() => "");
      if (r.status() >= 200 && r.status() < 300 && !bodyToken) {
        // The 201 body is `{ pat: {...}, token: "corelink_…"(96 chars) }`. Take the
        // `token` FIELD directly — a regex over the text truncates the 96-char token
        // at its non-word separator (~char 29), yielding an invalid PAT that 401s.
        try {
          const t = JSON.parse(text)?.token;
          if (typeof t === "string" && t.startsWith("corelink_")) bodyToken = t;
        } catch {
          /* fall through */
        }
      } else if (r.status() >= 400 && !createBody) {
        createBody = text.slice(0, 200);
      }
    }
  });

  const resp = await page.goto("/corelink/en/customer/keys", { waitUntil: "domcontentloaded", timeout: 30_000 }).catch(() => null);
  if (!resp || page.url().includes("/sign-in")) return { pat: null, createStatuses, createBody };
  await page.waitForLoadState("networkidle").catch(() => {});

  // Name the token + grant at least one scope (cache:r — the canonical short form
  // the mint now accepts after server PR #867) so it is usable.
  const name = page.locator('[data-testid="keys-create-name"], input[name="keys-create-name"], #keys-create-name').first();
  await name.fill(`e2e-undercover-${Date.now()}`).catch(() => {});
  const scope = page.locator('[data-testid="keys-scope-cache:r"], input[name="keys-scope-cache:r"]').first();
  if (await scope.isVisible().catch(() => false)) await scope.check().catch(() => {});

  const createBtn = page.getByRole("button", { name: /^create token$/i }).first();
  await createBtn.click().catch(() => {});
  await page.waitForTimeout(3000);

  // Prefer the FULL token from the 201 body; fall back to DOM only if absent.
  if (bodyToken) return { pat: bodyToken, createStatuses, createBody };
  const inputVal = await page
    .locator("input,textarea,code,pre")
    .evaluateAll((els) => els.map((e) => (e as HTMLInputElement).value || e.textContent || "").join("\n"))
    .catch(() => "");
  const m = inputVal.match(CORELINK_PAT_RE) || (await page.content()).match(CORELINK_PAT_RE);
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

// ── PART 2: stranger → runner PAT → real lease — GREEN end-to-end (2026-07-20) ──
// The full cold chain now runs undercover: a fresh stranger mints a REAL 96-char
// PAT in the console (201) and the fabric introspects it (/v1/usage 200), then the
// runner cap gate fires. History: this 401'd until server PR #867 fixed a scope-
// vocabulary bug (the form POSTs `cache:r`; the mint classifier only accepted
// `cache:read`/`cas:r`, not the canonical short form → 401). NOT a race, NOT
// Bearer-vs-cookie (both earlier theories retracted). Fixed + proven live.
test("undercover: a fresh stranger mints a runner PAT and the fabric treats them as a real customer", async ({
  authedPage: page,
  user,
}) => {
  const note = (s: string) => console.log(`[undercover] ${s}`);
  note(`fresh tenant: ${user.email}`);

  // READINESS (hygiene, not load-bearing): Clerk user.created → the signup-worker
  // provisions the tenant + free entitlement in ~3s (server-TL measured). The mint
  // is NOT timing-gated (the #867 fix made it correct, not a race), but a short wait
  // guarantees the tenant row exists before the first authed call.
  await page.waitForTimeout(4000);

  // Mint a runner PAT the way a real user would — in the REAL console
  // (/corelink/en/customer/keys — server-TL confirmed, 00-discover verified).
  const { pat, createStatuses, createBody } = await mintPatInConsole(page);
  if (!pat) {
    // The console-mint 401 (scope-vocab) was fixed in server PR #867 + proven live
    // (pre-roll 401 → post-roll 201). So a mint failure HERE is an unexpected
    // REGRESSION, not the known gap — surface it loudly with the observed status.
    note(`REGRESSION?: console PAT-mint failed — POST /v1/customer/keys statuses=${JSON.stringify(createStatuses)} body=${createBody}`);
    note("This 401'd historically due to a scope-vocab bug fixed in server PR #867 (proven live 201).");
    note("A failure now is unexpected — check: did the create POST use scopes:[\"cache:r\"]? server rollback?");
    test.info().annotations.push({
      type: "regression",
      description: `console PAT-mint failed (statuses ${createStatuses.join(",")}) — expected 201 post-#867`,
    });
    throw new Error(
      `console PAT-mint failed (statuses ${createStatuses.join(",")}; ${createBody}) — expected 201 after server PR #867; possible regression.`,
    );
  }
  expect(pat, "the minted PAT must be the FULL 96-char token from the 201 body").toMatch(CORELINK_PAT_RE);
  expect(pat.length, "a truncated token (regex over body) 401s at introspect — take the JSON field").toBeGreaterThan(60);
  note(`minted a runner PAT in-console (len=${pat.length}, prefix=corelink_…, create=${createStatuses.join(",")}) — value never logged`);

  // 3) Hit the REAL fabric /v1 with the fresh PAT — public Bearer surface only.
  // A VALID full PAT MUST introspect (200); a 401 here would mean a truncated/invalid
  // token capture (the 96-char token has a non-word separator — take the JSON `token`
  // field, never a regex over the body).
  const u = await usage(pat);
  note(`GET /v1/usage → ${u.status} plan_cap=${u.cap ?? "null"} activeNow=${u.activeNow ?? "?"}`);
  expect(u.status, "the fabric must introspect a REAL fresh PAT → 200 (401 ⇒ bad token capture)").toBe(200);

  // 4) Attempt a real lease — assert the ACTUAL entitlement behavior honestly.
  const a = await acquire(pat, { image: VALID_IMAGE, expiryMs: 15_000, tmpRoot: `/tmp/undercover-${Date.now()}` });
  note(`POST /v1/leases → ${a.status} state=${a.state ?? "-"} lease=${a.leaseId ?? "-"}`);

  if (a.status === 429) {
    // OUTCOME-CAP — the correct gate for a fresh FREE tenant: it has NO runner plan
    // (the console shows "No runners plan / Concurrency 0"), so the concurrency gate
    // caps admission at 0 → 429 before the box even provisions. To run a job the
    // tenant must buy a runner plan (concurrency>0) AND install the GitHub App
    // (populates repo_allowlist). This is the honest end-state of a cold free signup.
    note("OUTCOME-CAP (429): fresh free tenant has 0 runner concurrency (no runner plan) → the cap");
    note("holds and admission is refused. PROVEN end-to-end: stranger → signup → REAL 201 PAT mint");
    note("→ /v1 introspects it (200) → acquire correctly gated at the runner-plan cap. Undercover.");
    return;
  }
  if (a.status === 401 || a.status === 403) {
    // OUTCOME-AUTHZ — if the tenant DOES have concurrency but no GitHub App install,
    // C1 authz fail-closes on the empty repo_allowlist (server-TL Q2). Also correct.
    note("OUTCOME-AUTHZ (401/403): acquire fail-closes on an empty repo_allowlist (no GitHub App");
    note("install). PROVEN: signup + prod session + REAL PAT mint + valid /v1 introspect all work.");
    expect([401, 403]).toContain(a.status);
    return;
  }

  // OUTCOME-ADMIT — entitled + allowlisted: a real HELD lease, then close/teardown.
  expect([200, 201], `unexpected acquire status ${a.status}`).toContain(a.status);
  expect(a.leaseId, "an admitted acquire must return a lease id").toBeTruthy();
  note(`OUTCOME-A: lease HELD as a brand-new customer — closing + teardown`);
  const c = await closeLease(pat, a.leaseId!, "succeeded");
  note(`POST /v1/leases/${a.leaseId}/close → ${c.status}`);
  expect([200, 202]).toContain(c.status);
  note("PROVEN end-to-end: stranger → signup → PAT → real lease → clean close, all undercover.");
});
