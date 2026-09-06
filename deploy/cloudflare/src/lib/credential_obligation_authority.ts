import type { CredentialIdentity, CredentialPage, CredentialSelection } from "./credential_authority_contract";
import type { AuthorityStorage, AuthorityTransaction } from "./authority_storage";

/** T8-W5: domain-owned storage logic. ContainmentDO retains RPC wiring. */
export class CredentialObligationAuthority {
  constructor(private readonly storage: AuthorityStorage) {}
  private tx<T>(fn: (storage: AuthorityTransaction) => Promise<T>): Promise<T> { return this.storage.transaction(fn); }

private static credentialKey(identity: CredentialIdentity): string { throw new Error("T8-W5 extraction pending"); }

async registerCredential(identity: CredentialIdentity): Promise<void> { throw new Error("T8-W5 extraction pending"); }

async requestCredentialRevocation(identity: CredentialIdentity): Promise<void> { throw new Error("T8-W5 extraction pending"); }

async revocationRequestedCredentials(cursor?: string): Promise<CredentialPage> { throw new Error("T8-W5 extraction pending"); }

async pendingCredentials(selection: CredentialSelection, cursor?: string, requestedStatus?: string): Promise<CredentialPage> { throw new Error("T8-W5 extraction pending"); }

async confirmCredentialRevoked(identity: CredentialIdentity): Promise<void> { throw new Error("T8-W5 extraction pending"); }
}
