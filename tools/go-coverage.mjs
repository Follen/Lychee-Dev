// Reports Go statement coverage and the evidence from the same test invocation.
// The caller supplies execution identity; this tool never runs tests or infers
// Lua, Node, business-scenario or real-client coverage from Go statements.
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const terminal = new Set(['pass', 'fail', 'skip']);
const actions = new Set(['start', 'run', 'pause', 'cont', 'output', 'pass', 'fail', 'skip', 'bench', 'attr', 'artifacts']);
const counts = () => ({ passed: 0, failed: 0, skipped: 0, incomplete: 0 });
const countKey = { pass: 'passed', fail: 'failed', skip: 'skipped' };
const digest = text => text === null ? null : createHash('sha256').update(text).digest('hex');
const percentage = (covered, total) => total === 0 ? null : Number((100 * covered / total).toFixed(4));
function add(a, b) {
  if (!Number.isSafeInteger(a + b)) throw new Error('statement count exceeds safe integer range');
  return a + b;
}

export function parseCoverProfile(text) {
  const lines = text.replace(/^\uFEFF/, '').split(/\r?\n/);
  if (lines.at(-1) === '') lines.pop();
  const mode = /^mode: (set|count|atomic)$/.exec(lines.shift() ?? '')?.[1];
  if (!mode) throw new Error('invalid or missing coverprofile mode');
  const blocks = new Map();
  let duplicates = 0;
  for (const [index, line] of lines.entries()) {
    const match = /^(.+\.go):(\d+)\.(\d+),(\d+)\.(\d+) (\d+) (\d+)$/.exec(line);
    if (!match) throw new Error(`invalid coverprofile line ${index + 2}`);
    const [, file, ...raw] = match;
    const [startLine, startCol, endLine, endCol, statements] = raw.slice(0, 5).map(Number);
    const positions = [startLine, startCol, endLine, endCol];
    if (positions.some(n => !Number.isSafeInteger(n) || n < 1) ||
        !Number.isSafeInteger(statements) || endLine < startLine ||
        (endLine === startLine && endCol < startCol)) throw new Error(`invalid coverprofile counts/positions at line ${index + 2}`);
    const hits = BigInt(raw[5]);
    if (hits > 9223372036854775807n || (mode === 'set' && hits > 1n)) throw new Error(`invalid coverprofile count at line ${index + 2}`);
    const importPath = file.slice(0, file.lastIndexOf('/'));
    if (!importPath || file.includes('\\') || file.startsWith('/') || /(?:^|\/)\.\.?(?:\/|$)/.test(file)) throw new Error(`invalid coverprofile source path: ${file}`);
    const key = `${file}:${positions.join(':')}`;
    const previous = blocks.get(key);
    if (previous) {
      if (previous.statements !== statements) throw new Error(`mismatched statement count for ${key}`);
      previous.covered ||= hits > 0n;
      duplicates++;
    } else blocks.set(key, { importPath, statements, covered: hits > 0n });
  }
  const packages = new Map();
  let statements = 0, coveredStatements = 0;
  for (const block of blocks.values()) {
    const pkg = packages.get(block.importPath) ?? { importPath: block.importPath, statements: 0, coveredStatements: 0, blocks: 0 };
    pkg.statements = add(pkg.statements, block.statements);
    statements = add(statements, block.statements);
    if (block.covered) {
      pkg.coveredStatements = add(pkg.coveredStatements, block.statements);
      coveredStatements = add(coveredStatements, block.statements);
    }
    pkg.blocks++;
    packages.set(block.importPath, pkg);
  }
  return { mode, statements, coveredStatements, percent: percentage(coveredStatements, statements),
    blocks: blocks.size, duplicateBlocks: duplicates,
    packages: [...packages.values()].sort((a, b) => a.importPath.localeCompare(b.importPath))
      .map(pkg => ({ ...pkg, percent: percentage(pkg.coveredStatements, pkg.statements) })) };
}

function validateIdentity(identity) {
  if (!identity || typeof identity !== 'object') throw new Error('identity must be an object');
  for (const key of ['testedCommit', 'testedTree', 'requestedPrHead']) {
    if (!/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(identity[key] ?? '')) throw new Error(`identity.${key} must be a full Git object ID`);
  }
  for (const key of ['goVersion', 'goos', 'goarch']) {
    if (typeof identity[key] !== 'string' || !identity[key].trim()) throw new Error(`identity.${key} is required`);
  }
  if (!Array.isArray(identity.command) || !identity.command.length || identity.command.some(v => typeof v !== 'string' || !v)) throw new Error('identity.command must be an argv array');
  if (identity.exitCode !== null && (!Number.isSafeInteger(identity.exitCode) || identity.exitCode < 0)) throw new Error('identity.exitCode must be a nonnegative integer or null');
  if (!Array.isArray(identity.packages) || !identity.packages.length) throw new Error('identity.packages must contain the selected go list packages');
  const seen = new Set();
  for (const pkg of identity.packages) {
    if (!pkg || typeof pkg.importPath !== 'string' || !pkg.importPath || seen.has(pkg.importPath) ||
        typeof pkg.hasTests !== 'boolean' || typeof pkg.instrumented !== 'boolean' ||
        (pkg.helper !== undefined && typeof pkg.helper !== 'boolean')) throw new Error('invalid or duplicate identity package');
    seen.add(pkg.importPath);
  }
  if (!identity.packages.some(p => p.instrumented)) throw new Error('identity has no instrumented packages');
}

export function parseTestEvents(text, selected) {
  const packages = new Map(selected.map(pkg => [pkg.importPath, { ...pkg, helper: pkg.helper ?? false,
    started: false, result: null, tests: new Map() }]));
  const errors = [], buildFailures = new Set();
  for (const [index, line] of (text ?? '').replace(/^\uFEFF/, '').split(/\r?\n/).entries()) {
    if (!line.trim()) continue;
    let event;
    try { event = JSON.parse(line); } catch { errors.push(`invalid test JSON at line ${index + 1}`); continue; }
    if (!event || typeof event !== 'object') { errors.push(`invalid test event at line ${index + 1}`); continue; }
    if (event.Action === 'build-output' || event.Action === 'build-fail') {
      if (typeof event.ImportPath !== 'string') errors.push(`invalid build event at line ${index + 1}`);
      if (event.Action === 'build-fail') buildFailures.add(event.ImportPath ?? '(unknown)');
      continue;
    }
    const pkg = packages.get(event.Package);
    if (!pkg || !actions.has(event.Action) || (event.Test !== undefined && (typeof event.Test !== 'string' || !event.Test))) {
      errors.push(`unexpected test event at line ${index + 1}`); continue;
    }
    if (pkg.result !== null) { errors.push(`event after package completion: ${event.Package}`); continue; }
    if (event.Action === 'start') {
      if (pkg.started || event.Test) errors.push(`duplicate or invalid package start: ${event.Package}`);
      pkg.started = true;
    } else if (!pkg.started) errors.push(`missing package start: ${event.Package}`);
    if (event.Test && (event.Action === 'run' || terminal.has(event.Action))) {
      const previous = pkg.tests.get(event.Test);
      if ((event.Action === 'run' && previous) || terminal.has(previous)) errors.push(`duplicate test result/run: ${event.Package}:${event.Test}`);
      if (terminal.has(event.Action) && !previous) errors.push(`missing test run: ${event.Package}:${event.Test}`);
      // Do not let a later passing event erase a recorded failure.
      if (previous !== 'fail') pkg.tests.set(event.Test, event.Action);
    } else if (!event.Test && terminal.has(event.Action)) pkg.result = event.Action;
  }
  const total = counts(), failed = [], skipped = [], incomplete = [];
  const results = [...packages.values()].map(pkg => {
    const tests = counts();
    for (const [name, result] of pkg.tests) {
      const key = countKey[result] ?? 'incomplete';
      tests[key]++; total[key]++;
      if (result === 'fail') failed.push({ package: pkg.importPath, test: name });
      if (result === 'skip') skipped.push({ package: pkg.importPath, test: name });
      if (!terminal.has(result)) incomplete.push({ package: pkg.importPath, test: name });
    }
    return { importPath: pkg.importPath, hasTests: pkg.hasTests, instrumented: pkg.instrumented, helper: pkg.helper,
      started: pkg.started, result: pkg.result, tests };
  });
  return { counting: 'Go test and subtest terminal events; skipped tests remain unrun', ...total,
    failedTests: failed, skippedTests: skipped, incompleteTests: incomplete,
    buildFailures: [...buildFailures], packages: results, errors };
}

export function createCoverageReport({ profile, events, identity }) {
  validateIdentity(identity);
  const tests = parseTestEvents(events, identity.packages);
  let coverage;
  try {
    if (profile === null) throw new Error('coverprofile missing');
    coverage = { state: 'valid', ...parseCoverProfile(profile) };
    const selected = new Set(identity.packages.filter(p => p.instrumented).map(p => p.importPath));
    for (const pkg of coverage.packages) {
      if (!selected.has(pkg.importPath)) throw new Error(`profile contains unselected package: ${pkg.importPath}`);
    }
    // Zero-statement packages can legitimately have no blocks. Keep them
    // visible without manufacturing a 0% row or an additional test gate.
    coverage.packagesWithoutBlocks = [...selected].filter(p => !coverage.packages.some(c => c.importPath === p));
  } catch (error) { coverage = { state: profile === null ? 'missing' : 'invalid', error: error.message }; }
  const failed = identity.exitCode !== null && identity.exitCode !== 0 || tests.failed > 0 || tests.buildFailures.length > 0 || tests.packages.some(p => p.result === 'fail');
  const incomplete = identity.exitCode === null || tests.incomplete > 0 || tests.packages.some(p => !p.started || p.result === null) ||
    coverage.state === 'missing';
  const invalid = tests.errors.length > 0 || coverage.state === 'invalid';
  const skipped = tests.skipped > 0 || tests.packages.some(p => p.hasTests && p.result === 'skip');
  const state = failed ? 'failed' : invalid ? 'invalid' : incomplete ? 'incomplete' : skipped ? 'passed_with_skips' : 'passed';
  const { packages, ...metadata } = identity;
  return { schema: 'lycheedev.go-coverage.v1', state,
    scope: 'Go statements only in the selected instrumented packages; no Lua, Node, business-scenario or real-client coverage claim',
    metadata: { ...metadata, testedRequestedHead: identity.testedCommit === identity.requestedPrHead },
    inputs: { profileSha256: digest(profile), eventsSha256: digest(events) }, coverage, tests };
}

export function main(argv = process.argv.slice(2)) {
  const opts = {};
  for (let i = 0; i < argv.length; i += 2) {
    const key = argv[i], value = argv[i + 1];
    if (!['--profile', '--events', '--identity', '--out'].includes(key) || key in opts || !value || value.startsWith('--')) throw new Error('usage: node tools/go-coverage.mjs --profile <cover.out> --events <go-test.jsonl> --identity <identity.json> --out <summary.json>');
    opts[key] = value;
  }
  if (Object.keys(opts).length !== 4) throw new Error('--profile, --events, --identity and --out are required');
  const input = path => {
    try { return readFileSync(path, 'utf8'); } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
  };
  const report = createCoverageReport({ profile: input(opts['--profile']), events: input(opts['--events']),
    identity: JSON.parse(readFileSync(opts['--identity'], 'utf8').replace(/^\uFEFF/, '')) });
  report.inputs = { ...report.inputs, profile: opts['--profile'], events: opts['--events'], identity: opts['--identity'] };
  const json = `${JSON.stringify(report, null, 2)}\n`;
  mkdirSync(dirname(resolve(opts['--out'])), { recursive: true });
  writeFileSync(opts['--out'], json);
  console.log(JSON.stringify(report));
  return ['passed', 'passed_with_skips'].includes(report.state) ? 0 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { process.exitCode = main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
