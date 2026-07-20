import { test as base, expect } from "../fixtures/auth.js";
import { signInExistingUser } from "../fixtures/auth.js";

/**
 * Ask 3 verification — does the console "Connect GitHub / Install" button (as the
 * cold tenant 3c7d77b1) redirect to the GitHub App install page with a signed state?
 *   redirect → github.com/apps/<slug>/installations/new?state=…  ⇒ admin-ui secrets
 *     (INSTALL_STATE_SIGNING_KEY + GITHUB_APP_SLUG) are bound in prod (server-TL Q1).
 *   401/500 / no redirect ⇒ admin-ui secrets NOT bound (owner/CF go-live fix).
 * Does NOT complete the GitHub-side install (that needs a fresh org + GitHub auth).
 * Requires COLD_USER_ID. Run manually.
 */
base("Ask3: the console Install button redirects to the GitHub App install (as 3c7d77b1)", async ({ page }) => {
  const note = (s: string) => console.log(`[install] ${s}`);
  const userId = process.env["COLD_USER_ID"];
  expect(userId, "COLD_USER_ID required").toBeTruthy();
  const shot = process.env["SHOT_DIR"] || "/tmp";

  await signInExistingUser(page, userId!);
  note(`signed in as 3c7d77b1 userId=${userId}`);

  // Try the connect + runners pages; dump their GitHub-connect triggers.
  for (const path of ["/corelink/en/customer/connect", "/corelink/en/customer/runners"]) {
    await page.goto(path, { waitUntil: "domcontentloaded" }).catch(() => {});
    await page.waitForTimeout(1500);
    const ui = await page
      .evaluate(() => {
        const vis = (el: Element) => {
          const r = (el as HTMLElement).getBoundingClientRect();
          return r.width > 0 && r.height > 0;
        };
        return Array.from(document.querySelectorAll("button, a, [role=button]"))
          .filter(vis)
          .map((b) => ({ t: (b.textContent || "").trim().slice(0, 40), tid: b.getAttribute("data-testid"), href: (b as HTMLAnchorElement).href || null }))
          .filter((x) => (x.t || x.tid) && /(github|install|connect)/i.test((x.t || "") + (x.tid || "") + (x.href || "")));
      })
      .catch(() => []);
    note(`${path} github/install/connect controls: ${JSON.stringify(ui).slice(0, 800)}`);
  }

  // Click the GitHub install/connect trigger and capture where it lands.
  await page.goto("/corelink/en/customer/runners", { waitUntil: "domcontentloaded" }).catch(() => {});
  await page.waitForTimeout(1500);
  const trigger = page
    .getByRole("button", { name: /connect github|install (the )?(github )?app|install app|connect a tool|install/i })
    .first();
  const linkTrigger = page.getByRole("link", { name: /connect github|install|github app/i }).first();

  const clicker = (await trigger.isVisible().catch(() => false)) ? trigger : linkTrigger;
  const label = (await clicker.textContent().catch(() => "") || "").trim();
  note(`clicking "${label}"`);

  // The install flow may open a popup OR navigate this page. Watch both.
  let popupUrl = "";
  page.on("popup", (p) => { popupUrl = p.url(); });
  const navP = page.waitForURL(/github\.com|\/api\/install\/github|installations\/new/i, { timeout: 20_000 }).catch(() => {});
  const respP = page.waitForResponse((r) => /\/api\/install\/github/i.test(r.url()), { timeout: 15_000 }).catch(() => null);
  await clicker.click().catch(() => {});
  const resp = await respP;
  await navP;
  await page.waitForTimeout(2500);
  await page.screenshot({ path: `${shot}/install-after-click.png`, fullPage: true }).catch(() => {});

  const url = page.url();
  note(`after click: page.url=${url.slice(0, 120)}`);
  if (popupUrl) note(`popup opened: ${popupUrl.slice(0, 120)}`);
  if (resp) note(`/api/install/github response: ${resp.status()} → location=${(resp.headers()["location"] || "").slice(0, 120)}`);

  const buttonTarget = url + " " + popupUrl + " " + (resp ? resp.headers()["location"] || "" : "");
  const buttonDroppedPrefix = /\/api\/install\/github/.test(url) && !/\/corelink\//.test(url);
  note(`button target dropped the /corelink prefix (→ marketing SPA): ${buttonDroppedPrefix}`);

  // The REAL test: hit the PREFIXED admin-ui route directly as the authed tenant. A
  // working route 302s to the GitHub App install with a signed state; 401/500 ⇒ the
  // admin-ui secrets (INSTALL_STATE_SIGNING_KEY / GITHUB_APP_SLUG) are not bound.
  const resp2 = await page.goto("https://humangr.com/corelink/api/install/github", { waitUntil: "domcontentloaded", timeout: 25_000 }).catch(() => null);
  await page.waitForTimeout(2000);
  await page.screenshot({ path: `${shot}/install-prefixed.png`, fullPage: true }).catch(() => {});
  const finalUrl = page.url();
  note(`PREFIXED /corelink/api/install/github → status=${resp2 ? resp2.status() : "ERR"} finalUrl=${finalUrl.slice(0, 130)}`);

  const reachedGitHub = /github\.com\/(apps\/[^/]+\/installations\/new|login|settings\/installations)/i.test(finalUrl) || /installations\/new\?.*state=/i.test(finalUrl + " " + buttonTarget);
  const secretsUnbound = resp2 ? resp2.status() === 401 || resp2.status() === 500 : false;
  note(`✅ RESULT — reached GitHub App install (route + admin-ui secrets OK): ${reachedGitHub}`);
  note(`admin-ui secrets NOT bound (401/500): ${secretsUnbound}`);
  note(`FRONTEND BUG confirmed — Install button drops /corelink prefix: ${buttonDroppedPrefix}`);
  if (!reachedGitHub && !secretsUnbound) note("PREFIXED route INDETERMINATE — see install-prefixed.png.");
});
