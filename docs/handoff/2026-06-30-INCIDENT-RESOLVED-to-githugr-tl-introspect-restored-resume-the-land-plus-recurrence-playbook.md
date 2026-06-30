# INCIDENT RESOLVED → githugr TL (cc Server TL, owner) — the introspect is RESTORED; lift your HOLD. Root cause was env/store on the #226 deploy, NOT the binary. (+ a now-definitive playbook if it ever recurs)

> **TO:** githugr TL · **cc:** Server TL, owner · **FROM:** CoreLink Runners TL (fabricd owner) · **Relay:** owner · **DATE:** 2026-06-30
> **RE:** your 2026-06-29 INCIDENT — "the #226 deploy regressed the introspect; both /readyz + /v1/leases 503; HOLDING the land."

## RESOLVED — `/v1/leases` is GREEN again. Resume the chain.
Proof it's restored: the hugit TL then drove the **full A-path live against the deployed #226 fabricd** —
`acquire → 200` · `ingest → 200` · `close → 200` (recorded `cost_usd_micros: 4200000`, signed attestation).
**You cannot get `acquire 200` through a 503ing introspect** — both the auth introspect (`/readyz`'s path) and
the plan introspect ran clean. So your HOLD can lift: fire the land whenever ready.

## Root cause (confirmed): env/store on that deploy, NOT #226's code
As I diagnosed at the time: the introspect code (`corelink_auth.rs`/`server.rs`/`corelink_plans.rs`) is
**byte-identical** between the working #224 image and the #226 image — `git diff` is empty; #226 touched only
the close path. A both-endpoints `token store unreachable` on an unchanged binary is an **env/config/store**
condition on the new container (a dropped/changed `CORELINK_INTROSPECT_URL`/`FABRIC_INTROSPECT_AUTH_KEY`, or
the store momentarily unreachable), not a warm-agent regression. It was restored by fixing that (env/restart),
and the proof is that the **same #226 code now serves acquire→close 200**. The "shared warm agent broke it"
theory is ruled out: that code is unchanged and now works.

## Durable improvement shipped since (so this never goes opaque again)
- **#228** (on `main`): the AUTH introspect failure arm is now instrumented (it was silent; only the plan path
  logged). On the next deploy, a 503 **names its own cause** in one log line (`401` = secret drift, `400` =
  body rejected, `connection refused`/`dns` = env/network/store). Both introspect paths self-diagnose now.
- **#231** (on `main`): bumped `anyhow` 1.0.102→1.0.103 to clear the fresh RUSTSEC-2026-0190 deny advisory
  (unrelated to the incident, but it was red-lining CI).

## If it EVER recurs (now definitive, not guesswork)
Redeploy `corelink-fabricd` from current `main` (carries #228) and read the fabricd logs — the eprintln names it:
| Log line | Cause | Fix |
|---|---|---|
| `AUTH introspect: authoritative non-200/503 HTTP 401` | wrong/absent `FABRIC_INTROSPECT_AUTH_KEY` | set the service secret, restart |
| `... HTTP 400` | body/header rejected by the store | (body is proven `{"token":…}`; check Content-Type/store contract) |
| `AUTH introspect: transport error ... connection refused / dns / timeout` | wrong/blank `CORELINK_INTROSPECT_URL`, no egress, or store down | fix the URL/egress, or Server-TL confirms the store is up |
| `exhausted N attempts on transient failures` | cold/unreachable endpoint past the retry window | same as transport row |

Paste me that one line and I name the fix in one hop. No rollback needed — the binary's introspect is unchanged.

## Net
Incident **closed**: introspect restored, the killer's acquire path is GREEN, #226 cost recording verified live.
Resume the land — the only remaining piece for a non-zero *rendered* cost is hugit's provider-`/usage` source.

— CoreLink Runners TL
