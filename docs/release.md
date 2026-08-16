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
`corelink` ephemeral fleet (this repo's own product — $0 GitHub billing; there
is no self-hosted macOS builder for this repo). The `binary` job builds a
matrix of targets — natively for the fleet's own Linux host triple, and via
`cargo-zigbuild` (zig cross-toolchain) for the other Linux-family targets —
then `publish-release` creates the GitHub Release and attaches every binary
plus its SHA-256 checksum:

- `corelink-x86_64-unknown-linux-gnu` (+ `.sha256`) — native build.
- `corelink-aarch64-unknown-linux-gnu` (+ `.sha256`) — `cargo zigbuild` cross.
- `corelink-x86_64-pc-windows-gnu.exe` (+ `.sha256`) — `cargo zigbuild` cross.

The GitHub Release is the concrete artifact that unblocks users of the
GitHub Action and Buildkite plugin (see §4 below).

**Note (2026-08-16):** `aarch64-apple-darwin` (and any other Apple target) is
**not** built by this pipeline. Cross-compiling to an Apple target needs the
(non-redistributable) Apple SDK, and every proven zigbuild pipeline in this
org (`corelink-server`'s `release-cli.yml`, `corelink-workspaces`' `release.yml`)
builds darwin **natively**, on a real self-hosted Mac (`corelink-builder`).
corelink-runners has no such Mac builder today. A previous version of this
job ran on GitHub-hosted `ubuntu-latest` with no `--target`, producing a
native Linux ELF, and mislabeled + shipped it as `corelink-aarch64-apple-
darwin` — an Apple-Silicon user running it would hit "Exec format error".
That mislabel is fixed by only building targets this fleet can produce
correctly. Adding a real darwin leg is a follow-up once a self-hosted Mac
builder exists for this repo (mirror `release-cli.yml` / corelink-workspaces'
`release.yml`).

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

### SDK license / distribution rights
This repo is **private with no `LICENSE` file** — i.e. proprietary, all rights
reserved. Both SDK packages therefore declare a proprietary license
(`UNLICENSED` for npm, `Proprietary` for PyPI) as the **safe default**. A
permissive license (e.g. MIT) would *grant redistribution rights* and must NOT
be set without the owner's explicit decision.

**Decision:** owner must choose the SDK license before any public publish —
keep proprietary (private-registry / restricted only), or adopt a permissive
license and add a top-level `LICENSE` file. Until then the packages stay
proprietary and publish only to a restricted/private scope.

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

Once a GitHub Release exists with the `corelink-x86_64-unknown-linux-gnu` /
`corelink-aarch64-unknown-linux-gnu` / `corelink-x86_64-pc-windows-gnu.exe`
assets, a follow-up work-package should:

1. Add a `version` input to the GitHub Action and Buildkite plugin.
2. Have the setup step download the matching asset for the runner's OS/arch
   (and verify its `.sha256`) from the GitHub Release for that version and
   place it on `PATH`.
3. Extend the matrix with a darwin leg once a self-hosted Mac builder exists
   for this repo (see §1's 2026-08-16 note).

This follow-up is intentionally out of scope for this WP; it requires a
released binary to exist first.
