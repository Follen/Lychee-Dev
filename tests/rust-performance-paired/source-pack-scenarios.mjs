// One six-scenario observation; also one fixed, authenticated cold continuation.
// No builds, historical sample import or acceptance change.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createReadStream, closeSync, openSync, readSync, copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {basename, isAbsolute, join, relative, resolve} from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {performance} from 'node:perf_hooks';
import os from 'node:os';
import readline from 'node:readline';
import {beginScratch, admission, contained, sha256, policy, defaults} from '../../tools/rust-disk.mjs';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {collectOwnedMemory} from '../../tools/rust-owned-memory.mjs';
import {inspectEmmy} from '../../tools/rust-emmy-identity.mjs';
import {splitIndexEvidence, verifyParts, PART_BYTES, INDEX_BYTES} from '../../tools/rust-source-pack-evidence.mjs';
import {assertEnvelope, assertCorpus, corpusDigest} from '../rust-source-full-corpus/assertions.mjs';
import {fixturePin} from '../rust-source-native-acceptance/assertions.mjs';
import {GO_COMMIT, elapsed, semantic} from './contract.mjs';
import {assertSeedCold, auditCorpus, cloneSeed, inventory, derivedIdentity, proveWarmReuse, verifyGoMetadata, verifyGoPackage, verifySeedMetadata} from './inputs.mjs';
import {RUNNER_FILES as HISTORICAL_RUNNER_FILES, sourcePhaseEnvironment, validateConditions} from './source-candidates.mjs';
import {verifyPackProfile} from './source-pack-resume.mjs';
import {readFactDelta, readRustFact, rustFactPath, comparedRustIndex} from './read-fact-delta.mjs';
import {captureGoCustody, openGoCustody, goFactRelative} from './source-pack-custody.mjs';

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
  candidateIdentity(value.candidate);
  exact(value.go, ['sourceRoot', 'packageRoot', 'luaLSArchive', 'packageSHA256']); digest(value.go.packageSHA256);
  const result = structuredClone(value);
  result.ownerRoot = realpathSync(value.ownerRoot);
  for (const side of ['candidate', 'go']) for (const key of side === 'candidate' ? ['sourceRoot', 'artifactRoot'] : ['sourceRoot', 'packageRoot']) result[side][key] = realpathSync(value[side][key]);
  result.candidate.artifact = contained(result.candidate.artifactRoot, value.candidate.artifact);
  return result;
}
export function candidateIdentity(input) {
  assert.match(input?.commit, /^[0-9a-f]{40}$/, 'explicit exact candidate commit required');
  for (const key of Object.keys(CANDIDATE_IDENTITY)) digest(input[key]);
  return {commit: input.commit, ...Object.fromEntries(Object.keys(CANDIDATE_IDENTITY).map(key => [key, input[key]]))};
}
export function assertHistoricalCandidate(request) { assert.deepEqual(candidateIdentity(request.candidate), {commit: CANDIDATE_COMMIT, ...CANDIDATE_IDENTITY}, 'historical continuations require the exact original 00d artifact'); }
export function verifyCandidateIdentity(input, artifact, sourceTree, emmy, channel) {
  candidateIdentity(input); assert(['release', 'symbols'].includes(artifact.profile), 'optimized existing artifact required');
  assert.equal(artifact.sameInvocation, true); assert(isAbsolute(artifact.sourceRoot), 'original artifact source provenance required');
  if (artifact.profile === 'symbols') assert(artifact.pdb, 'matching symbols required');
  assert.deepEqual(artifact.command, ['cargo', 'build', '--locked', '--offline', '-j', '1', '--profile', artifact.profile, '-p', 'lycheedev-cli'], 'fixed existing build recipe required');
  assert.equal(artifact.binarySHA256, input.binarySHA256, 'candidate EXE identity'); assert.equal(artifact.inputHashes?.sourceTree, input.sourceTreeSHA256, 'candidate artifact source identity'); assert.equal(sourceTree, input.sourceTreeSHA256, 'product source/build identity changed');
  assert.equal(emmy.identity, 'public-upstream-unmodified-v1');
  for (const key of ['identitySHA256', 'recipeSHA256', 'patchedTreeSHA256']) { digest(emmy[key]); assert.equal(emmy[key], artifact.inputHashes.emmy?.[key], `Emmy ${key} differs from build`); }
  assert.equal(artifact.inputHashes.emmy.schema, emmy.schema); assert.equal(artifact.inputHashes.emmy.historicalPrivatePatchRecovered, false);
  assert.equal(channel.version, '3.5.0'); assert.equal(channel.distTag, 'beta'); return artifact;
}
export function verifyCandidateRecord(input) {
  candidateIdentity(input); assert.equal(hashFile(input.artifact, 65536), input.artifactSHA256, 'sealed artifact record changed');
  const artifact = verifyArtifact(input.artifactRoot, input.artifact); assert.equal(lstatSync(artifact.sourceBinary).size, artifact.bytes, 'artifact byte size changed');
  return verifyCandidateIdentity(input, artifact, sourceDigest(input.sourceRoot), inspectEmmy(input.sourceRoot), JSON.parse(readFileSync(join(input.sourceRoot, 'release/npm-channel.json'))));
}
export function verifyCleanCheckout(root, commit, git) {
  assert.match(commit, /^[0-9a-f]{40}$/); assert.equal(realpathSync(git(['rev-parse', '--show-toplevel'], root).toString().trim()), root, 'exact checkout root required');
  assert.equal(git(['rev-parse', 'HEAD'], root).toString().trim(), commit, 'exact configured checkout commit');
  assert.equal(git(['status', '--porcelain', '--untracked-files=all'], root).toString().trim(), '', 'source checkout must be clean');
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
  if (allowance) {
    if (remaining) (remaining.schema === 'lycheedev.source-pack2-retained-go.v1' ? validateGoReference : validateRemainingReference)(remaining);
    assert.deepEqual(allowance, remaining ? REMAINING_ALLOWANCE : FRESH_ALLOWANCE, 'only explicitly authorized task-local 6 GiB allowance');
    return {...observed, scratchBytes: allowance.scratchBytes};
  }
  return observed;
}
const variantAuditBytes = oracle => { assert(Number.isSafeInteger(oracle.allBytes) && oracle.allBytes >= 0); return oracle.allBytes; };
const saveJSON = (path, value) => { const raw = Buffer.from(JSON.stringify(value, null, 2) + '\n'); assert(raw.length <= 16 << 20, 'small report evidence budget'); writeFileSync(path, raw); };
export function footprint({cold = true} = {}) {
  assert.equal(cold, true, 'changed admission uses measured own-engine inventories');
  const scratchReserveBytes = 257058598 + 131416885 + (64 << 20);
  const evidenceBytes = 4 * INDEX_BYTES + 4 * RUST_CUSTODY_BYTES + 2 * 32665129 + (128 << 20);
  return {requiredBytes: scratchReserveBytes + evidenceBytes, scratchReserveBytes,
    basis: 'Historical b3d Go/Rust complete home bytes plus 64 MiB setup/growth allowance; an estimate, not observed 00d size or a hard allocation cap', evidenceBytes};
}
export function metadataPageSize(home, identity) {
  const path = contained(home, join(home, 'state/toolkit.sqlite')), expected = identity.rows.find(row => row.path === 'state/toolkit.sqlite'); assert(expected && expected.bytes <= 1 << 20); const stat = lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size === expected.bytes, 'bounded regular metadata sizing input');
  const raw = readFileSync(path); assert.equal(raw.length, expected.bytes); assert.equal(sha256(raw), expected.sha256, 'base metadata changed before sizing'); assert.equal(raw.subarray(0, 16).toString(), 'SQLite format 3\0');
  const encoded = raw.readUInt16BE(16), page = encoded === 1 ? 65536 : encoded; assert(page >= 512 && page <= 65536 && (page & (page - 1)) === 0 && raw.length % page === 0, 'SQLite page layout'); return page;
}
// Logical-file peak submission for these four fixed recipes only. Publication
// stages become their destinations by rename: do not double-count both names.
// SQLite/Git structural allowances are conservative finite-fixture estimates,
// not filesystem allocation enforcement or a claim to measure transient peaks.
export function mutationFootprint(engine, id, base, indexBytes, rows, oracle, pageSize = 4096) {
  assert(['go', 'rust'].includes(engine)); const entry = CASES.find(row => row.id === id); assert(entry?.mode === 'changed');
  for (const n of [base.bytes, indexBytes, oracle.allBytes, ...base.rows.map(row => row.bytes)]) assert(Number.isSafeInteger(n) && n >= 0);
  assert(indexBytes <= INDEX_BYTES); assert.equal(base.bytes, base.rows.reduce((n, row) => n + row.bytes, 0));
  assert(new Set(base.rows.map(row => row.path)).size === base.rows.length); assert(rows.length <= 100000);
  const changed = id === 'single-content-change' || id === 'root-insert', factBytes = base.rows.filter(row => engine === 'go' ? row.path.startsWith('source/v1/facts/files/') : row.path.startsWith('cache/source-facts-rust/')).reduce((n, row) => n + row.bytes, 0);
  const directories = new Set(['']); let treeBytes = 0, indexEntries = 0;
  for (const row of rows) { assert(typeof row.path === 'string' && row.path && !row.path.includes('\\') && row.path.split('/').every(p => p && p !== '.' && p !== '..')); const parts = row.path.split('/'); treeBytes += Buffer.byteLength(parts.at(-1)) + 32; indexEntries += Buffer.byteLength(row.path) + 80; for (let j = 1; j < parts.length; j++) directories.add(parts.slice(0, j).join('/')); }
  for (const dir of directories) if (dir) treeBytes += Buffer.byteLength(dir.split('/').at(-1)) + 32;
  // All original trees (not merely affected ancestors), one <=2534 B blob,
  // and the <1 KiB deterministic commit. 2*n+128 bounds loose zlib bytes;
  // another factor 2 includes the single concurrently staged loose object.
  const gitObjects = 2 * (2 * (treeBytes + 2534 + 1024) + 128 * (directories.size + 2));
  // Explicit v2 non-split index: 62 B fixed entry + path/NUL/alignment,
  // conservative 256 B per cache-tree node; old index and .lock coexist.
  const gitIndexes = 2 * (64 + indexEntries + 80 + Buffer.byteLength(INSERT_PATH) + directories.size * 256);
  const db = base.rows.find(row => row.path === 'state/toolkit.sqlite')?.bytes ?? 0; assert(db <= 1 << 20, 'fixed healthy metadata size precondition');
  assert(Number.isInteger(pageSize) && pageSize >= 512 && pageSize <= 65536 && (pageSize & (pageSize - 1)) === 0, 'SQLite page size');
  const transactions = engine === 'go' ? 1 : 3, page = pageSize;
  // Fixed project pin; Rust additionally index manifest and capture. Their
  // <=1 KiB values get 64 whole new pages EACH, then every page is budgeted
  // again for every transaction in WAL plus a full MiB SHM allowance.
  const dbGrowth = transactions * 64 * page, dbPages = Math.ceil((db + dbGrowth) / page);
  const metadata = dbGrowth + 32 + transactions * dbPages * (page + 24) + (1 << 20);
  const index = engine === 'go' ? indexBytes + (id === 'root-insert' ? 2 * ((1 << 20) + 1) : 0) : INDEX_BYTES;
  assert(index <= INDEX_BYTES, 'fixed recipe index evidence bound');
  const components = {baseClone: base.bytes, recordsOrIndex: index, goOffsets: engine === 'go' ? 128 << 20 : 0, goManifest: engine === 'go' ? 1 << 20 : 0,
    newFacts: engine === 'go' ? changed ? 32 << 20 : 0 : factBytes + 76 * entry.documents + (changed ? 32 << 20 : 0),
    originalCache: 0, assetBlobs: engine === 'rust' ? oracle.allBytes : 0, publicCaptureBlob: engine === 'rust' ? 128 << 20 : 0,
    metadataAndSidecars: metadata, gitObjectsAndStage: gitObjects, gitIndexAndLock: gitIndexes, projectFiles: 16384, lockFileContents: 0};
  const scratchReserveBytes = Object.values(components).reduce((n, bytes) => n + bytes, 0);
  // Each completed Go result has one complete records copy and all needed
  // Facts; Rust has one complete index and every referenced Facts object.
  // The latter is bounded by all base objects plus the complete new generation.
  // Raw Git audit retains all input
  // bytes. Owned memory has an existing 64 MiB hard evidence ceiling; combined
  // stdout/stderr is 16 MiB bounded (reserve two slots conservatively). Other bounded
  // manifests/metadata/comparison/owner reports get four 16 MiB slots.
  const evidence = {indexParts: INDEX_BYTES, rustFacts: engine === 'rust' ? factBytes + components.newFacts : 0, goFacts: engine === 'go' ? factBytes + (changed ? 32 << 20 : 0) : 0, rawAudit: oracle.allBytes + rows.reduce((n, row) => n + Buffer.byteLength(row.path) + 256, 0), memory: 64 << 20, stdoutStderr: 32 << 20, metadataAndReports: 64 << 20};
  const evidenceBytes = Object.values(evidence).reduce((n, bytes) => n + bytes, 0);
  assert(Number.isSafeInteger(scratchReserveBytes + evidenceBytes));
  return {schema: 'lycheedev.source-pack2-footprint.v2', engine, scenario: id, scratchReserveBytes, requiredBytes: scratchReserveBytes + evidenceBytes, components, evidence, evidenceBytes, baseBytes: base.bytes, indexBytes, factsBytes: factBytes,
    basis: 'Go323 and authenticated candidate using the pack2 format/writing contract, exact four recipes, immutable healthy base; conservative logical-byte estimate inherited from 00d, not a proof for arbitrary product changes, physical allocation or a runtime hard cap.'};
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
function packIndexIdentities(repository, product, commit, parser) {
  return ['rust-index-v14', 'rust-index-v15'].map(version => sha256([version, ...(version === 'rust-index-v15' ? ['assets-v2-go323-metadata'] : []), 'facts-pack-v2-path-radix512', 'facts-layout-v11-go323-generated-origins', 'TS0.26.3', 'Lua0.5.0', 'XML0.38', 'generator-v6', repository, product, commit, parser].join('\0')));
}
export function indexEvidence(home, engine, pin, directory, root, {layout = 'v11-pack2'} = {}) {
  assert(['v11-pack2', 'v13-postcard', 'v13-postcard-index18'].includes(layout), 'explicit supported index layout required');
  const source = pin.source; let path, manifest;
  const metadata = captureMetadata(home, pin, directory, engine);
  if (engine === 'go') {
    const key = sha256(['lycheedev.source-map.v2', source.repository, source.product, source.exactCommit, source.parserRevision].join('\0'));
    const base = join(home, 'source/v1/facts/snapshots', key); path = join(base, 'records.jsonl');
    manifest = JSON.parse(readFileSync(join(base, 'manifest.json')));
    assert.equal(manifest.recordsHash, hashFile(path, INDEX_BYTES)); assert.equal(manifest.recordBytes, lstatSync(path).size);
  } else {
    const identities = layout !== 'v11-pack2' ? [sha256([layout === 'v13-postcard-index18' ? 'rust-index-v18' : 'rust-index-v17', 'assets-v2-go323-metadata', 'facts-pack-v2-path-radix512', 'facts-layout-v13-postcard-1.1.3', 'TS0.26.3', 'Lua0.5.0', 'XML0.38', 'generator-v6', source.repository, source.product, source.exactCommit, source.parserRevision].join('\0'))] : packIndexIdentities(source.repository, source.product, source.exactCommit, source.parserRevision);
    const db = new DatabaseSync(metadata.path, {readOnly: true});
    try {
      const matches = identities.map(identity => ({identity, row: db.prepare('SELECT value FROM documents WHERE key = ?').get(`source/rust-index/${identity}`)})).filter(match => match.row);
      assert.equal(matches.length, 1, 'exact pack2 index manifest required: one authenticated recipe only');
      const {identity, row} = matches[0];
      manifest = JSON.parse(Buffer.from(row.value)); assert.equal(manifest.identity, identity); assert.equal(manifest.schema, layout === 'v13-postcard-index18' ? 'lycheedev.source-index.rust.v7' : 'lycheedev.source-index.rust.v6');
    } finally { db.close(); }
    digest(manifest.blob.sha256); path = join(home, 'cache/objects', manifest.blob.sha256.slice(0, 2), manifest.blob.sha256.slice(2));
    assert.equal(lstatSync(path).size, manifest.blob.bytes); assert.equal(hashFile(path, INDEX_BYTES), manifest.blob.sha256);
  }
  saveJSON(join(directory, `${engine}-index-manifest.json`), manifest);
  const sourceRef = {path, bytes: lstatSync(path).size, sha256: hashFile(path, INDEX_BYTES)};
  const parts = splitIndexEvidence(root, sourceRef, join(directory, `${engine}-index-parts`));
  return {...sourceRef, manifest, parts, metadata};
}
// Immutable output evidence only. Keep the exact files referenced by this index,
// never the whole cache/home or a reconstruction from the equal Go Facts.
const RUST_FACT_BYTES = 32 << 20, RUST_CUSTODY_BYTES = 512 << 20, RUST_DOCUMENTS = 100000;
const FACT_SECTIONS = ['declarations', 'relationships', 'loads', 'headers', 'diagnostics'];
const jsonBytes = value => Buffer.from(JSON.stringify(value, null, 2) + '\n');
function plannedPartsBytes(source) { return jsonBytes({schema: 'lycheedev.source-index-parts.v1', source, partBytes: PART_BYTES, parts: Array.from({length: Math.max(1, Math.ceil(source.bytes / PART_BYTES))}, (_, j) => ({path: `index.part${String(j).padStart(3, '0')}.bin`, offset: j * PART_BYTES, bytes: Math.min(PART_BYTES, source.bytes - j * PART_BYTES), sha256: '0'.repeat(64)}))}).length; }
function custodyBytes(path, maximum) { const stat = lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, 'bounded regular custody file'); return readFileSync(path); }
function rustReferences(index, home) {
  assert(Array.isArray(index.documents) && index.documents.length > 0 && index.documents.length <= RUST_DOCUMENTS, 'Rust custody document budget');
  assert.equal(index.summary?.documents, index.documents.length); assert.equal(index.summary.parser, 'source-v1');
  const paths = new Set(), files = new Map();
  for (const document of index.documents) {
    assert(typeof document.path === 'string' && !isAbsolute(document.path) && !document.path.includes('\\') && !document.path.includes(':') && document.path.split('/').every(p => p && p !== '.' && p !== '..'), 'Rust custody document path');
    assert(/\.(lua|xml|toc)$/i.test(document.path)); digest(document.sha256); assert(Number.isSafeInteger(document.bytes) && document.bytes >= 0); assert(!paths.has(document.path), 'duplicate Rust custody document'); paths.add(document.path);
    const locator = document.factsLocator;
    if (Object.hasOwn(document, 'factsLocator')) { exact(locator, ['group', 'offset', 'bytes', 'sha256']); digest(locator.group); digest(locator.sha256); assert(Number.isSafeInteger(locator.offset) && locator.offset >= 8 && Number.isSafeInteger(locator.bytes) && locator.bytes > 0 && locator.bytes <= RUST_FACT_BYTES && locator.offset + 68 + locator.bytes <= RUST_FACT_BYTES, 'Rust custody locator bounds'); }
    const name = `cache/source-facts-rust/${basename(rustFactPath(home, document, 'v11-pack2'))}`, kind = locator ? 'pack' : 'loose';
    if (!files.has(name)) files.set(name, {relative: name, kind, documents: []});
    const file = files.get(name); assert.equal(file.kind, kind, 'Rust custody object kind collision'); file.documents.push(document);
  }
  for (const file of files.values()) if (file.kind === 'pack') { let end = 8; for (const doc of [...file.documents].sort((a, b) => a.factsLocator.offset - b.factsLocator.offset)) { assert(doc.factsLocator.offset >= end, 'overlapping Rust custody locators'); end = doc.factsLocator.offset + 68 + doc.factsLocator.bytes; } }
  return [...files.values()];
}
function custodyManifest(directory, name, expectedSHA, maximum, expectedSource) {
  const path = contained(directory, join(directory, name)), raw = custodyBytes(path, 65536); digest(expectedSHA); assert.equal(sha256(raw), expectedSHA, 'Rust custody parts manifest changed');
  const manifest = JSON.parse(raw); assert(Number.isSafeInteger(manifest.source?.bytes) && manifest.source.bytes <= maximum, 'Rust custody source budget'); assert.deepEqual(manifest.source, expectedSource); verifyParts(resolve(path, '..'), manifest);
  return {directory: resolve(path, '..'), manifest};
}
function custodyRange(parts, offset, bytes) {
  assert(Number.isSafeInteger(offset) && Number.isSafeInteger(bytes) && offset >= 0 && bytes >= 0 && offset + bytes <= parts.manifest.source.bytes, 'Rust custody range');
  const output = Buffer.alloc(bytes); let used = 0;
  for (const part of parts.manifest.parts) {
    const first = Math.max(offset, part.offset), last = Math.min(offset + bytes, part.offset + part.bytes); if (first >= last) continue;
    const path = contained(parts.directory, join(parts.directory, part.path)), stat = lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size === part.bytes);
    const fd = openSync(path, 'r'); try { let count = 0; while (count < last - first) { const n = readSync(fd, output, used + count, Math.min(65536, last - first - count), first - part.offset + count); assert(n > 0, 'truncated custody range'); count += n; } used += count; } finally { closeSync(fd); }
  }
  assert.equal(used, bytes); return output;
}
function verifyCustodyFact(parts, document, home) {
  const locator = document.factsLocator; let raw;
  if (locator) {
    assert(custodyRange(parts, 0, 8).equals(Buffer.from('LYCFPK01')), 'Rust custody pack header');
    const header = custodyRange(parts, locator.offset, 68);
    assert.equal(header.readUInt32LE(0), locator.bytes); assert.equal(header.subarray(4, 36).toString('hex'), basename(rustFactPath(home, document, 'v11'), '.json')); assert.equal(header.subarray(36).toString('hex'), locator.sha256);
    raw = custodyRange(parts, locator.offset + 68, locator.bytes); assert.equal(sha256(raw), locator.sha256, 'Rust custody record digest');
  } else raw = custodyRange(parts, 0, parts.manifest.source.bytes);
  const value = JSON.parse(new TextDecoder('utf-8', {fatal: true}).decode(raw)); assert.equal(value.schema, 'lycheedev.source-facts.rust.v11'); assert.equal(value.inputSHA256, document.sha256); exact(value.facts, FACT_SECTIONS); for (const section of FACT_SECTIONS) assert(Array.isArray(value.facts[section]));
  const stable = v => Array.isArray(v) ? v.map(stable) : v && typeof v === 'object' ? Object.fromEntries(Object.keys(v).sort().map(k => [k, stable(v[k])])) : v;
  assert.equal(sha256(JSON.stringify(stable(value.facts))), value.factsSHA256, 'Rust custody Facts digest');
}
export function verifyRustCustody(reference, {comparison} = {}) {
  exact(reference, ['directory', 'catalog', 'catalogSHA256', 'candidate']); candidateIdentity(reference.candidate); digest(reference.catalogSHA256);
  const directory = realpathSync(reference.directory), catalogPath = contained(directory, reference.catalog); assert.equal(catalogPath, join(directory, 'rust-custody.json')); const raw = custodyBytes(catalogPath, 16 << 20); assert.equal(sha256(raw), reference.catalogSHA256, 'Rust custody catalog changed');
  const catalog = JSON.parse(raw); exact(catalog, ['schema', 'candidate', 'layout', 'home', 'documents', 'factsBytes', 'index', 'indexManifest', 'files']);
  assert.equal(catalog.schema, 'lycheedev.source-rust-facts-custody.v1'); exact(catalog.candidate, ['commit', ...Object.keys(CANDIDATE_IDENTITY)]); assert.deepEqual(candidateIdentity(catalog.candidate), reference.candidate); assert.equal(catalog.layout, 'v11-pack2'); assert(isAbsolute(catalog.home) && resolve(catalog.home) === catalog.home);
  exact(catalog.index, ['path', 'bytes', 'sha256', 'manifest', 'manifestSHA256']); assert.equal(catalog.index.manifest, 'rust-index-parts/manifest.json');
  const records = custodyManifest(directory, catalog.index.manifest, catalog.index.manifestSHA256, INDEX_BYTES, {path: catalog.index.path, bytes: catalog.index.bytes, sha256: catalog.index.sha256});
  assert.equal(catalog.index.path, resolve(catalog.home, 'cache/objects', catalog.index.sha256.slice(0, 2), catalog.index.sha256.slice(2)), 'Rust custody original index path');
  const index = comparison === undefined ? JSON.parse(new TextDecoder('utf-8', {fatal: true}).decode(custodyRange(records, 0, catalog.index.bytes))) : comparedRustIndex(comparison, catalog.index), references = rustReferences(index, catalog.home);
  exact(catalog.indexManifest, ['path', 'bytes', 'sha256']); assert.equal(catalog.indexManifest.path, 'rust-index-manifest.json');
  const manifestPath = contained(directory, join(directory, catalog.indexManifest.path)), manifestRaw = custodyBytes(manifestPath, 1 << 20); assert.equal(manifestRaw.length, catalog.indexManifest.bytes); assert.equal(sha256(manifestRaw), catalog.indexManifest.sha256);
  const manifest = JSON.parse(manifestRaw), s = index.summary; assert.equal(manifest.schema, 'lycheedev.source-index.rust.v6'); assert.equal(manifest.blob.sha256, catalog.index.sha256); assert.equal(manifest.blob.bytes, catalog.index.bytes);
  assert(packIndexIdentities(s.repository, s.product, s.commit, s.parser).includes(manifest.identity), 'exact authenticated pack2 recipe required');
  assert.equal(catalog.documents, index.documents.length); assert(Number.isSafeInteger(catalog.factsBytes) && catalog.factsBytes > 0 && catalog.factsBytes <= RUST_CUSTODY_BYTES, 'Rust custody total budget'); assert(Array.isArray(catalog.files)); assert.equal(catalog.files.length, references.length, 'complete Rust custody files required');
  let total = 0;
  for (const [i, file] of catalog.files.entries()) {
    exact(file, ['relative', 'kind', 'bytes', 'sha256', 'manifest', 'manifestSHA256']); const expected = references[i]; assert.equal(file.relative, expected.relative, 'Rust custody referenced file/order'); assert.equal(file.kind, expected.kind); assert.equal(file.manifest, `rust-fact-${String(i).padStart(5, '0')}/manifest.json`); assert(Number.isSafeInteger(file.bytes) && file.bytes > 0 && file.bytes <= RUST_FACT_BYTES, 'Rust custody file budget');
    total += file.bytes; assert(total <= RUST_CUSTODY_BYTES, 'Rust custody total budget');
    const parts = custodyManifest(directory, file.manifest, file.manifestSHA256, RUST_FACT_BYTES, {path: resolve(catalog.home, file.relative), bytes: file.bytes, sha256: file.sha256});
    for (const document of expected.documents) verifyCustodyFact(parts, document, catalog.home);
    verifyParts(parts.directory, parts.manifest); assert.equal(hashFile(contained(directory, join(directory, file.manifest)), 65536), file.manifestSHA256, 'Rust custody manifest changed during verify');
  }
  assert.equal(total, catalog.factsBytes); verifyParts(records.directory, records.manifest); assert.equal(hashFile(contained(directory, join(directory, catalog.index.manifest)), 65536), catalog.index.manifestSHA256); assert.equal(hashFile(manifestPath, 1 << 20), catalog.indexManifest.sha256); assert.equal(hashFile(catalogPath, 16 << 20), reference.catalogSHA256);
  return {documents: catalog.documents, files: catalog.files.length, factsBytes: total, indexSHA256: catalog.index.sha256};
}
export function captureRustCustody(home, index, directory, root, {admit = admission, limits = assertDiskPolicy(root), candidate, comparison} = {}) {
  const authenticatedCandidate = candidateIdentity(candidate);
  home = realpathSync(home); directory = contained(root, directory);
  const indexPath = contained(home, index.path); let parsed;
  if (comparison === undefined) {
    const raw = custodyBytes(indexPath, INDEX_BYTES); assert.equal(raw.length, index.bytes); assert.equal(sha256(raw), index.sha256);
    parsed = JSON.parse(new TextDecoder('utf-8', {fatal: true}).decode(raw));
  } else {
    const stat = lstatSync(indexPath); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= INDEX_BYTES, 'bounded regular custody file'); assert.equal(stat.size, index.bytes);
    assert.equal(hashFile(indexPath, INDEX_BYTES), index.sha256);
    parsed = comparedRustIndex(comparison, index);
  }
  const references = rustReferences(parsed, home), files = []; let total = 0, partsBytes = 0;
  const manifestPath = contained(directory, join(directory, 'rust-index-manifest.json')), manifestRaw = custodyBytes(manifestPath, 1 << 20); assert.deepEqual(JSON.parse(manifestRaw), index.manifest);
  const sources = references.map(item => { const path = contained(home, join(home, item.relative)), stat = lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size > 0 && stat.size <= RUST_FACT_BYTES, 'bounded regular Rust Facts source'); total += stat.size; assert(total <= RUST_CUSTODY_BYTES, 'Rust custody total budget'); return {path, bytes: stat.size}; });
  for (const [i, item] of references.entries()) {
    const {path, bytes} = sources[i], sha = hashFile(path, RUST_FACT_BYTES);
    for (const document of item.documents) readRustFact(home, document, 'v11-pack2');
    partsBytes += plannedPartsBytes({path, bytes, sha256: sha});
    files.push({relative: item.relative, kind: item.kind, bytes, sha256: sha, manifest: `rust-fact-${String(i).padStart(5, '0')}/manifest.json`, manifestSHA256: '0'.repeat(64)});
  }
  const catalog = {schema: 'lycheedev.source-rust-facts-custody.v1', candidate: authenticatedCandidate, layout: 'v11-pack2', home, documents: references.reduce((n, item) => n + item.documents.length, 0), factsBytes: total,
    index: {path: index.path, bytes: index.bytes, sha256: index.sha256, manifest: 'rust-index-parts/manifest.json', manifestSHA256: hashFile(contained(directory, join(directory, 'rust-index-parts/manifest.json')), 65536)}, indexManifest: {path: 'rust-index-manifest.json', bytes: manifestRaw.length, sha256: sha256(manifestRaw)}, files};
  assert(jsonBytes(catalog).length <= 16 << 20, 'Rust custody catalog budget');
  // Exact measured file bytes and encoded manifests/catalog, plus the existing
  // bounded 16 MiB admission report slot. No scratch copy is allocated.
  for (const name of ['rust-custody.json', 'rust-custody-admission.json', ...files.map(file => file.manifest.split('/')[0])]) { let stat; try { stat = lstatSync(join(directory, name)); } catch (error) { if (error.code !== 'ENOENT') throw error; } assert(!stat, 'new Rust custody paths required'); }
  const requiredBytes = total + partsBytes + jsonBytes(catalog).length + (16 << 20);
  const admitted = admit(root, {limits, requiredBytes, scratchReserveBytes: 0});
  saveJSON(join(directory, 'rust-custody-admission.json'), {requiredBytes, factsBytes: total, partsManifestBytes: partsBytes, catalogBytes: jsonBytes(catalog).length, admission: admitted});
  for (const file of files) { const source = {path: contained(home, join(home, file.relative)), bytes: file.bytes, sha256: file.sha256}; splitIndexEvidence(root, source, resolve(directory, file.manifest, '..')); file.manifestSHA256 = hashFile(contained(directory, join(directory, file.manifest)), 65536); }
  const catalogPath = join(directory, 'rust-custody.json'), bytes = jsonBytes(catalog); writeFileSync(catalogPath, bytes, {flag: 'wx'});
  const reference = {directory, catalog: catalogPath, catalogSHA256: sha256(bytes), candidate: authenticatedCandidate}; verifyRustCustody(reference, {comparison});
  assert.equal(hashFile(index.path, INDEX_BYTES), index.sha256, 'Rust index changed during custody'); for (const file of files) assert.equal(hashFile(contained(home, join(home, file.relative)), RUST_FACT_BYTES), file.sha256, 'Rust Facts changed during custody');
  return reference;
}

// Fresh cold/warm need the same immutable Go body custody as changed cases.
// Only an existing retained Go custody can bypass this new copy, never a cache flag.
export async function captureComparedGo(home, index, directory, root, {admit = admission, limits = assertDiskPolicy(root)} = {}) {
  home = realpathSync(home); directory = contained(root, directory); assert.equal(hashFile(contained(home, index.path), INDEX_BYTES), index.sha256);
  for (const name of ['go-custody.json', 'go-custody-admission.json', 'go-original-manifest.json']) { let stat; try { stat = lstatSync(join(directory, name)); } catch (error) { if (error.code !== 'ENOENT') throw error; } assert(!stat, 'new Go custody paths required'); }
  const files = new Map(); let total = 0, documents = 0, partsBytes = 0;
  for await (const line of readline.createInterface({input: createReadStream(index.path), crlfDelay: Infinity})) {
    if (!line) continue; const doc = JSON.parse(line); if (doc.kind !== 'document') continue; assert(++documents <= RUST_DOCUMENTS);
    const item = goFactRelative(doc, index.manifest.summary.parser); if (files.has(item.relative)) continue;
    const path = contained(home, join(home, item.relative)), stat = lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= RUST_FACT_BYTES, 'bounded regular Go Facts source'); total += stat.size; assert(total <= RUST_CUSTODY_BYTES, 'Go custody total budget');
    const source = {path, bytes: stat.size, sha256: hashFile(path, RUST_FACT_BYTES)}; files.set(item.relative, source); partsBytes += plannedPartsBytes(source);
  }
  assert.equal(documents, index.manifest.summary.documents); assert.equal(hashFile(index.path, INDEX_BYTES), index.sha256);
  const originalManifestBytes = custodyBytes(contained(home, join(resolve(index.path, '..'), 'manifest.json')), 1 << 20).length;
  const originalManifestReserveBytes = 1 << 20; // Existing helper limit, including equivalent JSON whitespace growth.
  const requiredBytes = total + partsBytes + originalManifestReserveBytes + (32 << 20); // Two bounded slots: catalog and admission report.
  for (let i = 0; i < files.size; i++) { let stat; try { stat = lstatSync(join(directory, `go-fact-${String(i).padStart(5, '0')}`)); } catch (error) { if (error.code !== 'ENOENT') throw error; } assert(!stat, 'new Go custody paths required'); }
  const admitted = admit(root, {limits, requiredBytes, scratchReserveBytes: 0}); saveJSON(join(directory, 'go-custody-admission.json'), {requiredBytes, factsBytes: total, partsManifestBytes: partsBytes, originalManifestBytes, originalManifestReserveBytes, admission: admitted});
  const reference = await captureGoCustody(home, index, directory, root, {expectedDocuments: documents, expectedSources: [...files].map(([relative, source]) => ({relative, bytes: source.bytes, sha256: source.sha256}))}); openGoCustody(reference).verify();
  for (const source of files.values()) assert.equal(hashFile(source.path, RUST_FACT_BYTES), source.sha256, 'Go Facts changed during custody'); return reference;
}

// Bind the exact bytes used by the semantic comparison, never a later rehash
// of the input paths. Packed record tuples are also tied to the archived index.
export function bindComparedCustody(delta, goReference, rustReference, {reuseIndex = false} = {}) {
  const readCatalog = reference => { const raw = custodyBytes(reference.catalog, 16 << 20); assert.equal(sha256(raw), reference.catalogSHA256); return JSON.parse(raw); };
  const go = readCatalog(goReference), rust = readCatalog(rustReference), sources = delta.factSources;
  assert(sources && Array.isArray(sources.go) && Array.isArray(sources.rust), 'comparison byte tuples required');
  assert.equal(go.documents, delta.documents); assert.equal(rust.documents, delta.documents);
  assert.deepEqual(sources.go, go.files.map(file => ({file: resolve(go.home, file.relative), bytes: file.bytes, sha256: file.sha256})), 'compared Go bytes differ from custody');
  assert.equal(delta.inputs.rustIndexSHA256, rust.index.sha256, 'compared Rust index differs from custody');
  const parts = custodyManifest(rustReference.directory, rust.index.manifest, rust.index.manifestSHA256, INDEX_BYTES, {path: rust.index.path, bytes: rust.index.bytes, sha256: rust.index.sha256});
  const index = reuseIndex ? comparedRustIndex(delta, rust.index) : JSON.parse(custodyRange(parts, 0, rust.index.bytes)), files = new Map(rust.files.map(file => [resolve(rust.home, file.relative), file])), used = new Set();
  assert.equal(sources.rust.length, index.documents.length, 'complete compared Rust document tuples');
  for (const [i, document] of index.documents.entries()) {
    const file = rustFactPath(rust.home, document, 'v11-pack2'), archived = files.get(file); assert(archived, 'compared Rust file missing from custody'); used.add(file);
    const locator = document.factsLocator, expected = {path: document.path, file, bytes: locator ? locator.bytes : archived.bytes, sha256: locator ? locator.sha256 : archived.sha256, ...(locator ? {record: locator} : {})};
    assert.deepEqual(sources.rust[i], expected, 'compared Rust bytes differ from custody');
  }
  assert.equal(used.size, files.size, 'complete compared Rust file set');
  openGoCustody(goReference).verify(); verifyRustCustody(rustReference, {comparison: reuseIndex ? delta : undefined});
}
export async function compareFacts(homes, outputs, pin, oracle, directory, root, retained = undefined, custodyOptions = {}) {
  const indexes = retained ? {go: retained.index, rust: indexEvidence(homes.rust, 'rust', pin, directory, root)} : Object.fromEntries(['go', 'rust'].map(engine => [engine, indexEvidence(homes[engine], engine, pin, directory, root)]));
  const custody = retained && openGoCustody(retained.reference);
  const delta = await readFactDelta(custody?.home ?? homes.go, homes.rust, indexes.go.path, 'v11-pack2', indexes.rust.path, indexes.rust.sha256, custody, {captureSources: true, retainIndex: true});
  custody?.verify();
  saveJSON(join(directory, 'facts-comparison.json'), delta); // Retain complete mismatch diagnostics before failing.
  assert.equal(delta.differentFiles.length, 0, 'complete five-array fields/order differ'); assert.equal(delta.documents, oracle.documents);
  const values = Object.fromEntries(['go', 'rust'].map(engine => [engine, semantic('B02-retail-index', outputs[engine], engine, oracle, pin.id).value]));
  assert.deepEqual(values.go, values.rust, 'complete shared summary differs');
  for (const engine of ['go', 'rust']) for (const field of ['declarations', 'relationships', 'diagnostics']) assert.equal(delta.totals[engine][field], values[engine][field], `${engine} full Facts totals`);
  const goCustody = retained?.reference ?? await captureComparedGo(homes.go, indexes.go, directory, root, custodyOptions);
  const rustCustody = captureRustCustody(homes.rust, indexes.rust, directory, root, {...custodyOptions, comparison: delta});
  bindComparedCustody(delta, goCustody, rustCustody, {reuseIndex: true});
  return {goCustody, rustCustody, documents: delta.documents, fullFieldsAndOrderEqual: true, totals: delta.totals, indexes: Object.fromEntries(Object.entries(indexes).map(([k, v]) => [k, {path: v.path, bytes: v.bytes, sha256: v.sha256}])), summary: values.go, evidence: join(directory, 'facts-comparison.json')};
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
function authenticateRecordedCall(row, home, stem, runId, pin, inputs, project = undefined) {
  assert.match(runId, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/);
  const req = boundedJSON(stem + '.request.json'), proof = boundedJSON(stem + '.receipt.json'), wrapper = boundedJSON(stem + '.request.json.wrapper.json');
    assert.equal(row.home, home); assert.equal(row.exit, 0); assert.equal(row.valid, true); assert.equal(row.handlesReaped, true); assert(!row.error); assert.equal(req.schema, 'lycheedev.owned-request.v2'); assert.equal(req.runId, runId); assert.equal(req.command, row.binary); assert.equal(req.cwd, row.cwd); assert.equal(req.timeoutMs, 210000); assert.equal(req.orphanMs, 30000); assert.equal(req.cargoBuild, false); assert.equal(req.msvcCompile, false); assert(!req.startupDiagnostics);
    assert.deepEqual(req.args, ['source', 'index', ...(project ? ['--project', project] : ['--snapshot', pin.id]), '--home', home, '--format', 'json']); assert.deepEqual(row.args, req.args);
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
  assertHistoricalCandidate(request);
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
  const custodyDirectory = resolve(custodyReceipt, '..'); assert.deepEqual(evidenceFiles(custodyDirectory).sort(), custody.sealedLogs.map(log => log.path).sort(), 'complete sealed cold custody files required');
  const factsCatalog = boundedJSON(join(custodyDirectory, 'cold-facts-custody.json'));
  if (factsCatalog.schema === 'lycheedev.source-pack2-cold-facts.v2') {
    exact(factsCatalog, ['schema', 'go', 'rust']); assert.deepEqual(factsCatalog.go, verified.facts.goCustody); assert.deepEqual(factsCatalog.rust, verified.facts.rustCustody);
    for (const side of ['go', 'rust']) assert.equal(factsCatalog[side].directory, custodyDirectory, 'cold custody must belong to revalidation owner');
    const go = openGoCustody(factsCatalog.go); assert.equal(go.home, paths.homes.go); assert.equal(JSON.parse(go.manifestRaw).summary.documents, 4036); go.verify();
    assert.deepEqual(factsCatalog.rust.candidate, {commit: CANDIDATE_COMMIT, ...CANDIDATE_IDENTITY}); assert.equal(verifyRustCustody(factsCatalog.rust).documents, 4036); assert.equal(boundedJSON(factsCatalog.rust.catalog).home, paths.homes.rust);
  } else assert.equal(factsCatalog.schema, 'lycheedev.source-pack2-cold-facts.v1', 'known complete cold custody required');
  for (const engine of ['go', 'rust']) { assert.equal(before[engine].sha256, reference.homeInventorySHA256[engine]); assert.equal(inventory(paths.homes[engine]).sha256, before[engine].sha256, 'borrow home changed before claim'); }
  const record = {schema: 'lycheedev.source-pack2-cold-borrow.v1', owner: owner.id, originalOwner: RETAINED_COLD.base, originalState: 'failed', reference, homes: paths.homes, before, custodyReceipt, writeScope: 'same-home warm only; changed scenarios clone these homes independently', cleanupScope: owner.directory, claimedAt: new Date().toISOString()};
  writeFileSync(paths.claim, JSON.stringify(record, null, 2) + '\n', {flag: 'wx'}); saveJSON(join(owner.evidence, 'borrow-claim.json'), record); return {path: paths.claim, sha256: hashFile(paths.claim)};
}
export function captureColdFactFiles(homes, inventories, directory, root, {rustArchived = false} = {}) {
  const files = [];
  for (const engine of (rustArchived ? ['go'] : ['go', 'rust'])) for (const row of inventories[engine].rows.filter(r => r.path.startsWith(engine === 'go' ? 'source/v1/facts/files/' : 'cache/source-facts-rust/'))) {
    const source = {path: contained(homes[engine], join(homes[engine], row.path)), bytes: row.bytes, sha256: row.sha256};
    const target = join(directory, `${engine}-cold-fact-${files.length}`); splitIndexEvidence(root, source, target); files.push({engine, relative: row.path, ...source, manifest: join(target, 'manifest.json'), manifestSHA256: hashFile(join(target, 'manifest.json'), 65536)});
  }
  assert(files.length > 0); saveJSON(join(directory, 'cold-facts-custody.json'), {schema: 'lycheedev.source-pack2-cold-facts.v1', files}); return files.length;
}

export const RETAINED_WARM = Object.freeze({outer: 'd1989b3b-573a-423e-9185-86b89fc6d2f4', base: 'bff2649b-3f34-4e15-b14e-48855cff42fe', custody: '786d460f-cfd9-4a21-9322-6b79d5e5a08d', runnerSHA256: '326d62596fdc66e2907e288d1658a2760e0d02faa668f17f1f28443aa347cf54', custodyReceiptSHA256: '2290e03f9c3df4a8a0f9ac93c9a21a76d274be4bf31ffb3bba4410c843afa461', coldSHA256: '9f0612c3b9b410b230c64ac2f550764356e14ef3ead725e9c0a3a9910060274a', claimSHA256: '514fb250a2d885f9f154609e658f5969f621c71c08ad49d65f93f64580f77a10'});
export const FRESH_ALLOWANCE = Object.freeze({schema: 'lycheedev.source-pack2-fresh-allowance.v1', scope: 'PR14-source-pack2-fresh', scratchBytes: 6 * 1024 ** 3, authorizationReference: 'Sentinel_a57db18d476081919e647f3195a645a1'});
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
export function authenticateRemaining(request, reference, bindings = RETAINED_WARM, retainedClaimSHA256 = undefined) {
  assertHistoricalCandidate(request);
  validateRemainingReference(reference); const paths = remainingPaths(request.ownerRoot), hashes = [], receipts = {};
  if (retainedClaimSHA256) { assert.equal(hashFile(paths.remainingClaim), retainedClaimSHA256, 'retained remaining claim changed'); hashes.push({path: paths.remainingClaim, sha256: retainedClaimSHA256}); }
  else assert(!existsSync(paths.remainingClaim), 'remaining variants already claimed; no replacement calls');
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

// Exactly the first successful Go call retained by 2d1/PR14#6237641406.
// This is a seven-call continuation, not a retry/reopen of its failed owner.
export const RETAINED_GO = Object.freeze({outer: '620316bf-04c6-4e21-9c47-f25c901fbcd5', base: 'dd94aed1-1137-4bb5-8d4c-659bf367468c', job: '6a2dc3a8-f133-4330-ac54-542ff403a1a4', run: '32ebcee6-42ee-46bf-ada0-5362b2cd457e', commit: '752e113c973639dc8eb8654b50808e65cf9847bd', runnerSHA256: '915aa174e90574c7a73152ac29197d5c20652abf765709710f89fd4567ccf7af', claimSHA256: '80fea14f348f94dcc5829da925c218d59f86575f1ff8dc9124b3bf8940e83d6d'});
const goContinuationPaths = root => ({...Object.fromEntries(['outer', 'base', 'job'].map(role => [role, contained(root, join(root, '.tmp/rust-disk/evidence', RETAINED_GO[role]))])), home: contained(root, join(root, '.tmp/rust-scratch', RETAINED_GO.job, 'go-home')), claim: contained(root, join(root, '.tmp/rust-disk', `source-pack2-seven-${RETAINED_GO.outer}.json`))});
export function validateGoReference(value) {
  exact(value, ['schema', 'outerReceiptSHA256', 'baseReceiptSHA256', 'jobReceiptSHA256', 'goHomeInventorySHA256']); assert.equal(value.schema, 'lycheedev.source-pack2-retained-go.v1');
  for (const key of ['outerReceiptSHA256', 'baseReceiptSHA256', 'jobReceiptSHA256', 'goHomeInventorySHA256']) digest(value[key]); return value;
}
export function inspectRetainedGo(request) {
  const paths = goContinuationPaths(request.ownerRoot), reference = {schema: 'lycheedev.source-pack2-retained-go.v1', ...Object.fromEntries(['outer', 'base', 'job'].map(role => [`${role}ReceiptSHA256`, hashFile(join(paths[role], 'receipt.json'), 16 << 20)])), goHomeInventorySHA256: inventory(paths.home).sha256};
  authenticateRetainedGo(request, reference); return reference;
}
export function authenticateRetainedGo(request, reference, bindings = RETAINED_GO) {
  assertHistoricalCandidate(request);
  validateGoReference(reference); const paths = goContinuationPaths(request.ownerRoot), hashes = [], receipts = {};
  assert(!existsSync(paths.claim), 'seven remaining calls already claimed; no replacement calls');
  for (const [role, owner] of [['outer', 'source-pack2-diagnostics'], ['base', 'source-pack2-remaining-cases'], ['job', 'source-pack2-single-content-change-go']]) receipts[role] = authenticateSealedOwner(request.ownerRoot, paths[role], RETAINED_GO[role], owner, 'failed', reference[`${role}ReceiptSHA256`], hashes);
  const config = boundedJSON(join(paths.outer, 'configuration.json')), prior = boundedJSON(join(paths.outer, 'report.json')), owned = boundedJSON(join(paths.outer, 'owned-report.json'));
  for (const value of [config, prior, owned]) { assert.deepEqual(value.request, request); assert.deepEqual(value.inputs, config.inputs); }
  assert.deepEqual(Object.keys(config.inputs).sort(), [...RUNNER_FILES].sort()); assert.equal(sha256(JSON.stringify(RUNNER_FILES.map(path => [path, config.inputs[path]]))), RETAINED_GO.runnerSHA256, 'exact original 2d1 runner required');
  assert.deepEqual(receipts.outer.inputHashes.runner, config.inputs); assert.deepEqual(receipts.outer.inputHashes.retainedWarm, config.resumeRemaining); assert.deepEqual(config.allowance, REMAINING_ALLOWANCE); assert.deepEqual(receipts.outer.inputHashes.allowance, config.allowance);
  assert.equal(config.evidence, paths.outer); assert.equal(config.scratch, receipts.outer.directory); assert.deepEqual(prior.diskPolicy, {...defaults, scratchBytes: REMAINING_ALLOWANCE.scratchBytes});
  assert.equal(prior.schema, 'lycheedev.source-pack-scenarios.v1'); assert.equal(prior.state, 'source_pack_scenarios_failed'); assert.equal(prior.blocker, 'actual scenario footprint exceeded admitted estimate'); assert.equal(prior.requiresOuterRetirement, false); assert.equal(prior.finalIntegrityVerified, false); assert.equal(prior.importedSamples, 0); assert.equal(prior.layout, 'v11-pack2'); assert.equal(prior.productCommit, CANDIDATE_COMMIT);
  assert(!prior.admissionFailure && !prior.remainingIntegrityFailure && !prior.cleanupFailure); assert.equal(owned.state, 'failed'); assert.equal(owned.schema, 'lycheedev.source-pack-scenarios-owned.v1'); assert.equal(owned.process.exit, 1); assert.equal(owned.process.handlesReaped, true); elapsed(owned.process.proof);
  const warm = authenticateRemaining(request, config.resumeRemaining, bindings.warm ?? RETAINED_WARM, bindings.claimSHA256);
  for (const key of ['hostname', 'platform', 'powerBefore']) assert.equal(prior.machine?.[key], warm.prior.machine?.[key], 'original machine/conditions changed');
  const oldClaim = boundedJSON(warm.paths.remainingClaim); assert.equal(oldClaim.owner, RETAINED_GO.base); assert.equal(oldClaim.priorOuter, RETAINED_WARM.outer); assert.equal(oldClaim.priorState, 'failed'); assert.deepEqual(oldClaim.reference, config.resumeRemaining); assert.deepEqual(oldClaim.readOnlyHomes, warm.paths.homes); assert.deepEqual(oldClaim.remainingCases, CASES.slice(2).map(row => row.id)); assert.equal(oldClaim.cleanupScope, receipts.base.directory); assert.deepEqual(boundedJSON(join(paths.base, 'remaining-claim.json')), oldClaim);
  assert.equal(prior.continuation.originalCallsReused, 4); assert.equal(prior.continuation.freshColdCalls, 0); assert.equal(prior.continuation.freshWarmCalls, 0); assert.equal(prior.continuation.priorOuter, RETAINED_WARM.outer); assert.deepEqual(prior.continuation.reference, config.resumeRemaining); assert.deepEqual(prior.continuation.claim, {path: warm.paths.remainingClaim, sha256: bindings.claimSHA256});
  assert.deepEqual(prior.scenarios.map(row => row.id), ['cold', 'warm', 'single-content-change']); assert.deepEqual(prior.scenarios.slice(0, 2), [warm.cold, warm.warm]);
  assert.deepEqual(prior.invocationAttempts, [{caseId: 'single-content-change', engine: 'go'}]); assert.deepEqual(prior.notRunCases, CASES.slice(3).map(row => row.id)); assert.deepEqual(prior.notRunInvocations, CASES.slice(2).flatMap((row, i) => (i ? ['go', 'rust'] : ['rust']).map(engine => ({caseId: row.id, engine}))));
  assert.deepEqual(prior.owners.map(row => [row.role, row.engine ?? null, row.receipt]), [['read-only-healthy-base-reference', null, join(paths.base, 'receipt.json')], ['single-content-change', 'go', join(paths.job, 'receipt.json')]]);
  for (const file of RUNNER_FILES) { assert.equal(receipts.base.inputHashes[file], config.inputs[file]); assert.equal(receipts.job.inputHashes[file], config.inputs[file]); }
  assert.deepEqual(receipts.base.inputHashes.retainedWarm, config.resumeRemaining); assert.deepEqual(receipts.base.inputHashes.readOnlyHomes, warm.paths.homes); assert.deepEqual(receipts.base.inputHashes.allowance, config.allowance); assert.equal(receipts.job.inputHashes.healthyBase, warm.baseIdentity.go.sha256);
  const row = prior.scenarios[2]; assert.equal(row.state, 'failed'); assert.equal(row.failure, prior.blocker); assert.equal(row.factsEquivalent, false); assert(!row.rust && !row.facts && !row.go.custody, 'only uncaptured successful Go result is eligible'); assert.deepEqual(row.order, ['go', 'rust']); assert.equal(row.mode, 'changed'); assert.deepEqual(boundedJSON(join(paths.job, 'single-content-change-go.json')), row);
  const variant = boundedJSON(join(paths.job, 'mutation.json')); assert.equal(row.go.fixture, join(paths.job, 'mutation.json')); assert.equal(variant.mutation.scenario, row.id); assert.equal(variant.mutation.baseCommit, BASE_COMMIT); assert.equal(variant.mutation.commit, RETAINED_GO.commit); assert.equal(variant.mutation.newBytes, 2534); assert.equal(variant.mutation.newSHA256, '5b11f2f208987a917396e2e189876b95c3eaa1b9414c820aa8ae048a139db3fd'); assert.equal(variant.mutation.originalSHA256, '5c48438fa06252e7eacb5dc752f4959cb6e2b20cbc639fc474229fe6fdbe4053'); assert.equal(variant.mutation.path, FIRST_PATH); assert.equal(variant.mutation.originalPath, FIRST_PATH);
  const rows = structuredClone(warm.rows), first = rows.find(value => value.path === FIRST_PATH); assert(first); Object.assign(first, {bytes: 2534, sha256: variant.mutation.newSHA256, gitSHA1: variant.mutation.newBlob}); assert.deepEqual(rows, variant.rows);
  const oracle = variantOracle(bindings.oracle ?? boundedJSON(join(import.meta.dirname, '../rust-source-full-corpus/retail-oracle.json')), rows, RETAINED_GO.commit); assert.deepEqual(variant.oracle, oracle); assertCorpus(rows, oracle);
  const pin = fixturePin({repository: oracle.repository, product: oracle.product, requestedRef: RETAINED_GO.commit, exactCommit: RETAINED_GO.commit, parserRevision: 'source-v1'}); assert.deepEqual(variant.pin, pin); assert.deepEqual(row.pin, pin); assert.equal(variant.project, join(receipts.job.directory, 'variant-project'));
  assert.deepEqual(boundedJSON(join(variant.project, 'lycheedev.json')), {schema: 'lycheedev.project.v1', product: oracle.product}); assert.deepEqual(boundedJSON(join(variant.project, 'lycheedev.lock.json')), {schema: 'lycheedev.project-lock.v1', selection: pin}); for (const name of ['lycheedev.json', 'lycheedev.lock.json']) { const path = join(variant.project, name); hashes.push({path, sha256: hashFile(path)}); }
  const output = authenticateRecordedCall(row.go, paths.home, join(paths.job, `process-${RETAINED_GO.run}`), RETAINED_GO.run, pin, config.inputs, variant.project); semantic('B02-retail-index', output.envelope, 'go', oracle, pin.id); assert.equal(output.envelope.context?.snapshot, pin.id);
  const homeIdentity = inventory(paths.home); assert.equal(homeIdentity.sha256, reference.goHomeInventorySHA256, 'retained successful Go home changed'); assert.equal(homeIdentity.bytes, row.go.actualHomeBytes); assert.equal(row.go.actualHomeBytes, bindings.homeBytes ?? 464365379); assert.equal(prior.owners[1].estimate.scratchReserveBytes, 380939478); metadataIdentity(paths.home);
  const verifyEvidence = () => { warm.verifyEvidence(); for (const item of hashes) assert.equal(hashFile(item.path, 16 << 20), item.sha256, 'retained Go evidence changed'); };
  const verifyHomes = () => { warm.verifyHomes(); assert.equal(inventory(paths.home).sha256, homeIdentity.sha256, 'retained Go home changed'); };
  return {...warm, prior, reference, pending: structuredClone(row), goPaths: paths, retainedGo: {row: structuredClone(row), output, variant, homeIdentity}, verifyEvidence, verifyHomes};
}
export function claimSeven(root, reference, owner, retained) {
  validateGoReference(reference); const paths = goContinuationPaths(root), receipt = boundedJSON(owner.receipt);
  assert.match(owner.id, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/); assert.equal(owner.receipt, contained(root, join(root, '.tmp/rust-disk/evidence', owner.id, 'receipt.json'))); assert.equal(owner.evidence, resolve(owner.receipt, '..')); assert.equal(owner.directory, contained(root, join(root, '.tmp/rust-scratch', owner.id))); assert.equal(receipt.directory, owner.directory); assert.equal(receipt.id, owner.id); assert.equal(receipt.owner, 'source-pack2-seven-calls'); assert.equal(receipt.state, 'active'); assert.deepEqual(receipt.inputHashes.retainedGo, reference); assert.deepEqual(receipt.inputHashes.readOnlyHomes, retained.paths.homes);
  retained.verifyEvidence(); retained.verifyHomes(); const record = {schema: 'lycheedev.source-pack2-seven-claim.v1', owner: owner.id, priorOuter: RETAINED_GO.outer, priorState: 'failed', reference, retainedRun: RETAINED_GO.run, readOnlyHomes: {...retained.paths.homes, retainedGo: paths.home}, cleanupScope: owner.directory, remainingInvocations: CASES.slice(2).flatMap((row, i) => (i ? ['go', 'rust'] : ['rust']).map(engine => ({caseId: row.id, engine})))};
  writeFileSync(paths.claim, JSON.stringify(record, null, 2) + '\n', {flag: 'wx'}); saveJSON(join(owner.evidence, 'seven-claim.json'), record); return {path: paths.claim, sha256: hashFile(paths.claim)};
}

export async function runController(config, overrides = {}) {
  const svc = {metadataPageSize, mutationFootprint, footprint, authenticateRetainedGo, claimSeven, authenticateRemaining, claimRemaining, assertCorpus, authenticateRetainedCold, claimColdBorrow, captureColdFactFiles, beginScratch, admission, ownedSpawnSync, collectOwnedMemory, runnerIdentity, verifyCandidateRecord, verifyGoPackage, cloneSeed, inventory, derivedIdentity, proveWarmReuse, prepareVariant, compareFacts, verifyPackProfile, indexEvidence, captureGoCustody, openGoCustody, ...overrides};
  const {request, runnerRoot, inputs, conditions, scratch, evidence: out} = config;
  const layout = config.layout ?? 'v11-pack2'; assert(['v11-pack2', 'v13-postcard', 'v13-postcard-index18'].includes(layout), 'explicit supported controller layout required');
  if (layout !== 'v11-pack2') {
    assert(!config.resumeCold && !config.resumeRemaining && !config.resumeGo, 'v13 uses fresh scenarios without historical continuation');
    if (config.allowance !== undefined) assert.deepEqual(config.allowance, FRESH_ALLOWANCE, 'only explicitly authorized task-local 6 GiB allowance');
    for (const hook of ['runnerIdentity', 'compareFacts', 'verifyAdditionalIdentity', 'verifyAdditionalEvidence']) assert.equal(typeof overrides[hook], 'function', 'v13 requires its authenticated exporter controller');
  }
  assert([config.resumeCold, config.resumeRemaining, config.resumeGo].filter(Boolean).length <= 1, 'one exact continuation mode required');
  if (config.resumeCold || config.resumeRemaining || config.resumeGo) assertHistoricalCandidate(request);
  assert(!config.resumeCold || !config.allowance, 'cold continuation keeps original disk policy');
  const diskPolicy = assertDiskPolicy(request.ownerRoot, process.env, config.allowance, config.resumeGo ?? config.resumeRemaining);
  const oracle = JSON.parse(readFileSync(join(runnerRoot, 'tests/rust-source-full-corpus/retail-oracle.json')));
  assert.equal(oracle.commit, BASE_COMMIT); assert.equal(oracle.documents, 4036);
  let fatal = false, sequence = 0, baseOwner, baseSuccess = false, retained, borrowClaim, remaining, remainingClaim;
  const start = performance.now();
  const report = {schema: 'lycheedev.source-pack-scenarios.v1', diskPolicy, state: 'running', request, inputs, runnerRoot, conditions, productCommit: request.candidate.commit, layout, importedSamples: 0, performanceAcceptanceEvaluated: false, fullB01B07Acceptance: false, preparation: [], scenarios: [], invocationAttempts: [], owners: [], finalIntegrityVerified: false,
    machine: {hostname: os.hostname(), platform: process.platform, node: process.version, cpus: os.cpus(), totalMemory: os.totalmem()},
    timing: 'Native total CreateProcessW through descendant reaping/output drain; 31 overlapping phases are attribution, never subtracted or added. Wrapper wall separate.', cacheDefinition: config.resumeGo ? 'Five original calls retained; authenticate/capture the completed Go mutation read-only, then only seven unexecuted calls on independent healthy-base clones.' : config.resumeRemaining ? 'Four original cold/warm calls retained; only the four unexecuted variants run, each on a fresh independent clone of the sealed healthy base. No base copies or baseline product calls.' : config.resumeCold ? 'Original fixed 7d8 init/sync/cold calls reauthenticated read-only; exact two retained homes borrowed for same-home warm; each changed engine copies this healthy base independently. No fresh cold calls or OS cache flush.' : 'Fresh own init/sync-only homes for cold; same successful base homes for warm; each changed pair copies the original healthy base independently. No OS cache flush.', metrics: {cpu: 'not_measured', io: 'not_measured', rss: 'not_measured; owned-Job sampled aggregate and gaps retained, not unique physical RSS'}};
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
    assert(!fatal); const run = spawnSync('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-c', 'index.version=2', '-c', 'core.splitIndex=false', '-c', 'core.untrackedCache=false', '-c', 'core.hooksPath=' + (process.platform === 'win32' ? 'NUL' : '/dev/null'), ...args], {cwd, input, env: {...gitEnv, ...environment}, timeout: 60000, maxBuffer: 16 << 20, windowsHide: true, shell: false});
    const prefix = join(out, `git-${sequence++}`), stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0);
    writeFileSync(prefix + '.stdout.bin', stdout); writeFileSync(prefix + '.stderr.bin', stderr);
    report.preparation.push({label: 'owned-outer-job Git fixture/audit', args, cwd, inputSHA256: input === undefined ? null : sha256(Buffer.from(input)), exit: run.status, error: run.error?.message, stdoutSHA256: sha256(stdout), stderrSHA256: sha256(stderr)});
    if (run.error || run.signal) fatal = true; save(); assert(!run.error && run.status === 0, 'Git fixture/audit failed'); return stdout;
  }
  function identities() {
    assert.deepEqual(svc.runnerIdentity(runnerRoot), inputs, 'runner changed');
    const candidate = svc.verifyCandidateRecord(request.candidate);
    svc.verifyAdditionalIdentity?.(request);
    if (!overrides.verifyCandidateRecord) {
      for (const [root, commit] of [[request.candidate.sourceRoot, request.candidate.commit], [request.go.sourceRoot, GO_COMMIT]]) verifyCleanCheckout(root, commit, (args, cwd) => git(args, undefined, {}, cwd));
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
      row.facts = await svc.compareFacts(homes, envelopes, selection.pin, selection.oracle, evidence, request.ownerRoot, undefined, {admit: svc.admission, limits: diskPolicy, candidate: request.candidate, layout});
      row.factsEquivalent = true; identities(); row.rustToGoRatio = row.rust.controllableMillis / row.go.controllableMillis; row.state = 'verified'; return row;
    } catch (error) { if (error.handlesReaped === false) fatal = true; if (error.postcardEvidence) row.factsFailure = error.postcardEvidence; row.state = fatal ? 'interrupted' : 'failed'; row.failure = error.message.slice(0, 4096); throw error; }
    finally { saveJSON(join(evidence, `${entry.id}-scenario.json`), row); save(); }
  }
  save();
  try {
    const binaries = identities();
    if (!overrides.prepareSeeds) report.goMetadata = verifyGoMetadata(prepare('go', ['version', '-m', binaries.go.binary], 'Go323 binary metadata').toString());
    report.machine.powerBefore = prepare('powercfg.exe', ['/getactivescheme'], 'power-before').toString();
    let homes, prepared, cold, coldEvidence;
    if (config.resumeRemaining || config.resumeGo) {
      remaining = config.resumeGo ? svc.authenticateRetainedGo(request, config.resumeGo) : svc.authenticateRemaining(request, config.resumeRemaining); homes = remaining.paths.homes; prepared = {pin: remaining.pin, rows: remaining.rows}; svc.assertCorpus(prepared.rows, oracle);
      for (const original of Object.values(remaining.outputs)) { const engine = original.row.home === homes.go ? 'go' : 'rust'; assert.equal(original.row.binary, binaries[engine].binary); assert.equal(original.row.cwd, binaries[engine].cwd); }
      assert.equal(report.machine.hostname, remaining.prior.machine.hostname); assert.equal(report.machine.platform, remaining.prior.machine.platform); assert.equal(report.machine.powerBefore, remaining.prior.machine.powerBefore);
      cold = structuredClone(remaining.cold); coldEvidence = remaining.paths.custody; report.scenarios.push(cold, structuredClone(remaining.warm));
      report.continuation = {schema: config.resumeGo ? 'lycheedev.source-pack2-seven-continuation.v1' : 'lycheedev.source-pack2-remaining-continuation.v1', reference: config.resumeGo ?? config.resumeRemaining, originalCallsReused: config.resumeGo ? 5 : 4, freshColdCalls: 0, freshWarmCalls: 0, ...(config.resumeGo ? {freshSingleGoCalls: 0, retainedRun: RETAINED_GO.run} : {}), priorOuter: config.resumeGo ? RETAINED_GO.outer : RETAINED_WARM.outer, priorState: 'failed', priorPendingScenario: remaining.pending, originalConditions: remaining.prior.conditions, originalContinuation: remaining.prior.continuation};
      const goFactsBytes = remaining.retainedGo?.homeIdentity.rows.filter(r => r.path.startsWith('source/v1/facts/files/')).reduce((n, r) => n + r.bytes, 0) ?? 0;
      baseOwner = svc.beginScratch(request.ownerRoot, config.resumeGo ? 'source-pack2-seven-calls' : 'source-pack2-remaining-cases', {command: [config.resumeGo ? 'remaining-seven-calls' : 'remaining-four-changed-cases'], inputs: {...inputs, ...(config.resumeGo ? {retainedGo: config.resumeGo} : {retainedWarm: config.resumeRemaining}), readOnlyHomes: homes, allowance: config.allowance ?? null}, limits: diskPolicy, admissionOptions: {scratchReserveBytes: 1 << 20, requiredBytes: config.resumeGo ? INDEX_BYTES + goFactsBytes + (64 << 20) : 16 << 20}});
      report.owners.push({role: 'read-only-healthy-base-reference', receipt: baseOwner.receipt}); saveJSON(join(baseOwner.evidence, 'prior-completed-cases.json'), {cold: remaining.cold, warm: remaining.warm, reference: config.resumeGo ?? config.resumeRemaining, priorPendingScenario: remaining.pending}); remaining.verifyHomes(); remaining.verifyEvidence(); save();
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
        cold.facts = await svc.compareFacts(homes, envelopes, prepared.pin, oracle, coldEvidence, request.ownerRoot, undefined, {admit: svc.admission, limits: diskPolicy, candidate: request.candidate, layout}); cold.factsEquivalent = true;
        saveJSON(join(coldEvidence, 'cold-facts-custody.json'), {schema: 'lycheedev.source-pack2-cold-facts.v2', go: cold.facts.goCustody, rust: cold.facts.rustCustody});
        retained.verifyHomes(); retained.verifyEvidence(); identities(); cold.state = 'verified'; cold.rustToGoRatio = cold.rust.controllableMillis / cold.go.controllableMillis;
        saveJSON(join(coldEvidence, 'cold-revalidated.json'), cold); complete = true;
      } finally { custody.seal({state: fatal ? 'interrupted' : complete ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: evidenceFiles(custody.evidence)}); if (complete && !fatal) custody.cleanup(); save(); }
      const warmGrowth = Math.max(cold.facts.indexes.go.bytes, cold.facts.indexes.rust.bytes) + (8 << 20);
      const warmFacts = Object.entries(retained.inventories).reduce((sum, [engine, inv]) => sum + inv.rows.filter(row => row.path.startsWith(engine === 'go' ? 'source/v1/facts/files/' : 'cache/source-facts-rust/')).reduce((n, row) => n + row.bytes, 0), 0);
      const warmEstimate = {scratchReserveBytes: warmGrowth, requiredBytes: warmGrowth + 2 * INDEX_BYTES + warmFacts + (128 << 20), basis: 'Borrow exact existing homes; sequential warm calls reserve the larger measured index for temporary replacement plus 8 MiB metadata/log growth; two complete evidence indexes, actual unchanged Go/Rust Facts and 128 MiB bounded report/catalog allowance'};
      baseOwner = svc.beginScratch(request.ownerRoot, 'source-pack2-cold-continuation', {command: ['source', 'index', 'warm-and-four-changes'], inputs: {...inputs, retainedCold: config.resumeCold, borrowedHomes: homes, beforeInventory: config.resumeCold.homeInventorySHA256, coldCustodyReceipt: custody.receipt}, limits: diskPolicy, admissionOptions: warmEstimate});
      report.owners.push({role: 'borrowed-healthy-base', receipt: baseOwner.receipt, estimate: warmEstimate});
      retained.verifyHomes(); retained.verifyEvidence(); borrowClaim = svc.claimColdBorrow(request.ownerRoot, config.resumeCold, baseOwner, retained.inventories, custody.receipt); report.continuation.borrow = borrowClaim; save();
    } else {
      const coldEstimate = svc.footprint({cold: true}); report.coldAdmissionEstimate = coldEstimate;
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
      if (remaining?.retainedGo && index === 2) {
        const saved = remaining.retainedGo; assert.equal(saved.row.go.binary, binaries.go.binary); assert.equal(saved.row.go.cwd, binaries.go.cwd);
        row.go = structuredClone(saved.row.go); row.pin = saved.variant.pin; row.lineage.retainedRun = RETAINED_GO.run; row.lineage.priorFailure = saved.row.failure;
        outputs.go = saved.output.envelope; expectedVariant = saved.variant; remaining.verifyEvidence(); remaining.verifyHomes();
        const directory = join(baseOwner.evidence, 'retained-go'); mkdirSync(directory);
        const index = svc.indexEvidence(row.go.home, 'go', row.pin, directory, request.ownerRoot), reference = await svc.captureGoCustody(row.go.home, index, directory, request.ownerRoot);
        goRetained = {index, reference}; row.go.custody = reference; row.go.comparisonState = 'awaiting-independent-rust'; saveJSON(join(directory, 'revalidated-go.json'), {original: saved.row.go, priorOwner: RETAINED_GO.job, priorState: 'failed', custody: reference}); remaining.verifyEvidence(); remaining.verifyHomes(); save();
      }
      for (const engine of row.order.filter(engine => !(remaining?.retainedGo && index === 2 && engine === 'go'))) {
        identities();
        for (const side of ['go', 'rust']) assert.equal(svc.inventory(homes[side]).sha256, baseIdentity[side].sha256, 'healthy base changed');
        const inventory = baseIdentity[engine], estimate = svc.mutationFootprint(engine, entry.id, inventory, cold.facts.indexes[engine].bytes, prepared.rows, oracle, svc.metadataPageSize(homes[engine], inventory));
        let job;
        try { job = svc.beginScratch(request.ownerRoot, `source-pack2-${entry.id}-${engine}`, {command: ['source', 'index'], inputs: {...inputs, healthyBase: inventory.sha256}, limits: diskPolicy, admissionOptions: estimate}); }
        catch (error) { row.state = row.go || row.rust ? 'incomplete' : 'not_run'; row.blockedBeforeOwner = engine; row.failure = error.message; row.admissionFailure = error.report; save(); throw error; }
        report.owners.push({role: entry.id, engine, receipt: job.receipt, estimate}); save(); let success = false;
        try {
          if (remaining && !remainingClaim) { remaining.verifyEvidence(); remaining.verifyHomes(); remainingClaim = config.resumeGo ? svc.claimSeven(request.ownerRoot, config.resumeGo, baseOwner, remaining) : svc.claimRemaining(request.ownerRoot, config.resumeRemaining, baseOwner, baseIdentity); report.continuation.claim = remainingClaim; save(); }
          const home = join(job.directory, `${engine}-home`); svc.cloneSeed(homes[engine], home, inventory);
          const variant = svc.prepareVariant({homes: {[engine]: home}, directory: job.directory, baseRows: prepared.rows, oracle, id: entry.id, git});
          if (expectedVariant) assert.deepEqual(variant.mutation, expectedVariant.mutation, 'same exact independent mutation/tree/commit required'); else expectedVariant = variant;
          row.pin = variant.pin; saveJSON(join(job.evidence, 'mutation.json'), variant);
          const preparedOwner = svc.inventory(job.directory, {maxBytes: estimate.scratchReserveBytes}); assert(preparedOwner.bytes <= inventory.bytes + estimate.components.gitObjectsAndStage + estimate.components.gitIndexAndLock + estimate.components.projectFiles, 'prepared owner exceeded fixture bound'); saveJSON(join(job.evidence, 'prepared-owner-inventory.json'), preparedOwner);
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
          const measured = svc.inventory(home, {maxBytes: estimate.scratchReserveBytes}), ownerMeasured = svc.inventory(job.directory, {maxBytes: estimate.scratchReserveBytes}); row[engine].actualHomeBytes = measured.bytes; row[engine].actualOwnerBytes = ownerMeasured.bytes; row[engine].ownerInventorySHA256 = ownerMeasured.sha256; saveJSON(join(job.evidence, 'post-owner-inventory.json'), ownerMeasured);
          assert(ownerMeasured.bytes <= estimate.scratchReserveBytes, 'actual scenario footprint exceeded admitted estimate'); row[engine].postAdmission = svc.admission(request.ownerRoot, {limits: diskPolicy});
          if (engine === 'go') {
            const index = svc.indexEvidence(home, engine, variant.pin, job.evidence, request.ownerRoot);
            const reference = await svc.captureGoCustody(home, index, job.evidence, request.ownerRoot); goRetained = {index, reference};
            row.go.custody = reference; row.go.comparisonState = 'awaiting-independent-rust';
          } else {
            row.facts = await svc.compareFacts({go: row.go.home, rust: home}, outputs, variant.pin, variant.oracle, job.evidence, request.ownerRoot, goRetained, {admit: svc.admission, limits: diskPolicy, candidate: request.candidate, layout});
            row.factsEquivalent = true; row.go.comparisonState = 'verified'; row.rustToGoRatio = row.rust.controllableMillis / row.go.controllableMillis;
          }
          identities(); for (const side of ['go', 'rust']) assert.equal(svc.inventory(homes[side]).sha256, baseIdentity[side].sha256, 'original healthy base changed');
          success = true;
        } catch (error) { if (error.handlesReaped === false) fatal = true; if (error.postcardEvidence) row.factsFailure = error.postcardEvidence; row.state = fatal ? 'interrupted' : 'failed'; row.failure = error.message.slice(0, 4096); throw error; }
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
    await svc.verifyAdditionalEvidence?.(report);
    if (retained) { retained.verifyEvidence(); assert.equal(hashFile(borrowClaim.path), borrowClaim.sha256); }
    if (remaining) { remaining.verifyEvidence(); remaining.verifyHomes(); assert.equal(hashFile(remainingClaim.path), remainingClaim.sha256); }
    report.finalIntegrityVerified = true; report.state = 'source_pack_scenarios_observed'; baseSuccess = true;
  } catch (error) { report.blocker = error.message.slice(0, 4096); const active = report.scenarios.at(-1); if (active?.state === 'running') { active.state = active.go || active.rust ? 'incomplete' : 'not_run'; active.failure = report.blocker; } if (error.report) report.admissionFailure = error.report; report.state = 'source_pack_scenarios_failed'; }
  finally {
    const scheduled = config.resumeRemaining || config.resumeGo ? CASES.slice(2) : config.resumeCold ? CASES.slice(1) : CASES;
    report.notRunInvocations = scheduled.flatMap(entry => ['go', 'rust'].filter(engine => !(config.resumeGo && entry.id === 'single-content-change' && engine === 'go') && !report.scenarios.find(row => row.id === entry.id)?.[engine] && !report.invocationAttempts.some(attempt => attempt.caseId === entry.id && attempt.engine === engine)).map(engine => ({caseId: entry.id, engine})));
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
export function verifyObservedReport(run, report, request, inputs, resumeCold = undefined, resumeRemaining = undefined, resumeGo = undefined, {layout = 'v11-pack2'} = {}) {
  assert(['v11-pack2', 'v13-postcard', 'v13-postcard-index18'].includes(layout)); if (layout !== 'v11-pack2') assert(!resumeCold && !resumeRemaining && !resumeGo, 'v13 has no historical continuation');
  assert(run.status === 0 && !run.error && run.handlesReaped); elapsed(run.proof);
  assert.equal(report?.state, 'source_pack_scenarios_observed'); assert.equal(report.finalIntegrityVerified, true); assert.equal(report.requiresOuterRetirement, false);
  assert.equal(report.performanceAcceptanceEvaluated, false); assert.equal(report.fullB01B07Acceptance, false); assert.equal(report.importedSamples, 0);
  assert.deepEqual(report.request, request); assert.deepEqual(report.inputs, inputs); assert.equal(report.layout, layout); assert.equal(report.productCommit, request.candidate.commit);
  assert([resumeCold, resumeRemaining, resumeGo].filter(Boolean).length <= 1);
  if (resumeCold || resumeRemaining || resumeGo) assertHistoricalCandidate(request);
  if (resumeGo) { validateGoReference(resumeGo); assert.deepEqual(report.continuation?.reference, resumeGo); assert.equal(report.continuation.originalCallsReused, 5); assert.equal(report.continuation.freshColdCalls, 0); assert.equal(report.continuation.freshWarmCalls, 0); assert.equal(report.continuation.freshSingleGoCalls, 0); assert.equal(report.continuation.priorOuter, RETAINED_GO.outer); assert.equal(report.scenarios[2].go.proof.runId, RETAINED_GO.run); assert.equal(report.invocationAttempts.length, 7); assert(!report.invocationAttempts.some(row => row.caseId === 'single-content-change' && row.engine === 'go')); }
  else if (resumeRemaining) { validateRemainingReference(resumeRemaining); assert.deepEqual(report.continuation?.reference, resumeRemaining); assert.equal(report.continuation.originalCallsReused, 4); assert.equal(report.continuation.freshColdCalls, 0); assert.equal(report.continuation.freshWarmCalls, 0); assert.equal(report.continuation.priorOuter, RETAINED_WARM.outer); for (const engine of ['go', 'rust']) assert.equal(report.scenarios[0][engine].proof.runId, RETAINED_COLD[engine]); }
  else if (resumeCold) { validateColdReference(resumeCold); assert.deepEqual(report.continuation?.reference, resumeCold); assert.equal(report.continuation.originalCallsReused, 2); assert.equal(report.continuation.freshColdCalls, 0); assert.equal(report.scenarios[0].lineage.resumedOriginalCalls, true); assert.deepEqual(report.scenarios[0].lineage.reference, resumeCold); for (const engine of ['go', 'rust']) assert.equal(report.scenarios[0][engine].proof.runId, RETAINED_COLD[engine]); } else assert.equal(report.continuation, undefined);
  assert.deepEqual(report.scenarios.map(row => row.id), CASES.map(row => row.id));
  const variantHomes = new Set();
  report.scenarios.forEach((row, i) => {
    assert.equal(row.state, 'verified'); assert.equal(row.factsEquivalent, true); assert.deepEqual(row.order, orderFor(i)); assert.equal(row.mode, CASES[i].mode);
    assert.equal(row.facts.documents, CASES[i].documents); assert.equal(row.facts.fullFieldsAndOrderEqual, true);
    if (i > 1 || !(resumeRemaining || resumeGo)) { exact(row.facts.rustCustody, ['directory', 'catalog', 'catalogSHA256', 'candidate']); assert.deepEqual(row.facts.rustCustody.candidate, candidateIdentity(request.candidate)); digest(row.facts.rustCustody.catalogSHA256); assert.equal(row.facts.rustCustody.catalog, join(row.facts.rustCustody.directory, layout !== 'v11-pack2' ? 'postcard-custody.json' : 'rust-custody.json')); exact(row.facts.goCustody, ['directory', 'catalog', 'catalogSHA256']); digest(row.facts.goCustody.catalogSHA256); }
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
