/** Stable stash lease identity for one exact minted credential. */
export function runnerCredentialLeaseId(jobId: string, tenant: string, patId: string): string {
  return `runner-pat:v1:${encodeURIComponent(jobId)}:${encodeURIComponent(tenant)}:${encodeURIComponent(patId)}`;
}
