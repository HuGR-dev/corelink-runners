/** T8-W5 transactional credential obligation contract (root-owned boundary).
 * No raw credential is permitted. KV may remain a compatibility projection;
 * it is never the authority for a current credential or confirmed revocation.
 */
export interface CredentialIdentity {
  jobId: string;
  tenant: string;
  patId: string;
}

export type CredentialSelection =
  | { kind: "job"; jobId: string }
  | { kind: "tenant"; tenant: string }
  | { kind: "all" };

export interface CredentialPage {
  records: CredentialIdentity[];
  cursor?: string;
  complete: boolean;
}

/** Implement on existing ContainmentDO storage; no new namespace or secret. */
export interface CredentialAuthority {
  /** Atomic immutable insert by exact identity; same identity is idempotent.
   * Register every minted patId before allowing JIT/container effects. A
   * replacement for one job never replaces the older cleanup obligation.
   */
  registerCredential(identity: CredentialIdentity): Promise<void>;
  /** Explicit durable transition registered -> revoke_requested. Registration
   * alone NEVER authorizes a background retry to revoke a live credential.
   * Exact already-requested/revoked identity is idempotent; missing/corrupt
   * identity refuses. Call before the first revoke HTTP request.
   */
  requestCredentialRevocation(identity: CredentialIdentity): Promise<void>;
  /** Only revoke_requested obligations; excludes healthy registered and
   * confirmed terminal identities. Scheduled retry uses ONLY this method.
   */
  revocationRequestedCredentials(cursor?: string): Promise<CredentialPage>;
  /** Bounded strongly consistent pages of obligations not confirmed revoked.
   * Cursor advances over scanned keys. Invalid stored records fail closed.
   */
  pendingCredentials(selection: CredentialSelection, cursor?: string): Promise<CredentialPage>;
  /** Called only AFTER confirmed revoke. Durable terminal state is retained;
   * no read/delete against a mutable KV job mapping establishes this proof.
   */
  confirmCredentialRevoked(identity: CredentialIdentity): Promise<void>;
}
