import { test } from 'node:test';
import assert from 'node:assert/strict';
import { expiredRuns, pruneRuns } from './ci-retention.mjs';

const runs = Array.from({ length: 30 }, (_, i) => ({ id: i + 1, created_at: new Date(1_700_000_000_000 + i * 1000).toISOString(), status: 'completed' }));
test('retains newest 25 across workflows, skips active/protected runs, deduplicates pages', () => {
  const input = [...runs, runs[0]].map((r) => r.id === 2 ? { ...r, status: 'in_progress' } : r);
  assert.deepEqual(expiredRuns(input.reverse(), 25, [3]).map((r) => r.id), [5, 4, 1]);
  assert.throws(() => expiredRuns(runs, 0));
});
test('dry run never mutates; apply rechecks each old run and leaves release assets/caches alone', async () => {
  const calls = [];
  const request = async (url, { method }) => {
    const path = new URL(url).pathname;
    calls.push({ path, method });
    if (url.includes('?')) return Response.json({ workflow_runs: runs });
    if (method === 'DELETE') return new Response(null, { status: 204 });
    return Response.json({ status: path.endsWith('/2') ? 'in_progress' : 'completed' });
  };
  const options = { repository: 'example/project', token: 'synthetic', request };
  assert.equal((await pruneRuns(options)).selected.length, 5);
  assert.equal(calls.length, 1);
  calls.length = 0;
  assert.deepEqual((await pruneRuns({ ...options, dryRun: false })).selected, [5, 4, 3, 1]);
  assert.equal(calls.filter((c) => c.method === 'DELETE').length, 4);
  assert.ok(calls.every((c) => c.path.includes('/actions/runs')));
});
