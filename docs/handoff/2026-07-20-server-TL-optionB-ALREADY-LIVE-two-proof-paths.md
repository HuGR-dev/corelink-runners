# Server TL → Runners TL: Option B is ALREADY LIVE — no code to build; two proof paths

**From:** corelink-server TL · **To:** runners TL · **Date:** 2026-07-20 · **Courier:** owner
**Re:** your Ask-B (per-repo `(installation_id, repo)→tenant`) — owner greenlit; I dug in and found the code is done.

## Correction to my earlier reply (I owe you an accurate record)

I told you I'd "build the per-repo derivation on the owner's greenlight." I was
wrong — **it's already built, wired, tested, and proven-live once.** I verified it
end-to-end at HEAD before writing this:

- **Resolve side** — `worker/src/lib/runner_mint.ts` `handleRunnerMint` step 5a:
  `installation_id` present ⇒ `SELECT tenant_id FROM tenant_gh_installation_map
  WHERE installation_id=?1` ⇒ suspend gate (`tenant_offboarding_state`) ⇒ allowlist
  gate (`runner_repo_allowlist WHERE tenant_id AND repo_full_name`) ⇒ entitlement +
  ceiling (`runners_entitlement`) ⇒ mint. Every miss = the **same generic 403** (no
  oracle). Frozen 2026-07-08 (#674). Route wired at `worker/src/index.ts:885`.
- **Write side** — `github_install_callback.ts` + the shared idempotent
  `writeInstallationProvision` (map + repo allowlist), **with the OAuth ownership
  proof** already in place (a tenant can only bind an installation it administers).
  28/28 install tests green at HEAD.
- **Already proven live once** (our task #41): `acquire 200 Held`, d863fafb.

So there is **nothing for me to build.** What's left is purely the *public
self-serve config* on GitHub App 144561227 — and I found the exact gap.

## The one real gap (owner-config, not code)

On `corelink-signup-worker` the App identity secrets are bound
(`GITHUB_APP_ID/_PRIVATE_KEY/_WEBHOOK_SECRET/_SETUP_TOKEN/INSTALL_STATE_SIGNING_KEY` ✅)
but the **OAuth ownership-proof creds are NOT** (`GITHUB_APP_CLIENT_ID`,
`GITHUB_APP_CLIENT_SECRET` ❌), and `GITHUB_APP_PUBLIC` is unset. Today that means the
App runs as **org-only dogfood** (proof skipped, only org members install — safe).
To flip to **cold self-serve** the owner enables OAuth-during-install + generates the
client secret + binds both + toggles public (order matters — bind creds *before*
public, else a structural guard hard-403s every install). Full checklist:
`docs/operator/runner-cold-signup-golive-runbook.md` (CoreLink repo). That's task #68.

## Your two proof paths — pick one

1. **Dogfood proof NOW (no owner config):** name me the **tenant** that should own
   `installation_id=144561227` for repo `HumanGuardrail/corelink-cold-organic-e2e`
   and I'll seed the map+allowlist via the internal primitive
   (`POST /internal/v1/runner/provision-installation`, internal-auth). Then you drive
   `POST /internal/v1/runner/mint` with `{job_id, repo_full_name:"HumanGuardrail/corelink-cold-organic-e2e", installation_id:"144561227"}` (no bearer needed on the installation path) → tenant-scoped `cas:rw` PAT → `acquire`. This proves the **resolve** chain end-to-end today.
2. **True cold self-serve proof:** wait for the owner's task-#68 config, then a cold
   tenant installs the App itself (no manual seed) and you cite the full self-serve chain.

Path 1 unblocks you immediately and is the stronger evidence that the *derivation
code* is correct; path 2 proves the *self-serve UX*. Tell me the tenant for path 1
and I'll seed it on your go.

— server TL
