# Owner runbook — external-customer activation (WP-4)

**Date:** 2026-07-11 · **For:** owner · **From:** runners TL (remediation wave, WP-4)
**Source of truth:** code/config file:line, verified against `main @ 0f79ea3` in this worktree — not
theory. Every step below states what I verified and, where I could not verify (GitHub App UI config
has no artifact in this repo), says so explicitly.

**Context:** the 6-lens audit (`docs/handoff/2026-07-11-AUDIT-findings-external-customer-path-gap.md`)
found a stranger's `runs-on: corelink` job cannot get a runner today — two breaks, one CODE
(App-installation-token minting, closed by WP-1, a sibling work-package in this same wave) and the
rest **config/secret/$/cross-repo**, which only the owner (or a cross-repo TL, via the sibling doc
`2026-07-11-RELAY-to-server-tl-external-webhook-routing.md`) can close. This doc is the complete,
verifiable checklist for the owner's half. Combined with WP-1's code + the RELAY doc's routing
decision, there is no unstated gap for external go-live.

---

## (a) Bind `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY` on the spawn-worker

**What:** two Worker secrets on `corelink-spawn-worker` (the CF Worker this repo deploys from
`deploy/cloudflare/`, name confirmed at `deploy/cloudflare/wrangler.jsonc:4`).

**Why:** `mintJit` (`deploy/cloudflare/src/index.ts:366-389`) today calls
`generate-jitconfig` with `Bearer ${env.GITHUB_MINT_TOKEN}` — a **static** first-party PAT scoped
only to HumanGuardrail repos (`index.ts:373`, comment at `index.ts:365`: *"using GITHUB_MINT_TOKEN
(repo Administration:write)"*). WP-1 (this wave, sibling work-package) adds a NEW file
`deploy/cloudflare/src/github_app.ts` that signs a GitHub App JWT from these two creds and exchanges
it for a per-installation access token — the credential shape a *customer's* repo actually needs.
Per the WP-1 contract (`docs/handoff/2026-07-11-REMEDIATION-wave-plan-audit-findings.md:49-51`), the
App path activates only when **both** `GITHUB_APP_ID` and `GITHUB_APP_PRIVATE_KEY` are present *and*
an `installationId` is available; absent, behavior is byte-identical to today (static-token fallback,
invariant I1). **⚠️ At the time of writing this doc, WP-1's code has not yet merged into this
worktree** (`deploy/cloudflare/src/github_app.ts` does not exist yet; verified `ls
deploy/cloudflare/src/` = `index.ts lib.ts metrics.ts` only, no `GITHUB_APP_ID`/`GITHUB_APP_PRIVATE_KEY`
reference anywhere in `index.ts`/`lib.ts`). **You can bind the secrets at any time — order-independent
of WP-1's merge** (wrangler secrets are independent of the Worker's TS `Env` type); they are simply
inert until WP-1's code lands and reads them.

**The key already exists — don't generate a new one.** The App (id `4222041`, slug `corelink-runners`,
owned by org HumanGuardrail — confirmed
`docs/handoff/2026-07-10-OWNER-RUNBOOK-github-app-settings-generate-key-set-setup-url.md:4`) already
has a private key generated and bound elsewhere: `docs/handoff/2026-07-10-OWNER-GO-LIVE-checklist-
corelink-standalone.md:34-36` records `wrangler secret list --name corelink-signup-worker` (owner-run,
read-only) confirming `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY` are already bound **on the
signup-worker** (a *different* Cloudflare Worker, in the sibling `corelink-server` repo — I cannot
read that repo directly under this session's fence, so I cite the committed handoff record, not a
live re-check). Reuse the **exact same `.pem` file contents** for the spawn-worker binding — do not
click "Generate a private key" again on the App (that adds a second, independently-valid key; harmless
but needless key-sprawl). Retrieve the `.pem` from wherever you stored it after
`2026-07-10-OWNER-RUNBOOK-github-app-settings-generate-key-set-setup-url.md` Part C (password manager
/ encrypted note — that doc explicitly warned not to paste it into plain chat).

**Exact commands** (run from this repo; `deploy/cloudflare/package.json:8` confirms `wrangler` is a
pinned local devDependency `^4.103.0`, so `npx wrangler` here resolves correctly — this is NOT the
`npx` footgun noted in a sibling repo's runbook where a *different* root-level `wrangler` was broken):

```bash
cd deploy/cloudflare

# App id (paste 4222041 — no trailing newline, printf avoids the corrupt-secret footgun
# documented in this repo's own history, docs/handoff/2026-06-26-...-5env-runbook.md:31)
printf '%s' '4222041' | npx wrangler secret put GITHUB_APP_ID

# Private key — paste the FULL .pem contents (BEGIN/END lines included), the SAME
# value already bound as GITHUB_APP_PRIVATE_KEY on the signup-worker.
npx wrangler secret put GITHUB_APP_PRIVATE_KEY
# (wrangler prompts for multi-line stdin interactively when not piped — paste + Ctrl-D,
#  or pipe from a chmod-600 local file: npx wrangler secret put GITHUB_APP_PRIVATE_KEY < /path/to/key.pem)
```

`wrangler.jsonc` has no `[env.*]` blocks (verified: only one match for `"env"` in the whole file, a
comment at line 39, not a config block) — this is a **single-deployment** Worker, so no `--env` flag
is needed (unlike the sibling `corelink-server` root worker's 5-live-env trap).

**Observable:**
```bash
cd deploy/cloudflare && npx wrangler secret list
```
must show two entries named `GITHUB_APP_ID` and `GITHUB_APP_PRIVATE_KEY` (wrangler lists secret
*names*, never values — that's expected, not a gap). This confirms the bind. **Full end-to-end
confirmation** (an installation-token mint actually working) is only observable once WP-1 merges:
a `workflow_job:queued` webhook for a repo the App is installed on but that is NOT
`HumanGuardrail/corelink-runners` produces a successful JIT mint (201, not the 404 `mintJit`
currently throws for a foreign repo per the audit's finding 1).

---

## (b) Grant the App (`4222041`) `Administration:write` on customer repos

**What:** on the App's **Permissions & events** settings tab, the **Repository permissions →
Administration** entry must be **"Read and write."**

**Why:** the code comment at `deploy/cloudflare/src/index.ts:114` and `:365` states the mint call
(`POST .../actions/runners/generate-jitconfig`) needs **"repo Administration:write."** That comment
describes the static `GITHUB_MINT_TOKEN`'s required PAT scope, but the same GitHub permission
requirement applies to a GitHub-App installation token — it inherits whatever permission set is
declared on the App's manifest. If the App's `Administration` permission is absent or read-only,
WP-1's installation-token path will call `generate-jitconfig` and get a `403`, which throws per
invariant I4 (`docs/handoff/2026-07-11-REMEDIATION-wave-plan-audit-findings.md:58-60`: an
installation-token *failure* must fail the mint loudly, never silently fall back) — so a missing
permission surfaces as a loud `spawn_failed`, not a silent 404.

**⚠️ Could not verify in code:** whether the App's Permissions & events tab currently has
`Administration: Read and write` set is GitHub UI/API-side state with no artifact anywhere in this
repo (grepped `docs/`, `deploy/` for any prior confirmation of the App's declared permission set —
none found). This must be checked directly on GitHub, not assumed from the static token working
today (the static `GITHUB_MINT_TOKEN` is an independent classic/fine-grained PAT, not derived from
the App's permission set at all).

**Exact click:**
1. `https://github.com/organizations/HumanGuardrail/settings/apps/corelink-runners`
   → **Permissions & events** tab.
2. Under **Repository permissions**, find **Administration**. If it reads "No access" or "Read-only,"
   change it to **"Read and write."**
3. If routing `workflow_job` through this App (see the companion RELAY doc), also check the
   **Subscribe to events** section for `Workflow job` — see that doc for the full contract.
4. Scroll to the bottom → **Save changes**.

**The re-approval implication (GitHub platform behavior, not this repo's code):** GitHub requires
**every existing installation** to re-approve a permission upgrade before the new scope takes effect
for that installation. This includes the **dogfood installation** (`144561227`, HumanGuardrail —
confirmed live and driving the dogfood fleet per
`docs/handoff/2026-07-10-OWNER-RUNBOOK-github-app-settings-generate-key-set-setup-url.md:4-5`). The
dogfood repo's *legacy* `GITHUB_MINT_TOKEN` static-PAT path is **unaffected** by this (it's an
independent credential, not derived from the App's permission grant) — but WP-1's own
App-JWT-based installation-token path for HumanGuardrail would sit pending-approval until you, as the
org owner, approve it. **Approve in the same sitting** as step 4 above to avoid a self-inflicted gap
in the App-token path for the org you also use for dogfood.

**Observable:** after saving, the App's **Advanced** tab (or the org's
`https://github.com/organizations/HumanGuardrail/settings/installations` page) shows a banner like
*"N installations pending upgrade"* for this App. After you approve the HumanGuardrail installation,
that count drops to reflect any OTHER pending installs (external customers who haven't re-approved
yet — each customer must separately re-approve on their own install, which is the "existing installs
get a re-approval prompt" cost of this permission change). A live API check
(`GET https://api.github.com/app/installations/144561227` with an App JWT) would show
`"permissions": {"administration": "write", ...}` post-approval — I did not run this (no live GitHub
credential in this sandboxed session); the owner or server-TL should confirm it.

---

## (c) Raise `RunnerContainer max_instances` — a cost decision, not a rubber stamp

**What's there today (verified):** `deploy/cloudflare/wrangler.jsonc:123` sets
`"max_instances": 6` for the `RunnerContainer` class, whose `instance_type` is `"standard-4"`
(`wrangler.jsonc:115`) — per the comment at `wrangler.jsonc:113-114`, that's **"4 vCPU / 12 GiB / 20
GB disk."** This is a **single, account-wide ceiling shared across every tenant** this one
`corelink-spawn-worker` serves — there is no per-tenant instance cap in `wrangler.jsonc`; the
per-tenant limit lives entirely in each tenant's `runners_entitlement.max_concurrency` row (enforced
downstream in the fabric/auth path, not by this container ceiling).

**Why 6 is already short even for ONE tenant:** the dogfood tenant's own entitlement is
`max_concurrency = 20` — confirmed in this repo's committed record of the server-TL's row check,
`docs/handoff/2026-07-05-REPLY-from-server-tl-runners_entitlement-row-CONFIRMED-20-100.md:9`
(`max_concurrency = 20`, reading `runners_entitlement` via
`auth_introspect.rs:318` on the server side — cross-repo, cited not re-verified by me). So **the
first tenant alone** is contractually entitled to more concurrent runners (20) than the container
ceiling currently allows (6) — a burst to the dogfood tenant's own cap today would already 429/stall
some jobs, independent of any external customer.

**The tradeoff (state it, don't just raise it):**
- `max_instances` is a **ceiling**, not a standing cost — Cloudflare Containers bill for
  instance-running time, so the number itself doesn't cost anything until instances are actually
  spawned and running. The real exposure is `vcpu(4) × max_instances × concurrent-runtime` — raising
  6→20 raises the WORST-CASE simultaneous compute footprint from 24 vCPU to 80 vCPU if every slot is
  saturated at once.
- Because the ceiling is **shared across all tenants** on this Worker, activating a second and third
  external customer means their entitlements compete for the SAME pool as dogfood's 20. Setting
  `max_instances` to the sum of every onboarded tenant's *theoretical* entitlement ceiling
  over-provisions (most tenants won't burst to their cap simultaneously); setting it too low
  under-serves a legitimate burst and, per the operational history below, can outright stall spawns —
  this is a genuine capacity-planning call, weighing observed peak concurrent demand against standing
  compute-cost exposure.
- **There is prior operational evidence this isn't just theoretical:** the comment at
  `wrangler.jsonc:116-122` documents that at `max_instances: 2`, "2 idle-but-healthy warm instances
  lingered (sleepAfter not yet reaped)... blocked" new spawns — i.e. under-provisioning didn't just
  cap throughput, it caused an outright spawn STALL (root-caused 2026-07-06). The 6→raise decision
  should account for this idle-instance-lag headroom, not just peak entitlement sum.

**Exact edit + deploy:**
```bash
# Edit deploy/cloudflare/wrangler.jsonc:123 — change:
#   "max_instances": 6
# to your chosen ceiling, e.g.:
#   "max_instances": 20
cd deploy/cloudflare
npm run deploy   # === npx wrangler deploy (deploy/cloudflare/package.json:8)
```

**Observable:** `npx wrangler containers info RunnerContainer` (or the Cloudflare dashboard →
Workers & Pages → your account → Containers) shows the updated `max_instances`. Behaviorally: use the
existing observability endpoint — `GET /internal/v1/metrics` on the spawn-worker
(`deploy/cloudflare/src/index.ts:728-734`, gated by `METRICS_OBSERVABILITY_KEY` /
`x-corelink-internal-auth`) — to watch for saturation signals (spawn failures / stalls) before and
after the change, rather than raising blind.

---

## (d) The size-taxonomy input (blocks multi-size only, not launch)

**What's already built (verified, inert):** `crates/corelink-fabric-server/src/size.rs` (245 lines,
landed `ea2a7c8`) — a pure, **unwired** `SizeSpec`/`SizeRegistry`/`resolve_from_labels` module that
derives a box size from the existing `corelink-<size>` managed label (`size.rs:26-32`), with a
**single-rung registry today resolving every acquire to the default size** — i.e. byte-identical to
the current single-size fabric until activated (module doc comment, `size.rs:1-24`). This is a single
default size; it launches fine as-is.

**Why it's not activated:** per the joint design ratified with the server-TL
(`docs/handoff/2026-07-10-server-tl-ANSWER-multi-size-ladder-design-steer.md`), full activation needs
three server-side seams landed in lockstep (spawn `instance_type` on the wire → regenerated
`conformance/cloudflare-spawn.json`; a `(kind, instance_type)` billing price-map dimension on
`UsageEventData`; per-tenant `allowed_sizes` on introspect, default = entry size only) — **none of
which move until the owner supplies the one true product input**, stated explicitly at
`docs/handoff/2026-07-10-server-tl-ANSWER-multi-size-ladder-design-steer.md:21-25`:

> *"The size taxonomy is a product/pricing decision, not an engineering one: which rungs (vcpu + CF
> instance_type per rung) and the $/slot-second per rung."*

**Exact input needed — fill this table:**

| Rung name (label suffix, e.g. `standard-4`) | vCPU | CF `instance_type` | $ / slot-second |
|---|---|---|---|
| *(entry — today's default, likely `standard-4`)* | 4 | `standard-4` | ? |
| *(rung 2, e.g. `standard-2` or `standard-8`)* | ? | ? | ? |
| *(rung 3, optional)* | ? | ? | ? |

The server-TL's steer recommends **starting with 2–3 rungs** to keep the Cloudflare
Durable-Object-class count small (`...design-steer.md:11` — CF binds one DO class per `instance_type`,
so each rung is a new class; more rungs later is cheap, fewer now is operationally simpler).

**Observable this is "done":** there isn't a code observable yet — this step's only output is the
filled table above, handed to the runners-TL + server-TL. Once supplied: I populate `SizeRegistry`
rungs in `size.rs`, the server-TL lands the price map + `allowed_sizes` entitlement column, and both
sides co-author `conformance/cloudflare-spawn.json` in lockstep before flipping the activation flag —
none of that is built now (would be built wrong before the input shapes it, per
`docs/handoff/2026-07-11-REMEDIATION-wave-plan-audit-findings.md:157-158`).

---

## Residual NOT covered by (a)-(d) — cross-repo, folded into the sibling RELAY doc

Two more owner/cross-repo items surfaced by the audit are **architecture questions**, not standalone
owner actions — they're documented in
`docs/handoff/2026-07-11-RELAY-to-server-tl-external-webhook-routing.md`:
webhook routing (audit finding 2) and the TS↔Rust label split-brain (audit finding D). See that doc
for the full contract; this runbook's (a)-(d) do not by themselves deliver a live external path
without that routing decision also landing.

---

## Summary — the complete "external customer live" gap

| # | Gap | Closed by |
|---|---|---|
| 1 | No App-installation-token minting (code) | WP-1 (sibling work-package, this wave) |
| 2 | Spawn-worker lacks the App creds to use WP-1's code | **(a)** above |
| 3 | The App may lack `Administration:write` on customer repos | **(b)** above |
| 4 | Container ceiling (6) undersized even for one tenant's entitlement (20) | **(c)** above |
| 5 | No size taxonomy to activate the (already-built, inert) multi-size resolver | **(d)** above |
| 6 | No webhook delivery path from an external repo to the spawn-worker | RELAY doc |
| 7 | TS↔Rust label-matching split-brain (dormant, must converge before multi-size) | RELAY doc |
