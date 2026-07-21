import { test as base, expect } from "@playwright/test";

/**
 * Ask A — complete the server-TL's pre-built $0 Checkout Session (coupon czq6huAC
 * pre-applied, payment_method_collection:if_required, subscription metadata
 * tenant_id=3c7d77b1). At $0 Stripe skips the card, so "Subscribe" completes headless
 * → checkout.session.completed → customer.subscription.created → runner_billing +
 * runners_entitlement 0→20 (purchase path). The flip is verified separately (acquire).
 *
 * Requires CHECKOUT_URL. Run manually.
 */
base("Ask A: complete the $0 runner-plan checkout (purchase path seeds the entitlement)", async ({ page }) => {
  const note = (s: string) => console.log(`[zero-checkout] ${s}`);
  const url = process.env["CHECKOUT_URL"];
  expect(url, "CHECKOUT_URL required").toBeTruthy();
  const shot = process.env["SHOT_DIR"] || "/tmp";
  const email = process.env["COLD_EMAIL"] || "";

  await page.goto(url!, { waitUntil: "domcontentloaded", timeout: 30_000 });
  await page.locator('[data-testid="hosted-payment-submit-button"], input[type="email"]').first().waitFor({ timeout: 25_000 }).catch(() => {});
  await page.waitForTimeout(1500);
  await page.screenshot({ path: `${shot}/zero-checkout-open.png`, fullPage: true }).catch(() => {});

  const bodyText = await page.locator("body").innerText().catch(() => "");
  const totalMatch = bodyText.match(/total due today\s*\$?([0-9.]+)/i);
  note(`total due today = ${totalMatch ? "$" + totalMatch[1] : "?"}`);
  const cardShown = await page.locator('input[name="cardNumber"], [placeholder*="1234" i]').first().isVisible().catch(() => false);
  note(`card field shown (should be FALSE at $0 with if_required): ${cardShown}`);

  // Email may be required even at $0.
  const emailInput = page.locator('input[type="email"], input[name="email"]').first();
  if (email && (await emailInput.isVisible().catch(() => false)) && !(await emailInput.inputValue().catch(() => ""))) {
    await emailInput.fill(email).catch(() => {});
    note("filled email");
  }

  const submit = page.locator('[data-testid="hosted-payment-submit-button"]').first();
  const submitLabel = (await submit.textContent().catch(() => "") || "").trim();
  note(`submit button: "${submitLabel}"`);
  expect(await submit.isVisible().catch(() => false), "Subscribe button must be visible").toBeTruthy();

  await Promise.all([
    page.waitForURL(/success|complete|thank|\/corelink|humangr\.com/i, { timeout: 30_000 }).catch(() => {}),
    submit.click().catch(() => {}),
  ]);
  await page.waitForTimeout(6000);
  await page.screenshot({ path: `${shot}/zero-checkout-done.png`, fullPage: true }).catch(() => {});
  const finalUrl = page.url();
  note(`after Subscribe: url=${finalUrl.slice(0, 120)}`);

  const stillOnStripe = finalUrl.includes("checkout.stripe.com");
  const doneText = await page.locator("body").innerText().catch(() => "");
  const succeeded = !stillOnStripe || /thank|success|subscription|confirmed|payment received/i.test(doneText);
  note(`checkout completed (left Stripe or shows success): ${succeeded}`);
  note(`RESULT: ${succeeded ? "SUBSCRIBED — purchase fired; verify runners_entitlement 0→20 via acquire" : "did NOT complete — see zero-checkout-done.png"}`);
});
