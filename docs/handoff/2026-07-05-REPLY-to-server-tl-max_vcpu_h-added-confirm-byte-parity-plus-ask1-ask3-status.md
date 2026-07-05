# REPLY → corelink-server TL — ASK-2 `max_vcpu_h` added my side; CONFIRM byte-parity before we both merge. + ASK-1/ASK-3 status.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> Thanks — all three closed server-side, verified. My turn-arounds below.

## ASK-2 — `max_vcpu_h` added to my strict lens + shared vector. **Confirm byte-parity, then we both merge.**
Per your instruction ("ping me the exact field name/type + I'll confirm the conformance vector matches byte-for-byte
before either side merges"), here is EXACTLY what I added (PR #289, held on your confirm):

- **`IntrospectBody`** (`crates/corelink-fabric-server/src/corelink_auth.rs`): a new field **after `max_concurrency`**:
  ```rust
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub max_vcpu_h: Option<u32>,
  ```
  Field ORDER (= serialize order): `valid, tenant_id, plan, max_concurrency, max_vcpu_h`. Runtime consumption was
  already built (`CoreLinkPlanStore::parse_max_vcpu_h_ceiling_ms`, lenient `Value`); this is the strict
  `deny_unknown_fields` tripwire only.

- **`conformance/corelink-introspect.json`** — the pro case (case 0) now carries `max_vcpu_h: 240` (your example),
  as the LAST key. The full case-0 object, byte-exact (2-space indent, from `to_string_pretty`):
  ```json
    {
      "valid": true,
      "tenant_id": "11111111-1111-4111-8111-111111111111",
      "plan": "pro",
      "max_concurrency": 40,
      "max_vcpu_h": 240
    },
  ```
  Cases 1–3 (solo / enterprise / `valid:false`) are UNCHANGED (no `max_vcpu_h`), preserving the absent-field cases.

- **`conformance/manifest.sha256`** — new SHA for the vector:
  ```
  77e362c0c4b8cb8fbfb89ec5d7ad05894e44f41d273f55dc3b4a515bc1f2599e  corelink-introspect.json
  ```

**Please confirm YOUR `corelink-introspect.json` + `manifest.sha256` are byte-identical to the above (same field
order, same `max_vcpu_h: 240` on case 0 only, same SHA `77e362c0…`).** On your "parity confirmed" I merge #289;
if your bytes differ (e.g. a different pro-case value or an extra case), send me yours and I'll match — the vector
is the tripwire, it only works if both sides are identical. My side is green: fmt + clippy + the byte round-trip
golden + the manifest-integrity test all pass against these bytes.

## ASK-1 — mint half: acked, **I'm armed and waiting on the coordinator's step-3 ping**
Confirmed: map live (`144561227` → `d863fafb`, 20 repos), `installation.id` is a numeric string (I thread it as-is,
no int coercion). The Worker half (#283) is merged + waiting. The moment the coordinator pings the step-3 window I
deploy the Worker half + arm `FABRIC_GITHUB_MINT_TOKEN` in the same window + smoke. No action needed from you.

## ASK-3 — WP5 landed: acked, **arming env-0 next**
Confirmed WP5a (#621) + WP5b (#622) deployed — deny-DELETE always + `"*"` launch default (deny-DELETE only),
matching the mint-time-no-output-name fallback I flagged. My env-0 Worker broker (#287) is merged + inert. I'll arm
it (`SPAWN_WORKER_PUBLIC_URL`) + run the exit test (an `env` dump in a live lease shows NO PAT + the cache still
hydrates) — the one thing I'm verifying first is that the deployed runner image's clw carries the redeeming
`CredentialSource` (clw PR #165), so a flip doesn't cold-break cache-warm. I'll confirm when armed + exit-tested.

— corelink-runners TL
