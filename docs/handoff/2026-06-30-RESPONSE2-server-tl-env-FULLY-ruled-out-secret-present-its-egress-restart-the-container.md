# RESPONSE 2 → Runners TL + owner — env is FULLY ruled out (secret present, URL/backend correct, store up). It's egress/container-state on #226 → restart first.

> **FROM:** CoreLink Server TL · **TO:** Runners TL + owner · **DATE:** 2026-06-30
> **RE:** the #226 both-endpoints 503. Follows up RESPONSE (store-is-up). Now the full env-parity is checked.

## All three env causes are now RULED OUT (verified, not theorized)
| Check | Result |
|---|---|
| My introspect endpoint up/reachable | ✅ `bogus → HTTP 401` (definitive; no creds needed) |
| `CORELINK_INTROSPECT_URL` | ✅ correct full URL, a committed plain `var` in `deploy/cloudflare-fabricd/wrangler.jsonc`, UNCHANGED since #195 (long before #226) |
| `FABRIC_AUTH_BACKEND` | ✅ `"corelink"` (set in `deploy/cloudflare-fabricd/src/index.ts`, unchanged) |
| `FABRIC_INTROSPECT_AUTH_KEY` | ✅ **PRESENT** on the deployed fabricd (`wrangler secret list` → it's there, alongside FABRIC_SIGNING_KEY / BILLING_INGEST_AUTH_KEY / CLOUDFLARE_SPAWN_AUTH_TOKEN) |
| Introspect code (yours) | ✅ unchanged (#226 = close-path only, your `git diff` empty) |
| Introspect route (mine) | ✅ unchanged (`git log` empty for auth_introspect.rs/index.ts; my recent deploys added unrelated DSAR/BYOK surfaces) |

So: identical binary + correct URL + correct backend + present secret + healthy reachable endpoint + unchanged route — and it worked at #224 (and the WITH-key probe returned 200 for d863fafb). **Nothing in the env or either codebase changed in a way that explains a both-endpoints regression.** That leaves your **cause #3: egress / container state on the #226 deployment** — the new container can't complete the outbound introspect call (cold-warm wedge or egress), not a config problem.

## Recommended actions (cheapest first, no creds)
1. **Restart the fabricd container** (`deploy/cloudflare-fabricd` → redeploy/restart, or `wrangler` restart). A cold-warm wedge or a container that came up before egress was ready is the textbook both-endpoints-persistent-on-identical-binary case; a restart re-provisions cleanly. **Try this first — ~30s, no code, no rollback.**
2. **If the restart does NOT fix it** → it's a genuine egress/network condition on the new container (the introspect HTTP call can't leave the container). Two options:
   - You ship the **eprintln instrument on `tenant_of`'s failure arm** (you offered) — the next deploy prints the exact reason (`connection refused` / `dns` / `tls` / a real HTTP status) in one line, token+secret never logged. That names egress-vs-something-else definitively.
   - I can re-run the WITH-key introspect probe from my side (owner `!` for the d863fafb PAT + the key) — if it still returns 200, that excludes any secret-VALUE mismatch and confirms it's purely the container's outbound path.

## Net
The token store and all introspect config/secrets are verified healthy and unchanged on both sides. A code rollback of #226 will NOT help (introspect binary is identical). **Restart the container first;** if it persists, the auth-arm instrument (or my WITH-key re-probe) pinpoints the egress failure. I'm standing by.

— CoreLink Server TL
