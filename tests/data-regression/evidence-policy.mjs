export const packageName = 'github.com/follenfang/lycheedev/internal/records/navigatetest';
export const oracleCommit = '38a34665624b8775bb875274b36191b21c38d97b';
export const benchmarkCases = ['offline-cdn', 'local-archive'].flatMap(physical =>
  ['single', 'join', 'static-effective', 'hotfix-delete', 'spell-navigation'].flatMap(scenario =>
    ['cold-decoded', 'warm-decoded'].map(state => `BenchmarkDataPreparation/${physical}/${scenario}/${state}`)));
const commonMetrics = ['ns/op', 'B/op', 'allocs/op', 'network-B/op', 'requests/op', 'charged-work/op',
  'charged-retained-B/op', 'charged-metadata-B/op'];
const sqlMetrics = ['prepare-ms/op', 'definitions-ms/op', 'execute-ms/op', 'source-B/op', 'decoded-hits/op',
  'decoded-reused-B/op', 'content-hashes/op', 'content-hash-B/op', 'content-decodes/op', 'extracted-B/op',
  'verified-blob-reads/op', 'verified-blob-B/op', 'fragment-loads/op', 'block-hits/op'];
export const oracleTests = ['TestCascLibRawFixtureOracle', ...['headered-N', 'headered-Z', 'checksum-failure',
  'missing-key-no-zero-fill'].map(name => `TestCascLibRawFixtureOracle/${name}`)];
const failed = reason => ({state: 'failed', reason});

export function assessEvidence({mode, text, exitCode, count = 1, comparison}) {
  if (exitCode !== 0) return failed('process did not exit successfully');
  if (mode === 'storage-oracle') {
    if (comparison?.schema !== 'lycheedev.test.casc-comparison.v1' || comparison.state !== 'verified' ||
        comparison.scope !== 'pinned-storage-file' || comparison.commit !== oracleCommit || comparison.complete !== true ||
        !/^[a-f0-9]{64}$/.test(comparison.sha256 ?? '') || !/^[a-f0-9]{32}$/.test(comparison.contentKey ?? '') ||
        !Number.isSafeInteger(comparison.bytes) || comparison.bytes < 0 || comparison.bytes > 512 * 1024 * 1024 ||
        !Number.isSafeInteger(comparison.fileDataID) || comparison.fileDataID < 1 ||
        !Number.isSafeInteger(comparison.localeMask) || comparison.localeMask < 1 ||
        !['standard', 'low-violence'].includes(comparison.contentVariant) || !comparison.pin || !comparison.oracle) {
      return failed('verified storage comparison artifact missing or incomplete');
    }
    return {state: 'verified'};
  }
  let events;
  try {
    events = text.split(/\r?\n/).filter(line => line.trim()).map(line => JSON.parse(line));
    if (events.some(event => !event || typeof event !== 'object' || Array.isArray(event) ||
        typeof event.Action !== 'string' || typeof event.Package !== 'string' ||
        event.Test !== undefined && typeof event.Test !== 'string' ||
        event.Output !== undefined && typeof event.Output !== 'string')) throw new Error('invalid event');
  }
  catch { return failed('invalid Go JSONL evidence'); }
  const relevant = events.filter(event => event.Package === packageName);
  if (relevant.some(event => event.Action === 'fail')) return failed('Go package or test failed');
  if (relevant.some(event => event.Action === 'skip')) return {state: 'not_run', reason: 'required evidence was skipped'};
  if (!relevant.some(event => event.Action === 'pass' && !event.Test)) return failed('Go package pass missing');
  if (mode === 'fixture-oracle') {
    const passed = new Set(relevant.filter(event => event.Action === 'pass').map(event => event.Test));
    const missing = oracleTests.filter(name => !passed.has(name));
    return missing.length ? failed(`required oracle passes missing: ${missing.join(', ')}`) : {state: 'verified'};
  }
  if (mode !== 'benchmark' || !Number.isSafeInteger(count) || count < 1) return failed('invalid benchmark count or mode');
  const results = new Map(benchmarkCases.map(name => [name, 0]));
  for (const event of relevant.filter(event => event.Action === 'output')) {
    for (const line of (event.Output ?? '').split('\n').filter(line => line.includes(' ns/op'))) {
      const name = (event.Test ?? '').replace(/-\d+$/, '');
      if (!results.has(name)) return failed(`unexpected benchmark result: ${name}`);
      const tokens = line.trim().split(/\s+/);
      if (tokens[0]?.startsWith('Benchmark')) tokens.shift();
      const iterations = Number(tokens.shift());
      if (!Number.isSafeInteger(iterations) || iterations < 1 || tokens.length % 2) return failed(`invalid benchmark metrics: ${name}`);
      const metrics = new Set();
      for (let i = 0; i < tokens.length; i += 2) {
        const value = Number(tokens[i]), unit = tokens[i + 1];
        if (!Number.isFinite(value) || value < 0 || metrics.has(unit)) return failed(`invalid benchmark metric: ${name}`);
        metrics.add(unit);
      }
      const required = name.includes('/spell-navigation/') ? commonMetrics : [...commonMetrics, ...sqlMetrics];
      if (required.some(unit => !metrics.has(unit))) return failed(`required benchmark metrics missing: ${name}`);
      results.set(name, results.get(name) + 1);
    }
  }
  const incomplete = [...results].filter(([, actual]) => actual !== count).map(([name, actual]) => `${name} (${actual}/${count})`);
  return incomplete.length ? failed(`benchmark results incomplete: ${incomplete.join(', ')}`) : {state: 'verified'};
}
