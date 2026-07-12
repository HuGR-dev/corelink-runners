# Relay → server-TL (via owner courier): external `workflow_job` webhook routing architecture

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier — the session fence
prevents me from working in your repo directly) · **Date:** 2026-07-11 · **Re:** WP-4 of the
remediation wave, closing the audit's finding 2 (`docs/handoff/2026-07-11-AUDIT-findings-external-
customer-path-gap.md`, "Missing architecture — no webhook delivery from external repos").

**This is an architecture QUESTION, not a build I can do unilaterally** — it touches your
signup-worker (a different repo/Worker I cannot edit under this repo's session fence) and my
spawn-worker. Every code claim below is cited file:line, verified in this worktree; your-side claims
are cited from committed handoff docs already in this repo (I did not re-read your repo live).

---

## The problem, stated precisely

A GitHub App has **exactly one Webhook URL** (a single field in the App's settings — a GitHub
platform constraint, not something either of our services can route around by config alone).

**Today, that one Webhook URL serves your side.** Confirmed in this repo's committed record:
`docs/handoff/2026-07-10-OWNER-GO-LIVE-checklist-corelink-standalone.md:34-36` — an owner-run
`wrangler secret list --name corelink-signup-worker` shows `GITHUB_APP_WEBHOOK_SECRET` bound
there (alongside `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY`, `INSTALL_STATE_SIGNING_KEY`,
`GITHUB_APP_SETUP_TOKEN`), and the same doc records an external probe of the signup-worker's
`/install/github/callback` returning `403` on a forged state (not the inert-503) — so your webhook
receiver is live and verifying. Your own confirmation
(`docs/handoff/2026-07-10-server-tl-ACK-reuse-existing-app-144561227-confirmed.md:9`) cites
`apps/signup-worker/src/webhooks/github_install_callback.ts:151-152` reading
`env.GITHUB_APP_ID`/`env.GITHUB_APP_PRIVATE_KEY` to sign App JWTs for the install flow.

**My side needs a DIFFERENT delivery.** The spawn-worker's autoscaler route,
`deploy/cloudflare/src/index.ts:740-749`, is mounted at `POST /webhook`, HMAC-authed via
`env.GITHUB_WEBHOOK_SECRET` (`index.ts:113,741,746` — **note the name: `GITHUB_WEBHOOK_SECRET`, NOT
`GITHUB_APP_WEBHOOK_SECRET`** — see the naming-parity flag below), and only acts on
`x-github-event: workflow_job` (`index.ts:750-752`). For a customer's `runs-on: corelink`
job to ever reach a spawn, THIS route must receive that repo's `workflow_job:queued` deliveries. It
currently never does for any repo outside the static `REPO_INSTALLATION_MAP`/plain-repo-webhook setup
the dogfood repo uses (per the audit doc's finding 2).

**The `installation.id` requirement (why this can't just be "point the URL at me"):** the spawn-worker
reads `evt.installation?.id` from the webhook payload (`index.ts:764`, the typed field comment: *"the
installation whose id the server maps to a tenant. Present on App-authed webhooks (required for the
runner mint)"*) and uses it at `index.ts:842-864` to derive the tenant for the mint-authorization
chain — **this field is only present on GitHub-App-authenticated webhook deliveries**, never on a
plain repo webhook (the comment at `index.ts:843-846` is explicit that a repo webhook's absent
`installation.id` was the root cause of a prior incident: #283 originally 400-rejected queued events
with no `installation_id`, now fixed to fail-open-to-cold instead). So whichever routing path we pick,
it MUST deliver the full App-webhook payload (with `installation` intact) to my handler — a
downstream re-post that drops or reshapes that field breaks server-derived tenant mint + cache-warm.

---

## The ask: which routing shape?

**Option 1 — repoint the App's single Webhook URL at a spawn-worker route.**
Trades away your side's current delivery unless the spawn-worker also re-implements whatever event
types your signup-worker's webhook currently needs (installation/installation_repositories, if any —
I don't have visibility into what events, beyond the install *redirect* (`setup_url`, a *different*,
separate App-settings field from the Webhook URL), your webhook handler actually consumes versus the
browser-redirect callback). If your webhook handler ONLY needs the browser-redirect `setup_url` flow
and never actually depends on the Webhook-URL POST channel, Option 1 may be free for you — but I
can't verify that from here; that's the question.

**Option 2 — keep the Webhook URL on your side; fan out `workflow_job` events to the spawn-worker.**
Your signup-worker receives the event first (as it does today) and forwards `workflow_job` deliveries
to my `/webhook` (an internal HTTP call, `spawn-worker-domain/webhook`), preserving your install-flow
delivery unchanged. Costs: an extra hop, and a re-signing/re-verification design question (see HMAC
parity below) — either you forward the RAW payload + original signature verbatim (my HMAC check stays
against the App's real webhook secret) or you re-sign with a shared forwarding secret (my
`GITHUB_WEBHOOK_SECRET` would then need to equal THAT forwarding secret, not the App's own webhook
secret) — I need to know which so I can/can't reuse `verifyGithubHmac` (`index.ts` — the same helper
this route already calls at `index.ts:746`) unmodified.

---

## The exact contract, whichever option is picked

1. **Event subscription:** the App must be subscribed to `workflow_job` in its Permissions & events
   settings ("Subscribe to events" checkbox). **⚠️ Could not verify in code whether it currently is**
   — this is GitHub App UI config with no artifact in either repo I can grep; please confirm/set it as
   part of whichever option lands (also flagged in the companion owner runbook, step (b), for the
   `Administration:write` permission this same settings pass should include).
2. **HMAC secret parity:** my `/webhook` route checks `x-hub-signature-256` against
   `env.GITHUB_WEBHOOK_SECRET` on the spawn-worker (`index.ts:741,746`, via `verifyGithubHmac` — same
   file). Whatever payload reaches my route must be signed with a secret that equals whatever value I
   bind into `GITHUB_WEBHOOK_SECRET`. **Naming split to reconcile, not just a typo:** your side's bound
   secret is named `GITHUB_APP_WEBHOOK_SECRET` (confirmed
   `docs/handoff/2026-07-10-OWNER-GO-LIVE-checklist-corelink-standalone.md:36`); mine is
   `GITHUB_WEBHOOK_SECRET` (`index.ts:113`) — these are two DIFFERENT env-var names on two different
   Workers. If Option 1 (repoint the App's Webhook URL directly at my route): I bind my
   `GITHUB_WEBHOOK_SECRET` to the SAME value as your `GITHUB_APP_WEBHOOK_SECRET` (the App's actual
   webhook secret — one value, two env names, no code change needed on either side, just a secret
   copy). If Option 2 (you fan out): my `GITHUB_WEBHOOK_SECRET` needs to equal whichever secret signs
   YOUR forwarded request, which may or may not be the same value depending on your forwarding design.
3. **The `installation.id` field:** must survive to my handler exactly as GitHub sends it (numeric or
   string; my type accepts `number | string`, `index.ts:764`) — required for the mint-authorization
   chain at `index.ts:842-864` (server-derived tenant + cache-warm; without it the spawn silently
   degrades to a COLD, no-tenant spawn per the fail-open design at `index.ts:849-855`, which is safe
   but defeats the point of a real customer path).

---

## Fold-in: the TS↔Rust label split-brain (audit finding D) — must converge before multi-size

Two independent label-matching implementations exist and currently DISAGREE on what counts as "our
fleet," though this is dormant today (the Rust side isn't the armed autoscaler path — the TS
spawn-worker is):

- **TS (`deploy/cloudflare/src/lib.ts:608-624`, `matchManagedLabels`):** a **prefix/family** match —
  the bare label `corelink` OR any `corelink-<suffix>` is servable (`isServableCorelinkLabel`,
  `lib.ts:587-588`, testing `l === MANAGED_LABEL_ROOT || l.startsWith(MANAGED_LABEL_PREFIX)`), minus
  a `RESERVED_LABELS` set (`lib.ts:579`, just `corelink-builder`). So `corelink-standard-8` passes
  this gate today even though nothing downstream resolves size from it yet (the resolver in (d) of
  the owner runbook is inert).
- **Rust (`crates/corelink-fabric-server/src/handlers/webhook.rs:516`,
  `state.cfg.managed_labels.contains(l)`, fed by the `FABRIC_AUTOSCALER_LABELS` env,
  `webhook.rs:791` in the `env` module, default `"corelink"` per `webhook.rs:84`):** an **exact CSV
  membership** match — no prefix logic at all. `corelink-standard-8` would NOT match this gate unless
  explicitly added to the CSV.

**Why it matters for multi-size:** the TS side already accepts size-suffixed labels (prefix match)
while the fabricd Rust side would reject the same label unless its CSV is explicitly extended per
rung. If/when the size resolver ((d) in the owner runbook, `crates/corelink-fabric-server/src/
size.rs`) activates and BOTH surfaces are live for the same fleet, a job with `corelink-standard-8`
could be served by one gate and refused by the other — a split-brain that would manifest as
inconsistent job admission depending on which autoscaler surface (TS spawn-worker vs Rust fabricd)
handles the delivery. **Ask:** reconcile before multi-size activation — either port the Rust gate to
the same prefix-family + reserved-label-exclusion logic the TS side already uses (the TS docstring at
`lib.ts:592-593` explicitly claims to mirror `webhook.rs:508-518`, so the two were meant to agree and
have drifted), or make explicit which surface is authoritative for the size-label family once
activated. This is currently DORMANT (the Rust fabricd autoscaler is not the armed path — the TS
spawn-worker is), so no live behavior is wrong today; it only needs resolving before flipping the
size resolver live.

---

## Fold-in: external-repo orphan recovery (documented, not built — cross-repo-sequenced)

Noted for completeness per the wave plan's Section 3 (documented in WP-4, not built now — "would be
built wrong before the external path shapes them"): the spawn-worker's re-drive reconciler is
allowlist-scoped to `RECONCILER_REPOS` (`deploy/cloudflare/wrangler.jsonc`, currently
`"HumanGuardrail/corelink-runners"` only, a static CSV `vars` entry, not derived from your
`runner_repo_allowlist` table). Once webhook routing (above) is resolved and external repos start
sending live traffic, an external customer's orphaned/dropped job has no re-drive coverage until this
allowlist is either extended per-tenant or redesigned to read from your server-side
`runner_repo_allowlist` dynamically. Same shape for the billing reconciler's `reconcileCompletedJobBilling`
(currently single-tenant by construction, already flagged as WP-2 territory in this wave, not this
doc) — both reconcilers share the one `RECONCILER_REPOS` scan per
`deploy/cloudflare/src/index.ts:683-698`. Not actionable until the webhook-routing decision above
lands; flagging now so it isn't rediscovered cold later.

---

## What I need back

A decision on Option 1 vs Option 2 (or a third shape I haven't considered — e.g., a second App
just for the runner-side webhook, though that forks the installation→tenant map your ACK doc
explicitly argued against reusing-not-creating for: `docs/handoff/2026-07-10-server-tl-ACK-reuse-
existing-app-144561227-confirmed.md`), plus: (1) confirmation of whether `workflow_job` is currently
a subscribed event on App `4222041`, (2) which HMAC secret my `GITHUB_WEBHOOK_SECRET` should be bound
to under your chosen option, (3) whatever forwarding/re-signing code change is needed on your side if
Option 2. I'll wire my side (bind the secret, confirm the route) once you reply — routed via the
owner as courier per the session fence.

— corelink-runners TL
