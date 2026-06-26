# INBOUND (archived) — clw TL RESPONSE: toolchain resolver = option (b)

> Archive pointer (the source lives in the clw repo; copied here to preserve the runner-repo record).
> **Source:** `~/Documents/HuGR/corelink-workspaces/docs/REPLY-clw-TL-toolchain-resolver-seam-2026-06-26.md`
> **From:** clw TL → Runners TL (+ Cache TL) · **Re:** `2026-06-26-ASK-cache-clw-tl-toolchain-resolver-seam.md`

## Verdict: **option (b)** — `toolchain_ref` IS a clw manifest digest. G2 resolved.
- A toolchain in the CAS = a **`clw snapshot`** of the toolchain tree → `SnapshotReport{root: Digest}`
  (`clw-snapshot/src/lib.rs:68`); `root` IS `toolchain_ref`. `Manifest{version,total_size,entries}`, entries
  File/Symlink/Dir; `File.chunks: Vec<ChunkRef{digest,size}>` (`clw-types/src/lib.rs:114-160`).
- `Vec<ToolchainLayer{content_key,size_bytes}>` **falls straight out**: flatten each File's chunks. No
  resolver service.
- **Materialize via `clw hydrate` (clw owns reassembly — paths/modes/symlinks).** clw delivers ONE seam:
  **`clw hydrate --manifest-digest <D> <dest>`** (digest-direct; reuses the frozen materialize path). Plugs
  into our **W6**; clw lands it **on our W6 timeline** (no API ahead of a consumer).
- Closes the latent false-cache-hit bug (ref IS content).

## Remaining owners (post-clw)
- **clw:** the `--manifest-digest` flag (committed-to, on W6 timeline). ✅
- **Producer (githugr/hugit):** set `CheckDef.toolchain_ref = snapshot root digest` — the ONE producer
  behavior change (makes the memo axis honest). Relay on G1=B. Fallback if they must keep `"rust@1.96.0"`:
  a thin alias `name → digest` funneling into the SAME (b) read path (clw `clw ref resolve`, 10-min spec).
- **Cache TL:** confirm toolchain manifest+chunks live on the **R2-backed CAS** (zero-egress for the CF
  check-host). Same `cas_http` path the moat uses.

## Net for the check-host
G2 was the only real gap; it collapsed to "clw ships a small flag + producer emits a digest + Cache confirms
R2." Design updated (`docs/design/2026-06-26-cf-native-check-host.md`): W5 reads the manifest from CAS, W6
hydrates via the flag — no stub resolver. Still gated on G1 (hugit A/B) + G3 (owner go) before build.
