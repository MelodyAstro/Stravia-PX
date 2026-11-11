import { pathToFileURL } from 'node:url';

export function expiredRuns(runs, keep = 25, protectedIds = []) {
  if (!Number.isSafeInteger(keep) || keep < 1) throw new Error('Invalid retention count');
  const unique = [...new Map(runs.map((run) => [String(run.id), run])).values()];
  unique.sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at) || b.id - a.id);
  const protectedSet = new Set(protectedIds.map(String));
  return unique.slice(keep).filter((run) => run.status === 'completed' && !protectedSet.has(String(run.id)));
}

export async function pruneRuns({ repository, token, currentRun, dryRun = true, request = fetch }) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository) || !token) throw new Error('Repository and token are required');
  const api = async (suffix, method = 'GET') => {
    const response = await request(`https://api.github.com/repos/${repository}${suffix}`, {
      method,
      headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
      signal: AbortSignal.timeout(30_000),
    });
    if (!response.ok) throw new Error(`GitHub ${method} failed: HTTP ${response.status}`);
    return response.status === 204 ? null : response.json();
  };
  const runs = [];
  for (let page = 1; ; page++) {
    const result = await api(`/actions/runs?per_page=100&page=${page}`);
    runs.push(...result.workflow_runs);
    if (result.workflow_runs.length < 100) break;
    if (page >= 1000) throw new Error('Run pagination exceeded safety limit');
  }
  const candidates = expiredRuns(runs, 25, [currentRun]);
  const deleted = [];
  for (const run of candidates) {
    // Recheck state immediately before deleting; active jobs must never be removed.
    if (!dryRun) {
      const latest = await api(`/actions/runs/${run.id}`);
      if (latest.status !== 'completed') continue;
      await api(`/actions/runs/${run.id}`, 'DELETE');
    }
    deleted.push(run.id);
  }
  return { keep: 25, dry_run: dryRun, observed: new Set(runs.map((run) => run.id)).size, selected: deleted };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const result = await pruneRuns({ repository: process.env.GITHUB_REPOSITORY, token: process.env.GH_TOKEN, currentRun: process.env.GITHUB_RUN_ID, dryRun: process.env.RETENTION_APPLY !== 'true' });
  console.log(JSON.stringify(result));
}
