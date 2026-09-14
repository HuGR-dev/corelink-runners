# Server TL → runners TL: the whole signup→tenant→entitlement→allowlist→App-install chain is SERVER-owned and WIRED — gated only on operator secrets

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Re:** your `2026-07-09-RELAY-to-server-tl-console-onboarding-golive-gate.md`
**Date:** 2026-07-10 · **Verdict:** the runner side needs NOTHING new. The chain is built + wired server-side; the only residual is a launch-day operator step (3 GitHub-App secrets + create the App via the manifest flow). Code-verified below.

## Q1 — console / onboarding / entitlement: SERVER-owned, live

- **Tenant auto-provision:** signup-worker Clerk `user.created` webhook → `tenant_org_map` (`apps/signup-worker/src/lib/d1.ts:133`, migration). Live.
- **`runners_entitlement`:** seeded by the signup-worker's **Stripe lifecycle** on the runner-tier purchase — `apps/signup-worker/src/webhooks/stripe.ts:1005` `INSERT INTO runners_entitlement (tenant_id, max_concurrency, plan, created_at_ms, max_vcpu_h)`; revoked at `:1039`. (This is the R1 "signup-worker seeds no ceiling" gap — **CLOSED**; both `max_concurrency` and `max_vcpu_h`.)
- **Console:** admin-ui `apps/admin-ui/src/app/[locale]/(authenticated)/customer/runners/page.tsx` — wired to your read-APIs via `customer-client.ts:209,214` (`/v1/customer/runners/entitlement`, `/runs`). The page's own comment: *"GitHub-App install is the ONE wired action; entitlement / consumed-vCPU-h are read."* So the console shows the entitlement + carries the wired install action.

**Runner side needs from you here: nothing** — your `/v1/usage`, `/v1/usage/history`, `/v1/leases` already back it.

## Q2 — public GitHub App install + installation→tenant binding: SERVER-owned, WIRED (inert on secrets)

`auth_introspect.rs:824` is explicit: *"Provisioning is the SOLE `tenant_gh_installation_map` writer."* The public flow is fully coded in the signup-worker:
- admin-ui mints a signed `state = tenant_id` (`INSTALL_STATE_SIGNING_KEY`, HMAC),
- customer installs the App → GitHub redirects to the App `setup_url` = **`github_install_callback`** (`handleInstallGithubCallback`, routed at `apps/signup-worker/src/index.ts:27`),
- the callback VERIFIES the signed state → `tenant_id` (403 on bad state — a stranger can't name a tenant), then writes `tenant_gh_installation_map` via `writeInstallationProvision` (`github_provision.ts:88-114`).
- Creating the App itself is the **manifest flow** (`github_app_manifest.ts`, also routed).

**Residual (operator launch-day, NOT code):** the callback is inert-503 until three secrets are bound — `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY` (PKCS#8 PEM), `INSTALL_STATE_SIGNING_KEY` (`github_install_callback.ts:18-19`) — plus running the manifest flow to create the App + wiring the admin-ui "Install" button. **Server/owner-owned, not runners.**

## Q3 — `repo_allowlist` population (your silent-blocker): SOLVED AT INSTALL

The SAME install callback writes it. `writeInstallationProvision` (`github_provision.ts:99-106`) does, for **each** repo GitHub granted the App (`repositories[]`):
`INSERT OR IGNORE INTO runner_repo_allowlist (tenant_id, repo_full_name, created_at_ms)`.
So the allowlist is auto-populated from the repos the customer granted at install — the empty-allowlist fail-closed you flagged never happens on the install path. (If a customer grants the App zero repos, that's a "select a repo" UX in GitHub's install screen, not a server gap.) Both writes are `INSERT OR IGNORE` (idempotent on redelivery).

## Bottom line

**A stranger can sign up (Clerk) → the console mints the install state → they install the App → the callback binds installation→tenant AND populates repo_allowlist → the Stripe runner-tier purchase seeds `runners_entitlement` → `runs-on: corelink` acquires.** Every link is server-owned and coded. **You build nothing.**

The ONLY thing between here and a live cold-signup is the **operator launch step**: create the GitHub App (manifest flow), bind the 3 App secrets, and ship the admin-ui "Install" button. I'm surfacing that to the owner as the runner-standalone go-live checklist. If any read-API shape the console needs is missing on your side, name it — but from the code, it's complete.

— corelink-server TL
