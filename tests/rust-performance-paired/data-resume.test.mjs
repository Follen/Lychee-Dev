// Pure synthetic evidence/selection tests. No product, Cargo or native claims.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join, posix, relative, resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {dataMode, remainingDataCases, verifyDataContinuation} from './data-resume.mjs';
import {digest} from './data-inputs.mjs';
import {summarizeData} from './data-contract.mjs';

const entries = ['first', 'second', 'third'].map(id => ({id, fixture: 'images', performance: true}));
const proof = {schema: 'lycheedev.owned-process.v1', rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true,
  activeProcesses: 0, forced: false, timing: {schema: 'lycheedev.owned-timing.v1', frequency: 10000, startTicks: '100', rootExitTicks: '1000', treeReapedTicks: '1100'}};
function rows() { return ['cold', 'warm'].map(cacheMode => ({caseId: 'first', iteration: 0, cacheMode, preflight: true, order: ['go', 'rust'], resultEquivalent: true,
  ...Object.fromEntries(['go', 'rust'].map(engine => [engine, {home: engine, binary: engine, snapshot: 'PIN-fixed', valid: true, reuse: {verified: true}, controllableMillis: 100, exit: 0, handlesReaped: true, proof}]))})); }
function previous() { return {state: 'failed', performanceAcceptanceEvaluated: false, preflightPairs: [], requiresOuterRetirement: false, pairs: rows()}; }
test('continuation is an explicit observation-only mode and rejects mode combinations', () => {
  assert.deepEqual(dataMode([]), {acceptance: false, resumeFrom: null}); assert.equal(dataMode(['--acceptance']).acceptance, true);
  assert.deepEqual(dataMode(['--resume-unattempted', 'old.json']), {acceptance: false, resumeFrom: resolve('old.json')});
  for (const args of [['--resume-unattempted'], ['--acceptance', '--resume-unattempted', 'old.json'], ['--acceptance', '--acceptance'], ['--unknown']]) assert.throws(() => dataMode(args));
});
test('documented separate stage contains the complete relative module dependency closure', () => {
  const root = resolve(import.meta.dirname, '../..'), readme = readFileSync(join(root, 'tests/rust-performance-paired/data.README.md'), 'utf8');
  const block = readme.match(/\$files = @\(([\s\S]*?)\n\)/); assert(block);
  const paths = [...block[1].matchAll(/"([^"]+)"/g)].map(match => match[1]); assert.equal(new Set(paths).size, paths.length);
  for (const path of paths) {
    const bytes = readFileSync(join(root, path));
    if (!path.endsWith('.mjs')) continue;
    for (const match of bytes.toString().matchAll(/\bfrom\s+['"]([^'"]+)['"]/g)) if (match[1].startsWith('.')) {
      assert(paths.includes(posix.normalize(posix.join(posix.dirname(path), match[1]))), `${path} missing dependency ${match[1]}`);
    }
  }
});
test('real Node entry authenticates and launches its own stage in every mode while preserving product cwd', t => {
  const directory = mkdtempSync(join(tmpdir(), 'paired-data-entry-')); t.after(() => rmSync(directory, {recursive: true, force: true}));
  const source = readFileSync(new URL('../../tools/rust-data-performance-paired.mjs', import.meta.url), 'utf8');
  const paths = [...source.matchAll(/['"]((?:tools|tests|release)\/[^'"]+)['"]/g)].map(match => match[1]).filter(path => /\.(mjs|ps1|json)$/.test(path));
  for (const [name, extra, separate, mutate] of [
    ['same-root', [], false, false], ['staged-observation', [], true, false], ['staged-acceptance', ['--acceptance'], true, false],
    ['staged-continuation', ['--resume-unattempted', 'old-owned-report.json'], true, false], ['changed-stage', ['--acceptance'], true, true],
  ]) {
    const root = join(directory, name, 'product'), stage = separate ? join(directory, name, 'runner') : root;
    const put = (base, path, body) => { const file = join(base, path); mkdirSync(dirname(file), {recursive: true}); writeFileSync(file, body); };
    for (const path of new Set(paths)) { put(root, path, 'old product runner'); if (separate) put(stage, path, 'new staged runner'); }
    put(stage, 'tools/rust-data-performance-paired.mjs', source);
    // Execute the unchanged real entry. Only its product and native-owner
    // boundaries are synthetic; no product, Windows Job or speedup is claimed.
    put(stage, 'tools/rust-build.mjs', `import assert from 'node:assert/strict'; import {readFileSync} from 'node:fs'; import {createHash} from 'node:crypto';
      export const hashFile = path => createHash('sha256').update(readFileSync(path)).digest('hex');
      export const sourceDigest = root => { assert.equal(root, ${JSON.stringify(root)}); return 'unchanged-product'; };
      export const verifyArtifact = root => ({profile:'release', binarySHA256:'rust', inputHashes:{sourceTree:sourceDigest(root)}});`);
    put(stage, 'tests/rust-performance-paired/inputs.mjs', `export const verifyGoPackage = root => ({root, packageSHA256:'go'});`);
    put(stage, 'tests/rust-performance-paired/data-resume.mjs', `export {dataMode} from ${JSON.stringify(new URL('./data-resume.mjs', import.meta.url).href)};`);
    put(stage, 'tools/rust-disk.mjs', `import assert from 'node:assert/strict'; import {mkdirSync,writeFileSync} from 'node:fs'; import {join} from 'node:path';
      export function beginScratch(root) { assert.equal(root, ${JSON.stringify(root)}); const evidence=join(root,'evidence'); mkdirSync(evidence); return {evidence,directory:join(root,'scratch'),
        seal(value) { writeFileSync(join(evidence,'sealed.json'),JSON.stringify(value)); }, cleanup() { writeFileSync(join(evidence,'cleaned'),'yes'); }}; }`);
    put(stage, 'tools/rust-owned-process.mjs', `import {readFileSync,writeFileSync} from 'node:fs'; import {join} from 'node:path';
      export function ownedSpawnSync(binary,args,options) { const config=JSON.parse(readFileSync(args[1]));
        writeFileSync(join(options.evidence,'launched.json'),JSON.stringify({binary,args,cwd:options.cwd,timeout:options.timeout,config}));
        const state=config.resumeFrom?'equivalent_data_continuation_only':config.acceptance?'passed_selected_data_paths_full_acceptance_open':'equivalent_data_observation_only';
        writeFileSync(join(options.evidence,'report.json'),JSON.stringify({state,continuation:config.resumeFrom?{priorEvidenceStable:true}:undefined}));
        ${mutate ? "writeFileSync(join(config.runnerRoot,'tests/rust-performance-paired/data-run.mjs'),'mutated while running');" : ''}
        return {status:0,handlesReaped:true,proof:{synthetic:true}}; }`);
    const conditions = join(root, 'conditions.json'); writeFileSync(conditions, JSON.stringify(Object.fromEntries(['machine', 'powerAndThermal', 'competingLoad', 'disk', 'operator'].map(key => [key, 'synthetic']))));
    const args = ['artifact.json', 'go-source', 'go-package', 'luals.zip', conditions, 'catalog', 'content', 'images', 'sharp', ...extra];
    const entry = join(stage, 'tools/rust-data-performance-paired.mjs');
    const bootstrap = `Object.defineProperty(process,'platform',{value:'win32'}); process.argv=[process.execPath,${JSON.stringify(entry)},...${JSON.stringify(args)}]; await import(${JSON.stringify(pathToFileURL(entry).href)});`;
    const result = spawnSync(process.execPath, ['--input-type=module', '--eval', bootstrap], {cwd: root, encoding: 'utf8', timeout: 15000});
    assert.equal(result.error, undefined); assert.equal(result.status, mutate ? 1 : 0, result.stderr);
    const launched = JSON.parse(readFileSync(join(root, 'evidence/launched.json'))), owned = JSON.parse(readFileSync(join(root, 'evidence/owned-report.json')));
    assert.equal(launched.args[0], join(stage, 'tests/rust-performance-paired/data-run.mjs')); assert.equal(launched.cwd, root); assert.equal(launched.timeout, 18000000);
    assert.equal(launched.config.root, root); assert.equal(launched.config.runnerRoot, stage); assert.equal(owned.runnerRoot, stage);
    assert.equal(launched.config.acceptance, extra[0] === '--acceptance'); assert.equal(Boolean(launched.config.resumeFrom), extra[0] === '--resume-unattempted');
    for (const [path, sha] of Object.entries(launched.config.inputs)) {
      if (mutate && path === 'tests/rust-performance-paired/data-run.mjs') continue;
      assert.equal(sha, digest(readFileSync(join(stage, path))), `executing stage identity: ${path}`);
    }
    assert.equal(owned.artifactAndRunnerStable, !mutate); assert.equal(owned.state === 'failed', mutate);
    assert.equal(JSON.parse(readFileSync(join(root, 'evidence/sealed.json'))).state, mutate ? 'failed' : 'succeeded');
    if (separate) assert.equal(readFileSync(join(root, 'tools/rust-data-performance-paired.mjs'), 'utf8'), 'old product runner');
  }
});
test('only unattempted whole paths are selected; failed or partial attempts never get replaced', () => {
  assert.deepEqual(remainingDataCases(entries, previous()).selected.map(entry => entry.id), ['second', 'third']);
  for (const change of [x => x.performanceAcceptanceEvaluated = true, x => x.requiresOuterRetirement = true, x => x.continuation = {}, x => x.state = 'passed', x => x.pairs.pop(), x => x.pairs.push(x.pairs[0]),
    x => x.pairs[0].resultEquivalent = false, x => x.pairs[0].go.handlesReaped = false, x => x.pairs[0].go.proof.forced = true, x => x.pairs[0].go.controllableMillis++, x => x.pairs[0].go.error = 'failed',
    x => x.pairs.forEach(row => row.caseId = 'second'), x => x.pairs = []]) {
    const value = structuredClone(previous()); change(value); assert.throws(() => remainingDataCases(entries, value));
  }
  assert(summarizeData(entries.slice(1), [], false).every(group => group.state === 'not_run'));
});

function fixture(t, mutate = () => {}) {
  const root = mkdtempSync(join(tmpdir(), 'paired-data-resume-')); t.after(() => rmSync(root, {recursive: true, force: true}));
  const id = '811dac59-a06f-4f11-b0b4-43b0c37bb7d6', pairId = 'ed2701b9-639e-4989-9e49-8626ae7a9db1';
  const evidence = join(root, '.tmp/rust-disk/evidence', id), pairEvidence = join(root, '.tmp/rust-disk/evidence', pairId); mkdirSync(evidence, {recursive: true}); mkdirSync(pairEvidence, {recursive: true});
  writeFileSync(join(root, 'old.mjs'), 'original runner');
  const config = {root, evidence, scratch: join(root, '.tmp/rust-scratch', id), acceptance: false, inputs: {'old.mjs': digest('original runner')},
    rust: {binarySHA256: 'rust', inputHashes: {sourceTree: 'source'}}, go: {root: 'go', binary: 'go.exe', binarySHA256: 'go', packageSHA256: 'package'},
    goSource: 'go-source', luaLSArchive: 'luals.zip', rustArtifact: 'artifact.json', sharpModule: 'sharp', fixtures: {images: 'images'}, conditions: {operator: 'synthetic'}};
  const report = {...previous(), schema: 'lycheedev.paired-data.v1', blocker: `EPERM, ${pairId}`, inputs: {fixtures: {images: {inventory: {sha256: 'fixture'}}}},
    pairOwners: [{caseId: 'first', iteration: 0, preflight: true, receipt: join(pairEvidence, 'receipt.json')}],
    machine: {hostname: 'machine', platform: 'win32', arch: 'x64', release: 'os', node: 'node', totalMemory: 123, powerBefore: 'power', toolchains: {nodeBinarySHA256: 'node-hash'}, cpus: [{model: 'CPU'}]},
    decoder: {entryPath: 'sharp', versions: {sharp: '0.35.5'}, loadedModuleSHA256: {'sharp': 'hash'}}};
  const owned = {schema: 'lycheedev.paired-data-owned.v1', state: 'failed', performanceAcceptanceEvaluated: false, artifactAndRunnerStable: true, process: {exit: 1, handlesReaped: true},
    observation: join(evidence, 'report.json'), inputs: config.inputs, rust: config.rust, go: config.go};
  const current = {config: structuredClone(config), report: structuredClone(report), entries}; current.config.inputs['tests/rust-performance-paired/data-resume.mjs'] = 'new';
  const pair = {caseId: 'first', iteration: 0, preflight: true, valid: true, rows: structuredClone(report.pairs)};
  const pairOwner = {schema: 'lycheedev.disk-owner.v1', id: pairId, owner: 'paired-data-first-preflight', state: 'succeeded', handlesReaped: true, activePid: null, directory: join(root, '.tmp/rust-scratch', pairId),
    inputHashes: {go: 'go', rust: 'rust', fixture: 'fixture'}};
  const owner = {schema: 'lycheedev.disk-owner.v1', id, owner: 'paired-data', state: 'failed', handlesReaped: true, activePid: null, directory: config.scratch};
  const objects = {root, config, report, owned, current, pair, pairOwner, owner}; mutate(objects);
  function seal(directory, value, records) {
    value.sealedLogs = []; value.logHashes = {};
    for (const [name, body] of Object.entries(records)) {
      const path = join(directory, name), bytes = Buffer.from(JSON.stringify(body)); writeFileSync(path, bytes);
      const sha256 = digest(bytes); value.sealedLogs.push({path, original: path, sha256}); value.logHashes[relative(root, path)] = sha256;
    }
    writeFileSync(join(directory, 'receipt.json'), JSON.stringify(value));
  }
  seal(pairEvidence, pairOwner, {'pair.json': pair, 'process.stdout.bin': {actual: 'raw process output'}});
  seal(evidence, owner, {'configuration.json': config, 'report.json': report, 'owned-report.json': owned, 'process.receipt.json': {actual: 'raw native proof'}});
  return {...objects, priorPath: join(evidence, 'owned-report.json'), evidence, pairEvidence};
}
test('sealed continuation authenticates old source, complete inputs and owners without importing samples', t => {
  const f = fixture(t), before = readFileSync(join(f.evidence, 'report.json'));
  const result = verifyDataContinuation(f.root, f.priorPath, f.current);
  assert.deepEqual(result.selected.map(entry => entry.id), ['second', 'third']); assert.equal(result.evidence.importedSamples, 0);
  assert.equal(result.evidence.priorState, 'failed'); assert.equal(result.evidence.completeOriginalRun, false);
  assert(readFileSync(join(f.evidence, 'report.json')).equals(before));
});
test('changed source, evidence, machine, binary, fixture, decoder or failed retirement blocks continuation', t => {
  for (const change of [f => f.current.config.rust.binarySHA256 = 'different', f => f.current.report.machine.hostname = 'other', f => f.current.report.decoder.versions.sharp = 'other',
    f => f.current.report.inputs.fixtures.images.inventory.sha256 = 'other', f => f.config.acceptance = true, f => f.config.runnerRoot = 'other-stage',
    f => f.owner.handlesReaped = false, f => f.pairOwner.state = 'failed', f => f.pairOwner.handlesReaped = false, f => f.report.blocker = 'other failure', f => f.pair.valid = false,
    f => f.pair.rows[0].go.valid = false, f => f.owned.artifactAndRunnerStable = false]) {
    const f = fixture(t, change); assert.throws(() => verifyDataContinuation(f.root, f.priorPath, f.current));
  }
  for (const path of ['old.mjs', '.tmp/rust-disk/evidence/811dac59-a06f-4f11-b0b4-43b0c37bb7d6/report.json',
    '.tmp/rust-disk/evidence/811dac59-a06f-4f11-b0b4-43b0c37bb7d6/process.receipt.json', '.tmp/rust-disk/evidence/ed2701b9-639e-4989-9e49-8626ae7a9db1/process.stdout.bin']) {
    const f = fixture(t); writeFileSync(join(f.root, path), '{}'); assert.throws(() => verifyDataContinuation(f.root, f.priorPath, f.current));
  }
  for (const name of ['process.receipt.json', 'process.stdout.bin']) {
    const f = fixture(t); rmSync(join(name.endsWith('.bin') ? f.pairEvidence : f.evidence, name)); assert.throws(() => verifyDataContinuation(f.root, f.priorPath, f.current));
  }
});
