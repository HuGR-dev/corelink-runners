# Repository webhook secret rotation

This is the bounded, forward-only rotation for the first-party repository
hook on `corelink-spawn-worker`. It rotates only the repository hook secret.
`GITHUB_WEBHOOK_SECRET`, which authenticates the GitHub App, is left untouched.

The Worker accepts `GITHUB_WEBHOOK_REPO_SECRET_NEXT` only for `workflow_job`
deliveries and only while `GITHUB_WEBHOOK_REPO_SECRET` remains present. App
installation events still require `GITHUB_WEBHOOK_SECRET`. A NEXT-only state is
fail-closed.

The executable is inert by default and refuses to run without the exact source
commit, owner-only secret file, fleet-idle proof, and explicit destructive
acknowledgement. It performs one bounded sequence:

1. Prove the fleet is `busy=0` and `unverifiable=0`, establish
   `FABRIC_ADMISSION_PAUSED=1`, and deploy the checked-out source while frozen.
2. Put the new value in `GITHUB_WEBHOOK_REPO_SECRET_NEXT`, deploy frozen, and
   send one signed `workflow_job.queued` probe to the Worker.
3. Patch exactly hook `675536825` in `HuGR-Labs/corelink-runners` through `gh`
   without putting the secret in command arguments or retained output. Request
   a ping and inspect deliveries when the GitHub API permits it.
4. Put the same value in `GITHUB_WEBHOOK_REPO_SECRET`, delete NEXT, deploy
   frozen, and prove the final secret-name set and fleet state.

No secret value is written to evidence or logs. The script never runs the live
path as part of tests; use its `--mode plan` output and the local shell checks
before an owner starts a maintenance window.

```sh
bash scripts/ops/github-repo-webhook-secret-rotation.sh \
  --mode live \
  --ack-destructive ACK-CORELINK-REPO-WEBHOOK-ROTATION-LIVE-20260909 \
  --repo-root "$PWD" \
  --expected-commit "<full checked-out commit>" \
  --new-secret-file "$HOME/.corelink/rotation-b2-20260909/repo-webhook-secret-next" \
  --fleet-key-file "$HOME/.corelink/rotation-b2-20260909/fleet-busy-read-key" \
  --evidence-file "$HOME/.corelink/rotation-b2-20260909/repo-webhook-rotation.json"
```

If any step fails, the exit path attempts one frozen deploy and exits RED. Do
not delete the primary secret or rerun from a different checkout until the
evidence and provider state have been reconciled.
