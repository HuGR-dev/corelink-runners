import type { Page } from "@playwright/test";

/**
 * Mint a PAT in the REAL console — `/corelink/en/customer/keys`. Returns the FULL
 * 96-char `corelink_…` token from the create-201 RESPONSE BODY (the DOM reveal is a
 * masked ~29-char preview; a regex over the body truncates the token at its non-word
 * separator → an invalid PAT that 401s at introspect). Proven green in
 * specs/10-undercover-signup-to-lease.spec.ts (server PR #867). Value never logged
 * by the caller.
 */
export interface MintResult {
  pat: string | null;
  createStatuses: number[];
  createBody: string;
}

export async function mintPatInConsole(page: Page, scopeTestid = "keys-scope-cache:r"): Promise<MintResult> {
  const createStatuses: number[] = [];
  let createBody = "";
  let bodyToken: string | null = null;
  page.on("response", async (r) => {
    if (r.url().includes("/v1/customer/keys") && r.request().method() === "POST") {
      createStatuses.push(r.status());
      const text = await r.text().catch(() => "");
      if (r.status() >= 200 && r.status() < 300 && !bodyToken) {
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

  const resp = await page
    .goto("/corelink/en/customer/keys", { waitUntil: "domcontentloaded", timeout: 30_000 })
    .catch(() => null);
  if (!resp || page.url().includes("/sign-in")) return { pat: null, createStatuses, createBody };
  await page.waitForLoadState("networkidle").catch(() => {});

  const name = page
    .locator('[data-testid="keys-create-name"], input[name="keys-create-name"], #keys-create-name')
    .first();
  await name.fill(`persistent-cold-${Date.now()}`).catch(() => {});
  const scope = page.locator(`[data-testid="${scopeTestid}"], input[name="${scopeTestid}"]`).first();
  if (await scope.isVisible().catch(() => false)) await scope.check().catch(() => {});

  const createBtn = page.getByRole("button", { name: /^create token$/i }).first();
  await createBtn.click().catch(() => {});
  await page.waitForTimeout(3000);

  return { pat: bodyToken, createStatuses, createBody };
}
