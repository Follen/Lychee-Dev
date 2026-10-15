// Execute only as the controller owned by tools/rust-source-full-corpus.mjs.
// Inputs are read as bytes; the corpus and its configuration are never executed.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {readFileSync, writeFileSync, existsSync} from 'node:fs';
import {join, resolve} from 'node:path';
import os from 'node:os';
import {beginMemoryPhase, endMemoryPhase, staticMemoryPhase} from '../../tools/rust-owned-memory.mjs';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {assertAnchor, assertCorpus, assertEnvelope, assertInspect, assertPageContinuation, assertQuery, assertStaticCall, sha256} from './assertions.mjs';

const [binaryArg, homeArg, evidenceArg, ...extra] = process.argv.slice(2);
if (!binaryArg || !homeArg || !evidenceArg || extra.length) throw new Error('Owned controller requires binary, fresh home, evidence directory');
const root = resolve('.'), binary = resolve(binaryArg), home = resolve(homeArg), out = resolve(evidenceArg);
if (existsSync(home)) throw new Error('Fresh source home required');
const oracle = JSON.parse(readFileSync(new URL('./retail-oracle.json', import.meta.url)));
const report = {
  schema: 'lycheedev.source-full-corpus.v1', state: 'running', startedAt: new Date().toISOString(),
  oracle, binary: {path: binary, sha256: sha256(readFileSync(binary))},
  machine: {platform: process.platform, release: os.release(), arch: process.arch, node: process.version, cpu: os.cpus()[0]?.model, cores: os.cpus().length, totalMemory: os.totalmem()},
  home, checks: [], commands: [],
  performance: {state: 'not_evaluated', targetRatio: 0.30, reason: 'Correctness run only; no same-machine Go sample set or controlled cold/warm repeats. CLI source sync includes inseparable network/local validation and is recorded separately, never subtracted as pure download time.', excludedDownloadMillis: null, cpu: 'not_measured', peakRSS: 'not_measured', io: 'not_measured', windowsMemory: 'outer owned Job observation; see owned-report.json; peakRSS remains unmeasured'},
  remaining: ['full semantic refs/context/flow', 'continuous retained native session and close', 'same-machine fixed Go 3.2.0 differential', 'independent cold/warm 20-repeat performance groups'],
};
const save = () => writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
save();
let sequence = 0, pin, fatalProcessFailure = false;
function invoke(caseId, args, {timeout = 210000, phase = 'local', cacheMode = 'retained-derived', expectedExit = 0} = {}) {
  if (fatalProcessFailure) throw new Error('Prior process failure requires outer Job retirement; no further child launched');
  const index = String(sequence++).padStart(3, '0'), fullArgs = [...args, '--home', home, '--format', 'json'];
  const startedAt = new Date().toISOString(), started = performance.now();
  const memoryPhase = beginMemoryPhase(staticMemoryPhase(caseId), caseId);
  const run = spawnSync(binary, fullArgs, {cwd: root, env: {...process.env}, windowsHide: true, shell: false, timeout, maxBuffer: 16 << 20});
  const wallMillis = performance.now() - started;
  endMemoryPhase(memoryPhase);
  const stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0);
  const stdoutPath = join(out, `${index}-${caseId}.stdout.json`), stderrPath = join(out, `${index}-${caseId}.stderr.bin`);
  writeFileSync(stdoutPath, stdout); writeFileSync(stderrPath, stderr);
  let envelope; try { envelope = JSON.parse(stdout.toString('utf8')); } catch {}
  const observation = {caseId, pid: run.pid, memoryPhase, args: fullArgs, startedAt, endedAt: new Date().toISOString(), phase, cacheMode, wallMillis, exit: run.status, signal: run.signal, error: run.error?.message, stdout: stdoutPath, stderr: stderrPath, stdoutSHA256: sha256(stdout), stderrSHA256: sha256(stderr), stdoutBytes: stdout.length, stderrBytes: stderr.length};
  if (run.error || run.signal) fatalProcessFailure = true;
  report.commands.push(observation); save();
  const result = assertEnvelope({...observation, envelope}, expectedExit);
  return {result, envelope, observation};
}
function check(caseId, work) {
  if (fatalProcessFailure) {
    report.checks.push({caseId, state: 'not_run', reason: 'Prior process failure requires outer Job retirement; no further child launched', causedBy: report.commands.find(row => row.error || row.signal)?.caseId ?? 'authenticate-every-original-file'});
    save(); return;
  }
  const started = performance.now();
  try { const detail = work(); report.checks.push({caseId, state: 'passed', wallMillis: performance.now() - started, ...detail}); }
  catch (error) { report.checks.push({caseId, state: 'failed', wallMillis: performance.now() - started, reason: error.message}); }
  save();
}
function fixed(args) { assert(pin, 'fixed pin unavailable'); return [...args, '--snapshot', pin]; }

// This Git audit is independent of the CLI's index and verifies every original
// byte, rather than trusting only the CLI's document count or a small fixture.
const gitCalls = [];
function auditGit(args, input) {
  const mirror = join(home, 'mirrors', `${oracle.repository}.git`);
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE'));
  Object.assign(env, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: process.platform === 'win32' ? 'NUL' : '/dev/null', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1'});
  const started = performance.now();
  const run = spawnSync('git', ['--no-optional-locks', '--git-dir', mirror, ...args], {input, cwd: root, env, timeout: 60000, maxBuffer: 16 << 20, windowsHide: true, shell: false});
  gitCalls.push({args, exit: run.status, error: run.error?.message, stderr: run.stderr?.toString('utf8'), wallMillis: performance.now() - started, bytes: run.stdout?.length, stdoutSHA256: sha256(run.stdout ?? Buffer.alloc(0))});
  writeFileSync(join(out, 'git-audit-calls.json'), JSON.stringify(gitCalls, null, 2) + '\n');
  if (run.error || run.signal) fatalProcessFailure = true;
  assert(!run.error && run.status === 0, `Independent pinned Git audit failed: ${run.error?.message ?? run.stderr}`);
  return run.stdout;
}
function auditCorpus() {
  const commit = auditGit(['cat-file', 'commit', oracle.commit]);
  assert.equal(createHash('sha1').update(`commit ${commit.length}\0`).update(commit).digest('hex'), oracle.commit);
  writeFileSync(join(out, 'git-commit.bin'), commit);
  const tree = auditGit(['ls-tree', '-r', '-l', '-z', '--full-tree', oracle.commit]);
  writeFileSync(join(out, 'git-tree.bin'), tree);
  const rows = tree.toString('utf8').split('\0').filter(Boolean).map(entry => {
    const match = /^(100644|100755) blob ([0-9a-f]{40})\s+(\d+)\t([^\0]+)$/.exec(entry);
    assert(match, 'Only ordinary tracked files expected in fixed corpus');
    return {path: match[4], bytes: Number(match[3]), gitSHA1: match[2]};
  });
  assert.equal(rows.length, oracle.allFiles, 'full Git tree file count');
  for (let offset = 0; offset < rows.length; offset += 32) {
    const batch = rows.slice(offset, offset + 32);
    const bytes = auditGit(['cat-file', '--batch'], batch.map(row => row.gitSHA1 + '\n').join(''));
    let cursor = 0;
    for (const row of batch) {
      const end = bytes.indexOf(10, cursor); assert(end >= cursor, 'Git batch header');
      assert.equal(bytes.subarray(cursor, end).toString('ascii'), `${row.gitSHA1} blob ${row.bytes}`);
      const raw = bytes.subarray(end + 1, end + 1 + row.bytes);
      assert.equal(raw.length, row.bytes); assert.equal(bytes[end + 1 + row.bytes], 10);
      assert.equal(createHash('sha1').update(`blob ${row.bytes}\0`).update(raw).digest('hex'), row.gitSHA1, 'Git blob object hash');
      row.sha256 = sha256(raw); cursor = end + row.bytes + 2;
    }
    assert.equal(cursor, bytes.length, 'no surplus batch bytes');
  }
  assertCorpus(rows, oracle);
  writeFileSync(join(out, 'verified-corpus-manifest.json'), JSON.stringify({schema: 'lycheedev.verified-full-corpus.v1', commit: oracle.commit, rootTree: /^tree ([0-9a-f]{40})/m.exec(commit.toString())?.[1], files: rows}, null, 2) + '\n');
  return {files: rows.length, documents: oracle.documents, bytes: oracle.documentBytes};
}

try {
  check('init-fresh-home', () => { invoke('init', ['init'], {cacheMode: 'fresh-home'}); });
  check('discover-fixed-repository', () => {
    const {result} = invoke('catalog', ['source', 'list']);
    const repository = result.find(row => row.key === oracle.repository);
    assert.equal(repository?.url, oracle.url); assert.equal(repository.tracks?.retail, 'live');
  });
  check('sync-exact-retail-commit', () => {
    const {result, observation} = invoke('sync', ['source', 'sync', '--source', oracle.repository, '--product', oracle.product, '--ref', oracle.commit], {phase: 'input-preparation-network-and-local', cacheMode: 'fresh-home'});
    assert.equal(result.source?.exactCommit, oracle.commit);
    assert.equal(result.source?.requestedRef, oracle.commit);
    assert.equal(result.source?.repository, oracle.repository);
    assert.equal(result.source?.product, oracle.product);
    assert.match(result.id, /^PIN-/); pin = result.id; report.pin = pin;
    report.performance.inputPreparationWallMillis = observation.wallMillis;
    return {pin, source: result.source};
  });
  if (!pin) throw new Error('Fixed source sync did not yield authenticated requested pin; dependent checks not_run');
  check('authenticate-every-original-file', auditCorpus);
  check('fresh-derived-readiness', () => {
    const {result} = invoke('readiness-before', fixed(['source', 'list']), {cacheMode: 'derived-cold'});
    assert.equal(result.ready, false); assert.equal(result.snapshotFiles, 0);
  });
  check('full-index-derived-cold', () => {
    const {result} = invoke('index-cold', fixed(['source', 'index']), {cacheMode: 'derived-cold'});
    for (const [key, value] of Object.entries({repository: oracle.repository, product: oracle.product, commit: oracle.commit, documents: oracle.documents, skippedDocuments: 0})) assert.equal(result[key], value, key);
    assert.equal(result.complete, true, 'full source map must report complete; preserve any real parser gaps as failures');
    return {documents: result.documents, diagnostics: result.diagnostics, astFiles: result.astFiles};
  });
  check('full-index-derived-warm', () => {
    const {result} = invoke('index-warm', fixed(['source', 'index']));
    assert.equal(result.documents, oracle.documents); assert.equal(result.complete, true);
    const status = invoke('readiness-after', fixed(['source', 'list'])).result;
    assert.equal(status.ready, true); assert.equal(status.snapshotFiles, oracle.documents); assert.equal(status.coverage?.analyzedDocuments, oracle.documents);
  });
  for (const anchor of oracle.anchors) {
    const id = anchor.symbol.replace(/[^a-zA-Z0-9]/g, '-');
    check(`inspect-${id}`, () => assertInspect(invoke(`inspect-${id}`, fixed(['source', 'inspect', '--path', anchor.path, '--line', String(anchor.line), '--count', String(anchor.count)])).result, anchor, oracle));
  }
  const symbols = new Map();
  for (const anchor of oracle.anchors.slice(0, 2)) {
    check(`query-${anchor.symbol}`, () => {
      const args = fixed(['source', 'query', anchor.symbol, '--mode', 'precise', '--topic', 'lua', '--limit', '10']);
      const first = invoke(`query-${anchor.symbol}-first`, args).result;
      const row = assertAnchor(first, anchor, oracle, pin); symbols.set(anchor.symbol, row);
      const second = invoke(`query-${anchor.symbol}-repeat`, args).result;
      assertAnchor(second, anchor, oracle, pin); assert.deepEqual(second.results, first.results, 'retained query result identity');
    });
  }
  check('query-generated-api', () => {
    const anchor = oracle.anchors[2];
    assertAnchor(invoke('query-api', fixed(['source', 'query', anchor.symbol, '--topic', 'api', '--limit', '10'])).result, anchor, oracle, pin);
  });
  for (const [caseId, term] of [['query-no-result', 'LYCHEEDEV_NoSuchSymbol_31c7f7b_FullCorpus'], ['query-chinese-utf8', oracle.chineseQuery.text]]) {
    check(caseId, () => {
      const result = assertQuery(invoke(caseId, fixed(['source', 'query', term, '--limit', '10'])).result, oracle, pin);
      assert.deepEqual(result.results, []); assert.equal(result.truncated, false); assert.equal(result.complete, true);
      assert(Array.isArray(result.suggestions) && result.suggestions.length > 0, 'actionable no-result suggestions');
      return caseId.includes('chinese') ? {scope: oracle.chineseQuery.basis} : {};
    });
  }
  check('query-large-result-pages', () => {
    const args = fixed(['source', 'query', 'Cooldown', '--mode', 'exploratory', '--topic', 'lua', '--limit', '3']);
    const first = assertQuery(invoke('page-1', args).result, oracle, pin);
    assert.equal(typeof first.nextCursor, 'string');
    const second = assertQuery(invoke('page-2', [...args, '--cursor', first.nextCursor]).result, oracle, pin);
    assertPageContinuation(first, second, 3);
    const error = invoke('page-wrong-query', fixed(['source', 'query', 'GetActionButtonForID', '--mode', 'exploratory', '--topic', 'lua', '--limit', '3', '--cursor', first.nextCursor]), {expectedExit: 2}).result;
    assert.equal(error.code, 'codebase.invalid_search_cursor');
    return {firstMatches: first.matchCoverage, secondMatches: second.matchCoverage};
  });
  check('inspect-out-of-range-error', () => {
    const anchor = oracle.anchors[0];
    const result = invoke('inspect-bad-line', fixed(['source', 'inspect', '--path', anchor.path, '--line', String(anchor.totalLines + 1), '--count', '1']), {expectedExit: 2}).result;
    assert.equal(result.code, 'codebase.line_out_of_range');
  });
  check('query-invalid-limit-error', () => { invoke('query-bad-limit', fixed(['source', 'query', 'Cooldown', '--limit', '201']), {expectedExit: 2}); });
  check('refs-static-original-callsite', () => {
    const anchor = oracle.anchors[0], symbol = symbols.get(anchor.symbol); assert(symbol, 'query symbol identity unavailable');
    const {result} = invoke('refs-static', fixed(['source', 'refs', '--symbol-id', symbol.symbolId, '--direction', 'incoming', '--limit', '25', '--static-only']));
    assert.equal(result.commit, oracle.commit); assert.equal(result.symbol?.id, symbol.symbolId);
    assertStaticCall(result, oracle.staticCall);
    assert.equal(result.coverage?.semantic, 'unavailable', 'static scope remains explicit');
    assert.equal(result.complete, false, 'static evidence cannot claim complete semantics');
    return {coverage: result.coverage, returned: result.relations.length};
  });
  check('context-static-original-snippet', () => {
    const anchor = oracle.anchors[0], symbol = symbols.get(anchor.symbol); assert(symbol, 'query symbol identity unavailable');
    const {result} = invoke('context-static', fixed(['source', 'context', '--symbol-id', symbol.symbolId, '--depth', '0', '--limit', '5', '--max-lines', '80', '--max-bytes', '16384', '--static-only']));
    assert.equal(result.commit, oracle.commit); assert.equal(result.symbol?.id, symbol.symbolId);
    assert(result.snippets?.some(row => row.path === anchor.path && row.text?.includes(anchor.declarationLine)), 'context returns original declaration source');
    assert.equal(result.coverage?.semantic, 'unavailable');
    assert.equal(result.complete, false);
    return {coverage: result.coverage, snippets: result.snippets.length, truncated: result.truncated};
  });
  check('query-after-errors', () => {
    const anchor = oracle.anchors[0];
    assertAnchor(invoke('query-after-errors', fixed(['source', 'query', anchor.symbol, '--topic', 'lua', '--limit', '10'])).result, anchor, oracle, pin);
  });
} catch (error) {
  report.blocker = error.message;
} finally {
  report.finishedAt = new Date().toISOString();
  report.state = !report.blocker && report.checks.every(row => row.state === 'passed') ? 'passed_selected_static_cases' : 'failed';
  report.completeSourceAndSkillAcceptance = false;
  save();
}
console.log(JSON.stringify({state: report.state, report: join(out, 'report.json'), passed: report.checks.filter(row => row.state === 'passed').length, failed: report.checks.filter(row => row.state === 'failed').length}));
if (report.state !== 'passed_selected_static_cases') process.exitCode = 1;
