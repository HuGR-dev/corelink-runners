# T8-W4b version-bound execution card

Status: `READY-PENDING-T3`

Target source: `b4bbc26ea0362c59384881263b65c17ca1137eba`.

This card will be prepared for one read-only probe after T3 provisions an ephemeral
runner. It does not authorize deployment or change any Worker/container state.

The deploy-bound runner image will be:

```text
registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-spawn-worker-runnercontainer@sha256:2f3dd8a166e890888d026669757c996750b42230a04710fa08f13db56b0cf029
```

The check-host image remains separately pinned to:

```text
registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-spawn-worker-checkhostcontainer@sha256:4e09fc21ddc437684a921fdeedbf5c98264a7a7db77dbc76dae3a4ea75017851
```

The runner bootstrap will be version-bound by `deploy/runner/Dockerfile`:

- GitHub Actions runner `2.335.1`;
- Ubuntu base `ubuntu:24.04@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54`;
- image labels identify runner `2.335.1`, clw `0.1.4`, Node `22.23.2` and pnpm `10.32.1`.

The JIT path is checked in source by `deploy/runner/entrypoint.sh` and
`deploy/runner/test/jitconfig-secret-surface.sh`: the initial JIT environment
is sealed through a `0600` file and clean `exec`, then the bridge pathname and
JIT variables are removed before `run.sh` starts. The workflow-side witness
must print `T8-W4b process witness: PASS` and must never print any of
`CORELINK_RUNNER_JITCONFIG`, `ACTIONS_RUNNER_INPUT_JITCONFIG`,
`CORELINK_RUNNER_JITCONFIG_FILE` or `JITCONFIG_SECRET_FILE`.

When the runner becomes online, set `CONTAINER_APP_ID` to the exact Cloudflare
RunnerContainer application id and then execute:

```text
EXPECTED_RUNNER_IMAGE='registry.cloudflare.com/6a1fc1c626fc2628823e60b9db01f5cd/corelink-spawn-worker-runnercontainer@sha256:2f3dd8a166e890888d026669757c996750b42230a04710fa08f13db56b0cf029' \
CONTAINER_APP_ID='<runner-application-id>' \
RUN_ID='<probe-workflow-run-id>' \
scripts/ops/t8-w4b-version-bound-probe.sh
```

The script performs a paginated provider-instance image equality check and
then inspects the probe log for the process witness and forbidden JIT
material. Any image drift, absent instance, leaked bridge name, or missing
witness fails closed. T3-W18 remains the only production dependency; this card will be
ready to execute as soon as its runner will be available.
