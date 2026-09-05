function canonicalRepoLookupKey(value: string): string | null {
  const edgeTrim = (part: string) => part.replace(/^[\t\n\v\f\r ]+|[\t\n\v\f\r ]+$/g, "");
  const parts = edgeTrim(value).split("/");
  const canonical = parts.length === 2 ? `${edgeTrim(parts[0]).toLowerCase()}/${edgeTrim(parts[1]).toLowerCase()}` : "";
  return /^[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?\/[A-Za-z0-9](?:[A-Za-z0-9_.-]*[A-Za-z0-9])?$/.test(canonical) ? canonical : null;
}
function canonicalRepoMapEntries(map: Record<string, unknown>): Array<[string, unknown]> | null {
  const seen = new Set<string>();
  const entries: Array<[string, unknown]> = [];
  for (const [key, value] of Object.entries(map)) {
    const canonical = canonicalRepoLookupKey(key);
    if (canonical === null) continue;
    if (seen.has(canonical)) return null;
    seen.add(canonical);
    entries.push([canonical, value]);
  }
  return entries;
}

/**
 * Look up a repo's installation_id from the REPO_INSTALLATION_MAP JSON. A repo
 * webhook carries no `installation.id`; for known first-party repos we inject it
 * so the server-derived mint (#283) runs WARM. Returns "" when the map is
 * absent/malformed or the repo isn't listed (⇒ COLD, fail-open — never throws).
 */
export function installationIdForRepo(json: string | undefined, repoFullName: string): string {
  if (!json || !repoFullName) return "";
  try {
    const map = JSON.parse(json) as Record<string, unknown>;
    const wanted = canonicalRepoLookupKey(repoFullName);
    if (wanted === null) return "";
    const entries = canonicalRepoMapEntries(map);
    if (entries === null) return "";
    for (const [key, value] of entries) {
      if (key !== wanted) continue;
      return typeof value === "string" ? value : typeof value === "number" ? String(value) : "";
    }
    return "";
  } catch {
    return "";
  }
}

/**
 * Option-C per-tenant-PAT dispatch (server-confirmed live 2026-07-21). Look up a
 * repo in REPO_TENANT_PAT_MAP — a JSON `{ "<owner/repo>": "<SECRET_ENV_NAME>" }`
 * that maps a repo to the NAME of the secret binding holding that tenant's
 * acquiring PAT (the raw PAT is a Worker secret, never in this var). Returns the
 * secret NAME, or "" when the map is absent/malformed or the repo isn't listed
 * (⇒ default installation-derived mint). Never throws. The caller reads
 * `env[<name>]` to get the PAT, so a mapped-but-unbound secret still falls back to
 * the default path (no PAT ⇒ no Option-C).
 */
export function tenantPatSecretForRepo(json: string | undefined, repoFullName: string): string {
  if (!json || !repoFullName) return "";
  try {
    const map = JSON.parse(json) as Record<string, unknown>;
    const wanted = canonicalRepoLookupKey(repoFullName);
    if (wanted === null) return "";
    const entries = canonicalRepoMapEntries(map);
    if (entries === null) return "";
    for (const [key, value] of entries) {
      if (key === wanted && typeof value === "string") return value;
    }
    return "";
  } catch {
    return "";
  }
}
