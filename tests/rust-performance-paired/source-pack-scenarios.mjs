// One fresh six-scenario observation. No builds, historical sample import or acceptance change.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isAbsolute, join, resolve} from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {performance} from 'node:perf_hooks';
import os from 'node:os';
import {beginScratch, admission, contained, sha256, policy, defaults} from '../../tools/rust-disk.mjs';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {collectOwnedMemory} from '../../tools/rust-owned-memory.mjs';
import {inspectEmmy} from '../../tools/rust-emmy-identity.mjs';
import {splitIndexEvidence, INDEX_BYTES} from '../../tools/rust-source-pack-evidence.mjs';
import {assertEnvelope, assertCorpus, corpusDigest} from '../rust-source-full-corpus/assertions.mjs';
import {fixturePin} from '../rust-source-native-acceptance/assertions.mjs';
import {GO_COMMIT, elapsed, semantic} from './contract.mjs';
import {assertSeedCold, auditCorpus, cloneSeed, inventory, derivedIdentity, proveWarmReuse, verifyGoMetadata, verifyGoPackage, verifySeedMetadata} from './inputs.mjs';
import {RUNNER_FILES as HISTORICAL_RUNNER_FILES, sourcePhaseEnvironment, validateConditions} from './source-candidates.mjs';
import {verifyPackProfile} from './source-pack-resume.mjs';
import {readFactDelta} from './read-fact-delta.mjs';
import {captureGoCustody, openGoCustody} from './source-pack-custody.mjs';

export const CANDIDATE_COMMIT = '00d215a937513609e482c1ef65f3b43f0288beee';
export const CANDIDATE_IDENTITY = Object.freeze({artifactSHA256: '02ebe1eb57d552d1c8ebf6c86697fdbc77d6eba4ee847edd33c4d7d19b6253ad', binarySHA256: '3169c2187b966b40ed3622fe14cfb81527e8dd3a52a6d99a888e8ab482c1b1fb', sourceTreeSHA256: '06a5a0e39c46c786d946b8c429c90c0eb4e9e1291028375e329fc9457776b4e3'});
export const BASE_COMMIT = '31c7f7b9cc79e56c986b365c06a6afbcf3c9177b';
export const FIRST_PATH = 'Interface/AddOns/Blizzard_APIDocumentation/BaseAPIMixin.lua';
export const INSERT_PATH = '000_LycheePackInserted.lua';
export const RENAME_PATH = 'ZZZ_LycheePackRenamed.lua';
export const INSERT_TEXT = 'function LycheePackInserted() end\n';
export const CASES = Object.freeze([
  {id: 'cold', mode: 'cold', documents: 4036}, {id: 'warm', mode: 'warm', documents: 4036},
  {id: 'single-content-change', mode: 'changed', documents: 4036, maxParsed: 64},
  {id: 'root-insert', mode: 'changed', documents: 4037, maxParsed: 513},
  {id: 'first-delete', mode: 'changed', documents: 4035, maxParsed: 512},
  {id: 'cross-prefix-rename', mode: 'changed', documents: 4036, maxParsed: 1025},
]);
export const RUNNER_FILES = Object.freeze([...new Set([...HISTORICAL_RUNNER_FILES,
  'tools/rust-source-pack-diagnostics.mjs', 'tools/rust-source-pack-evidence.mjs',
  'tests/rust-performance-paired/source-pack-scenarios.mjs', 'tests/rust-performance-paired/source-pack-resume.mjs', 'tests/rust-performance-paired/source-pack-custody.mjs',
  'tests/rust-source-native-acceptance/assertions.mjs', 'tests/rust-skill-usage/assertions.mjs',
])]);
export const runnerIdentity = root => Object.fromEntries(RUNNER_FILES.map(file => [file, hashFile(join(root, file))]));
export const orderFor = index => index === 1 ? ['rust', 'go'] : ['go', 'rust'];
const exact = (value, keys) => assert.deepEqual(Object.keys(value ?? {}).sort(), [...keys].sort(), 'exact configuration fields required');
const digest = value => assert.match(value, /^[0-9a-f]{64}$/);
export function validateRequest(value) {
  exact(value, ['schema', 'ownerRoot', 'candidate', 'go']);
  assert.equal(value.schema, 'lycheedev.source-pack-scenarios-config.v1');
  for (const path of [value.ownerRoot, value.candidate?.sourceRoot, value.candidate?.artifactRoot, value.candidate?.artifact, value.go?.sourceRoot, value.go?.packageRoot, value.go?.luaLSArchive]) assert(typeof path === 'string' && isAbsolute(path), 'absolute input paths required');
  exact(value.candidate, ['sourceRoot', 'artifactRoot', 'artifact', 'commit', 'artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']);
  assert.equal(value.candidate.commit, CANDIDATE_COMMIT, 'fixed pack2 product commit required');
  for (const key of ['artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']) { digest(value.candidate[key]); assert.equal(value.candidate[key], CANDIDATE_IDENTITY[key], `fixed 00d sealed ${key} required`); }
  exact(value.go, ['sourceRoot', 'packageRoot', 'luaLSArchive', 'packageSHA256']); digest(value.go.packageSHA256);
  const result = structuredClone(value);
  result.ownerRoot = realpathSync(value.ownerRoot);
  for (const side of ['candidate', 'go']) for (const key of side === 'candidate' ? ['sourceRoot', 'artifactRoot'] : ['sourceRoot', 'packageRoot']) result[side][key] = realpathSync(value[side][key]);
  result.candidate.artifact = contained(result.candidate.artifactRoot, value.candidate.artifact);
  return result;
}
export function verifyCandidateRecord(input) {
  assert.equal(input.commit, CANDIDATE_COMMIT); for (const key of Object.keys(CANDIDATE_IDENTITY)) assert.equal(input[key], CANDIDATE_IDENTITY[key]);
  assert.equal(hashFile(input.artifact, 65536), input.artifactSHA256, 'sealed artifact record changed');
  const artifact = verifyArtifact(input.artifactRoot, input.artifact);
  assert(['release', 'symbols'].includes(artifact.profile), 'optimized existing artifact required');
  assert.equal(artifact.sameInvocation, true); assert(isAbsolute(artifact.sourceRoot), 'original artifact source provenance required');
  if (artifact.profile === 'symbols') assert(artifact.pdb, 'matching symbols required');
  assert.equal(artifact.binarySHA256, input.binarySHA256);
  assert.equal(artifact.inputHashes?.sourceTree, input.sourceTreeSHA256);
  assert.equal(sourceDigest(input.sourceRoot), input.sourceTreeSHA256, 'product source/build identity changed');
  const emmy = inspectEmmy(input.sourceRoot);
  assert.equal(emmy.identity, 'public-upstream-unmodified-v1'); assert.equal(emmy.patchedTreeSHA256, artifact.inputHashes.emmy?.patchedTreeSHA256);
  const channel = JSON.parse(readFileSync(join(input.sourceRoot, 'release/npm-channel.json')));
  assert.equal(channel.version, '3.5.0'); assert.equal(channel.distTag, 'beta');
  return artifact;
}
export function evidenceFiles(directory) {
  const result = [];
  for (const name of readdirSync(directory).sort()) {
    if (name === 'receipt.json') continue;
    const path = join(directory, name), stat = lstatSync(path);
    assert(!stat.isSymbolicLink(), 'evidence reparse boundary');
    if (stat.isDirectory()) result.push(...evidenceFiles(path));
    else { assert(stat.isFile() && stat.size <= (16 << 20), 'bounded evidence required before sealing'); result.push(path); }
  }
  return result;
}
export function assertDiskPolicy(root, environment = process.env) {
  assert(!Object.keys(environment).some(key => /^LYCHEEDEV_DISK_/i.test(key)), 'diagnostic disk policy overrides are forbidden');
  const observed = policy(root, {}); assert.deepEqual(observed, {...defaults}, 'fixed original disk policy required');
  return observed;
}
const variantAuditBytes = oracle => { assert(Number.isSafeInteger(oracle.allBytes) && oracle.allBytes >= 0); return oracle.allBytes; };
const saveJSON = (path, value) => { const raw = Buffer.from(JSON.stringify(value, null, 2) + '\n'); assert(raw.length <= 16 << 20, 'small report evidence budget'); writeFileSync(path, raw); };
export function footprint({cold = true} = {}) {
  assert.equal(cold, true, 'changed admission uses measured own-engine inventories');
  const scratchReserveBytes = 257058598 + 131416885 + (64 << 20);
  const evidenceBytes = 4 * INDEX_BYTES + 2 * 32665129 + (64 << 20);
  return {requiredBytes: scratchReserveBytes + evidenceBytes, scratchReserveBytes,
    basis: 'Historical b3d Go/Rust complete home bytes plus 64 MiB setup/growth allowance; an estimate, not observed 00d size or a hard allocation cap', evidenceBytes};
}
export function mutationRows(base, id, original) {
  assert(CASES.some(c => c.id === id && c.mode === 'changed'), 'explicit changed scenario required');
  assert.equal(base.filter(row => /\.(lua|xml|toc)$/i.test(row.path))[0]?.path, FIRST_PATH, 'fixed first eligible path');
  assert.equal(original.length, 2533); assert.equal(sha256(original), '5c48438fa06252e7eacb5dc752f4959cb6e2b20cbc639fc474229fe6fdbe4053');
  assert.equal(base.find(row => row.path === FIRST_PATH)?.gitSHA1, '0083f8b1a5957be88807f2eb60642929b3712012');
  assert(!base.some(row => [INSERT_PATH, RENAME_PATH].includes(row.path)), 'fixed new root names must be absent');
  const rows = structuredClone(base), first = rows.findIndex(row => row.path === FIRST_PATH);
  let bytes, path;
  if (id === 'single-content-change') { path = FIRST_PATH; bytes = Buffer.concat([original, Buffer.from('\n')]); }
  if (id === 'root-insert') { path = INSERT_PATH; bytes = Buffer.from(INSERT_TEXT); }
  if (id === 'cross-prefix-rename') { path = RENAME_PATH; bytes = original; }
  if (id !== 'root-insert') rows.splice(first, 1);
  const blob = bytes && createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
  if (bytes) rows.push({path, bytes: bytes.length, sha256: sha256(bytes), gitSHA1: blob});
  rows.sort((a, b) => Buffer.compare(Buffer.from(a.path), Buffer.from(b.path)));
  return {rows, path: path ?? FIRST_PATH, bytes, blob, originalPath: FIRST_PATH, originalSHA256: sha256(original)};
}
export function variantOracle(baseOracle, rows, commit) {
  const docs = rows.filter(row => /\.(lua|xml|toc)$/i.test(row.path));
  return {...baseOracle, commit, allFiles: rows.length, allBytes: rows.reduce((n, r) => n + r.bytes, 0), allPathSHA256SizeDigest: corpusDigest(rows), documents: docs.length, documentBytes: docs.reduce((n, r) => n + r.bytes, 0), documentPathSHA256SizeDigest: corpusDigest(docs), counts: Object.fromEntries(['.lua', '.xml', '.toc'].map(ext => [ext, docs.filter(row => row.path.toLowerCase().endsWith(ext)).length]))};
}
// Only newly cloned, registered homes are writable. Build each descendant from
// the unchanged base tree with Git's isolated temporary index; no worktree/hook.
export function prepareVariant({homes, directory, baseRows, oracle, id, git}) {
  const engines = Object.keys(homes); assert(engines.length > 0 && engines.every(e => ['go', 'rust'].includes(e)));
  const source = homes[engines[0]], original = git(['--git-dir', join(source, 'mirrors', `${oracle.repository}.git`), 'cat-file', 'blob', `${BASE_COMMIT}:${FIRST_PATH}`]);
  const change = mutationRows(baseRows, id, original), commits = {};
  for (const engine of engines) {
    const mirror = join(homes[engine], 'mirrors', `${oracle.repository}.git`), prefix = ['--git-dir', mirror];
    const index = join(directory, `${engine}-variant.git-index`), environment = {GIT_INDEX_FILE: index};
    git([...prefix, 'read-tree', BASE_COMMIT], undefined, environment);
    if (change.bytes) assert.equal(git([...prefix, 'hash-object', '-w', '--stdin'], change.bytes).toString().trim(), change.blob);
    let update = '';
    if (id !== 'root-insert') update += `0 ${'0'.repeat(40)}\t${FIRST_PATH}\0`;
    if (change.bytes) update += `100644 ${change.blob}\t${change.path}\0`;
    git([...prefix, 'update-index', '-z', '--index-info'], Buffer.from(update), environment);
    const tree = git([...prefix, 'write-tree'], undefined, environment).toString().trim(); assert.match(tree, /^[0-9a-f]{40}$/);
    const raw = Buffer.from(`tree ${tree}\nparent ${BASE_COMMIT}\nauthor Pack diagnostics <fixture@example.invalid> 0 +0000\ncommitter Pack diagnostics <fixture@example.invalid> 0 +0000\n\nIsolated pack2 ${id}\n`);
    const commit = git([...prefix, 'hash-object', '-t', 'commit', '-w', '--stdin'], raw).toString().trim();
    assert.equal(commit, createHash('sha1').update(`commit ${raw.length}\0`).update(raw).digest('hex'));
    commits[engine] = {commit, tree, commitRaw: raw.toString()};
  }
  if (engines.length === 2) assert.deepEqual(commits.go, commits.rust, 'identical independently constructed variant trees required');
  const constructed = commits[engines[0]];
  const pin = fixturePin({repository: oracle.repository, product: oracle.product, requestedRef: constructed.commit, exactCommit: constructed.commit, parserRevision: 'source-v1'});
  const project = join(directory, 'variant-project'); mkdirSync(project);
  saveJSON(join(project, 'lycheedev.json'), {schema: 'lycheedev.project.v1', product: oracle.product});
  saveJSON(join(project, 'lycheedev.lock.json'), {schema: 'lycheedev.project-lock.v1', selection: pin});
  return {pin, project, oracle: variantOracle(oracle, change.rows, constructed.commit), mutation: {scenario: id, baseCommit: BASE_COMMIT, ...constructed, path: change.path, originalPath: FIRST_PATH, originalSHA256: change.originalSHA256, newBytes: change.bytes?.length ?? null, newSHA256: change.bytes ? sha256(change.bytes) : null, newBlob: change.blob ?? null, recipe: id === 'cross-prefix-rename' ? 'New pack2 scenario; unchanged original bytes moved across root prefix' : 'PR14 comment 6235614924 exact mutation'}, rows: change.rows};
}
export function captureMetadata(home, pin, directory, engine) {
  const path = join(home, 'state/toolkit.sqlite'), info = lstatSync(path);
  assert(info.isFile() && !info.isSymbolicLink() && info.size <= 16 << 20, 'bounded metadata');
  assert(!['-wal', '-shm', '-journal'].some(suffix => existsSync(path + suffix)), 'closed checkpointed metadata required');
  const expected = hashFile(path), copy = join(directory, `${engine}-metadata.sqlite`); copyFileSync(path, copy); assert.equal(hashFile(copy), expected);
  const db = new DatabaseSync(copy, {readOnly: true});
  try {
    assert.equal(db.prepare('PRAGMA user_version').get().user_version, 1);
    assert.deepEqual(db.prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name").all().map(row => ({...row})), [{type: 'table', name: 'deleted_generations'}, {type: 'table', name: 'documents'}]);
    const row = db.prepare('SELECT value FROM documents WHERE key = ?').get(`pin/${pin.id}`); assert(row, 'actual persisted command pin required'); assert.deepEqual(JSON.parse(Buffer.from(row.value)), pin);
  } finally { db.close(); }
  assert.equal(hashFile(path), expected, 'metadata changed during capture'); assert(!['-wal', '-shm', '-journal'].some(suffix => existsSync(path + suffix)));
  return {path: copy, sha256: expected};
}
export function indexEvidence(home, engine, pin, directory, root) {
  const source = pin.source; let path, manifest;
  const metadata = captureMetadata(home, pin, directory, engine);
  if (engine === 'go') {
    const key = sha256(['lycheedev.source-map.v2', source.repository, source.product, source.exactCommit, source.parserRevision].join('\0'));
    const base = join(home, 'source/v1/facts/snapshots', key); path = join(base, 'records.jsonl');
    manifest = JSON.parse(readFileSync(join(base, 'manifest.json')));
    assert.equal(manifest.recordsHash, hashFile(path, INDEX_BYTES)); assert.equal(manifest.recordBytes, lstatSync(path).size);
  } else {
    const identity = sha256(['rust-index-v14', 'facts-pack-v2-path-radix512', 'facts-layout-v11-go323-generated-origins', 'TS0.26.3', 'Lua0.5.0', 'XML0.38', 'generator-v6', source.repository, source.product, source.exactCommit, source.parserRevision].join('\0'));
    const db = new DatabaseSync(metadata.path, {readOnly: true});
    try {
      const row = db.prepare('SELECT value FROM documents WHERE key = ?').get(`source/rust-index/${identity}`); assert(row, 'exact pack2 index manifest required');
      manifest = JSON.parse(Buffer.from(row.value)); assert.equal(manifest.identity, identity); assert.equal(manifest.schema, 'lycheedev.source-index.rust.v6');
    } finally { db.close(); }
    digest(manifest.blob.sha256); path = join(home, 'cache/objects', manifest.blob.sha256.slice(0, 2), manifest.blob.sha256.slice(2));
    assert.equal(lstatSync(path).size, manifest.blob.bytes); assert.equal(hashFile(path, INDEX_BYTES), manifest.blob.sha256);
  }
  saveJSON(join(directory, `${engine}-index-manifest.json`), manifest);
  const sourceRef = {path, bytes: lstatSync(path).size, sha256: hashFile(path, INDEX_BYTES)};
  const parts = splitIndexEvidence(root, sourceRef, join(directory, `${engine}-index-parts`));
  return {...sourceRef, manifest, parts, metadata};
}
export async function compareFacts(homes, outputs, pin, oracle, directory, root, retained = undefined) {
  const indexes = retained ? {go: retained.index, rust: indexEvidence(homes.rust, 'rust', pin, directory, root)} : Object.fromEntries(['go', 'rust'].map(engine => [engine, indexEvidence(homes[engine], engine, pin, directory, root)]));
  const custody = retained && openGoCustody(retained.reference);
  const delta = await readFactDelta(custody?.home ?? homes.go, homes.rust, indexes.go.path, 'v11-pack2', indexes.rust.path, indexes.rust.sha256, custody);
  custody?.verify();
  saveJSON(join(directory, 'facts-comparison.json'), delta); // Retain complete mismatch diagnostics before failing.
  assert.equal(delta.differentFiles.length, 0, 'complete five-array fields/order differ'); assert.equal(delta.documents, oracle.documents);
  const values = Object.fromEntries(['go', 'rust'].map(engine => [engine, semantic('B02-retail-index', outputs[engine], engine, oracle, pin.id).value]));
  assert.deepEqual(values.go, values.rust, 'complete shared summary differs');
  for (const engine of ['go', 'rust']) for (const field of ['declarations', 'relationships', 'diagnostics']) assert.equal(delta.totals[engine][field], values[engine][field], `${engine} full Facts totals`);
  return {documents: delta.documents, fullFieldsAndOrderEqual: true, totals: delta.totals, indexes: Object.fromEntries(Object.entries(indexes).map(([k, v]) => [k, {path: v.path, bytes: v.bytes, sha256: v.sha256}])), summary: values.go, evidence: join(directory, 'facts-comparison.json')};
}

export async function runController(config, overrides = {}) {
  const svc = {beginScratch, admission, ownedSpawnSync, collectOwnedMemory, runnerIdentity, verifyCandidateRecord, verifyGoPackage, cloneSeed, inventory, derivedIdentity, proveWarmReuse, prepareVariant, compareFacts, verifyPackProfile, indexEvidence, captureGoCustody, openGoCustody, ...overrides};
  const {request, runnerRoot, inputs, conditions, scratch, evidence: out} = config;
  const diskPolicy = assertDiskPolicy(request.ownerRoot);
  const oracle = JSON.parse(readFileSync(join(runnerRoot, 'tests/rust-source-full-corpus/retail-oracle.json')));
  assert.equal(oracle.commit, BASE_COMMIT); assert.equal(oracle.documents, 4036);
  let fatal = false, sequence = 0, baseOwner, baseSuccess = false;
  const start = performance.now();
  const report = {schema: 'lycheedev.source-pack-scenarios.v1', diskPolicy, state: 'running', request, inputs, runnerRoot, conditions, productCommit: CANDIDATE_COMMIT, layout: 'v11-pack2', importedSamples: 0, performanceAcceptanceEvaluated: false, fullB01B07Acceptance: false, preparation: [], scenarios: [], owners: [], finalIntegrityVerified: false,
    machine: {hostname: os.hostname(), platform: process.platform, node: process.version, cpus: os.cpus(), totalMemory: os.totalmem()},
    timing: 'Native total CreateProcessW through descendant reaping/output drain; 31 overlapping phases are attribution, never subtracted or added. Wrapper wall separate.', cacheDefinition: 'Fresh own init/sync-only homes for cold; same successful base homes for warm; each changed pair copies the original healthy base independently. No OS cache flush.', metrics: {cpu: 'not_measured', io: 'not_measured', rss: 'not_measured; owned-Job sampled aggregate and gaps retained, not unique physical RSS'}};
  const save = () => saveJSON(join(out, 'report.json'), report);
  function controlled(binary, args, cwd, evidence, measured = false, timeout = 210000, engine) {
    assert(!fatal, 'unverified process retirement; no further launch'); fatal = true;
    const at = performance.now(); const run = svc.ownedSpawnSync(binary, args, {cwd, evidence, env: sourcePhaseEnvironment(process.env, measured && engine === 'rust'), timeout, orphanMs: 30000, memoryObservation: measured});
    fatal = run.handlesReaped !== true;
    const row = {binary, args, cwd, wrapperWallMillis: performance.now() - at, exit: run.status, error: run.error?.message, handlesReaped: run.handlesReaped, proof: run.proof, logs: run.logs, stdoutSHA256: sha256(run.stdout), stderrSHA256: sha256(run.stderr), valid: false};
    if (measured) row.memory = svc.collectOwnedMemory(run, {evidence, sourceBinary: binary, runState: run.status === 0 && !run.error ? 'passed_invocation' : 'failed_invocation'});
    return {run, row};
  }
  function prepare(binary, args, label, cwd = request.ownerRoot, timeout) {
    const {run, row} = controlled(binary, args, cwd, out, false, timeout); report.preparation.push({...row, label}); save();
    assert(run.handlesReaped && !run.error && run.status === 0, `${label}: product/preparation failure`); elapsed(run.proof); return run.stdout;
  }
  const gitEnv = Object.fromEntries(Object.entries(sourcePhaseEnvironment(process.env)).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE'));
  Object.assign(gitEnv, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: process.platform === 'win32' ? 'NUL' : '/dev/null', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1'});
  function git(args, input, environment = {}, cwd = request.ownerRoot) {
    assert(!fatal); const run = spawnSync('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-c', 'core.hooksPath=' + (process.platform === 'win32' ? 'NUL' : '/dev/null'), ...args], {cwd, input, env: {...gitEnv, ...environment}, timeout: 60000, maxBuffer: 16 << 20, windowsHide: true, shell: false});
    const prefix = join(out, `git-${sequence++}`), stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0);
    writeFileSync(prefix + '.stdout.bin', stdout); writeFileSync(prefix + '.stderr.bin', stderr);
    report.preparation.push({label: 'owned-outer-job Git fixture/audit', args, cwd, inputSHA256: input === undefined ? null : sha256(Buffer.from(input)), exit: run.status, error: run.error?.message, stdoutSHA256: sha256(stdout), stderrSHA256: sha256(stderr)});
    if (run.error || run.signal) fatal = true; save(); assert(!run.error && run.status === 0, 'Git fixture/audit failed'); return stdout;
  }
  function identities() {
    assert.deepEqual(svc.runnerIdentity(runnerRoot), inputs, 'runner changed');
    const candidate = svc.verifyCandidateRecord(request.candidate);
    if (!overrides.verifyCandidateRecord) {
      for (const [root, commit] of [[request.candidate.sourceRoot, CANDIDATE_COMMIT], [request.go.sourceRoot, GO_COMMIT]]) {
        assert.equal(realpathSync(git(['rev-parse', '--show-toplevel'], undefined, {}, root).toString().trim()), root, 'exact checkout root required');
        assert.equal(git(['rev-parse', 'HEAD'], undefined, {}, root).toString().trim(), commit);
        assert.equal(git(['status', '--porcelain', '--untracked-files=all'], undefined, {}, root).toString().trim(), '', 'fixed source checkout must be clean');
      }
    }
    const go = svc.verifyGoPackage(request.go.packageRoot, request.go.luaLSArchive); assert.equal(go.packageSHA256, request.go.packageSHA256);
    return {rust: {binary: candidate.sourceBinary, cwd: request.candidate.sourceRoot}, go: {binary: go.binary, cwd: request.go.sourceRoot}};
  }
  function prepareSeeds(homes, binaries, evidence) {
    const pins = {}, rows = {};
    for (const engine of ['go', 'rust']) {
      assert(!existsSync(homes[engine]));
      assertEnvelope({exit: 0, envelope: JSON.parse(prepare(binaries[engine].binary, ['init', '--home', homes[engine], '--format', 'json'], `${engine}-init`, binaries[engine].cwd))});
      pins[engine] = assertEnvelope({exit: 0, envelope: JSON.parse(prepare(binaries[engine].binary, ['source', 'sync', '--source', oracle.repository, '--product', oracle.product, '--ref', BASE_COMMIT, '--home', homes[engine], '--format', 'json'], `${engine}-sync-network-and-local-preparation`, binaries[engine].cwd, 240000))});
      const pin = pins[engine]; assert.deepEqual(pin, fixturePin({repository: oracle.repository, product: oracle.product, requestedRef: BASE_COMMIT, exactCommit: BASE_COMMIT, parserRevision: 'source-v1'}));
      auditCorpus(join(homes[engine], 'mirrors', `${oracle.repository}.git`), oracle, (args, input) => git(args, input), evidence, engine + '-');
      const metadata = verifySeedMetadata(homes[engine], pin, join(evidence, `${engine}-seed-metadata.sqlite`));
      const identity = assertSeedCold(homes[engine]); rows[engine] = JSON.parse(readFileSync(join(evidence, `${engine}-verified-corpus-manifest.json`))).files;
      report.preparation.push({label: `${engine}-authenticated-sync-only`, home: homes[engine], pin, metadata, identity}); save();
    }
    assert.deepEqual(pins.go, pins.rust); assert.deepEqual(rows.go, rows.rust); assertCorpus(rows.go, oracle);
    return {pin: pins.go, rows: rows.go};
  }
  async function scenario(entry, index, homes, selection, evidence, binaries, lineage) {
    const row = {id: entry.id, mode: entry.mode, order: orderFor(index), pin: selection.pin, lineage, state: 'running', factsEquivalent: false}; report.scenarios.push(row); save();
    const outputs = {}, before = {};
    try {
      if (entry.mode === 'warm') for (const engine of ['go', 'rust']) before[engine] = svc.derivedIdentity(homes[engine], engine);
      // Fixture, tree, source, metadata and warm-before preparation all finish
      // before either timed invocation. Never subtract product work afterward.
      for (const engine of row.order) {
        const args = ['source', 'index', ...(selection.project ? ['--project', selection.project] : ['--snapshot', selection.pin.id]), '--home', homes[engine], '--format', 'json'];
        const output = controlled(binaries[engine].binary, args, binaries[engine].cwd, evidence, true, 210000, engine);
        row[engine] = {...output.row, home: homes[engine]}; outputs[engine] = output; save();
        assert(!fatal, 'unverified process retirement');
      }
      const envelopes = {};
      for (const engine of ['go', 'rust']) {
        const {run} = outputs[engine]; assert(run.status === 0 && !run.error && run.handlesReaped, `${engine} index failed`);
        Object.assign(row[engine], elapsed(run.proof)); const envelope = JSON.parse(run.stdout); envelopes[engine] = envelope;
        semantic('B02-retail-index', envelope, engine, selection.oracle, selection.pin.id);
        assert.equal(envelope.context?.snapshot, selection.pin.id, 'actual command selection must match prepared pin');
        if (engine === 'rust') {
          row.rust.sourceProfile = svc.verifyPackProfile(run.stderr, envelope, {mode: entry.mode, documents: entry.documents});
          if (entry.maxParsed) assert(envelope.result.io.parsedDocuments <= entry.maxParsed, 'healthy pack2 incremental parse bound exceeded');
        }
        if (entry.mode === 'warm') {
          const after = svc.derivedIdentity(homes[engine], engine);
          saveJSON(join(evidence, `${engine}-warm-before.json`), before[engine]); saveJSON(join(evidence, `${engine}-warm-after.json`), after);
          row[engine].warmReuse = svc.proveWarmReuse(engine, before[engine], after, envelope);
        }
        row[engine].valid = true;
      }
      row.facts = await svc.compareFacts(homes, envelopes, selection.pin, selection.oracle, evidence, request.ownerRoot);
      row.factsEquivalent = true; identities(); row.rustToGoRatio = row.rust.controllableMillis / row.go.controllableMillis; row.state = 'verified'; return row;
    } catch (error) { row.state = fatal ? 'interrupted' : 'failed'; row.failure = error.message.slice(0, 4096); throw error; }
    finally { saveJSON(join(evidence, `${entry.id}-scenario.json`), row); save(); }
  }
  save();
  try {
    const binaries = identities();
    if (!overrides.prepareSeeds) report.goMetadata = verifyGoMetadata(prepare('go', ['version', '-m', binaries.go.binary], 'Go323 binary metadata').toString());
    report.machine.powerBefore = prepare('powercfg.exe', ['/getactivescheme'], 'power-before').toString();
    const coldEstimate = footprint({cold: true}); report.coldAdmissionEstimate = coldEstimate;
    baseOwner = svc.beginScratch(request.ownerRoot, 'source-pack2-healthy-base', {command: ['source', 'index'], inputs, admissionOptions: coldEstimate}); report.owners.push({role: 'healthy-base', receipt: baseOwner.receipt}); save();
    const homes = Object.fromEntries(['go', 'rust'].map(engine => [engine, join(baseOwner.directory, `${engine}-home`)]));
    const prepared = (overrides.prepareSeeds ?? prepareSeeds)(homes, binaries, baseOwner.evidence);
    const selection = {pin: prepared.pin, oracle};
    const coldEvidence = join(baseOwner.evidence, 'cold'); mkdirSync(coldEvidence);
    const cold = await scenario(CASES[0], 0, homes, selection, coldEvidence, binaries, {state: 'authenticated-own-sync-only'});
    const coldInventory = Object.fromEntries(['go', 'rust'].map(engine => [engine, svc.inventory(homes[engine])]));
    report.actualColdHomeBytes = coldInventory.go.bytes + coldInventory.rust.bytes;
    assert(report.actualColdHomeBytes <= coldEstimate.scratchReserveBytes, 'actual cold footprint exceeded admitted estimate');
    report.postColdAdmission = svc.admission(request.ownerRoot);
    const warmEvidence = join(baseOwner.evidence, 'warm'); mkdirSync(warmEvidence);
    await scenario(CASES[1], 1, homes, selection, warmEvidence, binaries, {state: 'same-cold-home', cold: coldEvidence});
    const baseIdentity = Object.fromEntries(['go', 'rust'].map(engine => [engine, svc.inventory(homes[engine])]));
    saveJSON(join(baseOwner.evidence, 'healthy-base-inventories.json'), baseIdentity);
    for (let index = 2; index < CASES.length; index++) {
      const entry = CASES[index], row = {id: entry.id, mode: entry.mode, order: orderFor(index), state: 'running', factsEquivalent: false,
        pairing: 'Sequential own-engine owners; independently prepared between samples, not an uninterrupted adjacent pair',
        lineage: {state: 'independent-healthy-base-copy', inventory: join(baseOwner.evidence, 'healthy-base-inventories.json'), sha256: Object.fromEntries(['go', 'rust'].map(e => [e, baseIdentity[e].sha256]))}};
      report.scenarios.push(row); save(); const outputs = {}; let goRetained, expectedVariant;
      for (const engine of row.order) {
        identities();
        for (const side of ['go', 'rust']) assert.equal(svc.inventory(homes[side]).sha256, baseIdentity[side].sha256, 'healthy base changed');
        const inventory = baseIdentity[engine], factBytes = inventory.rows.filter(r => engine === 'go' ? r.path.startsWith('source/v1/facts/files/') : r.path.startsWith('cache/source-facts-rust/')).reduce((n, r) => n + r.bytes, 0);
        const indexBytes = cold.facts.indexes[engine].bytes;
        // Go's content-keyed per-file cache adds at most one changed/inserted
        // file in these exact recipes. Reserve the largest existing fact plus
        // 1 MiB for that bounded new tiny source; Rust conservatively reserves
        // a complete additional Facts generation, not predicted cache hits.
        const growth = engine === 'go' ? Math.max(0, ...inventory.rows.filter(r => r.path.startsWith('source/v1/facts/files/')).map(r => r.bytes)) + (1 << 20) : factBytes;
        const estimate = {scratchReserveBytes: inventory.bytes + indexBytes + growth + (8 << 20), requiredBytes: inventory.bytes + 2 * indexBytes + growth + factBytes + variantAuditBytes(oracle) + (48 << 20),
          basis: 'Measured own healthy home, complete new index, bounded exact-recipe Facts growth, 8 MiB fixture/metadata allowance; complete output custody separately included in free-space estimate', engine, baseBytes: inventory.bytes, indexBytes, growthBytes: growth, factsBytes: factBytes};
        const job = svc.beginScratch(request.ownerRoot, `source-pack2-${entry.id}-${engine}`, {command: ['source', 'index'], inputs: {...inputs, healthyBase: inventory.sha256}, admissionOptions: estimate});
        report.owners.push({role: entry.id, engine, receipt: job.receipt, estimate}); save(); let success = false;
        try {
          const home = join(job.directory, `${engine}-home`); svc.cloneSeed(homes[engine], home, inventory);
          const variant = svc.prepareVariant({homes: {[engine]: home}, directory: job.directory, baseRows: prepared.rows, oracle, id: entry.id, git});
          if (expectedVariant) assert.deepEqual(variant.mutation, expectedVariant.mutation, 'same exact independent mutation/tree/commit required'); else expectedVariant = variant;
          row.pin = variant.pin; saveJSON(join(job.evidence, 'mutation.json'), variant);
          if (!overrides.prepareVariant) auditCorpus(join(home, 'mirrors', `${oracle.repository}.git`), variant.oracle, (args, input) => git(args, input), job.evidence, engine + '-variant-');
          if (goRetained && !overrides.openGoCustody) { const authenticated = svc.openGoCustody(goRetained.reference); authenticated.recordsStream.destroy(); authenticated.verify(); }
          const args = ['source', 'index', '--project', variant.project, '--home', home, '--format', 'json'];
          const output = controlled(binaries[engine].binary, args, binaries[engine].cwd, job.evidence, true, 210000, engine);
          row[engine] = {...output.row, home, fixture: join(job.evidence, 'mutation.json')}; save();
          assert(!fatal && output.run.status === 0 && !output.run.error, `${engine} changed invocation failed`);
          Object.assign(row[engine], elapsed(output.run.proof));
          const envelope = JSON.parse(output.run.stdout); semantic('B02-retail-index', envelope, engine, variant.oracle, variant.pin.id);
          assert.equal(envelope.context?.snapshot, variant.pin.id); outputs[engine] = envelope;
          if (engine === 'rust') {
            row.rust.sourceProfile = svc.verifyPackProfile(output.run.stderr, envelope, {mode: 'changed', documents: entry.documents});
            assert(envelope.result.io.parsedDocuments <= entry.maxParsed, 'healthy pack2 parse bound exceeded');
          }
          row[engine].valid = true;
          // Check actual footprint after the owned process is reaped. Never
          // expand the policy or remove another owner to conceal an overrun.
          const measured = svc.inventory(home); row[engine].actualHomeBytes = measured.bytes;
          assert(measured.bytes <= estimate.scratchReserveBytes, 'actual scenario footprint exceeded admitted estimate'); row[engine].postAdmission = svc.admission(request.ownerRoot);
          if (engine === 'go') {
            const index = svc.indexEvidence(home, engine, variant.pin, job.evidence, request.ownerRoot);
            const reference = await svc.captureGoCustody(home, index, job.evidence, request.ownerRoot); goRetained = {index, reference};
            row.go.custody = reference; row.go.comparisonState = 'awaiting-independent-rust';
          } else {
            row.facts = await svc.compareFacts({go: row.go.home, rust: home}, outputs, variant.pin, variant.oracle, job.evidence, request.ownerRoot, goRetained);
            row.factsEquivalent = true; row.go.comparisonState = 'verified'; row.rustToGoRatio = row.rust.controllableMillis / row.go.controllableMillis;
          }
          identities(); for (const side of ['go', 'rust']) assert.equal(svc.inventory(homes[side]).sha256, baseIdentity[side].sha256, 'original healthy base changed');
          success = true;
        } catch (error) { row.state = fatal ? 'interrupted' : 'failed'; row.failure = error.message.slice(0, 4096); throw error; }
        finally {
          saveJSON(join(job.evidence, `${entry.id}-${engine}.json`), row); save();
          job.seal({state: fatal ? 'interrupted' : success ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: evidenceFiles(job.evidence)});
          if (success && !fatal) job.cleanup();
        }
      }
      assert(row.factsEquivalent); row.state = 'verified'; save();
    }
    identities(); report.machine.powerAfter = prepare('powercfg.exe', ['/getactivescheme'], 'power-after').toString(); assert.equal(report.machine.powerAfter, report.machine.powerBefore);
    assert.equal(report.scenarios.length, 6); assert(report.scenarios.every(row => row.state === 'verified'));
    report.finalIntegrityVerified = true; report.state = 'source_pack_scenarios_observed'; baseSuccess = true;
  } catch (error) { report.blocker = error.message.slice(0, 4096); if (error.report) report.admissionFailure = error.report; report.state = 'source_pack_scenarios_failed'; }
  finally {
    report.requiresOuterRetirement = fatal; report.wallMillis = performance.now() - start;
    if (baseOwner) {
      try { baseOwner.seal({state: fatal ? 'interrupted' : baseSuccess ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: evidenceFiles(baseOwner.evidence)}); if (baseSuccess && !fatal) baseOwner.cleanup(); }
      catch (error) { report.state = 'source_pack_scenarios_failed'; report.cleanupFailure = error.message; }
    }
    save();
  }
  return report;
}
export function verifyObservedReport(run, report, request, inputs) {
  assert(run.status === 0 && !run.error && run.handlesReaped); elapsed(run.proof);
  assert.equal(report?.state, 'source_pack_scenarios_observed'); assert.equal(report.finalIntegrityVerified, true); assert.equal(report.requiresOuterRetirement, false);
  assert.equal(report.performanceAcceptanceEvaluated, false); assert.equal(report.fullB01B07Acceptance, false); assert.equal(report.importedSamples, 0);
  assert.deepEqual(report.request, request); assert.deepEqual(report.inputs, inputs); assert.equal(report.layout, 'v11-pack2'); assert.equal(report.productCommit, CANDIDATE_COMMIT);
  assert.deepEqual(report.scenarios.map(row => row.id), CASES.map(row => row.id));
  const variantHomes = new Set();
  report.scenarios.forEach((row, i) => {
    assert.equal(row.state, 'verified'); assert.equal(row.factsEquivalent, true); assert.deepEqual(row.order, orderFor(i)); assert.equal(row.mode, CASES[i].mode);
    assert.equal(row.facts.documents, CASES[i].documents); assert.equal(row.facts.fullFieldsAndOrderEqual, true);
    for (const engine of ['go', 'rust']) { assert.equal(row[engine].valid, true); elapsed(row[engine].proof); if (i > 1) { assert(!variantHomes.has(row[engine].home)); variantHomes.add(row[engine].home); assert.notEqual(row[engine].home, report.scenarios[0][engine].home); } }
    if (i === 1) for (const engine of ['go', 'rust']) { assert.equal(row[engine].home, report.scenarios[0][engine].home); assert.equal(row[engine].warmReuse.verified, true); }
  });
  return true;
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  assert.equal(process.platform, 'win32'); assert.equal(process.argv.length, 3);
  const file = process.argv[2], info = lstatSync(file); assert(info.isFile() && !info.isSymbolicLink() && info.size <= 1 << 20, 'bounded owned controller configuration');
  const config = JSON.parse(readFileSync(file)); config.request = validateRequest(config.request); validateConditions(config.conditions);
  const owner = JSON.parse(readFileSync(join(config.evidence, 'receipt.json')));
  assert.equal(owner.schema, 'lycheedev.disk-owner.v1'); assert.equal(owner.owner, 'source-pack2-diagnostics'); assert.equal(owner.state, 'active');
  assert.match(owner.id, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/);
  assert.equal(resolve(owner.directory), resolve(config.scratch));
  assert.equal(resolve(config.scratch), join(config.request.ownerRoot, '.tmp/rust-scratch', owner.id));
  assert.equal(resolve(config.evidence), join(config.request.ownerRoot, '.tmp/rust-disk/evidence', owner.id));
  const report = await runController(config); console.log(JSON.stringify({state: report.state, report: join(config.evidence, 'report.json')}));
  if (report.state !== 'source_pack_scenarios_observed') process.exitCode = 1;
}
