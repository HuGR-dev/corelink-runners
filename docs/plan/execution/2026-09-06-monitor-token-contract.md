# T6-W15 token primitives — root frozen contract

Baseline: 3334ff23d3efffdeea0ebaa8a46133fe2614410c. Three disjoint Luna authors, independent cold reviews, integration owner runner_final_composition. No changes to shared files, index, configuration, existing ACK wire or producer code. These primitives are dependencies of the durable authorization services; they do not satisfy those services or earn WP delivery by themselves.

Use actual acks.ts AsyncSigner/PublicSigningIdentity/verifyOrderedFields, Node RSA-3072/PSS SHA256 salt32, canonical base64url, and JSON ordered-array UTF8 bytes. String key epochs remain strings. All objects reject missing/extra fields and wrong primitive types. Versions are literal '1'; ids/epochs nonempty bounded256 characters, lowercase SHA256 digests exactly64hex, counts/timestamps safe positive integers. PEM and key validation retain existing foundation constraints. Numeric coercion, local wall-clock authority, verification callbacks returning true, fake declaration dependencies and type suppressions are prohibited. Typed validation error for creation/canonicalization; verification of malformed untrusted input returns false. Create verifies signer role/key/epoch, signs, and locally verifies resulting signature before returning. Cryptographic verification is NOT current authorization: no function named isCurrent or trusted status in these modules.

## Manifest codec

Owner files src/signer_manifest.ts and test/signer-manifest.test.ts. Exports SignerManifestFields, SignerRotationManifest extends fields with signature, canonicalSignerManifestPayload(fields):Uint8Array, signerManifestDigest(token):string, createSignerManifest(fields,signer):Promise<SignerRotationManifest>, verifySignerManifest(value:unknown,issuer:PublicSigningIdentity):boolean.

Exactly nineteen signed fields in this order:
manifest_version, manifest_generation, active_signer_key_id, active_signer_epoch, next_signer_key_id, next_signer_epoch, revoked_signer_set_digest, overlap_started_at, overlap_expires_at, recovery_custody_digest, monitor_rearm_tuple_digest, previous_manifest_digest, manifest_issuer_key_id, manifest_issuer_epoch, worm_log_id, witness_checkpoint_sequence, witness_previous_root_digest, witness_root_digest, issued_at.

manifest_generation and witness_checkpoint_sequence are numbers; overlap_started_at, overlap_expires_at, issued_at are numeric timestamps. All other fields are strings. Issuer role must be manifest with exact issuer id/epoch. overlap_expires_at must be strictly greater than overlap_started_at. Active and next (keyId,epoch) pairs must differ. Next signer fields are required, never null/default. Zero previous manifest/witness roots are allowed as codec input for genesis; nonzero resulting witness_root_digest is required. Digest is SHA256 of full ordered twenty-field array including signature, not object insertion order. No claim that a syntactically valid generation/root is current; registry owns chain/high-water/witness validation. Tests real RSA valid bytes, mutation of every field/signature, extra/missing/type errors, wrong role/key/epoch, digest stable across input property order, interval/gen/root validation.

## Recovery token codec

Owner files src/recovery_token.ts and test/recovery-token.test.ts. Exports RecoveryFields, RecoveryToken, canonicalRecoveryPayload(fields):Uint8Array, createRecoveryToken(fields,signer):Promise<RecoveryToken>, verifyRecoveryToken(value:unknown,identity:PublicSigningIdentity):boolean.

Exactly twenty signed fields in this order:
recovery_version, event_id, producer_seq, payload_digest, source, service, application, key_id, credential_epoch, original_monitor_rearm_tuple_digest, ingest_commit_id, original_ack_digest, revocation_record_digest, signer_rotation_manifest_digest, signer_manifest_generation, signer_manifest_witness_root_digest, current_monitor_rearm_tuple_digest, recovery_signer_key_id, recovery_signer_epoch, issued_at.

producer_seq, signer_manifest_generation, issued_at are numbers; remaining fields strings. Role recovery with exact recovery_signer_key_id/epoch. original_ack_digest is externally supplied digest of complete ordered original ACK including signature, not a freshly issued ACK. Codec never retrieves/creates an ingest record or resets a deadline; ack_recovery.ts durable service remains separately required. Tests real RSA, all field substitutions, wrong role/identity/signature, malformed generation/sequence/timestamp/digests and exact byte tuple. A verifier accepts cryptographic validity only; original CAS/current manifest/revocation authorization is not inferred.

## Human page token codec

Owner files src/page_ack_token.ts and test/page-ack-token.test.ts. Exports PageAckFields, PageAckToken, canonicalPageAckPayload(fields):Uint8Array, createPageAckToken(fields,signer):Promise<PageAckToken>, verifyPageAckToken(value:unknown,identity:PublicSigningIdentity):boolean.

Exactly fifteen signed fields in this order:
page_ack_version, incident_id, page_id, delivery_id, destination, on_call_identity, on_call_schedule_digest, action, payload_digest, monitor_rearm_tuple_digest, signer_rotation_manifest_digest, acknowledged_at, expires_at, signer_key_id, signer_epoch.

acknowledged_at/expires_at are numeric timestamps and expires_at must exceed acknowledged_at. Other fields strings. Role page-ack exact signer identity. Destination and principal remain bounded strings, not interpreted as authenticated because present in token. action is signed bounded string; authenticated service later pins exact authorized action. No incident/state writes, no transport success to human-ACK conversion, and no accepting token solely because signature validates. page_ack.ts service still must verify authenticated human principal/schedule/current manifest/trusted expiry and durable replay prevention. Tests real RSA, all binding substitutions, wrong role/identity, malformed timestamps/digests/signature and exact ordered bytes.

## Integration and acceptance

Each author creates a uniquely named isolated worktree from exact baseline and owns only its two paths. Use real node_modules from integration deploy/cost-monitor; no reinstall or dependency mutation. Return full SHA, precise files, focused test exit/result, actual tsc exit, limitations. At most two heavy test jobs across session, coordinated with integrator; source writing/review can proceed in parallel. Cold reviewer must independently inspect field ordering and cross-role rejection. Codecs are integrated before services that consume them. Source partial status remains explicit; no live resource, message, signer generation or production high-water may be mutated by fixtures.
