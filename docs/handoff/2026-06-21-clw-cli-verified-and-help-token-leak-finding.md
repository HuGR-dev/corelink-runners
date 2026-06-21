# clw CLI verified (for the effective-hydration build) + a `--help` token-leak finding for the clw TL

> **Author:** CoreLink **Runners** TL · **Date:** 2026-06-21 · **Type:** verified contract record + cross-TL
> security finding (clw TL). Captured from `clw 0.1.1` running in the live dogfood container (wire-truth).

## A. Verified clw CLI (v0.1.1) — the contract the runner builds against

Global options (env-driven; the autoscaler already injects these): `--endpoint` (`CLW_ENDPOINT`),
`--tenant` (`CLW_TENANT`), `--token` (`CLW_TOKEN`), `--cache-dir` (`CLW_CACHE_DIR`, default `~/.clw/cache`),
`--ref-domain` (`CLW_REF_DOMAIN=runner` → keyspace `clw/ref/runner/v1/`), `--json`, `--concurrency`.

Commands: `init · snapshot · hydrate · status · run · ls · rm · prune · erase · doctor · auth · uninstall`.
The three that matter for the moat:
```
clw snapshot [OPTIONS] --name <NAME> [PATH]    # snapshot a workspace dir to CoreLink (default PATH = .)
clw hydrate  [OPTIONS] --name <NAME> <DEST>    # hydrate a snapshot into DEST — DEST MUST be empty/absent
clw run      ...  # "Run a command, memoizing its output to CoreLink"  ← the memoize primitive
```
Key constraints learned:
- **`hydrate <DEST>` must be empty/absent** → you cannot hydrate a snapshot *over* a populated dir (e.g. the
  image's pre-seeded `~/.cargo`). The snapshot/hydrate model is whole-workspace-tree, not merge-into-existing.
- **`clw run` is the real moat primitive** (memoized CI): it hashes inputs, returns the cached output on a
  HIT *without re-running* (the "re-runs ≈ 0" claim), runs+caches on a miss. This is the `ClwBoxDrive`
  `snapshot→hydrate→run` model — exit `125` = clw-internal (frozen §4 contract).

⇒ **Design implication for effective hydration:** the increment is to wrap the check in **`clw run`**
(memoize), not naive dir save/restore (which `hydrate`'s empty-dir rule blocks anyway). Next: capture
`clw run --help`'s exact flags (masked) and wire a real `clw run -- cargo build` benchmark.

## B. ⚠️ Security finding for the clw TL — `clw --help` echoes `CLW_TOKEN`'s VALUE

`clw --help` (and any `-h`) prints, for each env-bound global option, a clap line `[env: NAME=<current
value>]` — **including `[env: CLW_TOKEN=<the actual PAT>]`**. So any `clw --help` invocation in a logged
context (CI, support bundle, screen-share) **leaks the per-job CAS PAT into the log**.

- **Observed:** running `clw --help` in a dogfood CI job printed the live `corelink_pat_…` value to the
  GitHub Actions log. (Blast radius bounded: it's a per-job, `cas:rw`-scoped, 5400s-TTL PAT, and the runner
  revokes it on completion — but it's still a plaintext secret in a log.)
- **Mitigation taken on our side:** deleted the leaking run's logs; re-probed with the token env stripped
  (`env -u CLW_TOKEN …`). We do **not** run `clw --help` in production (only `hydrate`/`run`), so prod isn't
  affected — this is a probe/diagnostics footgun.
- **Suggested clw fix (clw TL's call):** mark the `--token` arg `hide_env_values(true)` (clap) — or don't
  expose `CLW_TOKEN` as an `env`-annotated arg whose help renders the value — so `--help`/`doctor` never
  print the secret. Same care for any other secret-bearing env. Low-effort, closes the footgun for everyone
  (clw's own `doctor` is "redacted" per its help, so help should be too).

**Ask (clw TL, no rush):** confirm + patch the `--help` token echo. This is a clw-side change; flagging so the
diagnostics surface matches clw's own redaction posture.

— CoreLink Runners TL · routed via owner
