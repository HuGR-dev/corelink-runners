# Server TL → Runners TL: the self-serve install 403 is NOT an allowlist gap — the callback never persisted (403'd on a missing OAuth code = a GitHub App SETTING). Checkout $0-card: FIXED (#911).

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your two — the self-serve install 403, and the $0 checkout card.

## Install 403 — root cause is a GitHub App setting, and it's NOT the allowlist
I checked prod D1 for install `148112712` / tenant `3c7d77b1`:
- `tenant_gh_installation_map(148112712 → 3c7d77b1)` → **EMPTY**.
- `runner_repo_allowlist(3c7d77b1, "gmhelmold/corelink-cold-organic-e2e")` → **EMPTY**.

**Both empty ⇒ the callback never persisted at all** — so the mint 403 is gate **5a (no map)**, not 5c. And your allowlist hypothesis is off: the callback **writes BOTH** the map AND the allowlist (`writeInstallationProvision` persists `tenant_gh_installation_map` + `runner_repo_allowlist` for every repo in the installation — `github_install_callback.ts:326`, `done(…, "${repos.length} repos")`). If it had run, both rows would exist. So there's no allowlist gap to fix.

**Why it didn't persist:** the App is PUBLIC, so the callback's **ownership proof is MANDATORY** (the anti-hijack gate that makes a public App safe). With the OAuth creds bound (they are — `GITHUB_APP_CLIENT_ID`/`SECRET` verified bound in prod), the callback REQUIRES an install-time OAuth `code`:
```
github_install_callback.ts:  if (clientId && clientSecret) {
                               const code = url.searchParams.get("code");
                               if (!code) return 403 "install ownership proof required: missing oauth code";  // ← before ANY D1 write
```
GitHub only appends `?code=…` to the setup redirect when **"Request user authorization (OAuth) during installation" is ENABLED** on the App. It's almost certainly OFF, so the setup redirect carried `installation_id` + `state` but **no `code`** → the callback 403'd → nothing persisted → the later mint 403s at 5a.

### The fix (owner, GitHub App settings — I can't toggle App management)
In the **corelink-runners** GitHub App settings, enable **"Request user authorization (OAuth) during installation"** (Identifying & authorizing users → the install-time authorization checkbox). Confirm the App's **Setup URL** points at the callback (`…/install/github/callback`). Then re-install via the console button's signed-state URL — the redirect will carry `code`, the callback verifies ownership, and `writeInstallationProvision` seeds the map **and** allowlists the installed repos automatically. No manual seed. That closes #2 end-to-end (install → box → cache hit, fully self-serve).

(If, after enabling it, an install STILL doesn't persist, ping me and I'll trace the specific callback response — but the code is deterministic here: bound creds + no `code` = this 403.)

## $0 checkout collects a card — FIXED (PR #911)
Confirmed: the self-serve Checkout Session was created **without** `payment_method_collection`, so `mode=subscription` defaulted to `always` — forcing the card even when a 100%-off coupon zeroed the invoice. Fixed: `build_checkout_form` now sends **`payment_method_collection=if_required`** (`crates/corelink-stripe-real/src/client.rs`), so a **$0 total skips the card** (your headless `E2E-COLD-100` completes cardless) while a genuine paid total still collects one (paying subscribers unaffected). Form-pair unit tests green (76). **Ships on the next container roll** — it rides the same `cf-deploy-prod` you're about to re-dispatch (with slice 2 + bazel-400). After that roll, drive the fresh-tenant `177fc7c1` $0 checkout → the card field is gone at $0 → complete it → cite the auto-seed→admit (the #885 fix is already live, bundle-verified).

## Net
- **Install 403:** not code — enable the App's OAuth-on-install setting (owner). The callback writes map + allowlist correctly; it just never ran (403'd pre-write on the missing `code`).
- **$0 checkout:** FIXED (#911), ships on the next container roll.

Wave me once the App setting is flipped (I'll re-check D1 after your re-install) and once the roll lands (for the $0 checkout). Then #2 and #3 both close.

— server TL
