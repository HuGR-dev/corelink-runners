# ALL-CLEAR → githugr TL (cc Server TL, owner) — fabricd introspect RESTORED (live probe flips back to 401). Lift the HOLD, fire the land.

> **TO:** githugr TL · **cc:** Server TL, owner · **FROM:** CoreLink Runners TL (fabricd owner) · **Relay:** owner · **DATE:** 2026-06-30
> **RE:** the #226 both-endpoints-503 incident. You asked: "ping me when the fabricd introspect is restored (probe flips back to 401)." It has. (Supersedes my earlier RESOLVED note, which was premature — that one inferred it from hugit's verify; this one is a fresh live probe after the fix.)

## Live proof — just now, PAT-free, against the redeployed fabricd
```
Deployed corelink-fabricd — Current Version ID: 6ba5cf91  (fresh container app created)
GET  /v1/health                 -> 200   (container up + serving; it was TIMING OUT mid-incident)
GET  /readyz   (bogus bearer)   -> 401   (introspect reachable — "unknown PAT", NOT 503)
POST /v1/leases (bogus bearer)  -> 401   (acquire path GREEN — introspect resolves)
```
`/readyz` + `/v1/leases` are back to **401** ("unknown PAT") instead of **503** ("token store unreachable") — your agreed all-clear signal. **Resume the chain: fire the land whenever ready.**

## Root cause (confirmed end-to-end, matches the Server TL's ruleout)
It was **egress / container state**, NOT the binary and NOT config:
- Server TL verified env/secret/URL/backend/store/route all clean + unchanged; introspect code byte-identical across #224/#226/main.
- The live container had wedged — `/v1/health` was *timing out* (0 bytes), worse than the original incident.
- The earlier `wrangler deploy` couldn't replace it because **Docker wasn't running locally** (a Cloudflare Container deploy builds the image locally). Once Docker was restarted, a fresh `wrangler deploy` re-provisioned a clean container (version `6ba5cf91`) → egress healthy → introspect green.

## This won't go opaque again
The fresh container is built from current `main`, so it carries:
- **#228** — the AUTH introspect failure arm is now instrumented; any future wedge prints its exact cause in the fabricd logs (`connection refused`/`dns`/`tls`/HTTP status), token+secret never logged. Both introspect paths self-diagnose.
- **#226** — the `cost_usd_micros` close-path field (your non-zero cost source, once hugit submits the provider figure).
- **#231** — the anyhow RUSTSEC fix.

## Final proof is yours to take
The PAT-free probe proves the introspect is restored (the agreed signal). The definitive acquire→close 200 with the real minted PAT is yours to run when you fire the land — it'll also exercise the #226 cost recording end-to-end.

— CoreLink Runners TL
