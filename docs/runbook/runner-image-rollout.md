# Runbook — rolling a new runner container image to prod

**Scope.** You changed `deploy/runner/Dockerfile` (or `deploy/check-host/Dockerfile`)
and need the change to actually be running on the boxes customers' jobs land on.

**The one thing people get wrong:** *a green build is not a rollout, and a green
`wrangler deploy` is not a reboot.* The image build, the pin, the Worker deploy and
the container **roll** are four separate actions, and only the last one replaces a
box that is already running. Step 4 exists because it has been skipped before.

Nothing here is automatic. `build-cf-container-images.yml` and
`deploy-spawn-worker.yml` are both `workflow_dispatch`-only by deliberate design
(the image build is a heavy Docker build; a Cloudflare deploy is a production
action and stays a human decision).

**Prerequisites**

- `gh` authenticated against `HuGR-Labs/corelink-runners`.
- Repo secret `CLOUDFLARE_API_TOKEN` armed (both workflows guard on it and fail
  fast with an explicit message if it is absent).
- For steps 4–5 you need a token with **Containers** scope. Note the split
  observed on this account: the general Workers token and the Containers token
  are *different* tokens (`cfut_`-prefixed for Containers). Wrangler's deploy
  path wants both scopes in one token; the REST calls below only need Containers.
- Account id: `6a1fc1c626fc2628823e60b9db01f5cd` (`gmhelmold`) — the same account
  that hosts the R2 CAS, which is the whole point of ADR-0008.

Throughout:

```sh
ACC=6a1fc1c626fc2628823e60b9db01f5cd
CF_TOKEN=…            # Containers-scoped token; never echo it, never commit it
```

---

## 1. Dispatch the image build

The change must be **merged to `main` first** — the workflow tags the image with
`GITHUB_SHA`, so a build off a branch produces an image tagged with a commit that
`main` does not contain.

```sh
gh workflow run build-cf-container-images.yml --ref main
gh run watch "$(gh run list --workflow=build-cf-container-images.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```

What it does (`.github/workflows/build-cf-container-images.yml`): on the
`corelink` self-hosted runner, `docker build -t <image>:<sha> <context>` first
loads each image into the local containerd store, then
`(cd deploy/cloudflare && npx wrangler containers push <image>:<sha>)`
publishes it for **both** container images —

| DO class | build context | image name |
|---|---|---|
| `RunnerContainer` | `deploy/runner` | `corelink-spawn-worker-runnercontainer` |
| `CheckHostContainer` | `deploy/check-host` | `corelink-spawn-worker-checkhostcontainer` |

Both are rebuilt every dispatch; there is no way to build only one. The
check-host step also compiles `corelink-check-exec-server` for musl and stages it
into that build context first.

The locked Wrangler version consumes a local image for its inspect/auth/tag/push
sequence. The workflow therefore runs the Docker build explicitly, then uses
Wrangler only for the authenticated registry push; the disk guard prunes the
local image after publication. If CI fails and you fall back to a local build,
produce the immutable digest by hand — everything from step 2 on is identical.

---

## 2. Take the pushed ref and pin it

The job's **step summary** ("CoreLink CF Container images pushed") prints the ref
to pin, per image:

```sh
gh run view <run-id> --log | grep -E 'Runner image pushed|Check-host image pushed'
```

The workflow captures Wrangler's push output and runs
`scripts/ci/resolve-pushed-ref.sh`, which extracts an immutable
`…@sha256:<64-hex>` from the manifest or pushed-image lines. A missing or
malformed digest fails the job; there is no `imagetools` lookup and no mutable
tag fallback. **Pin the `@sha256:` form** — a tag pin violates the X4
supply-chain floor.

Edit `deploy/cloudflare/wrangler.jsonc` → `containers[]` → the entry whose
`class_name` is `RunnerContainer`, and replace its `"image"` value. This is the
*only* place the runtime image is pinned: per the ADR-0008 wrinkle, the image is
bound **at deploy time, not per spawn**. The spawn request's `image_digest` is
merely an *assertion* checked against the optional `PINNED_IMAGE_DIGEST` var
(`deploy/cloudflare/src/index.ts`) — if that var is set, update it in the same
change or spawns start 409-ing.

Commit the re-pin through a normal PR (branch → PR → merge; no direct push to
`main`), and record in the PR body which build run produced the digest.

---

## 3. Deploy the Worker

```sh
gh workflow run deploy-spawn-worker.yml --ref main
```

The `deploy` job is gated behind the spawn-Worker vitest + typecheck job, then
runs `npx wrangler deploy` from `deploy/cloudflare/`.

Two things this step does **not** do:

- It does not build any image (that was step 1; this deploy is Docker-free).
- **It does not reboot a container that is already running.** It updates the
  application's target configuration. Boxes already up — including idle-but-healthy
  warm instances that `sleepAfter` has not yet reaped — keep running the image
  they booted with. That is step 4.

Before deploying, check for **var drift**: `wrangler deploy` REPLACES all `vars`
(secrets survive). Compare the live worker's `plain_text` bindings against the
repo's `wrangler.jsonc`:

```sh
curl -s -H "Authorization: Bearer $CF_TOKEN" \
  "https://api.cloudflare.com/client/v4/accounts/$ACC/workers/scripts/corelink-spawn-worker/settings" | jq '.result.bindings'
```

---

## 4. Force the roll (do NOT skip)

Find the container application id:

```sh
curl -s -H "Authorization: Bearer $CF_TOKEN" \
  "https://api.cloudflare.com/client/v4/accounts/$ACC/containers/applications" \
  | jq -r '.result[] | "\(.id)  \(.name)  v\(.version)  \(.configuration.image)"'
```

The runner application is named `corelink-spawn-worker-runnercontainer` (the CF
convention is `<worker>-<classlower>`). Keep its `id` as `$APP`, and keep its
current `configuration` object — you pass it back verbatim as the rollout target,
changing only the `image`:

```sh
APP=…   # from the listing above

curl -s -X POST -H "Authorization: Bearer $CF_TOKEN" \
  "https://api.cloudflare.com/client/v4/accounts/$ACC/containers/applications/$APP/rollouts" \
  -d '{
        "description": "sccache 0.17.0 baked into the runner image",
        "strategy": "rolling",
        "kind": "full_auto",
        "step_percentage": 100,
        "target_configuration": { … current configuration, with the new image … }
      }' | jq '.result | {id, status}'
```

Notes that cost time when they are not known:

- The response comes back `status: "progressing"` with
  `instances: {active: 0, assigned: 0, …}`. **Do not read that as failure.**
  Observed timing on this account: the new instance appears ~14 s in, the rollout
  reaches `completed` ~38 s in. Poll; never conclude from a single probe.
- For a singleton the **instance id does not change** across a roll. The fields
  that prove a reboot are the application's `version` (n → n+1) and the instance
  `CREATED` timestamp.
- The blunter alternative used historically on `fabricd`
  (`npx wrangler containers delete <app-id> && npx wrangler deploy`) also
  replaces the box, but it deletes the application — do not reach for it on the
  runner class unless a rollout genuinely will not take.

**Why this step matters even though runner boxes are ephemeral.** Each job gets a
distinct DO → a distinct container, so the *next* job normally boots on the new
image anyway. But warm instances from earlier jobs linger until reaped, and a
lingering box is still the old image. Rolling makes "the fleet is on the new
image" true now instead of eventually.

---

## 5. Verify — from the Containers API, not from a green checkmark

A successful workflow run proves a workflow ran. It does not prove the running
container changed. Verify all three of these:

**5a. The application's configured image is the new digest, and the version bumped.**

```sh
curl -s -H "Authorization: Bearer $CF_TOKEN" \
  "https://api.cloudflare.com/client/v4/accounts/$ACC/containers/applications/$APP" \
  | jq '{version, image: .configuration.image, health: .health}'
```

**5b. No instance is still on the old image, and health is clean.**

```sh
npx wrangler containers info RunnerContainer   # from deploy/cloudflare/
```

Expect `failed: 0`. `healthy: N, active: 0, assigned: 0` is normal for an idle
runner class — that is warm capacity, not a fault. Watch for `startup_failure`
counts, which is how a broken image shows up here.

**5c. Behavioural proof — the actual thing you changed.**

The API telling you the digest matches proves the *pin*. It does not prove the
binary you added works. Run a real job on the fleet and read its log:

```sh
gh workflow run dogfood-smoke.yml --ref main
```

For the sccache change specifically, the proof is the disappearance of a notice:
the `runs-on: corelink` lane in the corelink-server `corelink-reapi` workflow
prints `sccache not on the box image — lane compiles cold, nothing breaks` when
the binary is absent. Its absence, plus non-zero sccache cache statistics on the
lane, is the proof. Until a job has run on a rolled box, "sccache is installed"
is a claim about a Dockerfile, not about production.

For the **Node.js + pnpm** bake the proof is a `run:` step — not a `uses:` step,
which would pass either way because the runner agent bundles its own private Node
under `externals/`. On a `runs-on: corelink` job:

```yaml
- run: node --version && npm --version && pnpm --version
```

Expect exactly `v22.23.2`, npm's bundled version, and `10.32.1`. The pnpm string
must match **character for character**: corelink-server's `setup-pnpm` composite
compares `pnpm --version` to its pinned default and silently falls back to
downloading pnpm on any mismatch, so a near-miss looks green while buying nothing.
The image build itself runs the same three `--version` calls (build fails closed if
a symlink is broken), but that proves the build, not the rolled fleet.

---

## Rollback

Re-pin the previous `@sha256:` digest in `deploy/cloudflare/wrangler.jsonc`
(git history has it), repeat steps 3 and 4. No rebuild is needed — the old image
is still in the registry. The last known-good runner digest at the time of
writing is recorded inline in `wrangler.jsonc`'s comment block, which is worth
keeping accurate for exactly this reason.

---

## Related

- `deploy/runner/README.md` — what is in the image, and the X4 digest-pin rules.
- `deploy/cloudflare/wrangler.jsonc` — the single place the runtime image is pinned.
- `docs/runbook/cloudflare-go-live.md` — the substrate's live state and the warm flip.
- `docs/deploy/post-redeploy-smoke-checklist.md` — what to smoke after any redeploy.
