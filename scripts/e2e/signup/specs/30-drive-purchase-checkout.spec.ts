import { test as base, expect } from "../fixtures/auth.js";
import { signInExistingUser } from "../fixtures/auth.js";

/**
 * MONEY PATH — drive the runner-plan checkout as the PERSISTENT cold tenant
 * `3c7d77b1`, enter the 100%-off promo code `E2E-COLD-100` → $0 → complete. The
 * webhook then seeds `runners_entitlement` (runner_starter = 20/100) via the
 * PURCHASE path. Proof of the flip is done separately (acquire 429 → admitted).
 *
 * Requires: COLD_USER_ID (the cold tenant's Clerk userId). Run manually.
 */
const PROMO = "E2E-COLD-100";

base("money path: cold tenant buys the runner plan at $0 via the promo code", async ({ page }) => {
  const note = (s: string) => console.log(`[purchase] ${s}`);
  const userId = process.env["COLD_USER_ID"];
  expect(userId, "COLD_USER_ID env required").toBeTruthy();

  await signInExistingUser(page, userId!);
  note(`signed in as cold tenant userId=${userId}`);

  // Go to the runners plan console + dump the checkout trigger.
  await page.goto("/corelink/en/customer/runners", { waitUntil: "domcontentloaded" }).catch(() => {});
  await page.waitForLoadState("networkidle").catch(() => {});
  note(`runners console url=${page.url().replace("https://humangr.com", "")}`);

  const ui = await page
    .evaluate(() => ({
      buttons: Array.from(document.querySelectorAll("button,a")).map((b) => ({
        t: (b.textContent || "").trim().slice(0, 40),
        testid: b.getAttribute("data-testid"),
        href: (b as HTMLAnchorElement).href || null,
      })).filter((x) => x.t || x.testid),
      headings: Array.from(document.querySelectorAll("h1,h2,h3")).map((h) => (h.textContent || "").trim()).filter(Boolean),
    }))
    .catch(() => ({ buttons: [], headings: [] }));
  note(`headings: ${JSON.stringify(ui.headings)}`);
  note(`clickables: ${JSON.stringify(ui.buttons).slice(0, 900)}`);

  // The nav hrefs drop the /corelink basePath (marketing-SPA trap). Navigate to the
  // billing page WITH the prefix and dump it to find the real checkout trigger.
  await page.goto("/corelink/en/customer/billing", { waitUntil: "domcontentloaded" }).catch(() => {});
  await page.waitForLoadState("networkidle").catch(() => {});
  note(`billing url=${page.url().replace("https://humangr.com", "")}`);
  const bui = await page
    .evaluate(() => ({
      headings: Array.from(document.querySelectorAll("h1,h2,h3")).map((h) => (h.textContent || "").trim()).filter(Boolean),
      buttons: Array.from(document.querySelectorAll("button,a")).map((b) => ({
        t: (b.textContent || "").trim().slice(0, 40),
        testid: b.getAttribute("data-testid"),
      })).filter((x) => x.t || x.testid),
      bodyText: (document.body.innerText || "").slice(0, 400),
    }))
    .catch(() => ({ headings: [], buttons: [], bodyText: "" }));
  note(`billing headings: ${JSON.stringify(bui.headings)}`);
  note(`billing clickables: ${JSON.stringify(bui.buttons).slice(0, 1100)}`);
  note(`billing bodyText: ${JSON.stringify(bui.bodyText)}`);

  // Enumerate each plan button with its CARD context so we pick the RUNNER plan
  // (runner_starter), not a cache plan.
  const cards = await page
    .evaluate(() => {
      const btns = Array.from(document.querySelectorAll('[data-testid="upgrade-open-button"]'));
      return btns.map((b, i) => {
        let card: HTMLElement | null = b as HTMLElement;
        for (let up = 0; up < 6 && card; up++) card = card.parentElement;
        return { i, btnText: (b.textContent || "").trim(), card: ((card?.innerText || "").replace(/\s+/g, " ")).slice(0, 160) };
      });
    })
    .catch(() => []);
  for (const c of cards) note(`plan[${c.i}] btn="${c.btnText}" card="${c.card}"`);

  // Pick the runner plan: a card mentioning runner + starter (fallback: parallel/runner).
  const allBtns = page.locator('[data-testid="upgrade-open-button"]');
  const n = await allBtns.count();
  let picked = -1;
  for (let i = 0; i < n; i++) {
    const ctx = (cards.find((c) => c.i === i)?.card || "").toLowerCase();
    if (/runner/.test(ctx) && /(starter|parallel|per runner|1 runner|solo)/.test(ctx)) {
      picked = i;
      break;
    }
  }
  if (picked < 0) for (let i = 0; i < n; i++) if (/runner/.test((cards.find((c) => c.i === i)?.card || "").toLowerCase())) { picked = i; break; }
  note(`picked runner plan index=${picked}`);
  const before = page.url();
  const shot = process.env["SHOT_DIR"] || "/tmp";
  if (picked >= 0) {
    // Click the runner "Get" — use the button's own visibility (Playwright auto-waits).
    await allBtns.nth(picked).scrollIntoViewIfNeeded().catch(() => {});
    await allBtns.nth(picked).click().catch(() => {});
    await page.waitForTimeout(2000);
    await page.screenshot({ path: `${shot}/after-get.png`, fullPage: true }).catch(() => {});
    const dumpAll = async (tag: string) => {
      const d = await page
        .evaluate(() => {
          const vis = (el: Element) => {
            const r = (el as HTMLElement).getBoundingClientRect();
            const s = getComputedStyle(el as HTMLElement);
            return r.width > 0 && r.height > 0 && s.visibility !== "hidden" && s.display !== "none";
          };
          return Array.from(document.querySelectorAll("button, [role=button], input[type=submit], a"))
            .filter(vis)
            .map((b) => ({ t: (b.textContent || (b as HTMLInputElement).value || "").trim().slice(0, 36), tid: b.getAttribute("data-testid") }))
            .filter((x) => x.t || x.tid);
        })
        .catch(() => []);
      note(`${tag} visible-buttons: ${JSON.stringify(d).slice(0, 1000)}`);
    };
    await dumpAll("AFTER-GET");

    // DPA gate is INLINE (testid `dpa-accept`) and DISABLED until the agreement is
    // scrolled to the end ("Please read to the end to enable acceptance"). Scroll the
    // DPA container to the bottom first, THEN accept → "straight to Stripe Checkout".
    const accept = page.locator('[data-testid="dpa-accept"]').first();
    if (await accept.count().catch(() => 0)) {
      await page.evaluate(() => {
        const els = Array.from(document.querySelectorAll("*")) as HTMLElement[];
        for (const el of els) {
          if (el.scrollHeight > el.clientHeight + 20 && /Data Processing Agreement|DPA|Controller|Processor/i.test(el.innerText || "")) {
            el.scrollTop = el.scrollHeight;
            el.dispatchEvent(new Event("scroll", { bubbles: true }));
          }
        }
      });
      await page.waitForTimeout(1200);
      const enabled = await accept.isEnabled().catch(() => false);
      note(`after DPA scroll: dpa-accept enabled=${enabled}`);
      await Promise.all([
        page.waitForURL(/checkout\.stripe\.com|stripe/i, { timeout: 20_000 }).catch(() => {}),
        accept.click().catch(() => {}),
      ]);
      note(`clicked dpa-accept; url=${page.url().slice(0, 70)}`);
      await page.waitForTimeout(2000);
      await page.screenshot({ path: `${shot}/after-accept.png`, fullPage: true }).catch(() => {});
      await dumpAll("AFTER-ACCEPT");
    }
    // Proceed to Stripe ONLY if the accept didn't already land us there (else we'd
    // click Stripe's own Subscribe button prematurely, before the promo).
    const proceed = page.getByRole("button", { name: /continue to (payment|checkout)|proceed|go to checkout|get starter|start subscription/i }).first();
    if (!page.url().includes("checkout.stripe.com") && (await proceed.isVisible().catch(() => false))) {
      note(`clicking proceed "${(await proceed.textContent().catch(() => "") || "").trim()}"`);
      await Promise.all([
        page.waitForURL(/checkout\.stripe\.com|stripe/i, { timeout: 25_000 }).catch(() => {}),
        proceed.click().catch(() => {}),
      ]);
    }
    note(`post-proceed url=${page.url().slice(0, 90)}`);
  } else {
    note("NO runner plan button identified — see the plan[] dump above.");
  }
  await page.waitForLoadState("domcontentloaded").catch(() => {});
  note(`after trigger: url=${page.url().slice(0, 80)} (was ${before.slice(0, 60)})`);

  const onStripe = page.url().includes("checkout.stripe.com");
  note(`reached Stripe Checkout: ${onStripe}`);
  if (!onStripe) {
    // Not yet on Stripe — dump current page so I can find the right path.
    const dump = await page
      .evaluate(() => ({
        url: location.href,
        buttons: Array.from(document.querySelectorAll("button,a")).map((b) => (b.textContent || "").trim()).filter(Boolean).slice(0, 30),
      }))
      .catch(() => ({}));
    note(`NOT on Stripe — page dump: ${JSON.stringify(dump).slice(0, 700)}`);
    return; // discovery: stop; I'll extend based on this.
  }

  // ── On Stripe Checkout: fill email, apply the promo, verify $0, subscribe ──
  // NOTE: never waitForLoadState("networkidle") on Stripe — its checkout keeps a
  // live connection open and never goes idle (hangs the run). Wait for an element.
  await page.locator('input[type="email"], [data-testid="hosted-payment-submit-button"]').first().waitFor({ timeout: 20_000 }).catch(() => {});
  // Email is required. Use the cold tenant's email.
  const email = process.env["COLD_EMAIL"] || "";
  const emailInput = page.locator('input[type="email"], input[name="email"]').first();
  if (email && (await emailInput.isVisible().catch(() => false))) {
    await emailInput.fill(email).catch(() => {});
    note(`filled email`);
  }
  // Apply the promo code.
  const promoOpener = page.getByText(/add promotion code|promo code|add coupon/i).first();
  if (await promoOpener.isVisible().catch(() => false)) {
    await promoOpener.click().catch(() => {});
    await page.waitForTimeout(800);
  }
  const promoInput = page.locator('input[name="promotionCode"], input[id*="promo" i], input[placeholder*="promo" i], input[placeholder*="code" i], input[placeholder*="Add" i]').first();
  if (await promoInput.isVisible().catch(() => false)) {
    await promoInput.fill(PROMO).catch(() => {});
    const apply = page.getByRole("button", { name: /^apply$/i }).first();
    await apply.click().catch(() => {});
    note(`entered promo ${PROMO} + applied`);
    await page.waitForTimeout(3000);
  } else {
    note("promo input not found");
  }
  await page.screenshot({ path: `${shot}/stripe-after-promo.png`, fullPage: true }).catch(() => {});

  const totalTxt = await page.locator("body").innerText().catch(() => "");
  const zero = /(\$0\.00|total due today\s*\$?0|due today[^0-9]*0\b)/i.test(totalTxt);
  const totalMatch = totalTxt.match(/total due today\s*\$?([0-9.]+)/i);
  note(`Stripe total due today = ${totalMatch ? "$" + totalMatch[1] : "?"} (zero=${zero})`);
  const stillNeedsCard = await page.locator('input[name="cardNumber"], [placeholder*="1234" i]').first().isVisible().catch(() => false);
  note(`card field still shown after promo: ${stillNeedsCard} — if true at $0, the session collects a card (payment_method_collection=always); headless can't complete without a real card`);

  // Only attempt Subscribe when it's genuinely $0 AND no card is demanded — else a
  // click just yields a "card required" validation error (headless can't satisfy it).
  const submit = page.locator('[data-testid="hosted-payment-submit-button"]').first();
  if (zero && !stillNeedsCard) {
    note("$0 and no card required → clicking Subscribe");
    await submit.click().catch(() => {});
    await page.waitForTimeout(6000);
    await page.screenshot({ path: `${shot}/stripe-after-subscribe.png`, fullPage: true }).catch(() => {});
    note(`after subscribe: url=${page.url().slice(0, 90)}`);
  } else {
    note(`NOT clicking Subscribe (zero=${zero}, cardRequired=${stillNeedsCard}) — see stripe-after-promo.png`);
  }
  note(`final url=${page.url().slice(0, 100)}`);
});
