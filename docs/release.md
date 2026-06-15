# Release runbook

This document covers how to cut a CoreLink Runners release: building and
attaching the binary, publishing the SDKs, keeping versions in sync, and
the open owner decisions that gate each step.

---

## 1. Cutting a release (binary)

```bash
# 1. Confirm all gates are green on main (CI must be green before tagging).
#    Never tag a red tree.

# 2. Tag and push:
git tag v0.1.0          # replace with actual version
git push origin v0.1.0
```

That push triggers `.github/workflows/release.yml` on the self-hosted
`corelink-builder` runner. The `binary` job:

1. Checks out the tagged commit.
2. Runs `cargo build -p corelink-cli --release --locked`.
3. Computes a SHA-256 checksum of the resulting binary.
4. Creates a GitHub Release for the tag (with auto-generated notes) and
   uploads two assets:
   - `corelink-aarch64-apple-darwin` — the `corelink` CLI binary.
   - `corelink-aarch64-apple-darwin.sha256` — the SHA-256 checksum.

The GitHub Release is the concrete artifact that unblocks users of the
GitHub Action and Buildkite plugin (see §4 below).

**Note:** x86_64/Linux binaries are a deliberate follow-up. The current
pipeline targets the aarch64 mac builder only. Cross-compilation will be
added once a Linux self-hosted runner is available.

---

## 2. SDK publish procedure

The two SDK publish jobs (`publish-npm`, `publish-pypi`) are **safe no-ops
by default** — each is gated on a repository secret. They are skipped (not
failed) when the secret is absent.

### npm — `@corelink/verify`

**Secret required:** `NPM_TOKEN` (a scoped Automation token from npmjs.com)

Add it in: GitHub → repo Settings → Secrets and variables → Actions →
New repository secret → name `NPM_TOKEN`.

Once the secret is present, pushing any `v*` tag triggers `npm publish
--access restricted` from `sdk/typescript/`.

### PyPI — `corelink-verify`

**Secret required:** `PYPI_TOKEN` (a PyPI API token, scoped to the
`corelink-verify` project)

Add it in: GitHub → repo Settings → Secrets and variables → Actions →
New repository secret → name `PYPI_TOKEN`.

Once the secret is present, pushing any `v*` tag builds sdist + wheel from
`sdk/python/` and runs `twine upload`.

---

## 3. Open owner decisions (do not decide unilaterally)

### npm access scope
The workflow uses `npm publish --access restricted` (private-scoped
`@corelink/verify`). The `@corelink` org must be registered on npmjs.com
and the decision to make the package public must be confirmed before
changing to `--access public`.

**Decision:** owner / HuGR npm account holder must confirm the npm org name,
access level (private vs. public), and whether the package should be
published to the public npm registry or a private registry.

### PyPI project name
The package is `corelink-verify` on PyPI. Confirm this name is unclaimed
and desired before the first publish.

**Decision:** owner must claim the PyPI project name (or confirm the
organisation-managed PyPI trusted publisher is in place) before the secret
is added.

---

## 4. Version sync

The canonical version lives in the workspace `[workspace.package] version`
in the root `Cargo.toml`. Currently: `0.1.0`.

Keep all three in sync before tagging:

| Artifact | Version field |
|---|---|
| `corelink` binary | `[workspace.package] version` in `Cargo.toml` |
| `@corelink/verify` | `version` in `sdk/typescript/package.json` |
| `corelink-verify` | `version` in `sdk/python/pyproject.toml` |

When bumping a release, update all three files in the same commit, then tag.

---

## 5. Follow-up: wire integrations to download the released binary

The GitHub Action (`integrations/github-action/`) and Buildkite plugin
(`integrations/buildkite-plugin/`) currently locate `corelink` via `PATH`.
This works for users who build from source but not for the common case.

Once a GitHub Release exists with the `corelink-aarch64-apple-darwin` asset,
a follow-up work-package should:

1. Add a `version` input to the GitHub Action and Buildkite plugin.
2. Have the setup step download `corelink-aarch64-apple-darwin` (and verify
   the `.sha256`) from the GitHub Release for that version and place it on
   `PATH`.
3. Extend the download logic for additional platforms (x86_64-linux) once
   those binaries are added to the release pipeline.

This follow-up is intentionally out of scope for this WP; it requires a
released binary to exist first.
