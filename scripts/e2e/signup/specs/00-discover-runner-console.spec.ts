import { test, expect } from "../fixtures/auth.js";

/**
 * DISCOVERY (not a gate) — dumps the RUNNER console surface for a brand-new
 * authed user so the undercover journey can use real selectors. Prints
 * headings/buttons/testids per candidate route, and flags any affordance that
 * looks like "create an API token / PAT" or a runner entitlement/usage panel.
 *
 * This mirrors corelink-server's 01-discover: the console is server-owned and
 * its selectors are not frozen, so we discover them live rather than guess.
 */

// CORRECTED 2026-07-19 (server-TL): the real authed app lives under the locale +
// the (authenticated) route group → `/corelink/en/customer/*`. The bare
// `/corelink/keys` etc. have no route and fall through to the marketing SPA (200,
// so they LOOK like pages) — that was my false-negative. Probe the real paths.
const RUNNER_ROUTES = [
  "/corelink/en/customer",
  "/corelink/en/customer/keys",
  "/corelink/en/customer/connect",
  "/corelink/en/customer/runners",
  "/corelink/en/customer/usage",
  "/corelink/en/customer/plan",
  // keep two bare paths to DOCUMENT the SPA-fallthrough trap in the same run:
  "/corelink/keys",
  "/corelink/dashboard",
];

test("discover the runner console surface (authed, fresh tenant)", async ({ authedPage: page }) => {
  for (const route of RUNNER_ROUTES) {
    let status = "?";
    try {
      const resp = await page.goto(route, { waitUntil: "domcontentloaded", timeout: 30_000 });
      status = String(resp?.status() ?? "?");
    } catch (e) {
      console.log(`\n### ${route} → NAV ERROR ${(e as Error).message.slice(0, 80)}`);
      continue;
    }
    await page.waitForLoadState("networkidle").catch(() => {});
    const finalUrl = page.url().replace("https://humangr.com", "");
    const bounced = finalUrl.includes("/sign-in");
    const info = await page.evaluate(() => {
      const txt = (el: Element) => (el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 60);
      const headings = [...document.querySelectorAll("h1,h2,h3")].map(txt).filter(Boolean).slice(0, 8);
      const buttons = [...document.querySelectorAll("button, a[role='button'], [type='submit']")]
        .map(txt).filter(Boolean).slice(0, 25);
      const testids = [...document.querySelectorAll("[data-testid]")]
        .map((e) => e.getAttribute("data-testid")).filter(Boolean).slice(0, 40);
      // Anything that looks like a PAT/token affordance or a runner entitlement.
      const tokenish = [...document.querySelectorAll("button,a,[data-testid],input")]
        .map((e) => `${e.tagName.toLowerCase()}:${txt(e) || e.getAttribute("data-testid") || e.getAttribute("name") || ""}`)
        .filter((s) => /token|key|pat|api|runner|concurren|entitle|plan/i.test(s))
        .slice(0, 25);
      return { headings, buttons: [...new Set(buttons)], testids: [...new Set(testids)], tokenish: [...new Set(tokenish)] };
    });
    console.log(`\n### ${route} → HTTP ${status} final=${finalUrl} bounced=${bounced}`);
    console.log(`  H: ${JSON.stringify(info.headings)}`);
    console.log(`  BTN: ${JSON.stringify(info.buttons)}`);
    console.log(`  TESTID: ${JSON.stringify(info.testids)}`);
    console.log(`  TOKENISH: ${JSON.stringify(info.tokenish)}`);
  }
  expect(true).toBeTruthy();
});
