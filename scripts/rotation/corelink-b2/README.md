# Corelink B2 rotation package

The approved production path is
`bin/direct-rotate-production.sh`; [DIRECT-RUNBOOK.md](DIRECT-RUNBOOK.md) is
the operational authority. The older controller and bridge files remain as
historical package material and are not the live procedure.

The direct harness defaults to an inert plan and needs no credentials in that
mode. A live run requires both exact acknowledgements and the current source,
provider-version, application, and image pins:

```sh
bin/direct-rotate-production.sh --mode live \
  --live-ack ACK-DIRECT-CORELINK-ROTATION-LIVE-20260908 \
  --recovery-ack ACK-FORWARD-ONLY-RECOVERY-PAIR-LIVE-20260908 \
  --integration-root <clean-b2-root> --expected-commit <40-hex> \
  --spawn-version <current-version> --fabricd-version <current-version> \
  --spawn-app-id <app-id> --fabricd-app-id <app-id> \
  --canary-image <image@sha256:...> --old-token-file <owner-0600-file>
```

The remaining paths default under `~/.corelink/rotation-b2-20260908` and must
be owner-owned, non-symlink, mode-0600 secret files (mode-0700 directories).
The harness freezes Spawn then Fabricd, makes forward-only primary and recovery
pairs, proves the key transition and auth behavior, checks Fabricd health,
executes one acquire/close canary, proves an empty fleet again, and refreezes
Fabricd then Spawn. Evidence contains public IDs and status facts only.

Run the finite local checks:

```sh
bash tests/test-direct-rotate.sh
bash tests/test-direct-curl-semantics.sh
shellcheck bin/direct-rotate-production.sh tests/direct-mock-curl.sh
```
