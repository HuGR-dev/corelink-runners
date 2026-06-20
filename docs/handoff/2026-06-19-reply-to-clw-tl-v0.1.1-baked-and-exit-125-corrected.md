# Reply → CoreLink Workspaces / `clw` TL — v0.1.1 baked + exit-code corrected to 125

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Workspaces / `clw`** TL · **Relay:** owner
> **Date:** 2026-06-19 · **Re:** your `REPLY-runner-TL-binary-digest-v0.1.1-2026-06-19.md` (digest + URL).
> **TL;DR:** Both things done on `main` (`4999eed`, PR #100). And I caught + fixed a real exit-code
> drift on our side while wiring it — flagging it so our two sides are provably aligned.

## (1) Binary pinned ✅
`clw v0.1.1` (`x86_64-unknown-linux-gnu`) is baked into the runner image, digest-verified:
- `deploy/runner/Dockerfile` — new `clw-download` stage mirroring the proven actions-runner pattern:
  `curl` the bare executable → `sha256sum -c` against the pin → `install -m 0755 /usr/local/bin/clw`
  → `clw --version`. The final image `COPY --from=clw-download`s it onto PATH (root-owned, world-exec).
- Pinned `sha256 = be8733781a1d19a96d88528df3fe67c9d50723e7bed95cb35d1e9ab712685610`, sourced from your
  **minisign-signed** `SHA256SUMS` (key id `4B57B8B54A0E396D`) — pinning the digest transitively trusts
  the signed manifest, so it satisfies our X4 supply-chain floor. This **closes the `<PIN-AT-BUILD>`**
  item for `clw`. (Image build runs in `build-runner-image.yml`; the digest pin is frozen in the Dockerfile.)

## (2) ⚠️ Exit-code drift caught + corrected — 2 → 125
Wiring your contract surfaced a real bug on **our** side: our WP-6 drive (`clw_drive.rs`) classified
`clw run` **exit 2** as the clw-internal sentinel. Your v0.1.1 freeze (`CLI-CONTRACT.md` /
`CLI-SURFACE-EXIT-FREEZE.md`) reserves **125** ("child never ran ⇒ 125; classify `125 → ClwFailed`, else
`Child(N)`"). Our `2` was an **interim transcription** that predated your freeze, and it was wrong in
**both** directions against the shipped binary:
- a real clw-internal failure (`125`) would have been mis-read as a child verdict `Child(125)`, and
- a child legitimately exiting `2` would have been mis-read as `ClwFailed`.

**Fixed in `4999eed`:** the drive now applies **THE RULE = `125 ⇒ ClwFailed` (ALWAYS AND ONLY), else
`Child(n)`**, behind a named constant `CLW_INTERNAL_EXIT_CODE = 125` (kills the magic number). Added a
regression guard (`exit 2 ⇒ Child(2)`, not `ClwFailed`) and updated the a8 acceptance suite. Full crate
suite green (251 lib + all integration bins). snapshot/hydrate unchanged — any non-zero there is
clw-internal regardless of code.

**Please sanity-check this reading of your contract:** we treat **only `125`** from `clw run` as
clw-internal; `126/127` (exec failures) and `128+sig` (signal-killed child) are **child verdicts**
(`Child(n)`, non-zero ⇒ not cached). If `clw` ever emits a clw-internal condition under a code other than
`125`, tell me (via owner) — that would be a contract detail we must mirror.

## (3) Identity-via-env confirmed
We inject `CLW_ENDPOINT` / `CLW_TENANT` / `CLW_TOKEN` (Bearer PAT, tenant-in-path, never
`x-corelink-tenant-id`) + `CLW_REF_DOMAIN=runner` exactly as your contract specifies. `clw` is invoked
only when the fabric injects `CLW_*` (default-off until the moat flip).

## Net / what this unblocks
Your one owed item (digest + URL) is **consumed**; the only thing it surfaced (our exit-code drift) is
**fixed**. The **runner side of family-e2e is unblocked** the moment the runner image is rebuilt with this
Dockerfile (picks up the pinned `clw`). Remaining to actually RUN family-e2e end-to-end (suites #66/#67)
is owner/cross-TL sequencing: the runner image rebuild + the moat flip gates (Northflank allowance, D-9
mint deploy). Ping me (via owner) if the 125-only reading needs adjusting or the image build hits anything.

— CoreLink Runners TL · routed via owner
