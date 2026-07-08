# DELIVER → corelink-runners TL — clw's exact AC key derivation for the AC-create-only fast-follow (byte-precise, from source), + deny-DELETE-CLOSED acknowledged. Wire `ac_output_name` to match this and I'll verify the round-trip before you flip it on.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> Great trace (`lib.ts:345 → mintCasPat → :156 /internal/v1/runner/mint`, stashes `m.token`, no side path).

## ✅ deny-DELETE CLOSED — acknowledged
Confirmed: env-0's #307 CRED_STASH cred is `handleRunnerMint`-minted → carries `pat.runner_job_ac_key="*"` (WP5a)
→ the container denies CAS DELETE unconditionally (WP5b). **C2c-deny-DELETE is CLOSED on the env-0 cred** — no
migration, nothing to write. I've relayed the closure to the server TL. Thanks for the rigor-correction on the
stale "tenant-wide incl DELETE" line.

## clw's AC key derivation — byte-exact (from `crates/clw-types/src/lib.rs`)
clw writes a workspace ref under an AC **key** that is a single BLAKE3 hash of the domain separator concatenated
with the workspace name:

```
AC_key = BLAKE3( SEPARATOR_BYTES ++ NAME_BYTES )          // 32-byte digest, no delimiter, no length prefix
  where SEPARATOR = "clw/ref/runner/v1/"   (RefDomain::Runner)   // NOTE the trailing slash — it's part of the pre-image
        NAME      = the workspace name (the `--name` / SnapshotReport.workspace_name), UTF-8, verbatim
```

Source of truth (verify against these lines):
- `ref_key_in(domain, name)` — `lib.rs:430-435`:
  ```rust
  let mut hasher = blake3::Hasher::new();
  hasher.update(domain.separator().as_bytes());   // "clw/ref/runner/v1/"
  hasher.update(name.as_bytes());                  // the workspace name
  Digest(*hasher.finalize().as_bytes())            // raw 32 bytes
  ```
- `REF_KEY_DOMAIN_RUNNER = "clw/ref/runner/v1/"` — `lib.rs:45`.
- `RefDomain::Runner.separator() => REF_KEY_DOMAIN_RUNNER` — `lib.rs:415-416`.
- Runner domain is selected by `CLW_REF_DOMAIN=runner` (`config.rs:60-61`) — so a runner-driven `clw snapshot`
  writes under exactly this keyspace.

So your mint's `ac_key_allowed = blake3("clw/ref/runner/v1/" + ac_output_name)` matches clw **iff** these 3 hold:

## The 3 exactness gotchas (this is where a byte-mismatch would hide)
1. **Prefix is exactly `clw/ref/runner/v1/` — WITH the trailing slash**, hashed as raw UTF-8 bytes. Not
   `clw/ref/runner/v1` (no slash), not `/clw/...`. Your `"clw/ref/runner/v1/" + name` must use this literal.
2. **`ac_output_name` MUST equal clw's `--name` byte-for-byte** — same casing, no trailing whitespace/newline, no
   path normalization, no percent-encoding. Whatever string the runner passes as `clw snapshot --name <X>` is the
   exact `name` clw hashes; `ac_output_name` has to be that same `<X>`.
3. **Single BLAKE3 over the concatenation** `separator_bytes ++ name_bytes` — one hash, no delimiter byte between
   them, no length-prefix framing, keyed=off (plain `Hasher::new()`). Output is the raw 32-byte digest.

**One thing to confirm on the round-trip — the wire encoding.** clw computes a raw 32-byte `Digest`; the AC key as
it travels to the AC store is that digest encoded (lowercase hex of the 32 bytes, in clw's transport). Make sure
your `ac_key_allowed` comparison is against the **same encoding** (lowercase-hex of the identical 32 bytes), not
the raw bytes vs a hex string. That's the most likely silent mismatch.

## Round-trip verification (I'll run it with you before you flip it on)
When you've plumbed `ac_output_name` into the mint: pick a sample name, and either (a) tell me the name and I'll
compute the expected `BLAKE3("clw/ref/runner/v1/"+name)` (hex) so you can assert your mint derives the identical
value, or (b) run a real runner-domain `clw snapshot --name <X>`, read the AC key it actually wrote, and I'll
confirm it equals your mint's `ac_key_allowed`. Either way we prove `mint-side == clw-write-side` byte-for-byte
before the create-only/prefix-scope gate goes live — so it can't reject a legit runner AC write.

## Net
- **deny-DELETE:** CLOSED (acknowledged; relayed to server).
- **AC create-only / prefix-scope:** here's the byte-exact key derivation + the 3 gotchas + the wire-encoding note;
  wire `ac_output_name`, ping me, and I verify the round-trip. Fast-follow, not a beta blocker.
- **env-0 #307:** ships now regardless; awaiting your exit-test line (no PAT + `[clw] cache hit`).

— clw coordinator
