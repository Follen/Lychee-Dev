// Test-only v13 evidence bridge. Postcard is decoded only by the authenticated
// same-source Rust test executable; this module authenticates bytes and custody.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {basename, dirname, isAbsolute, join, relative, resolve, sep} from 'node:path';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import readline from 'node:readline';
import {openGoCustody} from './source-pack-custody.mjs';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {contained} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {processPassed, processReaped, verifyOwnerEvidence} from '../rust-parity/formal-lua-3.2.3/assertions.mjs';
import {splitIndexEvidence, verifyParts, INDEX_BYTES} from '../../tools/rust-source-pack-evidence.mjs';

export const POSTCARD_LAYOUT = 'v13-postcard';
export const POSTCARD_INDEX_LAYOUT = 'v13-postcard-index18';
export const POSTCARD_PREPARATION_POLICY = 'owned-600s-no-one-shot.v1';
export const INDEX_EXPORT_TEST = 'index::persistence_wire::export_tests::export_binary_index_manifest';
export const INDEX_EXPORT_ARGS = Object.freeze(['--exact', INDEX_EXPORT_TEST, '--ignored', '--nocapture']);
export const POSTCARD_KEY = 'facts-layout-v13-postcard-1.1.3';
export const EXPORT_TEST = 'facts::binary::export_tests::export_binary_facts_manifest';
export const EXPORT_ARGS = Object.freeze(['--exact', EXPORT_TEST, '--ignored', '--nocapture']);
export const EXPORT_BUILD_ARGS = Object.freeze(['test', '--locked', '--offline', '-j', '1', '-p', 'lycheedev-source', '--lib', '--no-run', '--message-format=json-render-diagnostics']);
export const TOOLCHAIN_OWNER = 'source-postcard-exporter-toolchain';
// Reader ceiling only: rust-disk does not impose a whole-receipt writer cap.
export const POSTCARD_OWNER_RECEIPT_BYTES = 16 << 20;
export const TOOLCHAIN_SCOPE = 'Owned rustc -Vv output and observed invoked binary; binary may be a rustup shim, not authentication of every compiler library or a Cargo version observation';
export const LIMITS = Object.freeze({container: 32 << 20, manifest: 8 << 20, total: 128 << 20, custody: 512 << 20, documents: 100000, row: (32 << 20) + (1 << 20)});
const sha = raw => createHash('sha256').update(raw).digest('hex');
const exact = (v, fields, label) => assert.deepEqual(Object.keys(v ?? {}).sort(), [...fields].sort(), label ?? 'exact Postcard evidence fields');
const digest = v => assert.match(v, /^[a-f0-9]{64}$/);
const integer = (v, min, max, label) => assert(Number.isSafeInteger(v) && v >= min && v <= max, label);
const json = raw => JSON.parse(new TextDecoder('utf-8', {fatal: true}).decode(raw));
const save = (file, value) => { const raw = Buffer.from(JSON.stringify(value, null, 2) + '\n'); assert(raw.length <= 16 << 20); fs.writeFileSync(file, raw, {flag: 'wx'}); return {path: file, bytes: raw.length, sha256: sha(raw)}; };
function regular(file, maximum) { const stat = fs.lstatSync(file); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, 'Postcard regular file budget'); return stat; }
export function readPostcardBytes(file, maximum, expected) {
  const before = regular(file, maximum); if (expected) assert.equal(before.size, expected.bytes, 'Postcard file size changed');
  const fd = fs.openSync(file, 'r'), raw = Buffer.alloc(before.size);
  const same = after => { for (const key of ['dev', 'ino', 'size', 'mtimeMs', 'ctimeMs']) assert.equal(after[key], before[key], 'Postcard file changed during read'); };
  try { same(fs.fstatSync(fd)); let at = 0; while (at < raw.length) { const n = fs.readSync(fd, raw, at, Math.min(65536, raw.length - at), null); assert(n > 0, 'Postcard file truncated'); at += n; } assert.equal(fs.readSync(fd, Buffer.alloc(1), 0, 1, null), 0, 'Postcard file grew'); same(fs.fstatSync(fd)); same(regular(file, maximum)); }
  finally { fs.closeSync(fd); }
  if (expected) assert.equal(sha(raw), expected.sha256, 'Postcard file SHA changed'); return raw;
}
function safePath(value) { assert(typeof value === 'string' && value && !isAbsolute(value) && !value.includes('\\') && !value.includes(':') && value.split('/').every(p => p && p !== '.' && p !== '..'), 'Postcard relative path required'); }
export function postcardFactKey(document) { safePath(document.path); digest(document.sha256); return sha(`${POSTCARD_KEY}\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0${document.path}\0${document.sha256}`); }
export function postcardFactPath(home, document) { const group = document.factsLocator?.group; if (group !== undefined) digest(group); return join(home, 'cache/source-facts-rust', `${group ?? postcardFactKey(document)}.json`); }
export function postcardIndexIdentity(summary, layout = POSTCARD_LAYOUT) { assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(layout)); return sha([layout === POSTCARD_INDEX_LAYOUT ? 'rust-index-v18' : 'rust-index-v17', 'assets-v2-go323-metadata', 'facts-pack-v2-path-radix512', POSTCARD_KEY, 'TS0.26.3', 'Lua0.5.0', 'XML0.38', 'generator-v6', ...['repository', 'product', 'commit', 'parser'].map(k => { assert(typeof summary[k] === 'string' && summary[k]); return summary[k]; })].join('\0')); }

export const postcardEnvironmentSHA = environment => sha(JSON.stringify(Object.entries(environment).sort(([a], [b]) => a.localeCompare(b))));
export function postcardCompilerEnvironment(environment) {
  for (const [key, value] of Object.entries(environment)) if (value !== undefined && value !== '')
    assert(!/^(?:RUSTC(?:_WRAPPER|_WORKSPACE_WRAPPER)?|RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_(?:RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|TARGET|RUSTFLAGS))$/i.test(key), `Postcard uncontrolled compiler override: ${key}`);
  return {...environment};
}
export function postcardToolchainPolicy(sourceRoot) {
  const path = join(sourceRoot, 'release/tools/rust.json'), raw = readPostcardBytes(path, 16384), value = json(raw);
  assert.equal(value.schema, 'lycheedev.rust-toolchain.v1'); assert.match(value.rustc, /^\d+\.\d+\.\d+$/);
  assert.equal(value.host, 'x86_64-pc-windows-msvc'); assert.equal(value.target, value.host); assert.equal(value.locked, true);
  return {path, bytes: raw.length, sha256: sha(raw), value};
}
export function postcardCompilerObservation(run, policy) {
  assert(processPassed(run), 'Postcard compiler observation incomplete');
  assert(Buffer.isBuffer(run.stdout) && run.stdout.length > 0 && run.stdout.length < 16384 && Buffer.isBuffer(run.stderr) && run.stderr.length === 0, 'bounded compiler stdout and empty stderr required');
  const versionVerbose = new TextDecoder('utf-8', {fatal: true}).decode(run.stdout);
  const field = name => { const rows = [...versionVerbose.matchAll(new RegExp(`^${name}: ([^\\r\\n]+)\\r?$`, 'gm'))]; assert.equal(rows.length, 1, `exact compiler ${name}`); return rows[0][1]; };
  const release = field('release'), host = field('host'), commit = field('commit-hash');
  assert.equal(release, policy.value.rustc, 'compiler release differs from pinned rust.json'); assert.equal(host, policy.value.host, 'compiler host differs'); assert.match(commit, /^[a-f0-9]{40}$/);
  assert(versionVerbose.startsWith(`rustc ${release} `), 'compiler version heading differs'); assert(isAbsolute(run.proof.binary), 'observed compiler binary path required');
  return {versionVerbose, stdoutBytes: run.stdout.length, stdoutSHA256: sha(run.stdout), release, host, commit,
    observedInvokedBinary: {path: run.proof.binary, bytes: regular(run.proof.binary, 128 << 20).size, sha256: hashFile(run.proof.binary, 128 << 20)}, scope: TOOLCHAIN_SCOPE};
}
export function verifyPostcardToolchain(root, artifact) {
  const ref = artifact.toolchain; exact(ref, ['record', 'recordSHA256', 'receipt', 'receiptSHA256'], 'authenticated exporter compiler observation required');
  digest(ref.recordSHA256); digest(ref.receiptSHA256);
  assert.equal(hashFile(contained(root, ref.record), 65536), ref.recordSHA256, 'compiler observation record changed');
  const owner = verifyOwnerEvidence(root, ref.receipt, TOOLCHAIN_OWNER, {receiptMaximumBytes: POSTCARD_OWNER_RECEIPT_BYTES}); assert.equal(owner.receiptSHA256, ref.receiptSHA256, 'compiler observation owner changed');
  assert.equal(ref.record, join(dirname(ref.receipt), 'toolchain.json')); const record = owner.json('toolchain.json');
  assert.equal(record.schema, 'lycheedev.source-postcard-toolchain.v1'); assert.equal(record.state, 'verified'); assert.equal(record.handlesReaped, true);
  assert.equal(record.sourceRoot, artifact.sourceRoot); assert.equal(record.sourceTreeSHA256, artifact.inputHashes.sourceTree);
  assert.deepEqual(record.policy, postcardToolchainPolicy(artifact.sourceRoot)); digest(record.environmentSHA256);
  assert.deepEqual(record.build, {receipt: artifact.buildEvidence, command: artifact.command, inputHashes: artifact.inputHashes, environmentSHA256: artifact.environmentSHA256, target: artifact.channel});
  assert.deepEqual(owner.receipt.inputHashes, {sourceTree: record.sourceTreeSHA256, toolchainPolicy: record.policy.sha256, environment: record.environmentSHA256});
  assert.deepEqual(owner.receipt.command, ['rustc', '-Vv', 'before-and-after-source-exporter-build']);
  assert.equal(record.observations.length, 2); assert.equal(owner.processes.length, 2);
  const used = new Set();
  for (const [i, row] of record.observations.entries()) {
    assert.equal(row.label, i ? 'after-build' : 'before-build'); assert(!used.has(row.runId)); used.add(row.runId);
    const match = owner.processes.filter(p => p.proof.runId === row.runId); assert.equal(match.length, 1, 'same owned compiler invocation required'); const {request, proof} = match[0];
    assert.equal(request.command, 'rustc'); assert.deepEqual(request.args, ['-Vv']); assert.equal(request.cwd, artifact.sourceRoot); assert.equal(request.cargoBuild, false); assert(request.msvcCompile !== true);
    assert.deepEqual(row.identity, postcardCompilerObservation({status: 0, handlesReaped: true, proof, stdout: owner.logs.get(request.stdout), stderr: owner.logs.get(request.stderr)}, record.policy));
  }
  assert.deepEqual(record.observations[0].identity, record.observations[1].identity, 'compiler changed across exporter build'); return ref;
}

// Only framing is inspected here. No positional schema, varint or Postcard
// decoder exists in Node; Rust's public Facts serialization owns the arrays.
export function verifyPostcardFrame(raw, inputSHA256) {
  integer(raw.length, 76, LIMITS.container, 'Postcard frame budget'); digest(inputSHA256);
  assert.equal(raw.subarray(0, 8).toString(), 'LYCFCT13', 'Postcard frame magic');
  assert.equal(raw.readUInt32LE(8), raw.length - 76, 'Postcard frame payload length');
  assert.equal(raw.subarray(12, 44).toString('hex'), inputSHA256, 'Postcard frame input SHA');
  assert.equal(sha(raw.subarray(76)), raw.subarray(44, 76).toString('hex'), 'Postcard payload SHA');
}
export function selectPostcardExecutable(stdout, target, sourceRoot) {
  assert(typeof stdout === 'string' && Buffer.byteLength(stdout) <= 8 << 20, 'bounded Cargo output required');
  const messages = stdout.trimEnd().split(/\r?\n/).map(line => JSON.parse(line));
  const rows = messages.filter(row => row.reason === 'compiler-artifact' && row.target?.name === 'lycheedev_source' && row.profile?.test === true && row.executable);
  assert.equal(rows.length, 1, 'one Source lib-test executable required'); const row = rows[0];
  assert.deepEqual(row.target.kind, ['lib']); assert.deepEqual(row.target.crate_types, ['lib']);
  assert.equal(resolve(row.target.src_path), join(sourceRoot, 'crates/lycheedev-source/src/lib.rs'));
  assert.equal(resolve(row.manifest_path), join(sourceRoot, 'crates/lycheedev-source/Cargo.toml'));
  assert(isAbsolute(row.executable)); const executable = resolve(row.executable), within = relative(resolve(target), executable);
  // The original rebuildable target may have been retired; the retained binary
  // is verified separately. This checks Cargo's sealed path, not a live target.
  assert(within && !isAbsolute(within) && within !== '..' && !within.startsWith(`..${sep}`), 'exporter executable outside build channel');
  assert.match(basename(executable), /^lycheedev_source-[a-f0-9]+\.exe$/);
  const finished = messages.filter(row => row.reason === 'build-finished'); assert.equal(finished.length, 1); assert.equal(finished[0].success, true);
  return {row, executable};
}
// Use as runCargo's onSuccess callback; this function does not invoke Cargo.
export function sealPostcardExporter(root, sourceRoot, directory, invocation, toolchain, {preparationPolicy} = {}) {
  if (preparationPolicy !== undefined) assert.equal(preparationPolicy, POSTCARD_PREPARATION_POLICY);
  root = resolve(root); sourceRoot = resolve(sourceRoot); directory = contained(root, resolve(directory));
  assert.equal(sourceDigest(sourceRoot), invocation.inputHashes.sourceTree, 'exporter source changed');
  const {row, executable} = selectPostcardExecutable(invocation.stdout, invocation.target, sourceRoot);
  const bytes = regular(executable, 128 << 20).size, binarySHA256 = hashFile(executable, 128 << 20);
  fs.mkdirSync(directory); const sourceBinary = join(directory, 'source-facts-exporter.exe'); fs.copyFileSync(executable, sourceBinary);
  assert.equal(hashFile(sourceBinary, 128 << 20), binarySHA256); assert.equal(hashFile(executable, 128 << 20), binarySHA256);
  const artifact = {schema: 'lycheedev.rust-build-artifact.v1', profile: 'source-lib-test', sourceRoot, sourceBinary, bytes, binarySHA256, sameInvocation: true,
    command: ['cargo', ...EXPORT_BUILD_ARGS], inputHashes: invocation.inputHashes, environmentSHA256: invocation.environmentSHA256, channel: invocation.target, buildEvidence: invocation.buildEvidence, cargoArtifact: row, toolchain,
    ...(preparationPolicy === undefined ? {} : {preparationPolicy})};
  verifyPostcardToolchain(root, artifact);
  save(join(directory, 'artifact.json'), artifact); return artifact;
}
export function verifyPostcardExporter(root, expected, candidate, {requireCurrentPolicy = false, limits} = {}) {
  exact(expected, ['artifact', 'artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']); for (const key of ['artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']) digest(expected[key]);
  assert.equal(hashFile(contained(root, expected.artifact), 65536), expected.artifactSHA256, 'exporter artifact SHA changed');
  const artifact = verifyArtifact(root, expected.artifact); assert.equal(artifact.profile, 'source-lib-test'); assert.equal(artifact.sameInvocation, true);
  if (requireCurrentPolicy || artifact.preparationPolicy !== undefined) assert.equal(artifact.preparationPolicy, POSTCARD_PREPARATION_POLICY, 'current exporter preparation policy required; historical receipt is read-only evidence');
  verifyPostcardToolchain(root, artifact);
  assert.equal(artifact.binarySHA256, expected.binarySHA256, 'exporter binary SHA differs'); assert.equal(regular(artifact.sourceBinary, 128 << 20).size, artifact.bytes);
  assert.equal(artifact.sourceRoot, candidate.sourceRoot, 'exporter source root differs');
  assert.equal(artifact.inputHashes?.sourceTree, candidate.sourceTreeSHA256, 'exporter is not same-source candidate');
  assert.equal(expected.sourceTreeSHA256, candidate.sourceTreeSHA256, 'exporter expected source differs');
  assert.equal(sourceDigest(candidate.sourceRoot), candidate.sourceTreeSHA256, 'exporter source changed');
  assert.deepEqual(artifact.command, ['cargo', ...EXPORT_BUILD_ARGS]);
  const evidence = verifyOwnerEvidence(root, artifact.buildEvidence, 'source-postcard-exporter-build', {receiptMaximumBytes: POSTCARD_OWNER_RECEIPT_BYTES}), command = evidence.json('command.json');
  assert.deepEqual(evidence.receipt.command, artifact.command); assert.equal(command.command, 'cargo'); assert.deepEqual(command.args, EXPORT_BUILD_ARGS);
  assert.equal(command.cwd, candidate.sourceRoot); assert.equal(command.exit, 0); assert(!command.error && !command.signal);
  // Unmarked artifacts retain their original historical receipt contract.
  // Current fresh entry points require the new identity above, so reading old
  // evidence cannot silently qualify it as a current preparation.
  assert.equal(command.oneShot, artifact.preparationPolicy === undefined);
  if (artifact.preparationPolicy !== undefined) assert.equal(command.timeoutMs, 600000, 'current exporter Cargo deadline');
  if (limits !== undefined) {
    assert.deepEqual(evidence.receipt.admission?.limits, limits, 'exporter Cargo admission differs');
    const compilerReceipt = json(readPostcardBytes(artifact.toolchain.receipt, POSTCARD_OWNER_RECEIPT_BYTES));
    assert.deepEqual(compilerReceipt.admission?.limits, limits, 'exporter compiler admission differs');
  }
  assert.deepEqual(command.inputHashes, artifact.inputHashes); assert.deepEqual(evidence.receipt.inputHashes, artifact.inputHashes); assert.equal(command.environmentSHA256, artifact.environmentSHA256);
  assert.equal(evidence.processes.length, 1); const {request} = evidence.processes[0];
  assert.equal(request.command, 'cargo'); assert.deepEqual(request.args, EXPORT_BUILD_ARGS); assert.equal(request.cwd, candidate.sourceRoot); assert.equal(request.cargoBuild, true);
  if (artifact.preparationPolicy !== undefined) assert.equal(request.timeoutMs, 600000, 'current exporter owned deadline');
  const selected = selectPostcardExecutable(evidence.logs.get(join(dirname(artifact.buildEvidence), 'stdout.bin')).toString('utf8'), artifact.channel, candidate.sourceRoot);
  assert.deepEqual(selected.row, artifact.cargoArtifact, 'exporter Cargo artifact differs');
  return {artifact, artifactSHA256: expected.artifactSHA256, buildReceiptSHA256: evidence.receiptSHA256};
}

const prepared = new WeakMap(), exported = new WeakMap();
export function preparePostcardExport(root, home, index, directory, {layout = POSTCARD_LAYOUT, indexExport} = {}) {
  directory = contained(root, directory); home = resolve(home);
  assert.equal(resolve(index.path), join(home, 'cache/objects', index.sha256.slice(0, 2), index.sha256.slice(2)), 'Postcard content-addressed index path');
  assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(layout));
  const binary = layout === POSTCARD_INDEX_LAYOUT;
  if (!binary) assert.equal(indexExport, undefined, 'historical JSON Index rejects binary exporter');
  const parsed = binary ? postcardIndexView(indexExport, index) : json(readPostcardBytes(contained(home, index.path), INDEX_BYTES, index));
  assert.equal(index.manifest?.schema, binary ? 'lycheedev.source-index.rust.v7' : 'lycheedev.source-index.rust.v6'); assert.equal(index.manifest.identity, postcardIndexIdentity(parsed.summary, layout), binary ? 'v18 index recipe required' : 'v17 index recipe required');
  assert.equal(index.manifest.blob.sha256, index.sha256); assert.equal(index.manifest.blob.bytes, index.bytes);
  assert(Array.isArray(parsed.documents)); integer(parsed.documents.length, 1, LIMITS.documents, 'Postcard document budget'); assert.equal(parsed.summary.documents, parsed.documents.length);
  const seen = new Set(), files = new Map(), records = [], sources = []; let total = 0, containerBytes = 0, cachedFile, cachedRaw;
  fs.mkdirSync(directory); fs.mkdirSync(join(directory, 'facts'));
  for (const document of parsed.documents) {
    safePath(document.path); assert(/\.(lua|xml|toc)$/i.test(document.path)); digest(document.sha256); integer(document.bytes, 0, Number.MAX_SAFE_INTEGER, 'document bytes'); assert(!seen.has(document.path), 'duplicate Postcard document'); seen.add(document.path);
    const original = contained(home, postcardFactPath(home, document)); let file = files.get(original);
    if (!file) {
      cachedRaw = undefined; cachedFile = undefined;
      const raw = readPostcardBytes(original, LIMITS.container); containerBytes += raw.length; assert(containerBytes <= LIMITS.custody, 'Postcard custody total budget');
      const relative = `facts/${String(files.size).padStart(5, '0')}.bin`, copy = join(directory, relative); fs.writeFileSync(copy, raw, {flag: 'wx'});
      file = {original, file: relative, path: copy, bytes: raw.length, sha256: sha(raw), ranges: []}; files.set(original, file);
      cachedFile = file; cachedRaw = raw;
    }
    // Read only one bounded container at a time, never a corpus-sized owner.
    if (cachedFile !== file) { cachedRaw = undefined; cachedRaw = readPostcardBytes(file.path, LIMITS.container, file); cachedFile = file; }
    const raw = cachedRaw, locator = document.factsLocator; let offset = 0, bytes = raw.length, recordSHA256 = file.sha256;
    if (Object.hasOwn(document, 'factsLocator')) {
      exact(locator, ['group', 'offset', 'bytes', 'sha256']); digest(locator.group); digest(locator.sha256);
      integer(locator.offset, 8, LIMITS.container, 'Postcard pack offset'); integer(locator.bytes, 76, LIMITS.container, 'Postcard record budget');
      assert(locator.offset + 68 + locator.bytes <= raw.length, 'Postcard record range'); assert.equal(raw.subarray(0, 8).toString(), 'LYCFPK01', 'Postcard pack magic');
      const header = raw.subarray(locator.offset, locator.offset + 68); assert.equal(header.readUInt32LE(0), locator.bytes); assert.equal(header.subarray(4, 36).toString('hex'), postcardFactKey(document), 'Postcard pack record key'); assert.equal(header.subarray(36).toString('hex'), locator.sha256, 'Postcard pack record SHA');
      offset = locator.offset + 68; bytes = locator.bytes; recordSHA256 = locator.sha256;
      file.ranges.push([locator.offset, offset + bytes]);
    } else assert.equal(file.ranges.length, 0, 'Postcard object kind collision');
    const kind = locator ? 'pack' : 'loose'; if (file.kind) assert.equal(file.kind, kind, 'Postcard object kind collision'); else file.kind = kind;
    const record = raw.subarray(offset, offset + bytes); assert.equal(sha(record), recordSHA256, 'Postcard record SHA changed'); verifyPostcardFrame(record, document.sha256);
    total += bytes; assert(total <= LIMITS.total, 'Postcard input total budget');
    records.push({path: document.path, file: file.file, fileBytes: file.bytes, fileSHA256: file.sha256, recordOffset: offset, recordBytes: bytes, recordSHA256, inputSHA256: document.sha256});
    sources.push({path: document.path, file: original, bytes, sha256: recordSHA256, ...(locator ? {record: locator} : {})});
  }
  for (const file of files.values()) { let end = 8; for (const [first, last] of file.ranges.sort((a, b) => a[0] - b[0])) { assert(first >= end, 'overlapping Postcard records'); end = last; } }
  const manifest = {schema: 'lycheedev.source-facts-export-input.v1', records}, raw = Buffer.from(JSON.stringify(manifest)); assert(raw.length <= LIMITS.manifest, 'Postcard manifest budget');
  const manifestPath = join(directory, 'manifest.json'); fs.writeFileSync(manifestPath, raw, {flag: 'wx'});
  const value = Object.freeze({manifest: manifestPath, manifestBytes: raw.length, manifestSHA256: sha(raw), records: records.length, recordBytes: total, containerBytes});
  prepared.set(value, {root, home, layout, indexExport, index: structuredClone(index), directory, manifest, files: [...files.values()], sources, documents: parsed.documents}); return value;
}
export function verifyPreparedPostcard(value) {
  const state = prepared.get(value); assert(state, 'authenticated Postcard preparation required');
  assert.equal(hashFile(value.manifest, LIMITS.manifest), value.manifestSHA256, 'Postcard manifest changed'); assert.equal(regular(value.manifest, LIMITS.manifest).size, value.manifestBytes);
  assert.equal(hashFile(state.index.path, INDEX_BYTES), state.index.sha256, 'Postcard index changed');
  if (state.indexExport) postcardIndexView(state.indexExport, state.index);
  for (const file of state.files) for (const path of [file.original, file.path]) { assert.equal(regular(path, LIMITS.container).size, file.bytes); assert.equal(hashFile(path, LIMITS.container), file.sha256, 'Postcard pack changed'); }
  return state;
}
export function postcardExportSummary(run, manifest) {
  assert(processPassed(run), 'Postcard exporter incomplete, cancelled, limited or unreaped');
  assert(Buffer.isBuffer(run.stdout) && run.stdout.length <= 65536 && Buffer.isBuffer(run.stderr) && run.stderr.length === 0, 'Postcard exporter bounded stdout and empty stderr required');
  const text = new TextDecoder('utf-8', {fatal: true}).decode(run.stdout);
  assert(text.includes(EXPORT_TEST) && /test result: ok\. 1 passed; 0 failed; 0 ignored;/.test(text), 'exact exporter test completion required');
  const lines = text.split(/\r?\n/).filter(line => line.includes('{')); assert.equal(lines.length, 1, 'one exporter summary required');
  const value = JSON.parse(lines[0].slice(lines[0].indexOf('{')));
  exact(value, ['schema', 'codec', 'layout', 'manifestSHA256', 'manifestBytes', 'records', 'recordBytes', 'outputBytes', 'outputSHA256']);
  assert.equal(value.schema, 'lycheedev.source-facts-export-summary.v1'); assert.equal(value.codec, 'postcard-1.1.3'); assert.equal(value.layout, 'LYCFCT13');
  for (const field of ['manifestSHA256', 'manifestBytes', 'records', 'recordBytes']) assert.equal(value[field], manifest[field], `exporter summary ${field}`);
  integer(value.outputBytes, 1, LIMITS.total, 'Postcard output total budget'); digest(value.outputSHA256); return value;
}
export function runPostcardExport(root, expected, candidate, preparation, output, evidence, {execute = ownedSpawnSync} = {}) {
  verifyPreparedPostcard(preparation); const identity = verifyPostcardExporter(root, expected, candidate);
  output = contained(root, output); assert(!fs.existsSync(output), 'fresh Postcard output required');
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('LYCHEEDEV_SOURCE_FACTS_EXPORT_')));
  Object.assign(env, {LYCHEEDEV_SOURCE_FACTS_EXPORT_INPUT: preparation.manifest, LYCHEEDEV_SOURCE_FACTS_EXPORT_BYTES: String(preparation.manifestBytes), LYCHEEDEV_SOURCE_FACTS_EXPORT_SHA256: preparation.manifestSHA256, LYCHEEDEV_SOURCE_FACTS_EXPORT_OUTPUT: output});
  const started = performance.now();
  const run = execute(identity.artifact.sourceBinary, [...EXPORT_ARGS], {cwd: candidate.sourceRoot, env, evidence, timeout: 600000, cargoBuild: false});
  const timing = Object.freeze({exporterWrapperWallMillis: performance.now() - started, scope: 'Separate test-exporter invocation only; not product elapsed and never subtracted from a product measurement'});
  save(join(evidence, 'export-invocation.json'), {binary: identity.artifact.sourceBinary, args: EXPORT_ARGS, exporter: identity, preparation, output, timing, exit: run.status, error: run.error?.message ?? null, signal: run.signal ?? null, handlesReaped: processReaped(run) === true, proof: run.proof ?? null});
  try {
    const summary = Object.freeze(postcardExportSummary(run, preparation));
    assert(isAbsolute(run.proof.binary), 'observed exporter executable path required');
    assert.equal(resolve(run.proof.binary).toLowerCase(), resolve(identity.artifact.sourceBinary).toLowerCase(), 'observed exporter executable differs');
    assert.equal(hashFile(run.proof.binary, 128 << 20), expected.binarySHA256, 'observed exporter executable SHA differs');
    assert.equal(regular(output, LIMITS.total).size, summary.outputBytes); assert.equal(hashFile(output, LIMITS.total), summary.outputSHA256, 'Postcard output SHA differs');
    verifyPreparedPostcard(preparation); assert.deepEqual(verifyPostcardExporter(root, expected, candidate), identity, 'Postcard exporter changed during invocation');
    save(join(evidence, 'export-summary.json'), summary);
    const token = Object.freeze({summary, output, exporter: identity, timing}); exported.set(token, {preparation, output, summary}); return token;
  } catch (error) { error.handlesReaped = processReaped(run) === true; throw error; }
}

async function* outputLines(file, expected) {
  assert.equal(regular(file, LIMITS.total).size, expected.outputBytes); assert.equal(hashFile(file, LIMITS.total), expected.outputSHA256, 'Postcard output SHA differs');
  const input = fs.createReadStream(file, {highWaterMark: 65536}), hash = createHash('sha256'); let chunks = [], length = 0, total = 0;
  try {
    for await (const chunk of input) {
      total += chunk.length; assert(total <= expected.outputBytes, 'Postcard output grew'); hash.update(chunk); let start = 0;
      for (;;) {
        const end = chunk.indexOf(10, start), part = chunk.subarray(start, end < 0 ? chunk.length : end);
        length += part.length; assert(length <= LIMITS.row, 'Postcard row budget'); chunks.push(part);
        if (end < 0) break;
        assert(length > 0, 'empty Postcard output row'); yield json(Buffer.concat(chunks, length)); chunks = []; length = 0; start = end + 1;
        if (start === chunk.length) break;
      }
    }
    assert.equal(length, 0, 'incomplete Postcard output final line'); assert.equal(total, expected.outputBytes); assert.equal(hash.digest('hex'), expected.outputSHA256, 'Postcard output changed while streaming');
    assert.equal(hashFile(file, LIMITS.total), expected.outputSHA256);
  } finally { input.destroy(); }
}
export function openPostcardFacts(token, index, documents) {
  const state = exported.get(token); assert(state, 'authenticated same-source Postcard export required'); const preparedState = verifyPreparedPostcard(state.preparation);
  assert.equal(index.sha256, preparedState.index.sha256); assert.equal(index.bytes, preparedState.index.bytes); assert.deepEqual(documents, preparedState.documents, 'exported index documents differ');
  const iterator = outputLines(state.output, state.summary)[Symbol.asyncIterator](); let ordinal = 0;
  return {
    async next(document) {
      const {value, done} = await iterator.next(); assert(!done, 'missing Postcard export row'); const expected = preparedState.manifest.records[ordinal], source = preparedState.sources[ordinal];
      exact(value, ['schema', 'ordinal', 'path', 'inputSHA256', 'recordSHA256', 'recordBytes', 'facts']); assert.equal(value.schema, 'lycheedev.source-facts-export-record.v1'); assert.equal(value.ordinal, ordinal, 'Postcard export row order');
      assert.equal(value.path, document.path, 'Postcard export document order'); assert.equal(value.inputSHA256, document.sha256); assert.equal(value.recordSHA256, expected.recordSHA256); assert.equal(value.recordBytes, expected.recordBytes);
      exact(value.facts, ['declarations', 'relationships', 'loads', 'headers', 'diagnostics']); for (const rows of Object.values(value.facts)) assert(Array.isArray(rows), 'Postcard Facts arrays required');
      const safeNumbers = value => { if (typeof value === 'number') assert(Number.isSafeInteger(value), 'Postcard JSON coordinate exceeds exact Node integer range'); else if (value && typeof value === 'object') for (const entry of Object.values(value)) safeNumbers(entry); }; safeNumbers(value.facts);
      ordinal++; return {file: source.file, bytes: source.bytes, fileSHA256: source.record ? null : source.sha256, ...(source.record ? {record: source.record, recordSHA256: source.sha256} : {}), facts: value.facts};
    },
    async finish() { assert.equal(ordinal, documents.length); assert.equal((await iterator.next()).done, true, 'extra Postcard export row'); verifyPreparedPostcard(state.preparation); },
    async close() { await iterator.return(); },
  };
}
export function capturePostcardExport(token, directory) {
  const state = exported.get(token); assert(state, 'authenticated Postcard export required'); const p = verifyPreparedPostcard(state.preparation), root = p.root;
  const references = [];
  for (const [i, file] of p.files.entries()) {
    const destination = join(directory, `postcard-fact-${String(i).padStart(5, '0')}`), source = {path: file.original, bytes: file.bytes, sha256: file.sha256};
    const manifest = splitIndexEvidence(root, source, destination); references.push({source, directory: destination, manifest, manifestSHA256: hashFile(join(destination, 'manifest.json'), 65536)});
  }
  const output = {path: state.output, bytes: state.summary.outputBytes, sha256: state.summary.outputSHA256}, outputDirectory = join(directory, 'postcard-output-parts');
  const manifest = splitIndexEvidence(root, output, outputDirectory); references.push({source: output, directory: outputDirectory, manifest, manifestSHA256: hashFile(join(outputDirectory, 'manifest.json'), 65536)});
  const manifestArchive = {path: join(directory, 'postcard-input-manifest.json'), bytes: state.preparation.manifestBytes, sha256: state.preparation.manifestSHA256};
  fs.copyFileSync(state.preparation.manifest, manifestArchive.path, fs.constants.COPYFILE_EXCL);
  assert.equal(hashFile(manifestArchive.path, LIMITS.manifest), manifestArchive.sha256, 'archived exporter input manifest differs');
  const report = {schema: 'lycheedev.source-postcard-custody.v1', layout: p.layout, index: p.index, manifest: state.preparation, manifestArchive, exporter: token.exporter, summary: state.summary, sources: p.sources, references};
  save(join(directory, 'postcard-custody.json'), report);
  verifyPreparedPostcard(state.preparation); verifyPostcardCustody(report); return report;
}
export function verifyPostcardCustody(report) {
  assert.equal(report.schema, 'lycheedev.source-postcard-custody.v1'); assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(report.layout)); let total = 0;
  const manifest = json(readPostcardBytes(report.manifestArchive.path, LIMITS.manifest, report.manifestArchive));
  assert.equal(report.manifestArchive.sha256, report.summary.manifestSHA256); assert.equal(report.manifestArchive.bytes, report.summary.manifestBytes);
  assert.equal(manifest.schema, 'lycheedev.source-facts-export-input.v1'); assert.equal(manifest.records.length, report.summary.records);
  assert.equal(manifest.records.reduce((n, row) => n + row.recordBytes, 0), report.summary.recordBytes);
  assert.equal(report.sources.length, manifest.records.length);
  for (const [i, row] of manifest.records.entries()) { const source = report.sources[i]; assert.equal(source.path, row.path); assert.equal(source.bytes, row.recordBytes); assert.equal(source.sha256, row.recordSHA256); }
  for (const ref of report.references) { assert.equal(hashFile(join(ref.directory, 'manifest.json'), 65536), ref.manifestSHA256); assert.deepEqual(ref.manifest.source, ref.source); verifyParts(ref.directory, ref.manifest); total += ref.source.bytes; }
  assert(total <= LIMITS.custody + LIMITS.total, 'Postcard archived total budget');
  const output = report.references.at(-1).source; assert.equal(output.sha256, report.summary.outputSHA256); assert.equal(output.bytes, report.summary.outputBytes); return {files: report.references.length, bytes: total};
}

// Main Index v18: only the 44-byte envelope is interpreted in Node. The
// authenticated same-source test executable owns every Postcard field.
const indexPrepared = new WeakMap(), indexExported = new WeakMap();
export function verifyPostcardIndexFrame(raw, identity) {
  integer(raw.length, 44, INDEX_BYTES, 'binary Index frame budget'); digest(identity);
  assert.equal(raw.subarray(0, 8).toString(), 'LYCIDX01', 'binary Index magic');
  assert.equal(raw.readUInt32LE(8), raw.length - 44, 'binary Index payload length');
  assert.equal(raw.subarray(12, 44).toString('hex'), identity, 'binary Index identity');
}
export function preparePostcardIndexExport(root, home, index, directory) {
  directory = contained(root, directory); home = resolve(home);
  assert.equal(resolve(index.path), join(home, 'cache/objects', index.sha256.slice(0, 2), index.sha256.slice(2)), 'binary Index content-addressed path');
  assert.equal(index.manifest?.schema, 'lycheedev.source-index.rust.v7');
  assert.deepEqual(index.manifest.blob, {sha256: index.sha256, bytes: index.bytes});
  const raw = readPostcardBytes(contained(home, index.path), INDEX_BYTES, index); verifyPostcardIndexFrame(raw, index.manifest.identity);
  fs.mkdirSync(directory); const file = join(directory, 'index.bin'); fs.writeFileSync(file, raw, {flag: 'wx'});
  const manifest = {schema: 'lycheedev.source-index-export-input.v1', records: [{file: 'index.bin', fileSHA256: index.sha256, fileBytes: index.bytes, identity: index.manifest.identity}]};
  const record = save(join(directory, 'manifest.json'), manifest); assert(record.bytes <= LIMITS.manifest);
  const token = Object.freeze({manifest: record.path, manifestSHA256: record.sha256, manifestBytes: record.bytes, records: 1, recordBytes: index.bytes});
  indexPrepared.set(token, {root, home, index: structuredClone(index), file, directory, manifest}); return token;
}
function verifyPreparedIndex(token) {
  const state = indexPrepared.get(token); assert(state, 'authenticated binary Index preparation required');
  assert.equal(hashFile(token.manifest, LIMITS.manifest), token.manifestSHA256); assert.equal(regular(token.manifest, LIMITS.manifest).size, token.manifestBytes);
  for (const file of [state.file, state.index.path]) { assert.equal(regular(file, INDEX_BYTES).size, state.index.bytes); assert.equal(hashFile(file, INDEX_BYTES), state.index.sha256, 'binary Index changed'); }
  return state;
}
export function postcardIndexExportSummary(run, manifest) {
  assert(processPassed(run), 'Index exporter incomplete, cancelled, limited or unreaped');
  assert(Buffer.isBuffer(run.stdout) && run.stdout.length <= 65536 && Buffer.isBuffer(run.stderr) && run.stderr.length === 0, 'Index exporter bounded stdout and empty stderr required');
  const text = new TextDecoder('utf-8', {fatal: true}).decode(run.stdout);
  assert(text.includes(INDEX_EXPORT_TEST) && /test result: ok\. 1 passed; 0 failed; 0 ignored;/.test(text), 'exact Index exporter completion required');
  const lines = text.split(/\r?\n/).filter(line => line.includes('{')); assert.equal(lines.length, 1, 'one Index exporter summary required');
  const value = JSON.parse(lines[0].slice(lines[0].indexOf('{')));
  exact(value, ['schema', 'codec', 'layout', 'manifestSHA256', 'manifestBytes', 'records', 'recordBytes', 'outputBytes', 'outputSHA256']);
  assert.equal(value.schema, 'lycheedev.source-index-export-summary.v1'); assert.equal(value.codec, 'postcard-1.1.3'); assert.equal(value.layout, 'LYCIDX01');
  for (const field of ['manifestSHA256', 'manifestBytes', 'records', 'recordBytes']) assert.equal(value[field], manifest[field], `Index exporter summary ${field}`);
  integer(value.outputBytes, 1, LIMITS.total, 'Index output budget'); digest(value.outputSHA256); return value;
}
export function runPostcardIndexExport(root, expected, candidate, preparation, output, evidence, {execute = ownedSpawnSync} = {}) {
  const prepared = verifyPreparedIndex(preparation), identity = verifyPostcardExporter(root, expected, candidate);
  output = contained(root, output); assert(!fs.existsSync(output), 'fresh Index output required');
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^LYCHEEDEV_SOURCE_(?:INDEX|FACTS)_EXPORT_/i.test(key)));
  Object.assign(env, {LYCHEEDEV_SOURCE_INDEX_EXPORT_INPUT: preparation.manifest, LYCHEEDEV_SOURCE_INDEX_EXPORT_BYTES: String(preparation.manifestBytes), LYCHEEDEV_SOURCE_INDEX_EXPORT_SHA256: preparation.manifestSHA256, LYCHEEDEV_SOURCE_INDEX_EXPORT_OUTPUT: output});
  const started = performance.now(), run = execute(identity.artifact.sourceBinary, [...INDEX_EXPORT_ARGS], {cwd: candidate.sourceRoot, env, evidence, timeout: 600000, cargoBuild: false});
  const timing = Object.freeze({exporterWrapperWallMillis: performance.now() - started, scope: 'Separate Index test-exporter invocation; outside product elapsed, never subtracted'});
  save(join(evidence, 'index-export-invocation.json'), {binary: identity.artifact.sourceBinary, args: INDEX_EXPORT_ARGS, exporter: identity, preparation, output, timing, exit: run.status, error: run.error?.message ?? null, signal: run.signal ?? null, handlesReaped: processReaped(run) === true, proof: run.proof ?? null});
  try {
    const summary = Object.freeze(postcardIndexExportSummary(run, preparation));
    assert(isAbsolute(run.proof.binary)); assert.equal(resolve(run.proof.binary).toLowerCase(), resolve(identity.artifact.sourceBinary).toLowerCase(), 'observed Index exporter executable differs');
    assert.equal(hashFile(run.proof.binary, INDEX_BYTES), expected.binarySHA256, 'observed Index exporter SHA differs');
    const raw = readPostcardBytes(output, LIMITS.total, {bytes: summary.outputBytes, sha256: summary.outputSHA256});
    assert.equal(raw.at(-1), 10, 'incomplete Index output final line'); assert.equal(raw.indexOf(10), raw.length - 1, 'exactly one Index output row required');
    const row = json(raw); exact(row, ['schema', 'ordinal', 'file', 'fileSHA256', 'fileBytes', 'identity', 'index']);
    assert.equal(row.schema, 'lycheedev.source-index-export-record.v1'); assert.equal(row.ordinal, 0);
    for (const key of ['file', 'fileSHA256', 'fileBytes', 'identity']) assert.equal(row[key], prepared.manifest.records[0][key], `Index row ${key}`);
    const safeNumbers = v => { if (typeof v === 'number') assert(Number.isSafeInteger(v), 'Index JSON coordinate exceeds exact Node integer range'); else if (v && typeof v === 'object') { for (const x of Object.values(v)) safeNumbers(x); Object.freeze(v); } }; safeNumbers(row.index);
    exact(row.index, ['documents', 'assets', 'symbols', 'summary', 'compatibilityGaps']); for (const key of ['documents', 'assets', 'symbols', 'compatibilityGaps']) assert(Array.isArray(row.index[key]));
    assert.equal(postcardIndexIdentity(row.index.summary, POSTCARD_INDEX_LAYOUT), prepared.index.manifest.identity, 'exported Index snapshot identity differs');
    verifyPreparedIndex(preparation); assert.deepEqual(verifyPostcardExporter(root, expected, candidate), identity, 'Index exporter changed');
    save(join(evidence, 'index-export-summary.json'), summary);
    const token = Object.freeze({summary, output, exporter: identity, timing}); indexExported.set(token, {preparation, prepared, index: row.index, output, summary}); return token;
  } catch (error) { error.handlesReaped = processReaped(run) === true; throw error; }
}

// Frozen f811/Go 3.2.3 public-shape oracle, independent of the new Rust codec.
// Go owns symbols, asset metadata and diagnostics. Only Rust-only historical
// fields are mapped here; Facts locators receive separate byte authentication.
export async function comparePostcardIndex(token, reference) {
  const state = indexExported.get(token); assert(state && !state.custody, 'fresh authenticated Index export required');
  verifyPreparedIndex(state.preparation); const index = state.index, go = openGoCustody(reference), summary = json(go.manifestRaw).summary;
  let documents = 0, symbols = 0, assets = 0, metadataAssets = 0, ordinaryAssets = 0, diagnostics = 0, declarations = 0, relationships = 0, bytes = 0;
  // f811 Rust pre-emits metadata-only assets; fixed Go appends them after
  // the normal visit. Preserve each group's original order and duplicates.
  const firstOrdinaryAsset = index.assets.findIndex(asset => asset.local !== null);
  const metadataPrefix = firstOrdinaryAsset < 0 ? index.assets.length : firstOrdinaryAsset;
  const failedPaths = new Set(), gaps = new Set(), sample = [], hash = createHash('sha256');
  const input = go.recordsStream; input.on('data', chunk => { bytes += chunk.length; assert(bytes <= INDEX_BYTES, 'Go Index oracle budget'); hash.update(chunk); });
  try {
    for await (const line of readline.createInterface({input, crlfDelay: Infinity})) {
      assert(line && Buffer.byteLength(line) <= 1 << 20, 'bounded complete Go oracle row required'); const row = JSON.parse(line);
      if (row.kind === 'document') {
        const actual = index.documents[documents++]; assert(actual, 'missing Index document');
        exact(actual, Object.hasOwn(actual, 'factsLocator') ? ['path', 'sha256', 'bytes', 'factsLocator'] : ['path', 'sha256', 'bytes']);
        const {factsLocator, ...value} = actual; assert.deepEqual(value, {path: row.path, sha256: row.sha256, bytes: row.bytes ?? 0}, 'full Index document/order differs from Go');
      } else if (row.kind === 'symbol') {
        assert.equal(row.path, row.symbol.path); assert.deepEqual(index.symbols[symbols++], row.symbol, 'full Index symbol/order/duplicates differ from Go');
        declarations += row.symbol.kind === 'declaration'; relationships += row.symbol.kind === 'relationship';
      } else if (row.kind === 'asset') {
        const source = row.asset; exact(source, ['path', 'contentHash', 'bytes', 'extension', 'mime', 'format', 'width', 'height']);
        assert.equal(row.path, source.path); assert.equal(row.bytes ?? 0, source.bytes);
        const metadataOnly = source.bytes > (64 << 20); assert.equal(source.contentHash === '', metadataOnly, 'Go metadata-only asset identity');
        const ordinal = metadataOnly ? metadataAssets++ : metadataPrefix + ordinaryAssets++; assets++;
        assert.deepEqual(index.assets[ordinal], {...source, local: source.contentHash ? {sha256: source.contentHash, bytes: source.bytes} : null}, 'full Index asset/historical stable order differs from Go and BlobRef');
      } else if (row.kind === 'diagnostic') {
        const d = row.diagnostic; assert.equal(row.path, d.path); diagnostics++; failedPaths.add(d.path); if (sample.length < 20) sample.push(d);
        const p = d.path.replace(/[A-Z]/g, c => c.toLowerCase());
        for (const category of p.endsWith('.lua') ? p.includes('/blizzard_apidocumentationgenerated/') ? ['mixin', 'api', 'event'] : ['mixin'] : p.endsWith('.xml') ? ['template', 'frame-type'] : p.endsWith('.toc') ? ['interface'] : []) gaps.add(category);
      } else assert.fail(`unsupported fixed Go Index row: ${row.kind}`);
    }
    const manifest = json(go.manifestRaw); assert.equal(bytes, manifest.recordBytes); assert.equal(hash.digest('hex'), manifest.recordsHash); go.verify();
    assert.equal(documents, index.documents.length); assert.equal(symbols, index.symbols.length); assert.equal(assets, index.assets.length); assert.equal(metadataAssets, metadataPrefix); assert.equal(ordinaryAssets + metadataPrefix, index.assets.length);
    assert.equal(summary.documents, documents); assert.equal(summary.declarations, declarations); assert.equal(summary.relationships, relationships); assert.equal(summary.assets, assets); assert.equal(summary.diagnostics, diagnostics);
    const expectedSummary = {schema: summary.schema, storage: summary.storage, repository: summary.repository, product: summary.product, commit: summary.commit, parser: summary.parser,
      documents, astFiles: index.documents.filter(d => !failedPaths.has(d.path)).length, skippedDocuments: summary.skippedDocuments ?? 0, declarations, relationships, assets, diagnostics, complete: summary.complete,
      ...(diagnostics ? {diagnosticSample: sample} : {})};
    assert.deepEqual(summary.diagnosticSample ?? [], sample, 'Go summary diagnostic sample differs from full records');
    assert.deepEqual(index.summary, expectedSummary, 'complete Index summary differs from Go/historical fields');
    assert.deepEqual(index.compatibilityGaps, [...gaps].sort(), 'all-diagnostic historical compatibility gaps/order differ');
    state.oracle = {schema: 'lycheedev.source-index-public-comparison.v1', complete: true, go: reference, goRecordsSHA256: manifest.recordsHash, goRecordsBytes: manifest.recordBytes,
      goManifestSHA256: sha(go.manifestRaw), source: {repository: summary.repository, product: summary.product, commit: summary.commit, parser: summary.parser}, documents, assets, symbols, diagnostics,
      fields: ['documents', 'assets', 'symbols', 'summary', 'compatibilityGaps'], arraysOrderedWithDuplicates: true, assetOrder: 'historical-f811-metadata-only-first-stable-partition',
      rustOnlyFields: 'f811 public mappings for local BlobRef, astFiles and compatibilityGaps; factsLocator authenticated against full Facts containers before success'};
    return structuredClone(state.oracle);
  } finally { input.destroy(); }
}
export function postcardIndexView(token, expected) {
  const state = indexExported.get(token); assert(state?.oracle, 'independently compared authenticated binary Index required');
  for (const key of ['path', 'bytes', 'sha256']) assert.equal(expected[key], state.prepared.index[key], `binary Index binding ${key}`);
  assert.equal(hashFile(expected.path, INDEX_BYTES), expected.sha256, 'current binary Index SHA changed');
  if (state.custody) verifyPostcardIndexCustody(state.custody); else { verifyPreparedIndex(state.preparation); assert.equal(hashFile(state.output, LIMITS.total), state.summary.outputSHA256); }
  return state.index;
}
export function capturePostcardIndexExport(token, directory) {
  const state = indexExported.get(token); assert(state?.oracle && !state.custody, 'independently compared Index export required');
  postcardIndexView(token, state.prepared.index);
  const manifestArchive = {path: join(directory, 'postcard-index-input-manifest.json'), bytes: state.preparation.manifestBytes, sha256: state.preparation.manifestSHA256};
  fs.copyFileSync(state.preparation.manifest, manifestArchive.path, fs.constants.COPYFILE_EXCL);
  const source = {path: state.output, bytes: state.summary.outputBytes, sha256: state.summary.outputSHA256}, outputDirectory = join(directory, 'postcard-index-output-parts');
  const manifest = splitIndexEvidence(state.prepared.root, source, outputDirectory);
  const report = {schema: 'lycheedev.source-postcard-index-custody.v1', layout: POSTCARD_INDEX_LAYOUT, index: state.prepared.index, exporter: token.exporter, summary: state.summary, manifestArchive,
    output: {directory: outputDirectory, manifest, manifestSHA256: hashFile(join(outputDirectory, 'manifest.json'), 65536)}, oracle: state.oracle};
  save(join(directory, 'postcard-index-custody.json'), report); verifyPostcardIndexCustody(report); state.custody = report;
  // Release this owner's completed Index staging only after every byte is
  // archived; Facts staging then uses the same unchanged scratch reservation.
  fs.unlinkSync(state.output); fs.unlinkSync(state.prepared.file); fs.unlinkSync(state.preparation.manifest); fs.rmdirSync(state.prepared.directory);
  state.index = Object.freeze({documents: Object.freeze(state.index.documents.map(d => Object.freeze({...d, ...(d.factsLocator ? {factsLocator: Object.freeze({...d.factsLocator})} : {})}))), summary: Object.freeze({...state.index.summary})});
  return report;
}
export function verifyPostcardIndexCustody(report) {
  assert.equal(report.schema, 'lycheedev.source-postcard-index-custody.v1'); assert.equal(report.layout, POSTCARD_INDEX_LAYOUT);
  const manifest = json(readPostcardBytes(report.manifestArchive.path, LIMITS.manifest, report.manifestArchive));
  assert.equal(report.manifestArchive.sha256, report.summary.manifestSHA256); assert.equal(report.manifestArchive.bytes, report.summary.manifestBytes);
  assert.deepEqual(manifest, {schema: 'lycheedev.source-index-export-input.v1', records: [{file: 'index.bin', fileSHA256: report.index.sha256, fileBytes: report.index.bytes, identity: report.index.manifest.identity}]});
  assert.equal(report.index.manifest.schema, 'lycheedev.source-index.rust.v7'); assert.deepEqual(report.index.manifest.blob, {sha256: report.index.sha256, bytes: report.index.bytes});
  assert.equal(report.summary.schema, 'lycheedev.source-index-export-summary.v1');
  assert.equal(report.summary.records, 1); assert.equal(report.summary.recordBytes, report.index.bytes); assert.equal(report.summary.layout, 'LYCIDX01'); assert.equal(report.summary.codec, 'postcard-1.1.3');
  const ref = report.output; assert.equal(hashFile(join(ref.directory, 'manifest.json'), 65536), ref.manifestSHA256); assert.deepEqual(json(readPostcardBytes(join(ref.directory, 'manifest.json'), 65536)), ref.manifest); verifyParts(ref.directory, ref.manifest);
  assert.equal(ref.manifest.source.bytes, report.summary.outputBytes); assert.equal(ref.manifest.source.sha256, report.summary.outputSHA256);
  assert.equal(report.oracle?.complete, true); assert.equal(postcardIndexIdentity(report.oracle.source, POSTCARD_INDEX_LAYOUT), report.index.manifest.identity, 'archived Index source identity differs'); assert.equal(report.oracle.arraysOrderedWithDuplicates, true); assert.equal(report.oracle.assetOrder, 'historical-f811-metadata-only-first-stable-partition'); assert.deepEqual(report.oracle.fields, ['documents', 'assets', 'symbols', 'summary', 'compatibilityGaps']);
  return {bytes: report.summary.outputBytes};
}
export function assertPostcardOutputBudget(indexBytes, factsBytes) {
  integer(indexBytes, 1, LIMITS.total, 'Index output budget'); integer(factsBytes, 1, LIMITS.total, 'Facts output budget');
  assert(indexBytes + factsBytes <= LIMITS.total, 'Index plus Facts JSONL exceed unchanged 128 MiB evidence output budget; raw output retained, no fields omitted');
}
