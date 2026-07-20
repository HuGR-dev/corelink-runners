import { test as base, expect } from "@playwright/test";
import { setupClerkTestingToken } from "@clerk/testing/playwright";
import type { Page } from "@playwright/test";

/**
 * Reusable auth fixture: mints a throwaway Clerk user + one-time sign-in ticket
 * (Backend API), drives a REAL in-browser Clerk sign-in (strategy=ticket), and
 * hands specs an authenticated `page` on humangr.com/corelink. The throwaway
 * user is DSR-deleted on teardown.
 *
 * This is the ONLY way to get a prod-accepted session: a headless FAPI JWT is
 * rejected by the worker's Clerk verification (401); a browser session is not.
 * So the resulting session is INDISTINGUISHABLE from a real customer's — the
 * "undercover" property the runner journey depends on.
 *
 * PROVENANCE: copied from corelink-server `tests/e2e-browser/fixtures/auth.ts`
 * (owner-authorised, 2026-07-19). Kept byte-faithful to the proven recipe; the
 * runner-specific work lives in specs/, not here.
 */

const CLERK_API = "https://api.clerk.com/v1";
const SK = process.env["CLERK_SECRET_KEY"] ?? "";

async function clerkPost(path: string, body: unknown): Promise<any> {
  const r = await fetch(`${CLERK_API}${path}`, {
    method: "POST",
    headers: { Authorization: `Bearer ${SK}`, "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(`Clerk ${path} → ${r.status} ${JSON.stringify(j).slice(0, 200)}`);
  return j;
}

async function clerkDelete(path: string): Promise<void> {
  await fetch(`${CLERK_API}${path}`, { method: "DELETE", headers: { Authorization: `Bearer ${SK}` } });
}

export interface AuthedUser {
  userId: string;
  email: string;
}

/** Sign `page` into a fresh throwaway user; returns the user for later assertions/cleanup. */
export async function signInFreshUser(page: Page): Promise<AuthedUser> {
  const email = `corelink-runners-e2e-${Date.now()}-${Math.floor(Math.random() * 1e6)}@corelink-e2e.dev`;
  const user = await clerkPost("/users", {
    email_address: [email],
    password: `Corelink-e2e-${Math.random().toString(36).slice(2)}!A9`,
    skip_password_checks: true,
  });
  const userId = user.id as string;
  const ticket = (await clerkPost("/sign_in_tokens", { user_id: userId })).token as string;

  await setupClerkTestingToken({ page });
  await page.goto("/corelink/sign-in");
  await page.waitForFunction(() => (window as any).Clerk !== undefined, { timeout: 30_000 });
  await page.evaluate(async () => {
    await (window as any).Clerk.load();
  });
  const res = await page.evaluate(async (t: string) => {
    const clerk = (window as any).Clerk;
    const r = await clerk.client.signIn.create({ strategy: "ticket", ticket: t });
    if (r.status !== "complete") return { ok: false, status: r.status };
    await clerk.setActive({ session: r.createdSessionId });
    return { ok: true, status: r.status };
  }, ticket);
  expect(res.ok, `sign-in status=${res.status}`).toBeTruthy();
  return { userId, email };
}

/** Base test extended with an `authedPage` + the `user` it belongs to. */
export const test = base.extend<{ authedPage: Page; user: AuthedUser }>({
  user: async ({ page }, use) => {
    const u = await signInFreshUser(page);
    await use(u);
    await clerkDelete(`/users/${u.userId}`).catch(() => {});
  },
  authedPage: async ({ page, user: _user }, use) => {
    await use(page);
  },
});

export { expect };
