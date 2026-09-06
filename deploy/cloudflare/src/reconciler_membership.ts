import { installationToken, type GithubAppEnv } from "./github_app.js";

export type MembershipCandidate = { repo: string; installationId: string };
export type ConfirmedMembership = { repo: string; installationId: string };

const API = "https://api.github.com/installation/repositories";
const PAGE_SIZE = 100;
const MAX_PAGES = 100;
const MAX_BODY = 256 * 1024;
const TIMEOUT_MS = 5_000;

function validCandidate(value: MembershipCandidate): boolean {
  return typeof value.repo === "string" && value.repo.length > 0 && value.repo.length <= 256 && value.repo === value.repo.trim() &&
    typeof value.installationId === "string" && value.installationId.length > 0 && value.installationId.length <= 128 && value.installationId === value.installationId.trim();
}

async function boundedBody(response: Response): Promise<unknown> {
  if (!response.body) {
    const text = await response.text();
    if (new TextEncoder().encode(text).byteLength > MAX_BODY) throw new Error("response too large");
    return JSON.parse(text);
  }
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    for (;;) {
      const part = await reader.read();
      if (part.done) break;
      total += part.value.byteLength;
      if (total > MAX_BODY) throw new Error("response too large");
      chunks.push(part.value);
    }
  } finally { reader.releaseLock(); }
  const body = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) { body.set(chunk, offset); offset += chunk.byteLength; }
  return JSON.parse(new TextDecoder().decode(body));
}

async function requestPage(fetcher: typeof fetch, token: string, page: number): Promise<{ body: unknown; nextPage: number | null }> {
  const controller = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const work = (async () => {
      const response = await fetcher(`${API}?per_page=${PAGE_SIZE}&page=${page}`, {
        method: "GET",
        redirect: "error",
        signal: controller.signal,
        headers: { authorization: `Bearer ${token}`, accept: "application/vnd.github+json", "user-agent": "corelink-spawn-worker", "x-github-api-version": "2022-11-28" },
      });
      if (response.status !== 200) throw new Error("membership request failed");
      const link = response.headers.get("link");
      let nextPage: number | null = null;
      if (link) {
        for (const entry of link.split(",")) {
          if (!/;\s*rel="?next"?/i.test(entry)) continue;
          const match = /^\s*<([^>]+)>\s*;\s*rel="?next"?\s*$/i.exec(entry);
          if (!match) throw new Error("invalid next link");
          let url: URL;
          try { url = new URL(match[1]); } catch { throw new Error("invalid next link"); }
          if (url.origin !== "https://api.github.com" || url.pathname !== "/installation/repositories") throw new Error("invalid next link");
          const raw = url.searchParams.get("page");
          if (!raw || !/^[1-9][0-9]*$/.test(raw)) throw new Error("invalid next page");
          const candidate = Number(raw);
          if (!Number.isSafeInteger(candidate) || candidate <= page || candidate > MAX_PAGES) throw new Error("invalid next page");
          if (nextPage !== null && nextPage !== candidate) throw new Error("conflicting next links");
          nextPage = candidate;
        }
      }
      return { body: await boundedBody(response), nextPage };
    })();
    return await Promise.race([work, new Promise<never>((_, reject) => { timer = setTimeout(() => { controller.abort(); reject(new Error("membership timeout")); }, TIMEOUT_MS); })]);
  } finally { if (timer !== undefined) clearTimeout(timer); }
}

export async function confirmInstallationRepositories(
  env: GithubAppEnv,
  candidates: ReadonlyArray<MembershipCandidate>,
  nowMs: number,
  fetcher: typeof fetch = fetch,
): Promise<Array<ConfirmedMembership> | null> {
  if (!Array.isArray(candidates) || candidates.length > 10_000 || candidates.some((candidate) => !validCandidate(candidate))) return null;
  if (candidates.length === 0) return [];
  const grouped = new Map<string, MembershipCandidate[]>();
  for (const candidate of candidates) grouped.set(candidate.installationId, [...(grouped.get(candidate.installationId) ?? []), candidate]);
  const found = new Map<string, Set<string>>();
  try {
    for (const installationId of [...grouped.keys()].sort()) {
      const token = (await installationToken(env, installationId, nowMs)).token;
      if (typeof token !== "string" || token.length === 0) return null;
      const names = new Set<string>();
      let declaredTotal: number | null = null;
      let complete = false;
      for (let page = 1; page <= MAX_PAGES; page++) {
        const pageResult = await requestPage(fetcher, token, page);
        const body = pageResult.body;
        if (body === null || typeof body !== "object" || Array.isArray(body)) return null;
        const repositories = (body as Record<string, unknown>).repositories;
        const totalCount = (body as Record<string, unknown>).total_count;
        if (!Number.isSafeInteger(totalCount) || (totalCount as number) < 0 || !Array.isArray(repositories) || repositories.length > PAGE_SIZE) return null;
        if (declaredTotal === null) declaredTotal = totalCount as number;
        else if (declaredTotal !== totalCount) return null;
        for (const repository of repositories) {
          if (repository === null || typeof repository !== "object" || typeof (repository as Record<string, unknown>).full_name !== "string") return null;
          names.add((repository as { full_name: string }).full_name.toLowerCase());
        }
        if (pageResult.nextPage !== null) { page = pageResult.nextPage - 1; continue; }
        if (names.size > totalCount) return null;
        if (names.size !== totalCount) return null;
        complete = true;
        break;
      }
      if (!complete) return null;
      found.set(installationId, names);
    }
  } catch { return null; }
  const result: ConfirmedMembership[] = [];
  const emitted = new Set<string>();
  for (const candidate of candidates) {
    if (!found.get(candidate.installationId)?.has(candidate.repo.toLowerCase())) continue;
    const key = `${candidate.installationId}\u0000${candidate.repo}`;
    if (!emitted.has(key)) { emitted.add(key); result.push({ repo: candidate.repo, installationId: candidate.installationId }); }
  }
  return result;
}
