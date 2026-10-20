// Optional old/new Rust CSV diagnostic. Default B04 and numeric-edge cases stay
// unchanged. Fixture production, products and this runner have distinct custody.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readFileSync, readdirSync, realpathSync, writeFileSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve, sep} from 'node:path';
import {performance} from 'node:perf_hooks';
import os from 'node:os';
import {DatabaseSync} from 'node:sqlite';
import {beginScratch, contained, defaults, policy} from '../../tools/rust-disk.mjs';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {inspectEmmy} from '../../tools/rust-emmy-identity.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {collectOwnedMemory} from '../../tools/rust-owned-memory.mjs';
import {elapsed, statistics} from './contract.mjs';
import {cloneSeed, inventory} from './inputs.mjs';
import {PROOF, auditCache, usage} from './data-contract.mjs';
import {blob, bounded, captureCanonical, decodedIdentity, digest, goJSON, loadDataFixture, parse, seedDataHome} from './data-inputs.mjs';
import {CSV_CARGO_LOCK_SHA256, NUMERIC_LAYOUT_SOURCE_SHA256, removedNumericLayoutCost, CSV_BYTES, CSV_COLUMNS, CSV_ROWS, CSV_SHA256, CSV_SQL, expectedNumericCSV, numericRow} from './data-inputs.mjs';
import {fixtureArgs, numericCSVPreparer} from './prepare-data.mjs';

export const CSV_PAIRS = 1;
export const CSV_OUTER_TIMEOUT_MS = 3600000;
export const CSV_COMMITS = Object.freeze({baseline: '63d47f29f6c9efe2511ada5969426a184eca1830', candidate: 'a8604d0dabf6b7ecefa8f15409bda2bf9fbaf8be'});
export const CSV_BASELINE = Object.freeze({binarySHA256: '027baf4725da41b1a085eaf457050912a8f3a96972d334aef51d5a3bb5326e97', sourceTreeSHA256: 'c3129a4b906142c92f4b048af363b501ff98fb3842b4bf80e10794bb4920da6e'});
export const CSV_SIDES = Object.freeze(['baseline', 'candidate']);
export const csvOrder = i => i % 2 ? ['candidate', 'baseline'] : ['baseline', 'candidate'];
export const CSV_RUNNER_FILES = Object.freeze([
  'tools/rust-data-csv-diagnostics.mjs', 'tests/rust-performance-paired/data-csv-diagnostics.mjs',
  'tests/rust-performance-paired/prepare-data.mjs',
  'tests/rust-performance-paired/data-inputs.mjs', 'tests/rust-performance-paired/data-contract.mjs',
  'tests/rust-performance-paired/contract.mjs', 'tests/rust-performance-paired/inputs.mjs',
  'tests/rust-source-full-corpus/assertions.mjs', 'tests/rust-parity/describe-contract.mjs', 'tests/rust-parity/oracle.mjs',
  'tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-emmy-identity.mjs', 'tools/rust-owned-process.mjs',
  'tools/rust-owned-process.ps1', 'tools/rust-owned-memory.mjs', 'tools/rust-build-auxiliary.json', 'tools/zip.mjs', 'release/rust-disk-policy.json',
]);
const keys = (value, names, label) => { assert(value && typeof value === 'object' && !Array.isArray(value), label); assert.deepEqual(Object.keys(value).sort(), [...names].sort(), label); };
const sha = (value, label) => assert.match(value, /^[0-9a-f]{64}$/, label);
export const csvRunnerIdentity = root => Object.fromEntries(CSV_RUNNER_FILES.map(path => [path, hashFile(join(root, path))]));
export function csvEnvironment(environment) {
  assert(!Object.keys(environment).some(key => /^LYCHEEDEV_DISK_/i.test(key)), 'diagnostic disk policy overrides are forbidden');
  for (const [key, value] of Object.entries(environment)) if (['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_DIR', 'RUST_MIN_STACK'].includes(key.toUpperCase())) assert(!value, 'unchanged compiler, target and default stack required');
  return Object.fromEntries(Object.entries(environment).filter(([key]) => !['LYCHEEDEV_SOURCE_PHASE_PROFILE', 'LYCHEEDEV_DATA_PHASE_PROFILE', 'LYCHEEDEV_DATA_CLI_PROFILE'].includes(key.toUpperCase())));
}
export function assertCSVPolicy(root) { assert.deepEqual(policy(root, {}), {...defaults}, 'unchanged original disk policy required'); }
export function validateCSVRequest(value) {
  keys(value, ['schema', 'ownerRoot', 'baseline', 'candidate', 'fixturePreparation', ...(Object.hasOwn(value, 'pairs') ? ['pairs'] : [])], 'CSV configuration');
  assert([1, 3].includes(value.pairs ?? CSV_PAIRS), 'only one or explicit three diagnostic pairs');
  assert.equal(value.schema, 'lycheedev.data-csv-diagnostics-config.v1'); assert(isAbsolute(value.ownerRoot));
  const result = {...value, pairs: value.pairs ?? CSV_PAIRS, ownerRoot: realpathSync(value.ownerRoot)};
  for (const side of CSV_SIDES) {
    const input = value[side]; keys(input, ['sourceRoot', 'artifactRoot', 'artifact', 'commit', 'artifactSHA256', 'binarySHA256', 'sourceTreeSHA256'], side);
    assert.equal(input.commit, CSV_COMMITS[side]);
    for (const field of ['sourceRoot', 'artifactRoot', 'artifact']) assert(isAbsolute(input[field]), `${side}.${field}: absolute path required`);
    for (const field of ['artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']) sha(input[field], field);
    if (side === 'baseline') for (const [field, expected] of Object.entries(CSV_BASELINE)) assert.equal(input[field], expected, 'sealed 63d identity');
    result[side] = {...input, sourceRoot: realpathSync(input.sourceRoot), artifactRoot: realpathSync(input.artifactRoot)};
    result[side].artifact = contained(result[side].artifactRoot, input.artifact);
  }
  assert.notEqual(result.baseline.sourceRoot, result.candidate.sourceRoot, 'independent original product source roots');
  assert.notEqual(result.baseline.binarySHA256, result.candidate.binarySHA256, 'distinct old/new binaries');
  keys(value.fixturePreparation, ['report', 'reportSHA256', 'sourceTreeSHA256'], 'fixture producer');
  assert(isAbsolute(value.fixturePreparation.report)); sha(value.fixturePreparation.reportSHA256); sha(value.fixturePreparation.sourceTreeSHA256);
  return result;
}
export function validateCSVConditions(value) {
  for (const key of ['machine', 'powerAndThermal', 'competingLoad', 'disk', 'operator']) assert(typeof value?.[key] === 'string' && value[key].trim(), `conditions.${key}`);
  return value;
}
export function verifyCSVArtifactRecord(input, artifact) {
  assert(CSV_SIDES.some(side => input.commit === CSV_COMMITS[side]), 'fixed product commit');
  if (input.commit === CSV_COMMITS.baseline) for (const [field, expected] of Object.entries(CSV_BASELINE)) assert.equal(input[field], expected);
  assert(['release', 'symbols'].includes(artifact.profile), 'optimized artifact required'); assert.equal(artifact.sameInvocation, true);
  if (artifact.profile === 'symbols') assert(artifact.pdb, 'matching symbols required');
  assert.equal(artifact.binarySHA256, input.binarySHA256); assert.equal(artifact.inputHashes?.sourceTree, input.sourceTreeSHA256);
  assert(isAbsolute(artifact.sourceRoot));
  assert(Number.isSafeInteger(artifact.bytes) && artifact.bytes > 0);
  assert.deepEqual(artifact.command, ['cargo', 'build', '--locked', '--offline', '-j', '1', '--profile', artifact.profile, '-p', 'lycheedev-cli'], 'fixed existing build recipe required');
}
export function verifyCSVBuildIdentity(input, artifact, tree, emmy, channel) {
  verifyCSVArtifactRecord(input, artifact); assert.equal(tree, input.sourceTreeSHA256, 'product source/artifact mismatch');
  assert.equal(emmy.identity, 'public-upstream-unmodified-v1');
  for (const key of ['identitySHA256', 'recipeSHA256', 'patchedTreeSHA256']) { sha(emmy[key], key); assert.equal(emmy[key], artifact.inputHashes.emmy?.[key], `Emmy ${key} differs from build`); }
  assert.equal(artifact.inputHashes.emmy.schema, emmy.schema); assert.equal(artifact.inputHashes.emmy.historicalPrivatePatchRecovered, false);
  assert.equal(channel.version, '3.5.0'); assert.equal(channel.distTag, 'beta');
}
export function verifyCSVProduct(input, git) {
  assert.equal(hashFile(input.artifact, 65536), input.artifactSHA256, 'artifact metadata changed');
  const artifact = verifyArtifact(input.artifactRoot, input.artifact); verifyCSVArtifactRecord(input, artifact);
  assert.equal(hashFile(join(input.sourceRoot, 'crates/lycheedev-core/src/public_json.rs')), NUMERIC_LAYOUT_SOURCE_SHA256, 'same reviewed NumberLayout source in both products and producer');
  assert.equal(hashFile(join(input.sourceRoot, 'Cargo.lock')), CSV_CARGO_LOCK_SHA256, 'same reviewed numeric dependencies');
  assert.equal(realpathSync(git(['rev-parse', '--show-toplevel'], input.sourceRoot).toString().trim()), input.sourceRoot);
  assert.equal(git(['rev-parse', 'HEAD'], input.sourceRoot).toString().trim(), input.commit, 'product checkout commit');
  assert.equal(git(['status', '--porcelain', '--untracked-files=all'], input.sourceRoot).toString().trim(), '', 'clean original product witness required');
  assert.equal(lstatSync(artifact.sourceBinary).size, artifact.bytes, 'artifact byte size changed');
  verifyCSVBuildIdentity(input, artifact, sourceDigest(input.sourceRoot), inspectEmmy(input.sourceRoot), parse(bounded(join(input.sourceRoot, 'release/npm-channel.json'))));
  return {...input, artifact};
}
function verifySealedLogs(receipt, directory) {
  assert(receipt.handlesReaped === true && ['succeeded', 'retired'].includes(receipt.state), 'successful reaped producer owner required');
  assert.equal(receipt.schema, 'lycheedev.disk-owner.v1'); assert.equal(receipt.activePid, null);
  assert(Array.isArray(receipt.sealedLogs) && receipt.sealedLogs.length > 0 && receipt.sealedLogs.length <= 4096);
  let total = 0; const seen = new Set();
  for (const log of receipt.sealedLogs) {
    const path = contained(directory, log.path); assert(!seen.has(path), 'duplicate fixture evidence'); seen.add(path);
    const bytes = bounded(path, 16 << 20); assert((total += bytes.length) <= 128 << 20, 'fixture evidence byte budget');
    assert.equal(digest(bytes), log.sha256, 'sealed fixture evidence changed');
  }
}
// The optional loader seam is used only by provenance fault tests; public
// configuration cannot supply it and production always audits actual bytes.
export function loadCSVPreparation(input, loadFixture = loadDataFixture) {
  const raw = bounded(input.report, 4 << 20); assert.equal(digest(raw), input.reportSHA256, 'fixture preparation identity');
  const report = parse(raw); assert.equal(report.schema, 'lycheedev.paired-data-preparation.v1'); assert.equal(report.state, 'prepared_synthetic_inputs');
  assert.equal(report.handlesReaped, true); assert.equal(report.inputs.sourceTree, input.sourceTreeSHA256, 'fixture producer source identity');
  assert.deepEqual(report.inputs.numericLayout, {sourceSHA256: NUMERIC_LAYOUT_SOURCE_SHA256, cargoLockSHA256: CSV_CARGO_LOCK_SHA256}, 'reviewed producer numeric ABI source');
  const receiptPath = contained(dirname(input.report), report.owner), receipt = parse(bounded(receiptPath, 4 << 20));
  verifySealedLogs(receipt, dirname(receiptPath)); assert.equal(receipt.inputHashes.sourceTree, input.sourceTreeSHA256);
  assert.equal(receipt.owner, 'paired-data-fixtures'); assert.equal(receipt.retention, 'evidence'); assert.equal(receipt.directory, report.directory); assert.deepEqual(receipt.inputHashes, report.inputs);
  assert(receipt.sealedLogs.some(log => resolve(log.path) === resolve(input.report) && log.sha256 === input.reportSHA256), 'producer owner must seal exact report');
  assert.equal(report.calls.length, 1); const call = report.calls[0];
  assert.equal(call.kind, 'csv-numeric'); assert.deepEqual(call.args, fixtureArgs(numericCSVPreparer));
  assert(call.launched && call.process?.handlesReaped && call.process.exit === 0 && !call.process.error); 
  const proof = call.process.proof; assert.equal(proof?.schema, 'lycheedev.owned-process.v1');
  for (const field of ['rootReaped', 'treeReaped', 'handlesClosed', 'toolIdentityLocksClosed', 'outputReadersComplete']) assert.equal(proof[field], true);
  assert.equal(proof.activeProcesses, 0); assert(!proof.error && !proof.deadlineHit && !proof.outputLimit);
  assert(!proof.forced || proof.knownAuxiliaryCleanup === true, 'unexpected forced fixture termination');
  assert.equal(call.invocation?.inputHashes?.sourceTree, input.sourceTreeSHA256);
  assert.equal(hashFile(call.buildEvidence, 4 << 20), call.cargoReceiptSHA256); const cargo = parse(bounded(call.buildEvidence, 4 << 20));
  assert.deepEqual(cargo, call.cargoReceipt); verifySealedLogs(cargo, dirname(call.buildEvidence));
  assert.equal(cargo.owner, 'paired-data-fixture-csv-numeric'); assert.deepEqual(cargo.command, ['cargo', ...fixtureArgs(numericCSVPreparer)]);
  assert.equal(cargo.inputHashes.sourceTree, input.sourceTreeSHA256);
  assert.equal(report.fixtures['csv-numeric'], call.destination);
  const directory = contained(report.directory, call.destination), fixture = loadFixture(directory, 'csv-numeric');
  assert.deepEqual(fixture.receipt, report.fixtureIdentity['csv-numeric'], 'complete original fixture inventory');
  return {fixture, provenance: {report: input.report, reportSHA256: input.reportSHA256, sourceTreeSHA256: input.sourceTreeSHA256, owner: report.owner, ownerSHA256: digest(bounded(report.owner)), cargoReceipt: call.buildEvidence, cargoReceiptSHA256: call.cargoReceiptSHA256, fixture: fixture.receipt}};
}
export function assertCSVOutputPath(directory, homes, output, fresh = true) {
  const path = contained(directory, output); assert.equal(dirname(path), realpathSync(directory), 'CSV output must be direct owned scratch child');
  assert(path.endsWith('.csv')); contained(directory, path + '.manifest.json');
  for (const home of Object.values(homes)) { const suffix = relative(resolve(home), path); assert(suffix === '..' || suffix.startsWith('..' + sep) || isAbsolute(suffix), 'CSV output inside managed home'); }
  if (fresh) { assert(!existsSync(path), 'fresh CSV required'); assert(!existsSync(path + '.manifest.json'), 'fresh sidecar required'); }
  return path;
}
const ordinaryPath = path => path.replace(/^\\\\\?\\/, '');
export function auditCSVQuery(query, fixture, cacheMode) {
  assert.equal(query.complete, true); assert.equal(query.partial, false); assert.deepEqual(query.query, {sql: CSV_SQL});
  assert.deepEqual(query.result.columns, CSV_COLUMNS); assert.equal(query.result.rows.length, CSV_ROWS);
  query.result.rows.forEach((row, i) => assert.deepEqual(row, numericRow(i), `complete numeric row ${i}`));
  assert.equal(query.sources.length, 1); const source = query.sources[0], file = source.file;
  assert.equal(source.complete, true); assert.equal(source.truncated, false); assert.equal(file.contentVerified, true); assert.equal(file.source, 'installation');
  assert.deepEqual(file.pin, fixture.identity.pin); assert.deepEqual(file.verification, PROOF);
  const table = fixture.identity.tables[0], object = fixture.objects[table.sha256], root = fixture.objects[file.root?.sha256]; assert(root && object);
  assert.deepEqual(file.content, {sha256: table.sha256, bytes: table.bytes}); assert.equal(file.entry.fileDataID, 404);
  assert.equal(file.entry.contentKey, object.ckey); assert.equal(file.payloadEncodingKey, object.ekey); assert.equal(file.root.bytes, root.decodedBytes);
  const reuse = auditCache(file.decodedCache, cacheMode, {encodedBytes: root.encodedBytes + object.encodedBytes, decodedBytes: root.decodedBytes + object.decodedBytes});
  usage(query.resources); const memory = query.memoryBudget;
  keys(memory, ['mode', 'ceilingBytes', 'concurrentQueries', 'retainedBytes', 'metadataBytes', 'peakScratchBytes', 'growths'], 'memory budget');
  assert.equal(memory.mode, 'windows_adaptive'); for (const field of Object.keys(memory).filter(k => k !== 'mode')) assert(Number.isSafeInteger(memory[field]) && memory[field] >= 0);
  assert(memory.concurrentQueries >= 1); assert(memory.retainedBytes <= memory.ceilingBytes && memory.retainedBytes <= 512 << 20); assert(memory.metadataBytes <= memory.ceilingBytes && memory.metadataBytes <= 128 << 20);
  assert(query.resources.retainedBytes + memory.peakScratchBytes <= memory.retainedBytes); assert(query.resources.metadataBytes <= memory.metadataBytes);
  keys(query.timings, ['definitionsMs', 'preparationMs', 'executionMs', 'decodedSourceBytes'], 'query timings');
  for (const field of ['definitionsMs', 'preparationMs', 'executionMs']) assert(Number.isFinite(query.timings[field]) && query.timings[field] >= 0);
  assert.equal(query.timings.decodedSourceBytes, table.bytes);
  const accounting = query.resourceAccounting;
  keys(accounting, ['scope', 'strategy', 'wholeQueryAccountingComplete', 'logicalProviderParityComplete', 'used', 'remainingGaps'], 'safety accounting');
  assert.equal(accounting.scope, 'admitted allocation and work across local CASC/Encoding/decoded cache/definitions/WDC/SQL');
  assert.equal(accounting.strategy, 'conservative cumulative admission'); assert.equal(accounting.wholeQueryAccountingComplete, false); assert.equal(accounting.logicalProviderParityComplete, false); usage(accounting.used);
  assert.deepEqual(accounting.remainingGaps, ['physical/effective provider logical counters not audited against Go 3.2', 'SQL plan/order-container transient allocations', 'uninterruptible single codec call']);
  return {reuse, removedLayout: removedNumericLayoutCost(fixture.identity.numericCsvABI), timings: structuredClone(query.timings), safetyUsed: structuredClone(accounting.used), availableMemory: memory.ceilingBytes};
}
export function validateCSVOutput({fixture, envelope, home, output, evidence, label, cacheMode}) {
  assert.equal(envelope.schema, 'lycheedev.result.v1'); assert.equal(envelope.ok, true); assert.equal(envelope.error, null); assert(Array.isArray(envelope.warnings));
  assert.equal(envelope.context.snapshot, fixture.identity.snapshot); assert.equal(envelope.captures.length, 2);
  const captures = envelope.captures, bytes = [], proofs = [], db = new DatabaseSync(join(home, 'state/toolkit.sqlite'), {readOnly: true});
  const archive = (name, raw) => { assert(/^[a-zA-Z0-9_.-]+$/.test(label)); const path = join(evidence, `${label}-${name}`); writeFileSync(path, raw, {flag: 'wx'}); return {path, sha256: digest(raw), bytes: raw.length}; };
  try {
    for (const [index, cap] of captures.entries()) {
      assert.equal(cap.schema, 'lycheedev.capture.v1'); assert.deepEqual({...cap, id: ''}, captureCanonical(cap));
      assert.equal(cap.id, 'CAP-' + digest(goJSON(captureCanonical(cap)))); assert.equal(cap.complete, true); assert.equal(cap.truncated, false); assert(Number.isFinite(Date.parse(cap.capturedAt)));
      assert.equal(cap.provenance.snapshot, fixture.identity.snapshot); assert.deepEqual(cap.dependencies, index === 1 ? [captures[0].id] : undefined);
      const metadata = db.prepare('SELECT key,generation,value FROM documents WHERE key=?').get('capture/' + cap.id); assert(metadata && Number.isSafeInteger(metadata.generation) && metadata.generation > 0);
      assert.deepEqual(parse(Buffer.from(metadata.value)), cap); assert(cap.blob.bytes <= ((index ? 32 : 16) << 20), 'original capture budget'); const raw = blob(home, cap.blob); bytes.push(raw);
      proofs.push({id: cap.id, blob: archive(`cap-${index}.bin`, raw), metadata: archive(`cap-${index}.json`, Buffer.from(JSON.stringify({key: metadata.key, generation: metadata.generation, value: cap}))), verified: true});
    }
  } finally { db.close(); }
  assert.equal(captures[0].mediaType, 'application/json'); assert.equal(captures[0].provenance.kind, 'data-query');
  assert.equal(captures[0].provenance.locator, 'sql:sha256:' + digest(goJSON({sql: CSV_SQL}))); assert.equal(captures[0].provenance.dataBuild, fixture.identity.pin.fullBuild);
  assert.equal(captures[1].mediaType, 'text/csv; charset=utf-8'); assert.equal(captures[1].provenance.kind, 'data-query-csv'); assert.equal(captures[1].provenance.locator, captures[0].id);
  const expected = expectedNumericCSV(); assert(bytes[1].equals(expected), 'actual CSV capture bytes differ');
  const actual = bounded(output, 1 << 20), sidecar = bounded(output + '.manifest.json', 1 << 20); assert(actual.equals(expected), 'published CSV bytes differ');
  const manifest = envelope.result;
  assert.deepEqual(manifest, {schema: 'lycheedev.query-csv.v1', snapshot: fixture.identity.snapshot, output: ordinaryPath(output), sha256: CSV_SHA256, bytes: CSV_BYTES, columns: CSV_COLUMNS, rows: CSV_ROWS, queryCapture: captures[0].id, csvCapture: captures[1].id, complete: true});
  assert.deepEqual(parse(sidecar), manifest, 'actual sidecar differs');
  const query = parse(bytes[0]), observations = auditCSVQuery(query, fixture, cacheMode), source = query.sources[0].file;
  const sourceProofs = [source.root, source.content].map((ref, index) => ({ref, archive: archive(`source-${index}.bin`, blob(home, ref))}));
  const value = structuredClone({envelope, query});
  for (const field of ['definitionsMs', 'preparationMs', 'executionMs']) value.query.timings[field] = '<observed-time>';
  value.query.memoryBudget.ceilingBytes = '<observed-available-memory>';
  for (const field of ['retainedBytes', 'decodeWork']) value.query.resourceAccounting.used[field] = '<audited-removed-numeric-layout-cost>';
  value.envelope.result.output = '<verified-owned-output>'; value.envelope.result.queryCapture = '<query-capture>'; value.envelope.result.csvCapture = '<csv-capture>';
  value.envelope.captures.forEach((cap, index) => {
    cap.id = index ? '<csv-capture>' : '<query-capture>'; cap.capturedAt = '<verified-time>';
    if (index) { cap.provenance.locator = '<query-capture>'; cap.dependencies = ['<query-capture>']; }
    else cap.blob = {sha256: '<fully-compared-query-body>', bytes: '<fully-compared-query-body-bytes>'};
  });
  return {value, observations, proofs, sourceProofs, output: archive('output.csv', actual), sidecar: archive('sidecar.json', sidecar)};
}
export function compareCSVOutputs(baseline, candidate) {
  assert.deepEqual(candidate.value, baseline.value, 'complete CSV envelopes/query CAP semantics differ');
  const old = baseline.observations.safetyUsed, next = candidate.observations.safetyUsed;
  assert.equal(old.decodeWork - next.decodeWork, CSV_ROWS * CSV_COLUMNS.length, 'removed numeric layout work must explain exact counter delta');
  assert.deepEqual(baseline.observations.removedLayout, candidate.observations.removedLayout, 'same authenticated fixture ABI required');
  assert.equal(old.retainedBytes - next.retainedBytes, baseline.observations.removedLayout.retainedBytes, 'exact removed numeric layout retained admission');
  return {equivalent: true, differences: {
    queryTimings: {baseline: baseline.observations.timings, candidate: candidate.observations.timings, qualification: 'executionMs is sampled before public numeric layout and cannot measure this optimization; use full native CLI wall time'},
    removedLayout: baseline.observations.removedLayout,
    safetyUsed: Object.fromEntries(Object.keys(old).map(key => [key, {baseline: old[key], candidate: next[key], delta: next[key] - old[key]}])),
    captureIdentity: 'Each actual CAP ID, signed metadata and blob verified independently. Time and audited query-body counters change query CAP identity, CSV locator/dependency and CSV CAP identity; none is required stable across runs.',
    availableMemory: {baseline: baseline.observations.availableMemory, candidate: candidate.observations.availableMemory},
  }};
}
export function evidenceFiles(directory) { return readdirSync(directory).filter(name => name !== 'receipt.json' && lstatSync(join(directory, name)).isFile()).map(name => join(directory, name)); }
// Recheck actual files and archived signed metadata after both warm commands
// and evidence reads, before the owner may seal and reclaim its homes.
export function recheckCSVRetained(sample) {
  const refs = [...sample.proofs.flatMap(row => [row.blob, row.metadata]), ...sample.sourceProofs.map(row => row.archive), sample.outputProof, sample.sidecarProof];
  for (const ref of refs) { const bytes = bounded(ref.path, 16 << 20); assert.equal(bytes.length, ref.bytes); assert.equal(digest(bytes), ref.sha256, 'archived CSV evidence changed'); }
  assert.equal(hashFile(sample.output, 1 << 20), sample.outputProof.sha256, 'final CSV changed');
  assert.equal(hashFile(sample.output + '.manifest.json', 1 << 20), sample.sidecarProof.sha256, 'final sidecar changed');
  const db = new DatabaseSync(join(sample.home, 'state/toolkit.sqlite'), {readOnly: true});
  try {
    for (const proof of sample.proofs) {
      const metadata = parse(bounded(proof.metadata.path)), row = db.prepare('SELECT generation,value FROM documents WHERE key=?').get(metadata.key);
      assert(row); assert.equal(row.generation, metadata.generation); assert.deepEqual(parse(Buffer.from(row.value)), metadata.value, 'final actual CAP metadata changed');
      assert.equal(digest(blob(sample.home, metadata.value.blob)), proof.blob.sha256);
    }
  } finally { db.close(); }
  for (const source of sample.sourceProofs) blob(sample.home, source.ref);
}
export function verifyCSVObserved(run, report) {
  assert(run.handlesReaped === true && run.status === 0 && !run.error, 'outer CSV controller failed'); elapsed(run.proof);
  assert.equal(report?.state, 'csv_candidates_observed'); assert.equal(report.performanceAcceptanceEvaluated, false); assert.equal(report.fullB04Acceptance, false);
  assert.equal(report.finalIntegrityVerified, true); assert.equal(report.requiresOuterRetirement, false);
  const count = report.request.pairs; assert([1, 3].includes(count)); assert.equal(report.requiredPairs, count);
  assert.equal(report.pairOwners.length, count); assert(report.pairOwners.every(owner => owner.cleanup?.state === 'retired'));
  assert.equal(report.pairs.length, count * 2);
  const homes = new Set();
  for (const [index, row] of report.pairs.entries()) {
    assert.equal(row.iteration, Math.floor(index / 2)); assert.equal(row.cacheMode, index % 2 ? 'warm' : 'cold'); assert.deepEqual(row.order, csvOrder(row.iteration));
    assert.equal(row.resultEquivalent, true); assert.equal(row.pairTerminal, 'verified');
    for (const side of CSV_SIDES) {
      const sample = row[side]; assert(sample.valid && sample.handlesReaped && sample.exit === 0 && !sample.error);
      assert.equal(sample.controllableMillis, elapsed(sample.proof).controllableMillis); assert.equal(sample.reuse?.verified, true);
      assert.equal(sample.evidenceVerify.length, 2); assert(sample.evidenceVerify.every(value => value === true));
      if (row.cacheMode === 'cold') { assert(!homes.has(sample.home)); homes.add(sample.home); }
      else { const cold = report.pairs[index - 1][side]; assert.equal(sample.home, cold.home); assert.equal(sample.binary, cold.binary); }
    }
  }
  assert.equal(homes.size, count * 2); return true;
}
// Injection is confined to test imports. The public entry never deserializes
// callbacks or accepts replacement ownership, identity, validation or budgets.
export function runCSVController(config, overrides = {}) {
  const services = {assertCSVPolicy, beginScratch, ownedSpawnSync, collectOwnedMemory, verifyCSVProduct, csvRunnerIdentity, loadCSVPreparation, cloneSeed, seedDataHome, decodedIdentity, ...overrides};
  const {request, runnerRoot, inputs, scratch, evidence: out, conditions} = config;
  let fatal = false; const started = performance.now();
  const report = {schema: 'lycheedev.data-csv-diagnostics.v1', state: 'running', performanceAcceptanceEvaluated: false, fullB04Acceptance: false, requiredPairs: request.pairs,
    request, runnerRoot, inputs, conditions, identities: {}, fixtureProducer: null, preparation: [], pairs: [], pairOwners: [], finalIntegrityVerified: false,
    machine: {hostname: os.hostname(), platform: process.platform, arch: process.arch, release: os.release(), cpus: os.cpus(), totalMemory: os.totalmem(), node: process.version, nodeSHA256: hashFile(process.execPath)},
    timing: 'Full native CreateProcess-to-owned-tree-reaped-and-output-drained wall time. CSV bytes, both CAPs, sidecar, validation and cleanup in product remain included. No phase or startup subtraction. Independent fixture/audit/evidence-verify CLI time is recorded separately.',
    qualification: 'One default or three explicitly requested independent cold/warm old/new Rust diagnostic pairs only; not Go B04 acceptance or proof of the <=0.30 target. executionMs precedes removed public numeric layout and cannot measure its cost. OS caches are not flushed. Memory samples retain their gaps.',
    startedAt: new Date().toISOString()};
  const save = () => writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  const environment = csvEnvironment(process.env);
  const gitEnvironment = Object.fromEntries(Object.entries(environment).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE'));
  Object.assign(gitEnvironment, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: 'NUL', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1'});
  function controlled(binary, args, cwd, evidence, measured = false, env = environment) {
    assert(!fatal, 'unverified prior process retirement'); fatal = true; const began = performance.now();
    const run = services.ownedSpawnSync(binary, args, {cwd, env, evidence, timeout: 330000, orphanMs: 30000, memoryObservation: measured});
    fatal = run.handlesReaped !== true;
    const row = {binary, args, cwd, wrapperWallMillis: performance.now() - began, exit: run.status, error: run.error?.message, handlesReaped: run.handlesReaped, proof: run.proof, logs: run.logs,
      stdoutSHA256: digest(run.stdout), stderrSHA256: digest(run.stderr), valid: false};
    return {run, row};
  }
  function prepare(binary, args, cwd, label, evidence = out, env = environment) {
    const {run, row} = controlled(binary, args, cwd, evidence, false, env); report.preparation.push({...row, label}); save();
    assert(run.handlesReaped && !run.error && run.status === 0, `${label}: preparation/evidence call failed`); return run.stdout;
  }
  const git = (args, cwd) => prepare('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', ...args], cwd, 'product Git identity', out, gitEnvironment);
  function identities() {
    services.assertCSVPolicy(request.ownerRoot);
    assert.deepEqual(services.csvRunnerIdentity(runnerRoot), inputs, 'executing runner changed');
    return Object.fromEntries(CSV_SIDES.map(side => [side, services.verifyCSVProduct(request[side], git)]));
  }
  save();
  try {
    const products = identities(), prepared = services.loadCSVPreparation(request.fixturePreparation), fixture = prepared.fixture;
    report.identities = products; report.fixtureProducer = prepared.provenance;
    report.machine.powerBefore = prepare('powercfg.exe', ['/getactivescheme'], request.ownerRoot, 'power before').toString().trim();
    const seeds = {};
    for (const side of CSV_SIDES) {
      const home = contained(scratch, join(scratch, `${side}-seed`)); assert(!existsSync(home));
      const init = parse(prepare(products[side].artifact.sourceBinary, ['init', '--home', home, '--format', 'json'], products[side].sourceRoot, `${side} init`));
      assert.equal(init.schema, 'lycheedev.result.v1'); assert.equal(init.ok, true); assert.equal(init.error, null);
      seeds[side] = {home, identity: services.seedDataHome(fixture, home)}; assert.deepEqual(services.decodedIdentity(home), []);
    }
    for (let iteration = 0; iteration < request.pairs; iteration++) {
      identities(); assert.deepEqual(services.loadCSVPreparation(request.fixturePreparation).provenance, prepared.provenance, 'fixture producer changed');
      const pair = services.beginScratch(request.ownerRoot, `paired-data-csv-${iteration}`, {command: ['data', 'sql', '--sql', CSV_SQL, '--encoding', 'csv'],
        inputs: {baseline: request.baseline.binarySHA256, candidate: request.candidate.binarySHA256, fixture: fixture.receipt.inventory.sha256}, admissionOptions: {requiredBytes: 256 << 20, scratchReserveBytes: 256 << 20}});
      const owner = {iteration, receipt: pair.receipt}; report.pairOwners.push(owner); const rows = []; let valid = false;
      try {
        const homes = Object.fromEntries(CSV_SIDES.map(side => [side, join(pair.directory, side + '-home')]));
        for (const side of CSV_SIDES) { services.cloneSeed(seeds[side].home, homes[side], seeds[side].identity); assert.deepEqual(services.decodedIdentity(homes[side]), []); }
        for (const cacheMode of ['cold', 'warm']) {
          const row = {iteration, cacheMode, order: csvOrder(iteration), resultEquivalent: false, pairTerminal: 'running'}; rows.push(row); report.pairs.push(row); save();
          const outputs = {}, before = {}, audited = {};
          for (const side of CSV_SIDES) {
            before[side] = services.decodedIdentity(homes[side]); assert.equal(before[side].length, cacheMode === 'cold' ? 0 : 2);
            assertCSVOutputPath(pair.directory, homes, join(pair.directory, `${side}-${cacheMode}.csv`));
          }
          // Keep measured commands adjacent; no complete audit or verify CLI is
          // inserted between old and new. Audit every actual result afterward.
          for (const side of row.order) {
            const output = join(pair.directory, `${side}-${cacheMode}.csv`), product = products[side];
            const args = ['data', 'sql', '--sql', CSV_SQL, '--encoding', 'csv', '--output', output, '--snapshot', fixture.identity.snapshot, '--installation', fixture.identity.installation, '--home', homes[side], '--format', 'json', '--offline'];
            outputs[side] = controlled(product.artifact.sourceBinary, args, product.sourceRoot, pair.evidence, true);
            row[side] = {...outputs[side].row, home: homes[side], output}; save(); assert(!fatal, 'unverified paired process retirement');
          }
          for (const side of CSV_SIDES) {
            const {run} = outputs[side], sample = row[side];
            sample.memory = services.collectOwnedMemory(run, {evidence: pair.evidence, sourceBinary: sample.binary, runState: run.status === 0 && !run.error ? 'passed_invocation' : 'failed_invocation'});
            Object.assign(sample, elapsed(run.proof)); assert(run.handlesReaped && !run.error && run.status === 0); assert.equal(run.stderr.toString(), '');
            assertCSVOutputPath(pair.directory, homes, sample.output, false);
            const began = performance.now(), envelope = parse(run.stdout);
            audited[side] = validateCSVOutput({fixture, envelope, home: sample.home, output: sample.output, evidence: pair.evidence, label: `${side}-${cacheMode}`, cacheMode});
            const after = services.decodedIdentity(sample.home); assert.equal(after.length, 2); if (cacheMode === 'warm') assert.deepEqual(after, before[side], 'warm decoded identities changed');
            sample.auditMillis = performance.now() - began; sample.reuse = audited[side].observations.reuse; sample.proofs = audited[side].proofs; sample.sourceProofs = audited[side].sourceProofs;
            sample.outputProof = audited[side].output; sample.sidecarProof = audited[side].sidecar; sample.observations = audited[side].observations; sample.evidenceVerify = [];
            for (const [index, capture] of envelope.captures.entries()) {
              const checked = parse(prepare(sample.binary, ['evidence', 'verify', capture.id, '--home', sample.home, '--format', 'json'], products[side].sourceRoot, `${iteration} ${side} ${cacheMode} actual CAP ${index} verify`, pair.evidence));
              assert.equal(checked.schema, 'lycheedev.result.v1'); assert.equal(checked.ok, true); assert.equal(checked.error, null); assert.deepEqual(checked.result, capture); assert.deepEqual(checked.captures, []); sample.evidenceVerify.push(true);
            }
            sample.valid = true; save();
          }
          row.comparison = compareCSVOutputs(audited.baseline, audited.candidate); row.resultEquivalent = true;
          row.candidateToBaselineRatio = row.candidate.controllableMillis / row.baseline.controllableMillis; save();
        }
        for (const row of rows) for (const side of CSV_SIDES) recheckCSVRetained(row[side]);
        identities(); assert.deepEqual(services.loadCSVPreparation(request.fixturePreparation).provenance, prepared.provenance, 'final fixture integrity');
        valid = rows.length === 2 && rows.every(row => row.resultEquivalent);
      } catch (error) { owner.failure = error.message.slice(0, 4096); }
      finally {
        for (const row of rows) row.pairTerminal = fatal ? 'interrupted' : valid ? 'verified' : 'failed';
        writeFileSync(join(pair.evidence, 'pair.json'), JSON.stringify({iteration, valid, rows, failure: owner.failure}, null, 2) + '\n');
        pair.seal({state: fatal ? 'interrupted' : valid ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: evidenceFiles(pair.evidence)});
        if (valid && !fatal) {
          try { pair.cleanup(); owner.cleanup = {state: 'retired'}; }
          catch (error) { owner.cleanup = {state: 'failed_retained', code: error.code, message: error.message}; save(); throw error; }
        }
        save();
      }
      if (!valid) break;
    }
    assert(!fatal, 'outer retirement required'); identities(); assert.deepEqual(services.loadCSVPreparation(request.fixturePreparation).provenance, prepared.provenance);
    report.machine.powerAfter = prepare('powercfg.exe', ['/getactivescheme'], request.ownerRoot, 'power after').toString().trim(); assert.equal(report.machine.powerAfter, report.machine.powerBefore);
    report.finalIntegrityVerified = true;
    const complete = report.pairs.length === request.pairs * 2 && report.pairs.every(row => row.pairTerminal === 'verified') && report.pairOwners.every(owner => owner.cleanup?.state === 'retired');
    if (complete) report.observation = Object.fromEntries(['cold', 'warm'].map(mode => {
      const rows = report.pairs.filter(row => row.cacheMode === mode), timings = Object.fromEntries(CSV_SIDES.map(side => [side, statistics(rows.map(row => row[side].controllableMillis))]));
      return [mode, {timings, ratioOfMedians: timings.candidate.median / timings.baseline.median, pairedRatios: statistics(rows.map(row => row.candidateToBaselineRatio))}];
    }));
    report.state = complete ? 'csv_candidates_observed' : 'csv_candidates_failed';
  } catch (error) { report.state = 'csv_candidates_failed'; report.blocker = error.message.slice(0, 4096); }
  finally { report.requiresOuterRetirement = fatal; report.wallMillis = performance.now() - started; report.finishedAt = new Date().toISOString(); save(); }
  return report;
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  assert.equal(process.platform, 'win32', 'owned Windows CSV controller required'); assert.equal(process.argv.length, 3);
  const config = parse(bounded(process.argv[2], 4 << 20)); config.request = validateCSVRequest(config.request); validateCSVConditions(config.conditions);
  const report = runCSVController(config); console.log(JSON.stringify({state: report.state, report: join(config.evidence, 'report.json'), observation: report.observation}));
  if (report.state !== 'csv_candidates_observed') process.exitCode = 1;
}
