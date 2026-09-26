# Trusted exact-head issue packs

`.github/workflows/issue-pack-dispatch.yml` is a manual acceptance route for
issues #566, #570, #575, #578, #580, #586, #602, #603, and #604. Start the workflow from the protected
`main` ref. Supply the exact current PR head SHA, the current `main` SHA that
forms the PR base, and the PR number. The job rejects any other workflow ref,
repository, malformed SHA, stale PR head, or PR merge ref whose two parents do
not match the supplied base and candidate SHAs. A short trusted metadata step
uses the read-only GitHub token to confirm the PR is open, targets `main`, and
has those exact base/head SHAs. Candidate command processes receive no token.

The trusted `prepare` job checks out controls at the dispatching protected-main
commit, checks out the candidate into a separate data directory, and binds the
PR number, exact head/base, and changed paths. Candidate code does not run in
this job. The `candidate-pack` job runs the allowlisted commands on its own
GitHub-hosted runner. A final `publish-receipt` job runs on a fresh runner after
the candidate job ends, and writes/uploads the receipt using trusted job
outputs and GitHub's candidate job conclusion. Candidate processes cannot
write or replace that receipt. The dispatcher never reads a command from the
candidate checkout, and candidate workflow files are never invoked. The
allowlisted `npm` commands can run scripts declared by the candidate package,
and Cargo commands build candidate crate code; those processes receive no
GitHub token or workflow credentials. Checkout credentials are ephemeral and
are removed before candidate commands run.

## Pack catalog

| Pack | Exact changed paths | Commands |
| --- | --- | --- |
| `issue-566` | `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/index.test.ts` | `npm ci`; `npm run typecheck`; `npm test -- --run test/index.test.ts` |
| `issue-570` | `deploy/cloudflare/src/durable_objects/runner_dev_env.ts`; `deploy/cloudflare/src/lib/devenv_credentials.ts`; `deploy/cloudflare/test/devenv-credentials.test.ts` | `npm ci`; `npm run typecheck`; `npm test -- --run test/devenv-credentials.test.ts` |
| `issue-575` | `.github/workflows/build-cf-container-images.yml` and the six explicit helper/selftest paths in `issue_pack_catalog.json` | Pinned actionlint on the changed image workflow; `bash scripts/ci/runner-image-static-check.selftest.sh`; `bash scripts/ci/runner-image-build-validation.selftest.sh` (includes bounded export/import and archive mutation controls) |
| `issue-578` | `README.md` | Trusted check that the linked run number, ID, and date match the latest successful `ci.yml` run on `main`, plus the explicit non-enforcement statement |
| `issue-580` | `Cargo.toml`; `Cargo.lock` | `cargo deny check`; `cargo audit --deny warnings`; `cargo check --workspace --all-targets --locked`; `cargo test -p corelink-fabric-server --test server_bin config_pg_tls --locked` |
| `issue-586` | The eleven explicit containment and normal-intake source/test paths listed in `issue_pack_catalog.json` | `npm ci`; `npm run typecheck`; `npm run test:coverage` |
| `issue-602` | `.github/workflows/spawn-worker-ci.yml`; `deploy/cloudflare/src/billing_recovery.ts`; `deploy/cloudflare/test/billing-recovery.test.ts` | `npm ci`; `npm run typecheck`; `npx vitest run test/billing-recovery.test.ts` |
| `issue-603` | `deploy/cloudflare/test/fixtures/issue-603/recovery-matrix.json`; `deploy/cloudflare/test/historical-settlement-reconcile.mjs`; `docs/historical-settlement-recovery.md` (all three required; bound to PR #626) | `node --test deploy/cloudflare/test/historical-settlement-reconcile.mjs` using Node 22.19.0 from the candidate repository root |
| `issue-604` | All thirteen exact paths in the trusted catalog (bound to PR #634), including the ACK vector/manifest, Worker and Rust consumers/tests, contract docs, and `scripts/ci/secret-scan.sh` | `npm ci`; `npm run typecheck`; six focused Worker tests listed in the catalog; `corelink-fabric-server` billing ACK unit tests; `corelink-runners-contracts` conformance tests |

For #575, the selftest uses stubs/fixtures and never invokes the manual image
builder. Its receipt is authored-head code evidence only; it does not satisfy
the issue's separate B-138 runtime/current-state evidence contract or authorize
a corelink runner, provider build, or image publication.

The Node toolchain is pinned to 22.19.0. The Rust toolchain is pinned to
1.96.0; cargo-deny 0.19.8 and cargo-audit 0.22.2 are installed by a SHA-pinned
action. actionlint 1.7.12 is downloaded by a checksum-verified bootstrap for
the workflow pack. All actions use full commit pins. Each job runs on
`ubuntu-latest` with a finite timeout and read-only `contents: read`,
`pull-requests: read`, and `actions: read` permissions. The workflow does not read
repository secrets, use provider credentials/calls, deploy, use self-hosted
runners, or execute candidate workflows. The explicit `GH_TOKEN` environment
is scoped to metadata binding; checkout actions use the read-only job token
transiently with `persist-credentials: false`. Neither token is passed to
candidate command processes. A JSON receipt is written and uploaded by the
fresh finalizer runner. It includes the exact SHAs, PR, trusted changed
paths, allowlisted commands, preparation and candidate job results, and run
URL; for #578 it also records the latest successful main CI run ID, number, and
date used by the README check. On candidate-job failure, command rows are marked unavailable rather
than inferred from candidate-writable files; use the workflow logs for the
failing step.

## Automatic workflow inventory and evidence classification

| Candidate surface | Automatic pull-request workflows | Classification |
| --- | --- | --- |
| #566, #570, and #586 (`deploy/cloudflare/**`) | `spawn-worker-ci.yml`, `ci.yml`, `dco.yml`, `secret-scan.yml` | The issue-specific Worker typecheck/test failure is candidate-owned. Failures in unrelated Rust gates are informational for these packs and remain on their owning issue. DCO and security findings are never waived. The exact pack is authoritative for its acceptance evidence; do not repeatedly rerun broad suites to seek a different result. |
| #575 (image workflow and `scripts/ci/**`) | `ci.yml`, `dco.yml`, `secret-scan.yml`, `plan-integrity.yml`, `selftests.yml` | The changed image workflow syntax, disk contract, OCI archive verifier, and helper selftest failures are candidate-owned. Unrelated Rust gates are informational and remain on their owning issue. DCO and security findings are never waived. The manual image build/publish workflow is not dispatched by this pack. |
| #578 (`README.md`) | `ci.yml`, `dco.yml`, `secret-scan.yml` | README diff failure is candidate-owned. Unrelated Rust/security results remain with their owners; DCO and actual secret findings remain blocking under normal repository policy. |
| #580 (`Cargo.toml`, `Cargo.lock`) | `ci.yml`, `pg-suite.yml`, `pending-cleanup-ci.yml`, `dco.yml`, `secret-scan.yml` | Dependency resolution, deny/audit, workspace compilation, and TLS-owner test failures are candidate-owned. A failure isolated to unrelated Worker or documentation coverage remains with its owner issue. Security findings are never suppressed. |
| #602 (`billing_recovery.ts`, its focused test, and `spawn-worker-ci.yml`) | `spawn-worker-ci.yml`, `ci.yml`, `dco.yml`, `secret-scan.yml` | Billing recovery typecheck and focused test failures are candidate-owned. An automatic run that tests the synthetic PR merge SHA does not prove the authored head; use this exact-head pack for #602. DCO and security findings remain blocking. |
| #603 (historical settlement fixture, focused reconciliation test, and operator documentation) | `ci.yml`, `dco.yml`, `secret-scan.yml` | The exact-head Node fixture command is candidate-owned. The trusted catalog binds this pack to issue #603 and PR #626, checks live PR metadata for the dispatch-time head and base, and requires all three declared paths. DCO and security findings remain blocking. |
| #604 (typed Worker and native billing acknowledgement consumers) | Existing Worker/Rust pull-request gates, `dco.yml`, `secret-scan.yml` | Focused ACK test failures are candidate-owned. The trusted catalog binds this pack to issue #604 and PR #634, checks live PR metadata for the dispatch-time head/base, and requires the exact thirteen candidate paths. The protected-main prepare job runs dispatcher selftests before candidate checkout; candidate processes receive no token. DCO and security findings remain blocking. |

The listed PR workflows are triggered by the current `on.pull_request` path
rules at the protected-main baseline. The pack does not disable or weaken those
workflows; it gives the central reviewer an exact-SHA acceptance receipt when
a broad result is unrelated to the changed acceptance criteria. Escalate to a
broader suite if a changed path leaves the catalog boundary, a shared build or
test contract changes, the exact pack itself fails, or security/release
behavior is affected.

## Selftest and operator steps

The #578 pack compares the README's linked run number, run ID, and completion
date to the latest successful `ci.yml` run on `main`, fetched by the trusted
metadata step; it also requires the explicit non-enforcement statement. The
trusted-control selftest runs before candidate checkout. It accepts every
catalog entry and rejects undeclared pack IDs, paths, SHA forms, commands,
non-main refs, and PR metadata that targets another base. To dispatch, choose
a pack and copy the current PR head SHA, current `main` SHA, and PR number
into the manual workflow inputs. Review the
uploaded JSON receipt for the matching artifact, exact SHAs, and candidate job
result. A `workflow_dispatch` result is evidence for central merge review;
it is not a PR status check. Keep the receipt and ordinary DCO/signature and
required-check evidence with the issue record. No pack result authorizes a
merge or issue closure by itself.

The #604 pack is bound to PR #634 and its thirteen-path diff recorded in the
trusted catalog. Refresh the current PR head and live `main` base SHA before
its single dispatch; do not reuse either SHA from an earlier run. The receipt
records that pair, the protected-main control SHA, exact changed paths, and
the bounded Worker/native ACK command list.
