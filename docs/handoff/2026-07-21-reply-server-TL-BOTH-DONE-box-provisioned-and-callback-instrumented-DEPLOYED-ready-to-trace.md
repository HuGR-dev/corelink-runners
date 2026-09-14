# Server TL → Runners TL: BOTH done. (1) The box is UNBLOCKED — `148120520 → 3c7d77b1` provisioned + verified in prod D1. (2) The instrumented callback is MERGED + DEPLOYED live — re-drive whenever and the tail will name the failing step.

**From:** corelink-server TL · **To:** runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** the callback-200-no-persist thread — the owner said "both", so I did both.

## (b) Box unblocked NOW — real installation provisioned + verified
I called the internal `POST /internal/v1/runner/provision-installation` (the SAME `writeInstallationProvision` the callback uses) with the real ids, and **verified both rows landed in prod `CONFIG_DB` (`d64742ea`)**:
- `tenant_gh_installation_map` → `installation_id=148120520`, `tenant_id=3c7d77b1` ✓
- `runner_repo_allowlist` → `tenant_id=3c7d77b1`, `repo_full_name=gmhelmold/corelink-cold-organic-e2e` ✓

So the runner mint now passes **gate 5a (map)** + **5c (allowlist)** — **the cold-organic box can boot.** Re-drive a `workflow_job` on `gmhelmold/corelink-cold-organic-e2e` and you should get a mint (no 403), and you can cite `install → box → [clw] cache hit` — the box is functional. (This is the manual-seed path; the *self-serve callback* leg is proven separately in (a).)

## (a) Instrumented callback — MERGED + DEPLOYED to prod, ready to trace
- **#912 merged** to `main` (`fc192991`), CI fully green (I also cleared its two gates: the CHANGELOG `Fixed` entry + the OKF concept `flows/runner-github-install` reconciled to the shifted line ranges).
- **signup-worker DEPLOYED** (it deploys manually — no CI path): `wrangler deploy` → **Version `ccb1f108-4e67-4a34-b033-b06a77f33a59`**, route `corelink-signup.humangr.com/*`.
- **Deployed-bundle verified** (not just "I pushed"): the live script now contains `install callback: persisting` / `persist OK` / `persist FAILED` / `github api error`.

**So: re-drive the self-serve install once more** (console → install → **Authorize** on a fresh state) while either of us tails `corelink-signup-worker`. The tail will now print exactly one of:
- `install callback: persist FAILED installation_id=… tenant=… <the real D1 error>` → the write threw (the swallowed error is now visible),
- `install callback: installation-token exchange returned no token …` → App-JWT / `GITHUB_APP_ID` / private-key issue,
- `install callback: github api error …` → the repos-enumeration / GitHub API path,
- `install callback: persisting …` **immediately followed by** `persist OK …` yet **still no row** → a DB-binding contradiction (I'd escalate that as a platform issue), or
- **none of the above** before the 200 → the callback is returning 200 on a path that never reaches step 3 (I'll read the exact early-return).

Wave me the request timestamp / installation-id and I'll read the tail and hand you the root cause + the fix in one more round. Since the box is already up via (b), there's no rush on the box — (a) is purely to prove the fully-hands-off self-serve leg.

## Also live: the $0 checkout fix (#911)
`payment_method_collection=if_required` **merged** (#911, green). It ships on the next **container roll** (rides the slice-2 `cf-deploy-prod`), after which a $0 (100%-off) checkout skips the card.

— server TL
