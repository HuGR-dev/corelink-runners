# OWNER ACTION — provision a GitHub App for the fabricd runner-broker (the last piece for the M1 acquire→box path)

> **FOR:** owner · **FROM:** corelink-runners TL · **DATE:** 2026-07-04

## Why
The fabricd box backend is live + the auth/lease seam is proven E2E. To provision a REAL box on a **direct acquire → box** (the M1 product API path), the fabricd must serve a **RUNNER lease**, which needs a **runner-broker** = a **GitHub App** that mints JIT runner configs. That's the one missing piece (creds only — the code passthrough is #278). The autoscaler path (`runs-on: corelink-dogfood`) already provisions boxes and does NOT need this.

## What to create (GitHub org/repo admin — only you)
A **GitHub App** (new, or reuse an existing HuGR one) installed on `humangr-labs` (or the target repo/org):
- **Permissions:** Repository → **Administration: Read & write** (required for `POST .../actions/runners/generate-jitconfig`). Actions: Read is fine to add.
- **Install it** on the repo/org the runners register against; note the **Installation ID**.
- Generate a **private key** (PEM) for the App.

Then you have three values:
| value | where |
|---|---|
| **App ID** | the App's settings page |
| **Installation ID** | the install URL / `GET /app/installations` |
| **Private key (PEM)** | the generated `.pem` |

## How I wire it (once you have them)
Set them as **fabricd secrets** (the `_B64` form for the key survives multi-line mangling):
```bash
cd deploy/cloudflare-fabricd
echo -n '<APP_ID>'          | npx wrangler secret put FABRIC_GITHUB_APP_ID
echo -n '<INSTALLATION_ID>' | npx wrangler secret put FABRIC_GITHUB_APP_INSTALLATION_ID
base64 -i app-private-key.pem | npx wrangler secret put FABRIC_GITHUB_APP_PRIVATE_KEY_B64
```
(You run these — they carry the secret; I never see the key. Or hand me the App ID + Installation ID (non-secret) and I set those, you set the key.)

Then I redeploy + **restart the fabricd container** (delete→redeploy — the ~1-2 min blip we did for the box backend; pg (#269) would remove the state-loss). The broker arms (all-three-or-none, #278). The Server-TL re-runs a **runner-lease** acquire → **box.** Proof closed.

## Net
- Autoscaler → box: ✅ already live.
- fabricd → box (M1 API): needs THIS GitHub App. Code side ready (#278); creds side is yours.

Give me the App ID + Installation ID (and you set the key secret), and I finish it.
