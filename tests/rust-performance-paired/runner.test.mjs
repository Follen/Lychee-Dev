// Synthetic runner contract tests. No product, benchmark or Windows pass claim.
import test from 'node:test';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join} from 'node:path';
import {runInNewContext} from 'node:vm';
import {CASES, GO_COMMIT, GO_VERSION, REPETITIONS, compareSamples, elapsed, orderFor, semantic, startupBreakdown, statistics, summarize} from './contract.mjs';
import {LUALS, assertSeedCold, cloneSeed, derivedIdentity, inventory, proveWarmReuse, safeRelative, verifyGoMetadata, verifyGoPackage, verifySeedMetadata} from './inputs.mjs';
import {DatabaseSync} from 'node:sqlite';
import {APPROVED_ENGINE_SUMMARIES} from '../rust-parity/describe-contract.mjs';
import {ownedUserEnvironment} from '../../tools/rust-build.mjs';
const oracle = JSON.parse(readFileSync(new URL('../rust-source-full-corpus/retail-oracle.json', import.meta.url)));
const pin = 'PIN-paired', anchor = oracle.anchors[0];
const envelope = result => ({schema: 'lycheedev.result.v1', ok: true, error: null, result});
function query() {
  return envelope({sourceId: oracle.repository, product: oracle.product, requestedRef: oracle.commit, matchedTag: null, resolvedCommit: oracle.commit, snapshotId: pin, complete: true, truncated: false,
    coverage: {analyzedDocuments: oracle.documents, skippedDocuments: 0, diagnostics: 0, complete: true},
    results: [{symbolId: 'SYM-1', kind: 'function', name: anchor.symbol, path: anchor.path, matchedBy: 'exact_symbol', role: 'declaration', excerpt: anchor.declarationLine, contentHash: anchor.sha256, line: anchor.line, score: 100, scoreParts: {exact: 100}}]});
}
function index() { return envelope({repository: oracle.repository, product: oracle.product, commit: oracle.commit, documents: oracle.documents, declarations: 22, relationships: 11, assets: 9, diagnostics: 0, complete: true, parser: 'source-v1', storage: 'file-cache', schema: 'lycheedev.source-map.v2'}); }
function proof() { return {schema: 'lycheedev.owned-process.v1', rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false, timing: {schema: 'lycheedev.owned-timing.v1', frequency: 10000000, startTicks: '9007199254740993000', rootExitTicks: '9007199254741993000', treeReapedTicks: '9007199254742993000'}}; }
function pairs() { return CASES.flatMap(({id}) => Array.from({length: REPETITIONS}, (_, iteration) => ['cold', 'warm'].map(cacheMode => ({caseId: id, iteration, cacheMode, order: orderFor(iteration), resultEquivalent: true, ...Object.fromEntries(['go', 'rust'].map(engine => [engine, {valid: true, home: `${id}/${iteration}/${engine}`, binary: engine, snapshot: pin, warmReuse: {verified: true}, controllableMillis: engine === 'go' ? 100 : 30}]))}))).flat()); }
test('fixed oracle, workload, exact pair count and 30% target cannot drift to historical five/50%', () => {
  assert.equal(GO_COMMIT, '29bdbdaff5f064ec212ff2064380b1ccac652eda'); assert.equal(GO_VERSION, '3.2.4'); assert.equal(REPETITIONS, 20);
  assert.equal(CASES.length, 3); assert.equal(oracle.documents, 4036); assert.equal(oracle.documentBytes, 32269051);
  assert.deepEqual(orderFor(0), ['go', 'rust']); assert.deepEqual(orderFor(1), ['rust', 'go']);
  assert(summarize(pairs()).every(row => row.state === 'passed_selected_path' && row.targetRatio === .30));
});
test('high-resolution integer clock preserves >2^53 ticks and includes worker drain', () => {
  const timing = elapsed(proof()); assert.equal(timing.controllableMillis, 200); assert.equal(timing.rootObservedMillis, 100); assert.equal(timing.reapAndDrainMillis, 100);
});
test('missing, forced, output-failed and unreaped process clocks cannot pass', () => {
  for (const mutate of [p => delete p.timing, p => p.forced = true, p => p.handlesClosed = false, p => p.outputReadersComplete = false, p => p.activeProcesses = 1, p => p.error = 'failed', p => p.deadlineHit = true, p => p.timing.rootExitTicks = '0', p => p.timing.treeReapedTicks = p.timing.startTicks, p => p.timing.frequency = 0]) { const value = proof(); mutate(value); assert.throws(() => elapsed(value)); }
});
test('near-resolution sample cannot be used to claim a speedup', () => { const value = proof(); value.timing.rootExitTicks = value.timing.startTicks; value.timing.treeReapedTicks = (BigInt(value.timing.startTicks) + 1n).toString(); assert.throws(() => elapsed(value), /100 clock ticks/); });
test('20-sample statistics use averaged middle pair and nearest-rank p95', () => { assert.deepEqual(statistics(Array.from({length: 20}, (_, i) => i + 1)), {count: 20, median: 10.5, p95: 19, min: 1, max: 20}); });
test('missing sample and duplicate/out-of-range iterations fail acceptance', () => {
  for (const mutate of [p => p.splice(0, 1), p => p[2].iteration = p[0].iteration, p => p[0].iteration = 20]) { const value = pairs(); mutate(value); assert(summarize(value).some(row => row.state === 'failed_correctness_or_missing_samples')); }
});
test('one intermediate mismatch cannot be hidden by final matching pair', () => { const value = pairs(); value[8].resultEquivalent = false; assert(summarize(value).some(row => !row.correctAndComplete)); });
test('invalid failure time is never dropped from a success-only distribution', () => {
  const value = pairs(); value[4].rust.valid = false; value[4].rust.controllableMillis = null;
  const summary = summarize(value)[0]; assert.equal(summary.state, 'failed_correctness_or_missing_samples'); assert.equal(summary.timings, null);
});
test('a path just above 0.30 fails regardless of other fast paths', () => {
  const value = pairs(); for (const row of value.filter(row => row.caseId === CASES[1].id && row.cacheMode === 'warm')) row.rust.controllableMillis = 30.01;
  const summary = summarize(value); assert.equal(summary.filter(row => row.state === 'failed_performance').length, 1);
});
test('warm requires its own successful cold with same home/binary/pin', () => {
  for (const field of ['home', 'binary', 'snapshot']) { const value = pairs(); value[1].rust[field] = 'other'; assert.equal(summarize(value)[1].correctAndComplete, false); }
});
test('reused homes and unbalanced engine order are rejected', () => {
  for (const mutate of [p => p[2].rust.home = p[0].rust.home, p => p[2].order = ['go', 'rust']]) { const value = pairs(); mutate(value); assert.equal(summarize(value)[0].correctAndComplete, false); }
});
test('query compares all public semantic values and ordered rows', () => {
  compareSamples(CASES[2].id, query(), query(), oracle, pin);
  for (const field of ['score', 'scoreParts', 'role', 'symbolId', 'excerpt', 'contentHash', 'line']) {
    const changed = query(); changed.result.results[0][field] = field === 'score' || field === 'line' ? 99 : 'different';
    assert.throws(() => compareSamples(CASES[2].id, query(), changed, oracle, pin));
  }
});
test('missing full coverage and incomplete/truncated query cannot count as equivalent', () => {
  for (const mutate of [r => r.coverage.analyzedDocuments--, r => r.coverage.skippedDocuments++, r => r.complete = false, r => r.truncated = true, r => r.resolvedCommit = 'wrong', r => r.results = []]) { const changed = query(); mutate(changed.result); assert.throws(() => semantic(CASES[2].id, changed, 'rust', oracle, pin)); }
});
test('Emmy-only proof fields are not erroneously demanded from Go', () => {
  const changed = query(); changed.result.matchCoverage = {exactMatches: 1}; changed.result.emmyProof = {extra: true}; changed.result.results[0].ownedProvenance = 'extra';
  assert.equal(compareSamples(CASES[2].id, query(), changed, oracle, pin).resultEquivalent, true);
});
test('relationships, suggestions, cursors and diagnostic values are not dropped', () => {
  for (const mutate of [r => r.relations = [{source: 'x', target: 'y', kind: 'call', confidence: 'high', path: 'x.lua', line: 1}], r => r.suggestions = ['changed'], r => r.nextCursor = 'unexpected', r => r.coverage.diagnostics = 1, r => r.coverage.diagnosticSample = [{path: 'x', line: 1, message: 'failed'}]]) {
    const changed = query(); mutate(changed.result); assert.throws(() => compareSamples(CASES[2].id, query(), changed, oracle, pin));
  }
});
test('index keeps public schema/storage/parser and all business counts exact', () => {
  compareSamples(CASES[1].id, index(), index(), oracle, pin);
  for (const field of ['schema', 'storage', 'parser']) { const changed = index(); changed.result[field] = 'different'; assert.throws(() => compareSamples(CASES[1].id, index(), changed, oracle, pin)); }
  for (const field of ['declarations', 'relationships', 'assets', 'diagnostics', 'documents']) { const changed = index(); changed.result[field]++; assert.throws(() => compareSamples(CASES[1].id, index(), changed, oracle, pin)); }
});
test('describe accepts only the two exact approved engine summary changes', () => {
  const base = envelope({commands: APPROVED_ENGINE_SUMMARIES.map(v => ({path: v.path, summary: v.expected, flags: [], arguments: [], mutates: false})), formats: ['json'], development: false});
  const actual = structuredClone(base); for (let i = 0; i < actual.result.commands.length; i++) actual.result.commands[i].summary = APPROVED_ENGINE_SUMMARIES[i].actual;
  assert.equal(compareSamples(CASES[0].id, base, actual).approvedEngineSummaryChanges.length, 2);
  actual.result.commands[0].summary += ' hidden change'; assert.throws(() => compareSamples(CASES[0].id, base, actual));
});
test('describe does not normalize changed flags, formats, extras or order', () => {
  const base = envelope({commands: [{path: 'a', flags: []}, {path: 'b', flags: []}], formats: ['json']});
  for (const mutate of [r => r.commands.reverse(), r => r.commands[0].flags.push('missing'), r => r.formats.push('jsonl'), r => r.unreviewed = true]) { const actual = structuredClone(base); mutate(actual.result); assert.throws(() => compareSamples(CASES[0].id, base, actual)); }
});
function goMetadata() { return `binary.exe: go1.27.1\n${Object.entries({'vcs.revision': GO_COMMIT, 'vcs.modified': 'false', CGO_ENABLED: '0', GOOS: 'windows', GOARCH: 'amd64', '-trimpath': 'true'}).map(([k, v]) => `\tbuild\t${k}=${v}`).join('\n')}\n`; }
test('Go metadata requires exact clean fixed build and optimized target identity', () => {
  assert.equal(verifyGoMetadata(goMetadata()).commit, GO_COMMIT);
  for (const [from, to] of [[GO_COMMIT, '0'.repeat(40)], ['modified=false', 'modified=true'], ['go1.27.1', 'go1.27.0'], ['CGO_ENABLED=0', 'CGO_ENABLED=1'], ['GOOS=windows', 'GOOS=linux'], ['-trimpath=true', '-trimpath=false']]) assert.throws(() => verifyGoMetadata(goMetadata().replace(from, to)));
  assert.throws(() => verifyGoMetadata(goMetadata() + '\tbuild\t-gcflags=all=-N -l\n'));
});
test('Go preparer owns caches and user paths, ignores user Go config, and preserves dependency networking', t => {
  const preparer = readFileSync(new URL('../../tools/rust-performance-prepare-go.mjs', import.meta.url), 'utf8');
  const setup = preparer.slice(preparer.indexOf('const env ='), preparer.indexOf('function run('));
  const parent = mkdtempSync(join(tmpdir(), 'owned-go-prepare-')); t.after(() => rmSync(parent, {recursive: true, force: true}));
  for (const proxy of [undefined, 'https://proxy.golang.org,direct']) {
    const inherited = {Path: 'installed-tools', HOME: 'real-user', userprofile: 'real-user', CODEX_HOME: 'real-agent', APPDATA: 'real-apps', LOCALAPPDATA: 'real-local', TEMP: 'real-temp', TMP: 'real-temp', LYCHEEDEV_BIN: 'real-tool', gocache: 'unowned-cache', GoModCache: 'unowned-modules', gotmpdir: 'unowned-temp', GoEnv: 'user-config', GoToolchain: 'auto', ...(proxy ? {GOPROXY: proxy} : {})};
    const before = {...inherited}, directory = join(parent, proxy ? 'proxy' : 'default');
    const actual = runInNewContext(setup + '\nenv;', {process: {env: inherited}, job: {directory}, join, ownedUserEnvironment});
    for (const [key, name] of Object.entries({GOCACHE: 'go-cache', GOMODCACHE: 'go-mod-cache', GOTMPDIR: 'go-tmp'})) assert.equal(actual[key], join(directory, name));
    assert.equal(actual.GOENV, 'off'); assert.equal(actual.GOTOOLCHAIN, 'local'); assert.equal(actual.Path, inherited.Path);
    assert.equal(actual.GOPROXY, proxy); assert.equal(Object.hasOwn(actual, 'GOPROXY'), proxy !== undefined);
    for (const key of ['HOME', 'USERPROFILE', 'CODEX_HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP', 'LYCHEEDEV_HOME']) assert(actual[key].startsWith(directory), key);
    assert.equal(actual.LYCHEEDEV_BIN, undefined); assert.equal(actual.userprofile, undefined); assert.deepEqual(inherited, before);
    for (const key of ['gocache', 'GoModCache', 'gotmpdir', 'GoEnv', 'GoToolchain']) assert.equal(Object.hasOwn(actual, key), false);
  }
  assert(preparer.includes('for (const directory of [env.GOCACHE, env.GOMODCACHE, env.GOTMPDIR]) mkdirSync(directory);'));
  assert(preparer.includes('ownedSpawnSync(command, args, {cwd: source, env, evidence: job.evidence, timeout})'));
});
test('current Go package rejects both old version and relabelled old commit before runtime use', t => {
  const directory = mkdtempSync(join(tmpdir(), 'paired-go-version-')); t.after(() => rmSync(directory, {recursive: true, force: true}));
  for (const [version, commit, actual, expected] of [
    ['3.2.3', GO_COMMIT, '3.2.3', GO_VERSION],
    [GO_VERSION, '3766f9cacb52e93a803a626187e2a1d54a445468', '3766f9cacb52e93a803a626187e2a1d54a445468', GO_COMMIT],
  ]) {
    writeFileSync(join(directory, 'release.json'), JSON.stringify({schema: 'lycheedev.release.v1', version, commit}));
    assert.throws(() => verifyGoPackage(directory, 'unused.zip'), error => error.code === 'ERR_ASSERTION' && error.actual === actual && error.expected === expected);
  }
  const workflow = readFileSync(new URL('../../.github/workflows/rust-performance.yml', import.meta.url), 'utf8');
  assert(workflow.includes(`ref: '${GO_COMMIT}'`)); assert(workflow.includes('.tmp/paired-go-3.2.4-artifact/dev-npm-stage'));
  assert(!workflow.includes('--one-shot'));
  const preparer = readFileSync(new URL('../../tools/rust-performance-prepare-go.mjs', import.meta.url), 'utf8');
  assert(preparer.includes('`lycheedev-local-${GO_VERSION}.tgz`')); assert(!preparer.includes('lycheedev-local-3.2.3.tgz'));
});
test('Source paired outer controller, preparation and Git inherit only their existing owner user paths', t => {
  const directory = mkdtempSync(join(tmpdir(), 'paired-user-owner-')); t.after(() => rmSync(directory, {recursive: true, force: true}));
  const inherited = {Path: 'installed-tools', HOME: 'real-home', UserProfile: 'real-profile', CodeX_Home: 'real-codex',
    APPDATA: 'real-appdata', LOCALAPPDATA: 'real-localappdata', TEMP: 'real-temp', TMP: 'real-tmp',
    CARGO_HOME: 'installed-cargo-cache', RUSTUP_HOME: 'installed-rustup-cache', GIT_CONFIG_COUNT: '1', GCM_INTERACTIVE: 'always'};
  const envFor = owner => ownedUserEnvironment(owner, inherited);
  const check = env => {
    assert.equal(env.HOME, join(directory, 'user')); assert.equal(env.USERPROFILE, env.HOME);
    assert.equal(env.CODEX_HOME, join(env.HOME, '.codex'));
    assert.equal(env.APPDATA, join(env.HOME, 'AppData/Roaming')); assert.equal(env.LOCALAPPDATA, join(env.HOME, 'AppData/Local'));
    assert.equal(env.TEMP, join(directory, 'temp')); assert.equal(env.TMP, env.TEMP);
    assert.equal(env.CARGO_HOME, inherited.CARGO_HOME); assert.equal(env.RUSTUP_HOME, inherited.RUSTUP_HOME);
    assert.equal(env.Path, inherited.Path); assert(!Object.hasOwn(env, 'UserProfile') && !Object.hasOwn(env, 'CodeX_Home'));
  };
  const outer = readFileSync(new URL('../../tools/rust-performance-paired.mjs', import.meta.url), 'utf8');
  const begin = outer.indexOf('  const run = ownedSpawnSync('), end = outer.indexOf('\n  handlesReaped =', begin);
  const calls = [];
  runInNewContext(outer.slice(begin, end), {process: {execPath: 'node'}, runnerRoot: 'runner', configPath: 'configuration', root: 'root',
    job: {directory, evidence: 'outer-evidence'}, join, ownedUserEnvironment: envFor,
    ownedSpawnSync: (binary, args, options) => calls.push({binary, args, options})});
  check(calls[0].options.env); assert.equal(calls[0].options.timeout, 18000000);
  const inner = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  runInNewContext(inner.slice(inner.indexOf('function controlled('), inner.indexOf('\nfunction prepare(')) + '\ncontrolled("tool", ["argument"], "evidence");', {
    config: {scratch: directory}, ownedUserEnvironment: envFor, root: 'root', fatal: false, assert, performance: {now: () => 1}, sha256: () => 'hash',
    ownedSpawnSync: (binary, args, options) => { calls.push({binary, args, options}); return {handlesReaped: true, status: 0, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0)}; },
  });
  check(calls[1].options.env); assert.equal(calls[1].options.timeout, 210000);
  const gitEnv = runInNewContext(inner.slice(inner.indexOf('const gitEnv ='), inner.indexOf('function git(')) + '\ngitEnv;',
    {config: {scratch: directory}, ownedUserEnvironment: envFor});
  check(gitEnv); assert(!Object.hasOwn(gitEnv, 'GIT_CONFIG_COUNT'));
  assert.equal(gitEnv.GIT_CONFIG_GLOBAL, 'NUL'); assert.equal(gitEnv.GIT_TERMINAL_PROMPT, '0'); assert.equal(gitEnv.GCM_INTERACTIVE, 'never');
});
test('Source measured invocations keep business homes and fixed pins while user paths use the existing pair owner', t => {
  const directory = mkdtempSync(join(tmpdir(), 'paired-sample-owner-')); t.after(() => rmSync(directory, {recursive: true, force: true}));
  const source = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8'), calls = [];
  const block = source.slice(source.indexOf('function invoke('), source.indexOf('\n\nsave();'));
  const context = {config: {go: {binary: 'go.exe'}, rust: {sourceBinary: 'rust.exe'}}, dirname, startupDiagnostics: false, oracle, assert,
    ownedUserEnvironment: owner => ownedUserEnvironment(owner, {HOME: 'real-home', CODEX_HOME: 'real-codex', TEMP: 'real-temp'}),
    elapsed: () => ({controllableMillis: 12}), semantic: () => {},
    controlled: (binary, args, evidence, options) => { calls.push({binary, args, evidence, options}); return {row: {}, run: {status: 0, handlesReaped: true, stdout: JSON.stringify(envelope({}))}}; },
  };
  runInNewContext(block + '\nthis.invoke = invoke;', context);
  for (const engine of ['go', 'rust']) {
    const home = join(directory, `${engine}-home`);
    const result = context.invoke(CASES[1], engine, home, pin, 'pair-evidence', 3, 'cold');
    const call = calls.at(-1);
    assert.deepEqual(Array.from(call.args), ['source', 'index', '--snapshot', pin, '--home', home, '--format', 'json']);
    assert.equal(call.options.env.HOME, join(directory, 'user')); assert.equal(call.options.env.CODEX_HOME, join(directory, 'user/.codex'));
    assert.equal(call.options.env.TEMP, join(directory, 'temp')); assert.equal(call.options.measure, true);
    assert.equal(call.evidence, 'pair-evidence'); assert.equal(result.row.home, home); assert.equal(result.row.snapshot, pin);
    assert.equal(result.row.controllableMillis, 12); assert.equal(result.row.valid, true);
  }
});
test('package/corpus relative paths reject traversal and absolute inputs', () => { for (const name of ['../x', '/x', 'C:/x', 'x\\y', 'a//b', 'a/./b']) assert.throws(() => safeRelative(name)); assert.equal(safeRelative('a/b.lua'), 'a/b.lua'); });
test('fresh seed clone proves independent equal bytes and detects prewarmed state', t => {
  const temp = mkdtempSync(join(tmpdir(), 'paired-runner-')); t.after(() => rmSync(temp, {recursive: true, force: true}));
  const seed = join(temp, 'seed'); mkdirSync(seed);
  for (const name of ['state', 'pins', 'mirrors', 'blobs', 'indexes', 'cache', 'runs', 'captures', 'locks', 'tmp']) mkdirSync(join(seed, name));
  writeFileSync(join(seed, 'workspace.json'), '{}'); writeFileSync(join(seed, 'pins', 'pin.json'), '{"pin":"same"}');
  const source = assertSeedCold(seed), a = join(temp, 'a'), b = join(temp, 'b'); cloneSeed(seed, a, source); cloneSeed(seed, b, source);
  assert.equal(inventory(a).sha256, inventory(b).sha256); assert.throws(() => cloneSeed(seed, a, source));
  writeFileSync(join(a, 'indexes', 'cache'), 'warm'); assert.equal(inventory(b).sha256, source.sha256); assert.throws(() => assertSeedCold(a), /preseeded/);
  writeFileSync(join(seed, 'pins', 'pin.json'), 'changed'); assert.throws(() => cloneSeed(seed, join(temp, 'c'), source), /seed changed/);
});
test('native timing starts at creation, ends after all output/tree reap, excludes observer shutdown', () => {
  const owner = readFileSync(new URL('../../tools/rust-owned-process.ps1', import.meta.url), 'utf8');
  const start = owner.indexOf('result.commandStartTicks=Stopwatch.GetTimestamp();');
  assert(start > owner.indexOf('var start=new SIX')); assert(start < owner.indexOf('if(!CreateProcessW(binary,command'));
  const end = owner.indexOf('result.treeReapedTicks=Stopwatch.GetTimestamp();'); assert(end > owner.indexOf('if(stdoutReader!=null&&stderrReader!=null)result.outputReadersComplete')); assert(end < owner.indexOf('if(memory!=null)try{result.memory=memory.Stop();}'));
  assert.match(owner, /startTicks=\$result\.commandStartTicks\.ToString/);
});

test('source warm repetition without verified reuse cannot pass', () => {
  const value = pairs(); const row = value.find(row => row.caseId === CASES[1].id && row.cacheMode === 'warm');
  delete row.go.warmReuse; assert.equal(summarize(value).find(row => row.caseId === CASES[1].id && row.cacheMode === 'warm').correctAndComplete, false);
});
test('seed refuses unexpected source facts outside generic cache directories', t => {
  const home = mkdtempSync(join(tmpdir(), 'paired-seed-root-')); t.after(() => rmSync(home, {recursive: true, force: true}));
  mkdirSync(join(home, 'source')); assert.throws(() => assertSeedCold(home), /unknown seed root/);
});
test('checkpointed seed metadata contains only the actual public pin document', t => {
  const home = mkdtempSync(join(tmpdir(), 'paired-seed-db-')); t.after(() => rmSync(home, {recursive: true, force: true}));
  mkdirSync(join(home, 'state')); mkdirSync(join(home, 'pins'));
  const file = join(home, 'state/toolkit.sqlite'), pinned = {schema: 'lycheedev.selection.v1', id: pin, source: {exactCommit: oracle.commit}};
  const read = (commit, path) => execFileSync('git', ['--no-replace-objects', 'show', `${commit}:${path}`], {encoding: 'utf8', maxBuffer: 2 << 20});
  const go = read(GO_COMMIT, 'internal/vault/metadata.go'), rust = read('HEAD', 'crates/lycheedev-core/src/metadata/historical_write.rs');
  const documents = go.match(/CREATE TABLE documents .*? WITHOUT ROWID; PRAGMA user_version=1;/)?.[0];
  const deleted = go.match(/CREATE TABLE IF NOT EXISTS deleted_generations .*? WITHOUT ROWID/)?.[0];
  assert(documents && deleted, 'use actual fixed Go writable DDL'); assert(rust.includes(documents) && rust.includes(deleted), 'Rust writer implements the same schema');
  const db = new DatabaseSync(file); db.exec(documents);
  db.prepare('INSERT INTO documents VALUES (?,1,?)').run(`pin/${pin}`, Buffer.from(JSON.stringify(pinned))); db.close();
  const evidence = join(home, 'metadata-copy.sqlite'); assert.throws(() => verifySeedMetadata(home, pinned, evidence), /complete fixed Go\/Rust writable schema/);
  const writable = new DatabaseSync(file); writable.exec(deleted); writable.close();
  const verified = verifySeedMetadata(home, pinned, evidence); assert.equal(verified.checkpointed, true); assert.equal(verified.deletedGenerations, 0);
  writeFileSync(file + '-wal', 'unfinished'); assert.throws(() => verifySeedMetadata(home, pinned, evidence), /checkpointed and closed/); rmSync(file + '-wal');
  const tombstone = new DatabaseSync(file); tombstone.prepare('INSERT INTO deleted_generations VALUES (?,1)').run('pin/previous'); tombstone.close();
  assert.throws(() => verifySeedMetadata(home, pinned, evidence), /no deleted generations/);
  const clean = new DatabaseSync(file); clean.exec('DELETE FROM deleted_generations'); clean.close();
  const changed = new DatabaseSync(file); changed.prepare('INSERT INTO documents VALUES (?,1,?)').run('source/rust-index/cached', Buffer.from('{}')); changed.close();
  assert.throws(() => verifySeedMetadata(home, pinned, evidence), /exactly one pin/);
});
test('warm proof keeps actual derived file identity and requires Rust public hit counter', t => {
  const home = mkdtempSync(join(tmpdir(), 'paired-derived-')); t.after(() => rmSync(home, {recursive: true, force: true}));
  for (const directory of ['source/v1/facts/snapshots', 'cache/objects']) { mkdirSync(join(home, directory), {recursive: true}); writeFileSync(join(home, directory, 'index'), 'derived'); }
  const go = derivedIdentity(home, 'go'), rust = derivedIdentity(home, 'rust');
  assert.equal(proveWarmReuse('go', go, derivedIdentity(home, 'go'), {}).verified, true);
  const hit = envelope({coverage: {io: {cachedIndex: true, parsedDocuments: 0}}});
  assert.equal(proveWarmReuse('rust', rust, derivedIdentity(home, 'rust'), hit).verified, true);
  assert.throws(() => proveWarmReuse('rust', rust, rust, envelope({})), /actual cachedIndex/);
  writeFileSync(join(home, 'cache/objects/index'), 'replaced'); assert.throws(() => proveWarmReuse('rust', rust, derivedIdentity(home, 'rust'), hit), /identity changed/);
});

test('package/runtime validation matches actual immutable Go 3.2.4 source', () => {
  const read = path => execFileSync('git', ['--no-replace-objects', 'show', `${GO_COMMIT}:${path}`], {encoding: 'utf8', maxBuffer: 2 << 20});
  const runtime = read('release/tools/luals.json'); assert.deepEqual(JSON.parse(runtime), LUALS);
  const release = read('tools/release.mjs'), lua = read('tools/luals.mjs'), open = read('internal/luals/runtime.go');
  assert(release.includes("return { schema: 'lycheedev.release.v1', version, commit, binaries, resources, correspondingSource };"));
  assert(release.includes("runOk('go', ['build', '-trimpath', '-buildvcs=true', '-o', output, './cmd/lycheedev']"));
  assert(release.includes("env.CGO_ENABLED = '0'")); assert(release.includes('verifyBinaryIdentity(stage,{commit:id.commit,dirty:id.dirty,version});'));
  assert(lua.includes("files.set('runtime.json', runtimeBytes)")); assert(lua.includes('const path = `tool/luals/${name}`'));
  assert(open.includes('bytes.Equal(bytes.TrimSpace(metadata), []byte(pinnedRuntimeJSON))'));
  for (const name of ['bin/lua-language-server.exe', 'bin/main.lua', 'main.lua', 'LICENSE']) assert(open.includes(`"${name}"`));
});

function startupProof() {
  const p = proof(), start = BigInt(p.timing.startTicks);
  const names = ['createReturnTicks', 'rootPersistBeginTicks', 'rootPersistEndTicks', 'observerSetupEndTicks', 'resumeBeforeTicks', 'resumeAfterTicks'];
  p.diagnostics = {schema: 'lycheedev.owned-startup-diagnostics.v1', ...Object.fromEntries(names.map((name, i) => [name, (start + BigInt(i + 1) * 100000n).toString()])), rootCpuObserved: true, rootKernel100ns: '120000', rootUser100ns: '30000', error: null};
  return p;
}
test('startup diagnostics remain optional for old receipts and never alter accepted elapsed', () => {
  const old = proof(); assert.equal(startupBreakdown(old), null);
  assert.throws(() => startupBreakdown(old, {required: true}), /missing/);
  const current = startupProof(); assert.deepEqual(elapsed(current), elapsed(old));
  current.diagnostics.error = 'observation failed';
  assert.deepEqual(elapsed(current), elapsed(old));
  assert.throws(() => startupBreakdown(current), /observation failed/);
});
test('startup diagnostics attribute all six QPC boundaries without subtracting any time', () => {
  const p = startupProof(), before = structuredClone(p), result = startupBreakdown(p, {required: true});
  assert.equal(result.controllableMillis, 200); assert.equal(result.preResumeMillis, 50);
  assert.equal(result.resumeThroughReapAndDrainMillis, 150);
  assert.deepEqual(result.phases, {createProcessMillis: 10, identityBeforePersistenceMillis: 10, rootIdentityDurabilityMillis: 10, outputAndObserverSetupMillis: 10, beforeResumeGapMillis: 10, resumeCallMillis: 10, resumedToObservedRootExitMillis: 40, rootExitToReapAndDrainMillis: 100});
  assert.equal(Object.values(result.phases).reduce((sum, n) => sum + n, 0), result.controllableMillis);
  assert.equal(result.rootCpu.kernelMillis, 12); assert.equal(result.rootCpu.userMillis, 3);
  assert.match(result.qualification, /not pure CPU or a performance pass/);
  assert.deepEqual(p, before);
});
test('incomplete or invalid startup observations cannot become a diagnostic success', () => {
  for (const mutate of [p => delete p.diagnostics.createReturnTicks, p => p.diagnostics.resumeBeforeTicks = p.timing.startTicks, p => p.diagnostics.resumeAfterTicks = p.timing.treeReapedTicks, p => p.diagnostics.rootCpuObserved = false, p => p.diagnostics.rootKernel100ns = '-1', p => p.diagnostics.rootUser100ns = '1.5', p => p.diagnostics.schema = 'unknown', p => p.diagnostics.error = 'GetProcessTimes failed']) {
    const value = startupProof(); mutate(value); assert.throws(() => startupBreakdown(value, {required: true}));
  }
  const p = startupProof(); p.diagnostics.rootKernel100ns = '0'; p.diagnostics.rootUser100ns = '0';
  assert.equal(startupBreakdown(p).rootCpu.kernelMillis, 0);
});
test('diagnostic mode retains strict identities, separate runner custody, one pair and no performance pass', () => {
  const outer = readFileSync(new URL('../../tools/rust-performance-paired.mjs', import.meta.url), 'utf8');
  const inner = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  assert(outer.includes("extra.length === 1 && extra[0] === '--startup-diagnostics'"));
  assert(outer.includes("const runnerRoot = diagnosticMode ? resolve(import.meta.dirname, '..') : root;"));
  assert(outer.includes('hashFile(join(runnerRoot, path)) === hash'));
  assert(outer.includes('const passed = !diagnosticMode && stable && handlesReaped'));
  assert(outer.includes("observation?.state === 'startup_diagnostics_observed'"));
  assert(inner.includes("assert.equal(sourceDigest(root), rust.inputHashes.sourceTree, 'Rust source/build identity changed')"));
  assert(inner.includes('assert.equal(emmy.patchedTreeSHA256, rust.inputHashes.emmy?.patchedTreeSHA256)'));
  const begin = inner.indexOf('  if (startupDiagnostics) {\n    const entry = CASES[0];');
  const end = inner.indexOf('  } else for (const entry of executionCases) {', begin);
  assert(begin > 0 && end > begin);
  const diagnostic = inner.slice(begin, end);
  assert.equal((diagnostic.match(/invoke\(entry, engine,/g) ?? []).length, 1);
  assert(diagnostic.includes('for (const engine of row.order)'));
  assert(diagnostic.includes('compareSamples(entry.id, outputs.go, outputs.rust, oracle, null)'));
  assert(!/REPETITIONS|prepareSeeds\(|cacheMode of/.test(diagnostic));
  assert(inner.includes('diagnosticMode ? [] : summarize(report.pairs)'));
});
function runDiagnosticFixture({mismatch = false, invalid = false, unreaped = false} = {}) {
  const source = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  const begin = source.indexOf('  if (startupDiagnostics) {\n    const entry = CASES[0];');
  const end = source.indexOf('  } else for (const entry of executionCases) {', begin);
  assert(begin > 0 && end > begin);
  const calls = [], seals = [], writes = [], cleaned = [];
  const pair = {directory: 'fixture', evidence: 'evidence', receipt: 'receipt', seal: value => seals.push(value), cleanup: () => cleaned.push(true)};
  const context = {startupDiagnostics: true, CASES, root: 'root', config: {go: {binarySHA256: 'go'}, rust: {binarySHA256: 'rust'}}, fatal: false,
    report: {pairOwners: [], diagnosticPairs: []}, orderFor, join, assert, compareSamples, oracle, files: () => ['sealed-output'], save: () => {},
    beginScratch: () => pair, writeFileSync: (path, bytes) => writes.push({path, value: JSON.parse(bytes)}),
    invoke: (entry, engine, home, pin, evidence, iteration, cacheMode) => {
      calls.push({entry, engine, home, pin, evidence, iteration, cacheMode});
      if (unreaped) context.fatal = true;
      return {row: {valid: !invalid, startup: startupBreakdown(startupProof())}, envelope: envelope({commands: [{path: 'describe', flags: mismatch && engine === 'rust' ? ['changed'] : []}], formats: ['json'], development: false})};
    }};
  let error;
  try { runInNewContext(source.slice(begin, end) + '\n  }', context); } catch (failure) { error = failure; }
  return {context, calls, seals, writes, cleaned, error};
}
test('diagnostic control flow invokes exactly the original Go/Rust describe pair and seals it', () => {
  const result = runDiagnosticFixture(); assert.equal(result.error, undefined);
  assert.deepEqual(result.calls.map(row => row.engine), ['go', 'rust']);
  for (const row of result.calls) {
    assert.equal(row.entry.id, 'B01-describe'); assert.deepEqual(row.entry.args, ['describe']);
    assert.equal(row.home, join('fixture', `${row.engine}-home`)); assert.equal(row.pin, null);
    assert.equal(row.iteration, 0); assert.equal(row.cacheMode, 'diagnostic_once');
  }
  assert.equal(result.context.report.diagnosticPairs.length, 1);
  assert.equal(result.context.report.diagnosticPairs[0].resultEquivalent, true);
  assert.equal(result.seals[0].state, 'succeeded'); assert.equal(result.seals[0].handlesReaped, true);
  assert.equal(result.cleaned.length, 1); assert.equal(result.writes[0].value.mode, 'startup_diagnostics');
});
test('diagnostic mismatches and invalid observations retain failed evidence without replacement samples', () => {
  for (const options of [{mismatch: true}, {invalid: true}]) {
    const result = runDiagnosticFixture(options); assert.equal(result.error, undefined);
    assert.equal(result.calls.length, 2); assert.equal(result.context.report.diagnosticPairs[0].resultEquivalent, false);
    assert(result.context.report.diagnosticPairs[0].failure); assert.equal(result.seals[0].state, 'failed');
    assert.equal(result.seals[0].handlesReaped, true); assert.equal(result.cleaned.length, 0);
  }
});
test('unreaped diagnostic process stops before the other engine and retains interrupted ownership', () => {
  const result = runDiagnosticFixture({unreaped: true});
  assert.match(result.error.message, /unverified process reap/); assert.equal(result.calls.length, 1);
  assert.equal(result.seals[0].state, 'interrupted'); assert.equal(result.seals[0].handlesReaped, false);
  assert.equal(result.context.report.diagnosticPairs[0].resultEquivalent, false); assert.equal(result.cleaned.length, 0);
});

function runColdIndexFixture({mismatch = false, invalid = false, unreaped = false} = {}) {
  const source = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  const begin = source.indexOf('for (const entry of executionCases) {');
  const end = source.indexOf('\n  identity();', begin);
  assert(begin > 0 && end > begin);
  const calls = [], copies = [], seals = [], cleaned = [], writes = [];
  let preparations = 0;
  const pair = {directory: 'new-fixture', evidence: 'evidence', receipt: 'receipt', seal: value => seals.push(value), cleanup: () => cleaned.push(true)};
  const context = {executionCases: [CASES[1]], executionRepetitions: 1, executionCacheModes: ['cold'],
    root: 'root', config: {go: {binarySHA256: 'go'}, rust: {binarySHA256: 'rust'}}, fatal: false,
    report: {pairs: [], pairOwners: [], preparation: []}, seedIdentities: {}, seeds: {}, pins: {},
    performance: {now: () => 1}, orderFor, join, assert, compareSamples, oracle, files: () => ['sealed-output'], save: () => {},
    prepareSeeds: () => { preparations++; for (const engine of ['go', 'rust']) { context.seeds[engine] = `${engine}-independent-seed`; context.seedIdentities[engine] = {sha256: engine}; context.pins[engine] = {id: pin}; } },
    cloneSeed: (seed, home, identity) => copies.push({seed, home, identity}),
    beginScratch: () => pair, writeFileSync: (path, bytes) => writes.push({path, value: JSON.parse(bytes)}),
    invoke: (entry, engine, home, selectedPin, evidence, iteration, cacheMode) => {
      calls.push({entry, engine, home, selectedPin, iteration, cacheMode});
      if (unreaped) context.fatal = true;
      const result = index(); if (mismatch && engine === 'rust') result.result.declarations++;
      return {row: {valid: !invalid, controllableMillis: 123}, envelope: result};
    }};
  let error; try { runInNewContext(source.slice(begin, end), context); } catch (failure) { error = failure; }
  return {context, calls, copies, seals, cleaned, writes, error, preparations};
}
test('cold index diagnostic runs only one real index pair with independently cloned cold homes', () => {
  const result = runColdIndexFixture(); assert.equal(result.error, undefined); assert.equal(result.preparations, 1);
  assert.deepEqual(result.calls.map(row => row.engine), ['go', 'rust']); assert.equal(result.copies.length, 2);
  for (const call of result.calls) {
    assert.equal(call.entry.id, 'B02-retail-index'); assert.deepEqual(call.entry.args, ['source', 'index']);
    assert.equal(call.cacheMode, 'cold'); assert.equal(call.iteration, 0); assert.equal(call.selectedPin, pin);
    assert.equal(call.home, join('new-fixture', `${call.engine}-home`));
    assert.equal(result.copies.find(row => row.home === call.home).seed, `${call.engine}-independent-seed`);
  }
  assert.equal(result.context.report.pairs.length, 1); assert.equal(result.context.report.pairs[0].resultEquivalent, true);
  assert.equal(result.seals[0].state, 'succeeded'); assert.equal(result.cleaned.length, 1);
});
test('cold index diagnostic keeps the entire mismatched pair and original timing without replacement or warm', () => {
  for (const options of [{mismatch: true}, {invalid: true}]) {
    const result = runColdIndexFixture(options); assert.equal(result.error, undefined); assert.equal(result.calls.length, 2);
    assert.equal(result.context.report.pairs.length, 1); const row = result.context.report.pairs[0];
    assert.equal(row.resultEquivalent, false); assert(row.failure); assert.equal(row.rust.controllableMillis, 123);
    assert.equal(result.seals[0].state, 'failed'); assert.equal(result.seals[0].handlesReaped, true); assert.equal(result.cleaned.length, 0);
  }
});
test('cold index unknown retirement halts the second engine and preserves its interrupted owner', () => {
  const result = runColdIndexFixture({unreaped: true}); assert.match(result.error.message, /unverified process reap/);
  assert.equal(result.calls.length, 1); assert.equal(result.seals[0].state, 'interrupted');
  assert.equal(result.seals[0].handlesReaped, false); assert.equal(result.cleaned.length, 0);
});
test('cold index mode cannot become twenty-pair acceptance or alter ordinary schedules', () => {
  const outer = readFileSync(new URL('../../tools/rust-performance-paired.mjs', import.meta.url), 'utf8');
  const inner = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  assert(outer.includes("extra.length === 1 && extra[0] === '--cold-index-diagnostics'"));
  assert(inner.includes("assert(!(startupDiagnostics && coldIndexDiagnostics)"));
  assert(inner.includes('const executionCases = coldIndexDiagnostics ? [CASES[1]] : CASES;'));
  assert(inner.includes('const executionRepetitions = coldIndexDiagnostics ? 1 : REPETITIONS;'));
  assert(inner.includes("const executionCacheModes = coldIndexDiagnostics ? ['cold'] : ['cold', 'warm'];"));
  for (const source of [inner, outer]) assert(source.includes('performanceAcceptanceEvaluated: !diagnosticMode'));
  assert(outer.includes('const passed = !diagnosticMode && stable && handlesReaped'));
  assert(outer.includes("observation.pairs[0].cacheMode === 'cold'"));
  assert(inner.includes('report.groups = diagnosticMode ? [] : summarize(report.pairs)'));
});
