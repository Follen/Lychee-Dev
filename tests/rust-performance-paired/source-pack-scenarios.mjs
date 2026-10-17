// One six-scenario observation; also one fixed, authenticated cold continuation.
// No builds, historical sample import or acceptance change.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isAbsolute, join, relative, resolve} from 'node:path';
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
export function assertDiskPolicy(root, environment = process.env, allowance = undefined, remaining = undefined) {
  assert(!Object.keys(environment).some(key => /^LYCHEEDEV_DISK_/i.test(key)), 'diagnostic disk policy overrides are forbidden');
  const observed = policy(root, {}); assert.deepEqual(observed, {...defaults}, 'fixed original disk policy required');
  if (allowance) { validateRemainingReference(remaining); assert.deepEqual(allowance, REMAINING_ALLOWANCE, 'only explicitly authorized fixed remaining-task 6 GiB allowance'); return {...observed, scratchBytes: allowance.scratchBytes}; }
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
export function metadataIdentity(home) {
  const path = contained(home, join(home, 'state/toolkit.sqlite'));
  const identities = {};
  for (const [suffix, maximum] of [['', 16 << 20], ['-wal', 0], ['-shm', 1 << 20], ['-journal', 0]]) {
    const file = path + suffix;
    let stat; try { stat = lstatSync(file, {bigint: true}); } catch (error) { if (error.code !== 'ENOENT') throw error; assert(suffix !== '', 'metadata database missing'); identities[suffix || 'database'] = null; continue; }
    assert(suffix !== '-journal', 'rollback journal is not admitted for a read-only snapshot');
    assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= BigInt(maximum), suffix === '-wal' ? 'nonempty WAL is not admitted for a read-only snapshot' : 'bounded metadata file/type');
    identities[suffix || 'database'] = {path: file, bytes: Number(stat.size), sha256: hashFile(file, maximum), device: stat.dev.toString(), fileId: stat.ino.toString(), modifiedNs: stat.mtimeNs.toString(), changedNs: stat.ctimeNs.toString()};
  }
  // An empty/no WAL has no committed frames. SHM is nonpersistent coordination
  // state, retained and authenticated but never copied/opened as input to SQLite.
  return identities;
}
export function captureMetadata(home, pin, directory, engine, {afterCopy} = {}) {
  const before = metadataIdentity(home), path = before.database.path, expected = before.database.sha256;
  const copy = join(directory, `${engine}-metadata.sqlite`); copyFileSync(path, copy); assert.equal(hashFile(copy), expected);
  afterCopy?.(); assert.deepEqual(metadataIdentity(home), before, 'original metadata/sidecars changed during copy');
  const db = new DatabaseSync(copy, {readOnly: true});
  try {
    assert.deepEqual(db.prepare('PRAGMA integrity_check').all().map(row => ({...row})), [{integrity_check: 'ok'}], 'complete copied SQLite integrity');
    assert.equal(db.prepare('PRAGMA user_version').get().user_version, 1);
    assert.deepEqual(db.prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name").all().map(row => ({...row})), [{type: 'table', name: 'deleted_generations'}, {type: 'table', name: 'documents'}]);
    const row = db.prepare('SELECT value FROM documents WHERE key = ?').get(`pin/${pin.id}`); assert(row, 'actual persisted command pin required'); assert.deepEqual(JSON.parse(Buffer.from(row.value)), pin);
  } finally { db.close(); }
  assert.equal(hashFile(copy), expected, 'copied database changed during read');
  assert.deepEqual(metadataIdentity(home), before, 'original metadata/sidecars changed during validation');
  saveJSON(join(directory, `${engine}-metadata-source.json`), {schema: 'lycheedev.source-metadata-snapshot.v1', mode: 'main-database-with-absent-or-empty-WAL', originalOpened: false, originalModified: false, before, after: before, copySHA256: expected});
  return {path: copy, sha256: expected, original: before};
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

// Deliberately limited to the one 7d8 metadata failure reported in PR 14/6236649962.
export const RETAINED_COLD = Object.freeze({outer: '2f0bcd33-f0f4-46b6-9bc5-5539dbc5b422', base: '058035ab-29e8-43c7-bbeb-ee54472f8147', go: 'bfae709f-783a-4eb0-ae9e-70e949163034', rust: '0ee84792-5d7e-4f59-b0a2-44d2e6779c61', runnerSHA256: 'aeeed67ad78777c8ad708d1c669684b1c7e653a6d0732fa77623d42d21c87e9e'});
const retainedPaths = root => ({outer: contained(root, join(root, '.tmp/rust-disk/evidence', RETAINED_COLD.outer)), base: contained(root, join(root, '.tmp/rust-disk/evidence', RETAINED_COLD.base)), homes: Object.fromEntries(['go', 'rust'].map(e => [e, contained(root, join(root, '.tmp/rust-scratch', RETAINED_COLD.base, `${e}-home`))])), claim: contained(root, join(root, '.tmp/rust-disk', `source-pack2-cold-${RETAINED_COLD.base}.json`))});
const boundedJSON = (path, maximum = 16 << 20) => { const stat = lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, 'bounded regular JSON evidence'); return JSON.parse(readFileSync(path)); };
function authenticateSealedOwner(root, directory, id, owner, state, expected, hashes) {
  const path = join(directory, 'receipt.json'); digest(expected); assert.equal(hashFile(path, 16 << 20), expected, 'prior owner receipt changed'); hashes.push({path, sha256: expected});
  const receipt = boundedJSON(path); assert.equal(receipt.schema, 'lycheedev.disk-owner.v1'); assert.equal(receipt.id, id); assert.equal(receipt.owner, owner); assert.equal(receipt.category, 'scratch'); assert.equal(receipt.retention, 'rebuildable'); assert.equal(receipt.state, state); assert.equal(receipt.handlesReaped, true); assert.equal(receipt.activePid, null); assert(receipt.sealedAt);
  assert.equal(resolve(receipt.directory), contained(root, join(root, '.tmp/rust-scratch', id))); assert.equal(directory, contained(root, join(root, '.tmp/rust-disk/evidence', id)));
  assert(Array.isArray(receipt.sealedLogs) && receipt.sealedLogs.length > 0 && receipt.sealedLogs.length < (owner === 'source-pack2-cold-revalidation' ? 10000 : 2000)); const known = new Set();
  for (const log of receipt.sealedLogs) { const path = contained(directory, log.path); assert.equal(log.original, path); assert(!known.has(path), 'duplicate sealed log'); known.add(path); digest(log.sha256); assert.equal(hashFile(path, 16 << 20), log.sha256, 'prior sealed log changed'); hashes.push({path, sha256: log.sha256}); }
  assert.deepEqual(evidenceFiles(directory).sort(), [...known].sort(), 'all prior evidence must be sealed');
  assert.deepEqual(receipt.logHashes, Object.fromEntries(receipt.sealedLogs.map(log => [relative(root, log.path), log.sha256]))); return receipt;
}
function authenticateRecordedCall(row, home, stem, runId, pin, inputs) {
  assert.match(runId, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/);
  const req = boundedJSON(stem + '.request.json'), proof = boundedJSON(stem + '.receipt.json'), wrapper = boundedJSON(stem + '.request.json.wrapper.json');
    assert.equal(row.home, home); assert.equal(row.exit, 0); assert.equal(row.valid, true); assert.equal(row.handlesReaped, true); assert(!row.error); assert.equal(req.schema, 'lycheedev.owned-request.v2'); assert.equal(req.runId, runId); assert.equal(req.command, row.binary); assert.equal(req.cwd, row.cwd); assert.equal(req.timeoutMs, 210000); assert.equal(req.orphanMs, 30000); assert.equal(req.cargoBuild, false); assert.equal(req.msvcCompile, false); assert(!req.startupDiagnostics);
    assert.deepEqual(req.args, ['source', 'index', '--snapshot', pin.id, '--home', home, '--format', 'json']); assert.deepEqual(row.args, req.args);
    for (const [key, suffix] of [['stdout', '.stdout.bin'], ['stderr', '.stderr.bin'], ['receipt', '.receipt.json'], ['identity', '.identity.json'], ['rootIdentity', '.root.json'], ['memoryPrefix', '.memory'], ['memoryMarker', '.phase']]) assert.equal(req[key], stem + suffix);
    const startup = boundedJSON(req.identity), rootIdentity = boundedJSON(req.rootIdentity); assert.equal(startup.schema, 'lycheedev.owned-start.v1'); assert.equal(startup.runId, req.runId); assert.equal(startup.requestSHA256, hashFile(stem + '.request.json')); assert.equal(startup.wrapperObservedSHA256, inputs['tools/rust-owned-process.ps1']); assert.equal(startup.rootAssignment, 'PROC_THREAD_ATTRIBUTE_JOB_LIST'); assert.equal(startup.historicalReapProof, false); assert.equal(rootIdentity.schema, 'lycheedev.owned-root.v1'); assert.equal(rootIdentity.runId, req.runId); assert.equal(rootIdentity.jobAssignedAtCreation, true); assert.equal(rootIdentity.historicalReapProof, false); assert.equal(rootIdentity.pid, proof.pid); assert.equal(rootIdentity.creationFileTime, proof.creationFileTime); assert.equal(proof.startupIdentity, req.identity); assert.equal(proof.rootIdentity, req.rootIdentity);
    assert.equal(proof.runId, req.runId); assert.equal(proof.binary, row.binary); assert.equal(proof.exitCode, 0); assert.deepEqual(proof, row.proof); const timing = elapsed(proof); for (const [key, value] of Object.entries(timing)) assert.equal(row[key], value);
    assert.equal(wrapper.status, 0); assert(!wrapper.error && !wrapper.signal); assert(row.memory && row.memory.state, 'original memory observation/gaps required');
    const raw = readFileSync(req.stdout), stderr = readFileSync(req.stderr); assert.equal(sha256(raw), row.stdoutSHA256); assert.equal(sha256(stderr), row.stderrSHA256); return {envelope: JSON.parse(raw), stderr, row: structuredClone(row)};
}
export function validateColdReference(value) {
  exact(value, ['schema', 'outerReceiptSHA256', 'baseReceiptSHA256', 'homeInventorySHA256']); assert.equal(value.schema, 'lycheedev.source-pack2-retained-cold.v1');
  digest(value.outerReceiptSHA256); digest(value.baseReceiptSHA256); exact(value.homeInventorySHA256, ['go', 'rust']); for (const hash of Object.values(value.homeInventorySHA256)) digest(hash); return value;
}
export function inspectRetainedCold(request) {
  const paths = retainedPaths(request.ownerRoot);
  const reference = {schema: 'lycheedev.source-pack2-retained-cold.v1', outerReceiptSHA256: hashFile(join(paths.outer, 'receipt.json'), 16 << 20), baseReceiptSHA256: hashFile(join(paths.base, 'receipt.json'), 16 << 20), homeInventorySHA256: Object.fromEntries(['go', 'rust'].map(e => [e, inventory(paths.homes[e]).sha256]))};
  authenticateRetainedCold(request, reference); return reference;
}
export function authenticateRetainedCold(request, reference) {
  validateColdReference(reference); const paths = retainedPaths(request.ownerRoot), receipts = {}, hashes = [];
  assert(!existsSync(paths.claim), 'this fixed cold continuation was already claimed; no automatic replacement samples');
  for (const [role, owner] of [['outer', 'source-pack2-diagnostics'], ['base', 'source-pack2-healthy-base']]) receipts[role] = authenticateSealedOwner(request.ownerRoot, paths[role], RETAINED_COLD[role], owner, 'failed', reference[`${role}ReceiptSHA256`], hashes);
  const oldConfig = boundedJSON(join(paths.outer, 'configuration.json')), oldReport = boundedJSON(join(paths.outer, 'report.json')), owned = boundedJSON(join(paths.outer, 'owned-report.json'));
  assert.deepEqual(oldConfig.request, request); assert.deepEqual(oldReport.request, request); assert.deepEqual(owned.request, request);
  assert.deepEqual(Object.keys(oldConfig.inputs).sort(), [...RUNNER_FILES].sort()); assert.equal(sha256(JSON.stringify(RUNNER_FILES.map(path => [path, oldConfig.inputs[path]]))), RETAINED_COLD.runnerSHA256, 'exact original 7d8 runner required');
  assert.deepEqual(oldReport.inputs, oldConfig.inputs); assert.deepEqual(owned.inputs, oldConfig.inputs); assert.deepEqual(receipts.outer.inputHashes.runner, oldConfig.inputs); assert.deepEqual(receipts.base.inputHashes, oldConfig.inputs);
  assert.equal(oldConfig.evidence, paths.outer); assert.equal(oldConfig.scratch, receipts.outer.directory);
  assert.equal(oldReport.schema, 'lycheedev.source-pack-scenarios.v1'); assert.equal(oldReport.state, 'source_pack_scenarios_failed'); assert.equal(oldReport.blocker, 'closed checkpointed metadata required'); assert.equal(oldReport.finalIntegrityVerified, false); assert.equal(oldReport.requiresOuterRetirement, false); assert.equal(oldReport.importedSamples, 0);
  assert.equal(owned.schema, 'lycheedev.source-pack-scenarios-owned.v1'); assert.equal(owned.state, 'failed'); assert.equal(owned.process.exit, 1); assert.equal(owned.process.handlesReaped, true); elapsed(owned.process.proof);
  assert.equal(oldReport.scenarios.length, 1); const cold = oldReport.scenarios[0]; assert.equal(cold.id, 'cold'); assert.equal(cold.mode, 'cold'); assert.equal(cold.state, 'failed'); assert.equal(cold.factsEquivalent, false); assert.equal(cold.failure, 'closed checkpointed metadata required'); assert.deepEqual(cold.order, ['go', 'rust']); assert.equal(cold.lineage.state, 'authenticated-own-sync-only');
  const pin = fixturePin({repository: 'wow-ui-source', product: 'retail', requestedRef: BASE_COMMIT, exactCommit: BASE_COMMIT, parserRevision: 'source-v1'}); assert.deepEqual(cold.pin, pin);
  const outputs = {}, inventories = {}, manifests = {};
  for (const engine of ['go', 'rust']) {
    outputs[engine] = authenticateRecordedCall(cold[engine], paths.homes[engine], join(paths.base, 'cold', `process-${RETAINED_COLD[engine]}`), RETAINED_COLD[engine], pin, oldConfig.inputs);
    inventories[engine] = inventory(paths.homes[engine]); assert.equal(inventories[engine].sha256, reference.homeInventorySHA256[engine], 'retained home inventory changed'); metadataIdentity(paths.homes[engine]);
    manifests[engine] = boundedJSON(join(paths.base, `${engine}-verified-corpus-manifest.json`)); assert.equal(manifests[engine].commit, BASE_COMMIT);
    const seed = oldReport.preparation.find(p => p.label === `${engine}-authenticated-sync-only`); assert(seed); assert.deepEqual(seed.pin, pin); assert.equal(seed.home, paths.homes[engine]); assert.equal(seed.metadata.checkpointed, true); assert.equal(seed.metadata.documents, 1); assert.equal(seed.metadata.deletedGenerations, 0); assert.equal(seed.metadata.documentKey, `pin/${pin.id}`); assert.equal(seed.metadata.evidenceCopy, join(paths.base, `${engine}-seed-metadata.sqlite`)); assert.equal(hashFile(seed.metadata.evidenceCopy, 16 << 20), seed.metadata.sha256); assert.equal(sha256(JSON.stringify(seed.identity.rows)), seed.identity.sha256); assert(seed.identity.rows.every(r => !/^(source|cache|indexes|blobs|runs|captures|tmp)\//.test(r.path)), 'original seed must contain no derived results');
  }
  assert.deepEqual(manifests.go, manifests.rust);
  const verifyEvidence = () => { for (const item of hashes) assert.equal(hashFile(item.path, 16 << 20), item.sha256, 'original failed evidence changed'); };
  const verifyHomes = () => { for (const engine of ['go', 'rust']) assert.equal(inventory(paths.homes[engine]).sha256, reference.homeInventorySHA256[engine], 'retained home changed before warm'); };
  return {paths, reference, oldReport, cold, pin, rows: manifests.go.files, outputs, inventories, verifyEvidence, verifyHomes};
}
export function claimColdBorrow(root, reference, owner, before, custodyReceipt) {
  validateColdReference(reference); const paths = retainedPaths(root);
  const receipt = boundedJSON(owner.receipt); assert.equal(receipt.owner, 'source-pack2-cold-continuation'); assert.equal(receipt.state, 'active'); assert.equal(receipt.id, owner.id);
  assert.equal(receipt.directory, contained(root, join(root, '.tmp/rust-scratch', owner.id))); assert.notEqual(owner.id, RETAINED_COLD.base);
  assert.deepEqual(receipt.inputHashes.retainedCold, reference); assert.deepEqual(receipt.inputHashes.borrowedHomes, paths.homes); assert.deepEqual(receipt.inputHashes.beforeInventory, reference.homeInventorySHA256);
  const custody = boundedJSON(custodyReceipt); assert.equal(custody.owner, 'source-pack2-cold-revalidation'); assert(['succeeded', 'retired'].includes(custody.state)); assert.equal(custody.handlesReaped, true); assert.deepEqual(custody.inputHashes.retainedCold, reference); assert.equal(custodyReceipt, contained(root, join(root, '.tmp/rust-disk/evidence', custody.id, 'receipt.json'))); assert(custody.sealedLogs.length > 0);
  for (const log of custody.sealedLogs) { const path = contained(resolve(custodyReceipt, '..'), log.path); assert.equal(hashFile(path, 16 << 20), log.sha256, 'cold custody changed before borrow'); }
  const verified = boundedJSON(join(resolve(custodyReceipt, '..'), 'cold-revalidated.json')); assert.equal(verified.state, 'verified'); assert.equal(verified.factsEquivalent, true); assert.equal(verified.facts.fullFieldsAndOrderEqual, true); assert.equal(verified.facts.documents, 4036); assert.deepEqual(verified.lineage.reference, reference);
  assert(custody.sealedLogs.some(log => log.path === join(resolve(custodyReceipt, '..'), 'cold-facts-custody.json')), 'complete cold Facts custody required before borrow');
  for (const engine of ['go', 'rust']) { assert.equal(before[engine].sha256, reference.homeInventorySHA256[engine]); assert.equal(inventory(paths.homes[engine]).sha256, before[engine].sha256, 'borrow home changed before claim'); }
  const record = {schema: 'lycheedev.source-pack2-cold-borrow.v1', owner: owner.id, originalOwner: RETAINED_COLD.base, originalState: 'failed', reference, homes: paths.homes, before, custodyReceipt, writeScope: 'same-home warm only; changed scenarios clone these homes independently', cleanupScope: owner.directory, claimedAt: new Date().toISOString()};
  writeFileSync(paths.claim, JSON.stringify(record, null, 2) + '\n', {flag: 'wx'}); saveJSON(join(owner.evidence, 'borrow-claim.json'), record); return {path: paths.claim, sha256: hashFile(paths.claim)};
}
export function captureColdFactFiles(homes, inventories, directory, root) {
  const files = [];
  for (const engine of ['go', 'rust']) for (const row of inventories[engine].rows.filter(r => r.path.startsWith(engine === 'go' ? 'source/v1/facts/files/' : 'cache/source-facts-rust/'))) {
    const source = {path: contained(homes[engine], join(homes[engine], row.path)), bytes: row.bytes, sha256: row.sha256};
    const target = join(directory, `${engine}-cold-fact-${files.length}`); splitIndexEvidence(root, source, target); files.push({engine, relative: row.path, ...source, manifest: join(target, 'manifest.json'), manifestSHA256: hashFile(join(target, 'manifest.json'), 65536)});
  }
  assert(files.length > 0); saveJSON(join(directory, 'cold-facts-custody.json'), {schema: 'lycheedev.source-pack2-cold-facts.v1', files}); return files.length;
}

export const RETAINED_WARM = Object.freeze({outer: 'd1989b3b-573a-423e-9185-86b89fc6d2f4', base: 'bff2649b-3f34-4e15-b14e-48855cff42fe', custody: '786d460f-cfd9-4a21-9322-6b79d5e5a08d', runnerSHA256: '326d62596fdc66e2907e288d1658a2760e0d02faa668f17f1f28443aa347cf54', custodyReceiptSHA256: '2290e03f9c3df4a8a0f9ac93c9a21a76d274be4bf31ffb3bba4410c843afa461', coldSHA256: '9f0612c3b9b410b230c64ac2f550764356e14ef3ead725e9c0a3a9910060274a', claimSHA256: '514fb250a2d885f9f154609e658f5969f621c71c08ad49d65f93f64580f77a10'});
export const REMAINING_ALLOWANCE = Object.freeze({schema: 'lycheedev.source-pack2-remaining-allowance.v1', scope: RETAINED_WARM.outer, scratchBytes: 6 * 1024 ** 3, authorizationReference: 'Sentinel_a57db18d476081919e647f3195a645a1'});
const remainingPaths = root => ({...retainedPaths(root), outer: contained(root, join(root, '.tmp/rust-disk/evidence', RETAINED_WARM.outer)), base: contained(root, join(root, '.tmp/rust-disk/evidence', RETAINED_WARM.base)), custody: contained(root, join(root, '.tmp/rust-disk/evidence', RETAINED_WARM.custody)), remainingClaim: contained(root, join(root, '.tmp/rust-disk', `source-pack2-remaining-${RETAINED_WARM.outer}.json`))});
export function validateRemainingReference(value) {
  exact(value, ['schema', 'outerReceiptSHA256', 'baseReceiptSHA256', 'homeInventorySHA256']); assert.equal(value.schema, 'lycheedev.source-pack2-retained-warm.v1');
  digest(value.outerReceiptSHA256); digest(value.baseReceiptSHA256); exact(value.homeInventorySHA256, ['go', 'rust']); for (const hash of Object.values(value.homeInventorySHA256)) digest(hash); return value;
}
export function inspectRemaining(request) {
  const paths = remainingPaths(request.ownerRoot), reference = {schema: 'lycheedev.source-pack2-retained-warm.v1', outerReceiptSHA256: hashFile(join(paths.outer, 'receipt.json'), 16 << 20), baseReceiptSHA256: hashFile(join(paths.base, 'receipt.json'), 16 << 20), homeInventorySHA256: Object.fromEntries(['go', 'rust'].map(e => [e, inventory(paths.homes[e]).sha256]))};
  authenticateRemaining(request, reference); return reference;
}
// Bindings injection is for synthetic Node fixtures only; no CLI/config field
// exposes it. Real callers always use the published exact be168 evidence pins.
export function authenticateRemaining(request, reference, bindings = RETAINED_WARM) {
  validateRemainingReference(reference); const paths = remainingPaths(request.ownerRoot), hashes = [], receipts = {};
  assert(!existsSync(paths.remainingClaim), 'remaining variants already claimed; no replacement calls');
  for (const [role, owner] of [['outer', 'source-pack2-diagnostics'], ['base', 'source-pack2-cold-continuation']]) receipts[role] = authenticateSealedOwner(request.ownerRoot, paths[role], RETAINED_WARM[role], owner, 'failed', reference[`${role}ReceiptSHA256`], hashes);
  const custody = authenticateSealedOwner(request.ownerRoot, paths.custody, RETAINED_WARM.custody, 'source-pack2-cold-revalidation', 'retired', bindings.custodyReceiptSHA256, hashes);
  assert.equal(hashFile(paths.claim, 16 << 20), bindings.claimSHA256, 'original single-use cold claim changed'); hashes.push({path: paths.claim, sha256: bindings.claimSHA256});
  const claim = boundedJSON(paths.claim), config = boundedJSON(join(paths.outer, 'configuration.json')), prior = boundedJSON(join(paths.outer, 'report.json')), owned = boundedJSON(join(paths.outer, 'owned-report.json'));
  assert.deepEqual(config.request, request); assert.deepEqual(prior.request, request); assert.deepEqual(owned.request, request); assert.deepEqual(Object.keys(config.inputs).sort(), [...RUNNER_FILES].sort()); assert.equal(sha256(JSON.stringify(RUNNER_FILES.map(path => [path, config.inputs[path]]))), RETAINED_WARM.runnerSHA256, 'exact be168 runner required');
  assert.deepEqual(prior.inputs, config.inputs); assert.deepEqual(owned.inputs, config.inputs); assert.deepEqual(receipts.outer.inputHashes.runner, config.inputs); assert.deepEqual(config.resumeCold, receipts.outer.inputHashes.retainedCold); validateColdReference(config.resumeCold);
  assert.equal(config.evidence, paths.outer); assert.equal(config.scratch, receipts.outer.directory);
  assert.equal(prior.schema, 'lycheedev.source-pack-scenarios.v1'); assert.equal(prior.state, 'source_pack_scenarios_failed'); assert.equal(prior.blocker, 'disk.scratch_quota'); assert.equal(prior.requiresOuterRetirement, false); assert.equal(prior.finalIntegrityVerified, false); assert.equal(prior.importedSamples, 0); assert.equal(prior.layout, 'v11-pack2'); assert.equal(prior.productCommit, CANDIDATE_COMMIT);
  assert.equal(owned.schema, 'lycheedev.source-pack-scenarios-owned.v1'); assert.equal(owned.state, 'failed'); assert.equal(owned.process.exit, 1); assert.equal(owned.process.handlesReaped, true); elapsed(owned.process.proof);
  assert.deepEqual(prior.scenarios.map(row => row.id), ['cold', 'warm', 'single-content-change']); const [cold, warm, pending] = prior.scenarios;
  assert.equal(pending.state, 'running'); assert.equal(pending.mode, 'changed'); assert.equal(pending.factsEquivalent, false); assert(!pending.go && !pending.rust && !pending.facts && !pending.pin, 'no prior variant invocation or preparation allowed');
  assert.deepEqual(prior.owners.map(row => row.role), ['read-only-cold-custody', 'borrowed-healthy-base']); assert.deepEqual(prior.owners.map(row => row.receipt), [join(paths.custody, 'receipt.json'), join(paths.base, 'receipt.json')]);
  assert.equal(prior.admissionFailure.scratchBytes, 5005736596); assert.equal(prior.admissionFailure.scratchReserveBytes, 380939478); assert.equal(prior.admissionFailure.limits.scratchBytes, defaults.scratchBytes); assert(prior.admissionFailure.scratchBytes + prior.admissionFailure.scratchReserveBytes > defaults.scratchBytes);
  assert.equal(prior.continuation.originalCallsReused, 2); assert.equal(prior.continuation.freshColdCalls, 0); assert.deepEqual(prior.continuation.reference, config.resumeCold); assert.equal(prior.continuation.custodyReceipt, join(paths.custody, 'receipt.json')); assert.deepEqual(prior.continuation.borrow, {path: paths.claim, sha256: bindings.claimSHA256});
  assert.equal(claim.schema, 'lycheedev.source-pack2-cold-borrow.v1'); assert.equal(claim.owner, RETAINED_WARM.base); assert.equal(claim.originalOwner, RETAINED_COLD.base); assert.deepEqual(claim.reference, config.resumeCold); assert.deepEqual(claim.homes, paths.homes); assert.equal(claim.custodyReceipt, join(paths.custody, 'receipt.json'));
  for (const file of RUNNER_FILES) { assert.equal(receipts.base.inputHashes[file], config.inputs[file]); assert.equal(custody.inputHashes[file], config.inputs[file]); }
  assert.deepEqual(receipts.base.inputHashes.beforeInventory, config.resumeCold.homeInventorySHA256); assert.equal(receipts.base.inputHashes.coldCustodyReceipt, join(paths.custody, 'receipt.json')); assert.deepEqual(claim.before, boundedJSON(join(paths.custody, 'borrow-before-inventories.json'))); for (const engine of ['go', 'rust']) assert.equal(claim.before[engine].sha256, config.resumeCold.homeInventorySHA256[engine]);
  assert.deepEqual(receipts.base.inputHashes.retainedCold, config.resumeCold); assert.deepEqual(receipts.base.inputHashes.borrowedHomes, paths.homes); assert.deepEqual(custody.inputHashes.retainedCold, config.resumeCold); assert.deepEqual(boundedJSON(join(paths.base, 'borrow-claim.json')), claim);
  for (const [role, owner] of [['outer', 'source-pack2-diagnostics'], ['base', 'source-pack2-healthy-base']]) { const old = retainedPaths(request.ownerRoot); authenticateSealedOwner(request.ownerRoot, old[role], RETAINED_COLD[role], owner, 'failed', config.resumeCold[`${role}ReceiptSHA256`], hashes); }
  assert.equal(hashFile(join(paths.custody, 'cold-revalidated.json'), 16 << 20), bindings.coldSHA256); assert.deepEqual(boundedJSON(join(paths.custody, 'cold-revalidated.json')), cold);
  assert.deepEqual(boundedJSON(join(paths.base, 'warm/warm-scenario.json')), warm);
  const baseIdentity = boundedJSON(join(paths.base, 'healthy-base-inventories.json')); assert.deepEqual(baseIdentity, boundedJSON(join(paths.base, 'borrow-after-inventories.json')));
  const pin = fixturePin({repository: 'wow-ui-source', product: 'retail', requestedRef: BASE_COMMIT, exactCommit: BASE_COMMIT, parserRevision: 'source-v1'}), oracle = bindings.oracle ?? boundedJSON(join(import.meta.dirname, '../rust-source-full-corpus/retail-oracle.json')), outputs = {};
  for (const [i, row] of [cold, warm].entries()) {
    assert.equal(row.state, 'verified'); assert.equal(row.mode, i === 0 ? 'cold' : 'warm'); assert.equal(row.factsEquivalent, true); assert.deepEqual(row.order, orderFor(i)); assert.deepEqual(row.pin, pin); assert.equal(row.facts.documents, 4036); assert.equal(row.facts.fullFieldsAndOrderEqual, true);
    const directory = i ? join(paths.base, 'warm') : paths.custody; assert.equal(row.facts.evidence, join(directory, 'facts-comparison.json')); const delta = boundedJSON(row.facts.evidence); assert.equal(delta.documents, 4036); assert.deepEqual(delta.differentFiles, []); assert.deepEqual(delta.firstDifferences, []); assert.deepEqual(delta.totals, row.facts.totals); assert.deepEqual(delta.totals.go, delta.totals.rust); assert.equal(delta.inputs.rustLayout, 'v11-pack2');
    const values = {};
    for (const engine of ['go', 'rust']) {
      const id = i ? row[engine].proof.runId : RETAINED_COLD[engine], sourceEvidence = i ? directory : join(retainedPaths(request.ownerRoot).base, 'cold');
      const call = authenticateRecordedCall(row[engine], paths.homes[engine], join(sourceEvidence, `process-${id}`), id, pin, config.inputs); outputs[`${row.id}-${engine}`] = call;
      values[engine] = semantic('B02-retail-index', call.envelope, engine, oracle, pin.id).value; assert.equal(call.envelope.context?.snapshot, pin.id);
      if (engine === 'rust') assert.deepEqual(verifyPackProfile(call.stderr, call.envelope, {mode: row.mode, documents: 4036}), row.rust.sourceProfile);
      if (i) { assert.equal(row[engine].warmReuse.verified, true); assert.deepEqual(JSON.parse(JSON.stringify(proveWarmReuse(engine, boundedJSON(join(directory, `${engine}-warm-before.json`)), boundedJSON(join(directory, `${engine}-warm-after.json`)), call.envelope))), row[engine].warmReuse); }
      const index = row.facts.indexes[engine]; digest(index.sha256); assert.equal(index.path, contained(paths.homes[engine], index.path)); assert.equal(hashFile(index.path, INDEX_BYTES), index.sha256); assert.equal(lstatSync(index.path).size, index.bytes);
      if (engine === 'rust') { assert.equal(delta.inputs.rustIndexSHA256, index.sha256); assert.equal(delta.inputs.rustIndexBytes, index.bytes); assert.equal(delta.inputs.rustIndexFile, index.path); } else assert.equal(delta.inputs.goRecords, index.path);
      for (const key of ['declarations', 'relationships', 'diagnostics']) assert.equal(delta.totals[engine][key], values[engine][key]);
    }
    assert.deepEqual(values.go, values.rust); assert.deepEqual(values.go, row.facts.summary);
  }
  for (const engine of ['go', 'rust']) { assert.equal(sha256(JSON.stringify(baseIdentity[engine].rows)), baseIdentity[engine].sha256); assert.equal(baseIdentity[engine].sha256, reference.homeInventorySHA256[engine]); assert.equal(inventory(paths.homes[engine]).sha256, baseIdentity[engine].sha256, 'healthy warm base changed'); metadataIdentity(paths.homes[engine]); }
  const rows = boundedJSON(join(paths.custody, 'go-retained-verified-corpus-manifest.json')).files; assertCorpus(rows, oracle); assert.deepEqual(rows, boundedJSON(join(paths.custody, 'rust-retained-verified-corpus-manifest.json')).files);
  const verifyEvidence = () => { for (const value of hashes) assert.equal(hashFile(value.path, 16 << 20), value.sha256, 'prior completed evidence changed'); };
  const verifyHomes = () => { for (const engine of ['go', 'rust']) assert.equal(inventory(paths.homes[engine]).sha256, baseIdentity[engine].sha256, 'healthy warm base changed'); };
  return {paths, reference, prior, cold, warm, pending, pin, rows, outputs, baseIdentity, verifyEvidence, verifyHomes};
}
export function claimRemaining(root, reference, owner, baseIdentity) {
  validateRemainingReference(reference); assert.match(owner.id, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/); const paths = remainingPaths(root), receipt = boundedJSON(owner.receipt);
  assert.equal(owner.receipt, contained(root, join(root, '.tmp/rust-disk/evidence', owner.id, 'receipt.json'))); assert.equal(owner.evidence, resolve(owner.receipt, '..')); assert.equal(owner.directory, contained(root, join(root, '.tmp/rust-scratch', owner.id))); assert.equal(receipt.directory, owner.directory);
  assert.equal(receipt.owner, 'source-pack2-remaining-cases'); assert.equal(receipt.id, owner.id); assert.equal(receipt.state, 'active'); assert.deepEqual(receipt.inputHashes.retainedWarm, reference); assert.deepEqual(receipt.inputHashes.readOnlyHomes, paths.homes);
  for (const engine of ['go', 'rust']) { assert.equal(baseIdentity[engine].sha256, reference.homeInventorySHA256[engine]); assert.equal(inventory(paths.homes[engine]).sha256, baseIdentity[engine].sha256); }
  const record = {schema: 'lycheedev.source-pack2-remaining-claim.v1', owner: owner.id, priorOuter: RETAINED_WARM.outer, priorState: 'failed', reference, readOnlyHomes: paths.homes, remainingCases: CASES.slice(2).map(row => row.id), cleanupScope: owner.directory};
  writeFileSync(paths.remainingClaim, JSON.stringify(record, null, 2) + '\n', {flag: 'wx'}); saveJSON(join(owner.evidence, 'remaining-claim.json'), record); return {path: paths.remainingClaim, sha256: hashFile(paths.remainingClaim)};
}

export async function runController(config, overrides = {}) {
  const svc = {authenticateRemaining, claimRemaining, assertCorpus, authenticateRetainedCold, claimColdBorrow, captureColdFactFiles, beginScratch, admission, ownedSpawnSync, collectOwnedMemory, runnerIdentity, verifyCandidateRecord, verifyGoPackage, cloneSeed, inventory, derivedIdentity, proveWarmReuse, prepareVariant, compareFacts, verifyPackProfile, indexEvidence, captureGoCustody, openGoCustody, ...overrides};
  const {request, runnerRoot, inputs, conditions, scratch, evidence: out} = config;
  assert(!(config.resumeCold && config.resumeRemaining), 'one exact continuation mode required');
  const diskPolicy = assertDiskPolicy(request.ownerRoot, process.env, config.allowance, config.resumeRemaining);
  const oracle = JSON.parse(readFileSync(join(runnerRoot, 'tests/rust-source-full-corpus/retail-oracle.json')));
  assert.equal(oracle.commit, BASE_COMMIT); assert.equal(oracle.documents, 4036);
  let fatal = false, sequence = 0, baseOwner, baseSuccess = false, retained, borrowClaim, remaining, remainingClaim;
  const start = performance.now();
  const report = {schema: 'lycheedev.source-pack-scenarios.v1', diskPolicy, state: 'running', request, inputs, runnerRoot, conditions, productCommit: CANDIDATE_COMMIT, layout: 'v11-pack2', importedSamples: 0, performanceAcceptanceEvaluated: false, fullB01B07Acceptance: false, preparation: [], scenarios: [], invocationAttempts: [], owners: [], finalIntegrityVerified: false,
    machine: {hostname: os.hostname(), platform: process.platform, node: process.version, cpus: os.cpus(), totalMemory: os.totalmem()},
    timing: 'Native total CreateProcessW through descendant reaping/output drain; 31 overlapping phases are attribution, never subtracted or added. Wrapper wall separate.', cacheDefinition: config.resumeRemaining ? 'Four original cold/warm calls retained; only the four unexecuted variants run, each on a fresh independent clone of the sealed healthy base. No base copies or baseline product calls.' : config.resumeCold ? 'Original fixed 7d8 init/sync/cold calls reauthenticated read-only; exact two retained homes borrowed for same-home warm; each changed engine copies this healthy base independently. No fresh cold calls or OS cache flush.' : 'Fresh own init/sync-only homes for cold; same successful base homes for warm; each changed pair copies the original healthy base independently. No OS cache flush.', metrics: {cpu: 'not_measured', io: 'not_measured', rss: 'not_measured; owned-Job sampled aggregate and gaps retained, not unique physical RSS'}};
  const save = () => saveJSON(join(out, 'report.json'), report);
  function controlled(binary, args, cwd, evidence, measured = false, timeout = 210000, engine) {
    assert(!fatal, 'unverified process retirement; no further launch');
    if (measured) { report.invocationAttempts.push({caseId: report.scenarios.at(-1).id, engine}); save(); }
    fatal = true;
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
    let homes, prepared, cold, coldEvidence;
    if (config.resumeRemaining) {
      remaining = svc.authenticateRemaining(request, config.resumeRemaining); homes = remaining.paths.homes; prepared = {pin: remaining.pin, rows: remaining.rows}; svc.assertCorpus(prepared.rows, oracle);
      for (const original of Object.values(remaining.outputs)) { const engine = original.row.home === homes.go ? 'go' : 'rust'; assert.equal(original.row.binary, binaries[engine].binary); assert.equal(original.row.cwd, binaries[engine].cwd); }
      assert.equal(report.machine.hostname, remaining.prior.machine.hostname); assert.equal(report.machine.platform, remaining.prior.machine.platform); assert.equal(report.machine.powerBefore, remaining.prior.machine.powerBefore);
      cold = structuredClone(remaining.cold); coldEvidence = remaining.paths.custody; report.scenarios.push(cold, structuredClone(remaining.warm));
      report.continuation = {schema: 'lycheedev.source-pack2-remaining-continuation.v1', reference: config.resumeRemaining, originalCallsReused: 4, freshColdCalls: 0, freshWarmCalls: 0, priorOuter: RETAINED_WARM.outer, priorState: 'failed', priorPendingScenario: remaining.pending, originalConditions: remaining.prior.conditions, originalContinuation: remaining.prior.continuation};
      baseOwner = svc.beginScratch(request.ownerRoot, 'source-pack2-remaining-cases', {command: ['remaining-four-changed-cases'], inputs: {...inputs, retainedWarm: config.resumeRemaining, readOnlyHomes: homes, allowance: config.allowance ?? null}, limits: diskPolicy, admissionOptions: {scratchReserveBytes: 1 << 20, requiredBytes: 16 << 20}});
      report.owners.push({role: 'read-only-healthy-base-reference', receipt: baseOwner.receipt}); saveJSON(join(baseOwner.evidence, 'prior-completed-cases.json'), {cold: remaining.cold, warm: remaining.warm, reference: config.resumeRemaining, priorPendingScenario: remaining.pending}); remaining.verifyHomes(); remaining.verifyEvidence(); save();
    } else if (config.resumeCold) {
      retained = svc.authenticateRetainedCold(request, config.resumeCold);
      homes = retained.paths.homes; prepared = {pin: retained.pin, rows: retained.rows}; svc.assertCorpus(prepared.rows, oracle);
      for (const engine of ['go', 'rust']) { assert.equal(retained.outputs[engine].row.binary, binaries[engine].binary); assert.equal(retained.outputs[engine].row.cwd, binaries[engine].cwd); }
      assert.equal(report.machine.hostname, retained.oldReport.machine.hostname, 'original cold hostname differs'); assert.equal(report.machine.platform, retained.oldReport.machine.platform, 'original cold platform differs');
      assert.equal(report.machine.powerBefore, retained.oldReport.machine.powerBefore, 'power plan changed since original cold samples');
      const factsBytes = Object.entries(retained.inventories).reduce((sum, [engine, inv]) => sum + inv.rows.filter(r => r.path.startsWith(engine === 'go' ? 'source/v1/facts/files/' : 'cache/source-facts-rust/')).reduce((n, r) => n + r.bytes, 0), 0);
      const estimate = {scratchReserveBytes: 1 << 20, requiredBytes: 2 * INDEX_BYTES + factsBytes + 2 * oracle.allBytes + (64 << 20), basis: 'Existing homes are already charged; only 1 MiB new owner scratch, two bounded indexes, actual complete Facts, two raw corpus audits and 64 MiB metadata/log allowance'};
      const custody = svc.beginScratch(request.ownerRoot, 'source-pack2-cold-revalidation', {command: ['read-only', 'retained-cold'], inputs: {...inputs, retainedCold: config.resumeCold}, limits: diskPolicy, admissionOptions: estimate});
      report.owners.push({role: 'read-only-cold-custody', receipt: custody.receipt, estimate}); save(); let complete = false;
      try {
        coldEvidence = custody.evidence;
        saveJSON(join(coldEvidence, 'original-failed-cold.json'), {reference: config.resumeCold, originalOuterReceipt: join(retained.paths.outer, 'receipt.json'), originalBaseReceipt: join(retained.paths.base, 'receipt.json'), cold: retained.cold, originalInputs: retained.oldReport.inputs});
        saveJSON(join(coldEvidence, 'borrow-before-inventories.json'), retained.inventories);
        const envelopes = {};
        for (const engine of ['go', 'rust']) {
          if (!overrides.authenticateRetainedCold) auditCorpus(join(homes[engine], 'mirrors', `${oracle.repository}.git`), oracle, (args, input) => git(args, input), coldEvidence, `${engine}-retained-`);
          const original = retained.outputs[engine]; envelopes[engine] = original.envelope;
          semantic('B02-retail-index', original.envelope, engine, oracle, prepared.pin.id); assert.equal(original.envelope.context?.snapshot, prepared.pin.id);
          if (engine === 'rust') assert.deepEqual(svc.verifyPackProfile(original.stderr, original.envelope, {mode: 'cold', documents: 4036}), retained.cold.rust.sourceProfile);
        }
        cold = structuredClone(retained.cold); delete cold.failure; cold.state = 'revalidating'; cold.lineage = {...cold.lineage, resumedOriginalCalls: true, originalState: 'failed', originalOuter: RETAINED_COLD.outer, originalBase: RETAINED_COLD.base, originalConditions: retained.oldReport.conditions, originalMachine: {hostname: retained.oldReport.machine.hostname, platform: retained.oldReport.machine.platform, node: retained.oldReport.machine.node}, reference: config.resumeCold};
        report.scenarios.push(cold); report.continuation = {schema: 'lycheedev.source-pack2-cold-continuation.v1', reference: config.resumeCold, originalCallsReused: 2, freshColdCalls: 0, originalState: 'failed', custodyReceipt: custody.receipt}; save();
        cold.facts = await svc.compareFacts(homes, envelopes, prepared.pin, oracle, coldEvidence, request.ownerRoot); cold.factsEquivalent = true;
        svc.captureColdFactFiles(homes, retained.inventories, coldEvidence, request.ownerRoot);
        retained.verifyHomes(); retained.verifyEvidence(); identities(); cold.state = 'verified'; cold.rustToGoRatio = cold.rust.controllableMillis / cold.go.controllableMillis;
        saveJSON(join(coldEvidence, 'cold-revalidated.json'), cold); complete = true;
      } finally { custody.seal({state: fatal ? 'interrupted' : complete ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: evidenceFiles(custody.evidence)}); if (complete && !fatal) custody.cleanup(); save(); }
      const warmGrowth = Math.max(cold.facts.indexes.go.bytes, cold.facts.indexes.rust.bytes) + (8 << 20);
      const warmEstimate = {scratchReserveBytes: warmGrowth, requiredBytes: warmGrowth + 2 * INDEX_BYTES + (64 << 20), basis: 'Borrow exact existing homes; sequential warm calls reserve the larger measured index for temporary replacement plus 8 MiB metadata/log growth; two complete evidence indexes plus 64 MiB allowance'};
      baseOwner = svc.beginScratch(request.ownerRoot, 'source-pack2-cold-continuation', {command: ['source', 'index', 'warm-and-four-changes'], inputs: {...inputs, retainedCold: config.resumeCold, borrowedHomes: homes, beforeInventory: config.resumeCold.homeInventorySHA256, coldCustodyReceipt: custody.receipt}, limits: diskPolicy, admissionOptions: warmEstimate});
      report.owners.push({role: 'borrowed-healthy-base', receipt: baseOwner.receipt, estimate: warmEstimate});
      retained.verifyHomes(); retained.verifyEvidence(); borrowClaim = svc.claimColdBorrow(request.ownerRoot, config.resumeCold, baseOwner, retained.inventories, custody.receipt); report.continuation.borrow = borrowClaim; save();
    } else {
      const coldEstimate = footprint({cold: true}); report.coldAdmissionEstimate = coldEstimate;
      baseOwner = svc.beginScratch(request.ownerRoot, 'source-pack2-healthy-base', {command: ['source', 'index'], inputs, limits: diskPolicy, admissionOptions: coldEstimate}); report.owners.push({role: 'healthy-base', receipt: baseOwner.receipt}); save();
      homes = Object.fromEntries(['go', 'rust'].map(engine => [engine, join(baseOwner.directory, `${engine}-home`)]));
      prepared = (overrides.prepareSeeds ?? prepareSeeds)(homes, binaries, baseOwner.evidence);
      coldEvidence = join(baseOwner.evidence, 'cold'); mkdirSync(coldEvidence);
      cold = await scenario(CASES[0], 0, homes, {pin: prepared.pin, oracle}, coldEvidence, binaries, {state: 'authenticated-own-sync-only'});
      const coldInventory = Object.fromEntries(['go', 'rust'].map(engine => [engine, svc.inventory(homes[engine])]));
      report.actualColdHomeBytes = coldInventory.go.bytes + coldInventory.rust.bytes;
      assert(report.actualColdHomeBytes <= coldEstimate.scratchReserveBytes, 'actual cold footprint exceeded admitted estimate');
      report.postColdAdmission = svc.admission(request.ownerRoot, {limits: diskPolicy});
    }
    const selection = {pin: prepared.pin, oracle};
    if (!remaining) { const warmEvidence = join(baseOwner.evidence, 'warm'); mkdirSync(warmEvidence);
      await scenario(CASES[1], 1, homes, selection, warmEvidence, binaries, {state: 'same-cold-home', cold: coldEvidence}); }
    const baseIdentity = remaining?.baseIdentity ?? Object.fromEntries(['go', 'rust'].map(engine => [engine, svc.inventory(homes[engine])]));
    saveJSON(join(baseOwner.evidence, 'healthy-base-inventories.json'), baseIdentity);
    if (retained) { retained.verifyEvidence(); assert.equal(hashFile(borrowClaim.path), borrowClaim.sha256); const beforeBytes = Object.values(retained.inventories).reduce((n, inv) => n + inv.bytes, 0), afterBytes = Object.values(baseIdentity).reduce((n, inv) => n + inv.bytes, 0); assert(afterBytes <= beforeBytes + Math.max(cold.facts.indexes.go.bytes, cold.facts.indexes.rust.bytes) + (8 << 20), 'borrowed warm growth exceeded admitted estimate'); report.postWarmAdmission = svc.admission(request.ownerRoot, {limits: diskPolicy}); }
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
        let job;
        try { job = svc.beginScratch(request.ownerRoot, `source-pack2-${entry.id}-${engine}`, {command: ['source', 'index'], inputs: {...inputs, healthyBase: inventory.sha256}, limits: diskPolicy, admissionOptions: estimate}); }
        catch (error) { row.state = row.go || row.rust ? 'incomplete' : 'not_run'; row.blockedBeforeOwner = engine; row.failure = error.message; row.admissionFailure = error.report; save(); throw error; }
        report.owners.push({role: entry.id, engine, receipt: job.receipt, estimate}); save(); let success = false;
        try {
          if (remaining && !remainingClaim) { remaining.verifyEvidence(); remaining.verifyHomes(); remainingClaim = svc.claimRemaining(request.ownerRoot, config.resumeRemaining, baseOwner, baseIdentity); report.continuation.claim = remainingClaim; save(); }
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
          assert(measured.bytes <= estimate.scratchReserveBytes, 'actual scenario footprint exceeded admitted estimate'); row[engine].postAdmission = svc.admission(request.ownerRoot, {limits: diskPolicy});
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
    if (retained) { retained.verifyEvidence(); assert.equal(hashFile(borrowClaim.path), borrowClaim.sha256); }
    if (remaining) { remaining.verifyEvidence(); remaining.verifyHomes(); assert.equal(hashFile(remainingClaim.path), remainingClaim.sha256); }
    report.finalIntegrityVerified = true; report.state = 'source_pack_scenarios_observed'; baseSuccess = true;
  } catch (error) { report.blocker = error.message.slice(0, 4096); if (error.report) report.admissionFailure = error.report; report.state = 'source_pack_scenarios_failed'; }
  finally {
    const scheduled = config.resumeRemaining ? CASES.slice(2) : config.resumeCold ? CASES.slice(1) : CASES;
    report.notRunInvocations = scheduled.flatMap(entry => ['go', 'rust'].filter(engine => !report.scenarios.find(row => row.id === entry.id)?.[engine] && !report.invocationAttempts.some(attempt => attempt.caseId === entry.id && attempt.engine === engine)).map(engine => ({caseId: entry.id, engine})));
    report.notRunCases = scheduled.filter(entry => report.notRunInvocations.filter(item => item.caseId === entry.id).length === 2).map(entry => entry.id);
    report.requiresOuterRetirement = fatal; report.wallMillis = performance.now() - start;
    if (baseOwner) {
      if (remaining) { try { remaining.verifyEvidence(); remaining.verifyHomes(); saveJSON(join(baseOwner.evidence, 'read-only-base-after.json'), remaining.baseIdentity); } catch (error) { report.state = 'source_pack_scenarios_failed'; report.remainingIntegrityFailure = error.message; baseSuccess = false; } }
      if (retained) { try { saveJSON(join(baseOwner.evidence, 'borrow-after-inventories.json'), fatal ? {state: 'not_measured_unreaped', requiresOuterRetirement: true} : Object.fromEntries(['go', 'rust'].map(e => [e, svc.inventory(retained.paths.homes[e])]))); retained.verifyEvidence(); } catch (error) { report.state = 'source_pack_scenarios_failed'; report.borrowIntegrityFailure = error.message; baseSuccess = false; } }
      try { baseOwner.seal({state: fatal ? 'interrupted' : baseSuccess ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: evidenceFiles(baseOwner.evidence)}); if (baseSuccess && !fatal) baseOwner.cleanup(); }
      catch (error) { report.state = 'source_pack_scenarios_failed'; report.cleanupFailure = error.message; }
    }
    save();
  }
  return report;
}
export function verifyObservedReport(run, report, request, inputs, resumeCold = undefined, resumeRemaining = undefined) {
  assert(run.status === 0 && !run.error && run.handlesReaped); elapsed(run.proof);
  assert.equal(report?.state, 'source_pack_scenarios_observed'); assert.equal(report.finalIntegrityVerified, true); assert.equal(report.requiresOuterRetirement, false);
  assert.equal(report.performanceAcceptanceEvaluated, false); assert.equal(report.fullB01B07Acceptance, false); assert.equal(report.importedSamples, 0);
  assert.deepEqual(report.request, request); assert.deepEqual(report.inputs, inputs); assert.equal(report.layout, 'v11-pack2'); assert.equal(report.productCommit, CANDIDATE_COMMIT);
  assert(!(resumeCold && resumeRemaining));
  if (resumeRemaining) { validateRemainingReference(resumeRemaining); assert.deepEqual(report.continuation?.reference, resumeRemaining); assert.equal(report.continuation.originalCallsReused, 4); assert.equal(report.continuation.freshColdCalls, 0); assert.equal(report.continuation.freshWarmCalls, 0); assert.equal(report.continuation.priorOuter, RETAINED_WARM.outer); for (const engine of ['go', 'rust']) assert.equal(report.scenarios[0][engine].proof.runId, RETAINED_COLD[engine]); }
  else if (resumeCold) { validateColdReference(resumeCold); assert.deepEqual(report.continuation?.reference, resumeCold); assert.equal(report.continuation.originalCallsReused, 2); assert.equal(report.continuation.freshColdCalls, 0); assert.equal(report.scenarios[0].lineage.resumedOriginalCalls, true); assert.deepEqual(report.scenarios[0].lineage.reference, resumeCold); for (const engine of ['go', 'rust']) assert.equal(report.scenarios[0][engine].proof.runId, RETAINED_COLD[engine]); } else assert.equal(report.continuation, undefined);
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
