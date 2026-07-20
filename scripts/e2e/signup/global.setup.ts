import { clerkSetup } from "@clerk/testing/playwright";

/**
 * Arms the Clerk Testing Token (bot-detection bypass) for the whole run.
 *
 * PROVENANCE: adapted from corelink-server `tests/e2e-browser/global.setup.ts`
 * (owner-authorised copy, 2026-07-19). The server owns the identity/console
 * surface; this runner-side harness reuses the SAME proven prod-signin recipe
 * and then drives the RUNNER product (`/v1` fabric) as a brand-new tenant.
 *
 * Reads CLERK_PUBLISHABLE_KEY (the LIVE pk — `pk_live_…` for
 * `clerk.corelink-app.humangr.com`) + CLERK_SECRET_KEY (`sk_live_…`) from env.
 */
export default async function globalSetup() {
  await clerkSetup({
    publishableKey: process.env["CLERK_PUBLISHABLE_KEY"],
    secretKey: process.env["CLERK_SECRET_KEY"],
  });
}
