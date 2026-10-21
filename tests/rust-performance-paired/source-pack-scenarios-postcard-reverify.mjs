// Read-only verification of the complete, sealed fresh run whose final owner
// receipt read alone exceeded the former 1 MiB ceiling. Never resumes a run.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {dirname, join, relative, resolve} from 'node:path';
import {contained, sha256} from '../../tools/rust-disk.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {processPassed, processReaped} from '../rust-parity/formal-lua-3.2.3/assertions.mjs';
import {elapsed, semantic} from './contract.mjs';
import {verifyGoPackage} from './inputs.mjs';
import {validateConditions} from './source-candidates.mjs';
import {verifyPackProfile} from './source-pack-resume.mjs';
import {HISTORICAL_GO_BASELINE, CASES, FRESH_ALLOWANCE, assertDiskPolicy, candidateIdentity, evidenceFiles, verifyCandidateRecord, authenticateRecordedCall, verifyScenarioObservations} from './source-pack-scenarios.mjs';
import {POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT, readPostcardBytes, verifyPostcardExporter} from './read-fact-delta-postcard.mjs';
import {FRESH_OWNER_RECEIPT_BYTES, FRESH_POSTCARD_FILES, freshPostcardIdentity, validatePostcardFreshRequest, verifyFreshPostcardReport} from './source-pack-scenarios-postcard-fresh.mjs';

const exact = (v, keys) => assert.deepEqual(Object.keys(v ?? {}).sort(), [...keys].sort(), 'exact reverify configuration fields required');
const digest = value => assert.match(value, /^[a-f0-9]{64}$/);
export const POSTCARD_REVERIFY_FILES = Object.freeze([...FRESH_POSTCARD_FILES, 'tests/rust-performance-paired/source-pack-scenarios-postcard-reverify.mjs']);
export const postcardReverifyIdentity = root => Object.fromEntries(POSTCARD_REVERIFY_FILES.map(file => [file, hashFile(join(root, file))]));
export function validatePostcardReverifyRequest(value) {
  exact(value, ['schema', 'ownerRoot', 'configuration', 'observation', 'ownedReport', 'outerReceipt', 'originalRunnerSHA256']);
  assert.equal(value.schema, 'lycheedev.source-postcard-reverify-config.v1'); digest(value.originalRunnerSHA256);
  const root = fs.realpathSync(value.ownerRoot);
  for (const key of ['configuration', 'observation', 'ownedReport', 'outerReceipt']) {
    exact(value[key], ['path', 'sha256']); digest(value[key].sha256);
    assert.equal(value[key].path, contained(root, value[key].path));
  }
  const directory = dirname(value.outerReceipt.path);
  for (const [key, name] of [['configuration', 'fresh-configuration.json'], ['observation', 'report.json'], ['ownedReport', 'postcard-owned-report.json'], ['outerReceipt', 'receipt.json']]) assert.equal(value[key].path, join(directory, name));
  return {...structuredClone(value), ownerRoot: root};
}

export function reverifySealedPostcardFailure(value, overrides = {}) {
  const reference = validatePostcardReverifyRequest(value), root = reference.ownerRoot;
  const svc = {verifyCandidateRecord, verifyPostcardExporter, verifyGoPackage, verifyFreshPostcardReport, ...overrides};
  const runnerRoot = resolve(import.meta.dirname, '../..'), verifierInputs = postcardReverifyIdentity(runnerRoot), snapshots = new Map();
  function read(path, expected, maximum = 16 << 20) {
    path = contained(root, path); let raw;
    try { raw = readPostcardBytes(path, maximum); } catch (error) { throw new Error(`sealed read (${maximum} byte ceiling): ${path}: ${error.message}`, {cause: error}); }
    const actual = sha256(raw);
    if (expected !== undefined) assert.equal(actual, expected, `sealed input changed: ${path}`);
    if (snapshots.has(path)) assert.equal(actual, snapshots.get(path).sha256, `sealed input changed during verification: ${path}`);
    snapshots.set(path, {path, bytes: raw.length, sha256: actual}); return JSON.parse(raw);
  }
  const config = read(reference.configuration.path, reference.configuration.sha256, 1 << 20);
  const report = read(reference.observation.path, reference.observation.sha256);
  const owned = read(reference.ownedReport.path, reference.ownedReport.sha256);
  const request = validatePostcardFreshRequest(config.request), layout = request.layout;
  assert.equal(request.ownerRoot, root); assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(layout)); assert.equal(config.layout, layout);
  validateConditions(config.conditions); assert.equal(request.exporter.mode, 'existing');
  if (config.allowance !== undefined) assert.deepEqual(config.allowance, FRESH_ALLOWANCE, 'only explicitly authorized task-local 6 GiB allowance');
  const limits = assertDiskPolicy(root, process.env, config.allowance); assert.deepEqual(report.diskPolicy, limits); assert.equal(report.owners.length, 9);
  exact(config.inputs, FRESH_POSTCARD_FILES); for (const hash of Object.values(config.inputs)) digest(hash);
  assert.equal(sha256(Buffer.from(JSON.stringify(config.inputs))), reference.originalRunnerSHA256, 'original runner identity differs');
  assert.deepEqual(freshPostcardIdentity(config.runnerRoot), config.inputs, 'original runner bytes changed');
  assert.equal(report.schema, 'lycheedev.source-pack-scenarios.v1'); assert.equal(report.state, 'source_pack_scenarios_failed');
  assert.equal(report.blocker, 'Postcard regular file budget', 'only the original final receipt-budget failure is supported');
  assert.equal(report.finalIntegrityVerified, false); assert.equal(report.requiresOuterRetirement, false);
  for (const field of ['performanceAcceptanceEvaluated', 'fullB01B07Acceptance']) assert.equal(report[field], false);
  for (const field of ['cleanupFailure', 'admissionFailure', 'borrowIntegrityFailure', 'remainingIntegrityFailure', 'continuation']) assert.equal(report[field], undefined);
  assert.deepEqual(report.notRunInvocations, []); assert.deepEqual(report.notRunCases, []); assert.equal(report.importedSamples, 0);
  assert.deepEqual(report.request, request); assert.deepEqual(report.inputs, config.inputs); assert.equal(report.runnerRoot, config.runnerRoot); assert.deepEqual(report.conditions, config.conditions);
  assert.equal(report.productCommit, request.candidate.commit); assert.equal(report.layout, layout); assert.equal(report.machine.platform, 'win32');
  assert.equal(report.machine.powerAfter, report.machine.powerBefore);
  assert.equal(owned.schema, 'lycheedev.source-postcard-fresh-owned.v1'); assert.equal(owned.state, 'failed');
  assert.equal(owned.error, 'fresh Postcard controller incomplete'); assert.equal(owned.handlesReaped, true); assert.equal(owned.measuredIndexInvocations, 12);
  assert.equal(owned.exporterInvocations, layout === POSTCARD_INDEX_LAYOUT ? 12 : 6); assert.equal(owned.exporterInvocationsUnknown, undefined);
  assert.equal(owned.performanceAcceptanceEvaluated, false); assert.equal(owned.importedSamples, 0);
  assert.deepEqual(owned.request, request); assert.deepEqual(owned.inputs, config.inputs); assert.deepEqual(owned.allowance, config.allowance ?? null); assert.deepEqual(owned.diskPolicy, report.diskPolicy);
  assert.equal(owned.observation, reference.observation.path); assert.equal(config.evidence, dirname(reference.outerReceipt.path));
  const child = owned.process; assert.equal(child.exit, 1); assert.equal(child.error, null); assert.equal(child.proof.exitCode, 1);
  assert(processReaped(child), 'original failed controller must be fully reaped'); elapsed(child.proof); assert(!child.proof.cancelled && !child.proof.canceled);

  function sealedOwner(path, expectedOwner, states, expectedSHA) {
    const receipt = read(path, expectedSHA, FRESH_OWNER_RECEIPT_BYTES), directory = dirname(path);
    assert.equal(receipt.schema, 'lycheedev.disk-owner.v1'); assert.match(receipt.id, /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/);
    assert.equal(path, contained(root, join(root, '.tmp/rust-disk/evidence', receipt.id, 'receipt.json')));
    assert.equal(receipt.directory, contained(root, join(root, '.tmp/rust-scratch', receipt.id)));
    assert.equal(receipt.owner, expectedOwner); assert.equal(receipt.category, 'scratch'); assert.equal(receipt.retention, 'rebuildable');
    assert(states.includes(receipt.state)); assert.equal(receipt.activePid, null); assert.equal(receipt.handlesReaped, true); assert.equal(typeof receipt.sealedAt, 'string');
    assert.deepEqual(receipt.admission.limits, report.diskPolicy, 'sealed owner admission differs');
    assert.equal(receipt.commandSHA256, sha256(Buffer.from(JSON.stringify(receipt.command))));
    assert(Array.isArray(receipt.sealedLogs) && receipt.sealedLogs.length > 0 && receipt.sealedLogs.length <= report.diskPolicy.maxEntries);
    const known = new Set();
    for (const log of receipt.sealedLogs) {
      const file = contained(directory, log.path); assert.equal(log.original, file); assert(!known.has(file), 'duplicate sealed log'); known.add(file); digest(log.sha256);
      const raw = readPostcardBytes(file, 16 << 20); assert.equal(sha256(raw), log.sha256, `sealed log changed: ${file}`);
      if (snapshots.has(file)) assert.equal(snapshots.get(file).sha256, log.sha256, `sealed input differs: ${file}`);
      snapshots.set(file, {path: file, bytes: raw.length, sha256: log.sha256});
    }
    assert.deepEqual(receipt.logHashes, Object.fromEntries(receipt.sealedLogs.map(log => [relative(root, log.path), log.sha256])));
    assert.deepEqual(evidenceFiles(directory).sort(), [...known].sort(), 'complete sealed evidence required');
    return {receipt, directory, known};
  }
  const outer = sealedOwner(reference.outerReceipt.path, 'source-postcard-fresh-diagnostics', ['failed'], reference.outerReceipt.sha256);
  assert.equal(config.scratch, outer.receipt.directory); assert.deepEqual(outer.receipt.inputHashes.runner, config.inputs); assert.deepEqual(outer.receipt.inputHashes.allowance, config.allowance);
  assert.equal(outer.receipt.command[0], 'source-postcard-fresh-diagnostics'); assert.equal(outer.receipt.command.length, config.allowance ? 4 : 3);
  if (config.allowance) assert.equal(outer.receipt.command[3], '--scratch-6gib');
  const controllerStem = join(outer.directory, `process-${child.proof.runId}`), native = read(controllerStem + '.receipt.json');
  assert.deepEqual(native, child.proof); const wrapper = read(controllerStem + '.request.json.wrapper.json'); assert.equal(wrapper.status, 0); assert(!wrapper.error && !wrapper.signal);
  const controllerRequest = read(controllerStem + '.request.json');
  assert.equal(controllerRequest.schema, 'lycheedev.owned-request.v2'); assert.equal(controllerRequest.timeoutMs, 3600000); assert.equal(controllerRequest.orphanMs, 30000);
  assert.equal(controllerRequest.cargoBuild, false); assert.equal(controllerRequest.msvcCompile, false); assert(!controllerRequest.startupDiagnostics);
  assert.equal(controllerRequest.runId, child.proof.runId); assert.equal(controllerRequest.command, child.proof.binary); assert.equal(controllerRequest.cwd, root);
  assert.deepEqual(controllerRequest.args, [join(config.runnerRoot, 'tests/rust-performance-paired/source-pack-scenarios-postcard-fresh.mjs'), '--inner', reference.configuration.path]);

  for (const [key, suffix] of [['stdout', '.stdout.bin'], ['stderr', '.stderr.bin'], ['receipt', '.receipt.json'], ['identity', '.identity.json'], ['rootIdentity', '.root.json']]) {
    assert.equal(controllerRequest[key], controllerStem + suffix); assert(outer.known.has(controllerRequest[key]), 'original controller logs must be sealed');
  }
  const startup = read(controllerRequest.identity), rootIdentity = read(controllerRequest.rootIdentity);
  assert.equal(startup.schema, 'lycheedev.owned-start.v1'); assert.equal(startup.runId, child.proof.runId);
  assert.equal(startup.requestSHA256, snapshots.get(controllerStem + '.request.json').sha256); assert.equal(startup.wrapperObservedSHA256, config.inputs['tools/rust-owned-process.ps1']);
  assert.equal(startup.rootAssignment, 'PROC_THREAD_ATTRIBUTE_JOB_LIST'); assert.equal(startup.historicalReapProof, false);
  assert.equal(rootIdentity.schema, 'lycheedev.owned-root.v1'); assert.equal(rootIdentity.runId, child.proof.runId); assert.equal(rootIdentity.jobAssignedAtCreation, true); assert.equal(rootIdentity.historicalReapProof, false);
  assert.equal(rootIdentity.pid, child.proof.pid); assert.equal(rootIdentity.creationFileTime, child.proof.creationFileTime);
  assert.equal(child.proof.startupIdentity, controllerRequest.identity); assert.equal(child.proof.rootIdentity, controllerRequest.rootIdentity);

  // This is the unmodified per-scenario gate, without fabricating an observed
  // outer state. Every product row is then bound to its archived native call.
  verifyScenarioObservations(report, request, {layout});
  const owners = report.owners.map((owner, i) => sealedOwner(owner.receipt, i === 0 ? 'source-pack2-healthy-base' : `source-pack2-${owner.role}-${owner.engine}`, i === 0 ? ['failed'] : ['retired']));
  assert.deepEqual(owners[0].receipt.inputHashes, config.inputs);
  for (const owner of owners.slice(1)) for (const [file, hash] of Object.entries(config.inputs)) assert.equal(owner.receipt.inputHashes[file], hash);
  // Changed owners were already retired at the original failing read. The base
  // was still active then and was sealed only afterward, so its terminal size
  // must not be presented as evidence of the former overflow.
  const oversizedRetired = owners.slice(1).findIndex(owner => snapshots.get(join(owner.directory, 'receipt.json')).bytes > 1 << 20);
  assert(oversizedRetired >= 0, 'original final receipt-budget failure must be reproducible on an already-retired owner');
  const oversized = oversizedRetired + 1;
  const candidate = svc.verifyCandidateRecord(request.candidate), {mode, ...exporter} = request.exporter;
  svc.verifyPostcardExporter(root, exporter, request.candidate); const go = svc.verifyGoPackage(request.go.packageRoot, request.go.luaLSArchive, HISTORICAL_GO_BASELINE); assert.equal(go.packageSHA256, request.go.packageSHA256);
  const summaries = [];
  for (const [i, row] of report.scenarios.entries()) {
    const entry = CASES[i], outputs = {};
    for (const engine of ['go', 'rust']) {
      const owner = i < 2 ? owners[0] : owners[1 + (i - 2) * 2 + (engine === 'rust' ? 1 : 0)], item = row[engine];
      assert.equal(item.binary, engine === 'rust' ? candidate.sourceBinary : go.binary); assert.equal(item.cwd, request[engine === 'rust' ? 'candidate' : 'go'].sourceRoot);
      let project;
      if (i > 1) { assert.equal(item.fixture, join(owner.directory, 'mutation.json')); const variant = read(item.fixture); assert.deepEqual(variant.pin, row.pin); project = variant.project; }
      const stem = join(i < 2 ? join(owner.directory, entry.id) : owner.directory, `process-${item.proof.runId}`);
      for (const suffix of ['.request.json', '.request.json.wrapper.json', '.identity.json', '.root.json', '.receipt.json', '.stdout.bin', '.stderr.bin']) assert(owner.known.has(stem + suffix), 'original measured call must be sealed by its scenario owner');
      assert(processPassed({status: item.exit, error: item.error, handlesReaped: item.handlesReaped, proof: item.proof}), 'complete original product invocation required');
      const observed = authenticateRecordedCall(item, item.home, stem, item.proof.runId, row.pin, config.inputs, project);
      outputs[engine] = semantic('B02-retail-index', observed.envelope, engine, row.facts.summary, row.pin.id).value;
      assert.equal(observed.envelope.context.snapshot, row.pin.id); assert.equal(outputs[engine].documents, entry.documents);
      if (engine === 'rust') { assert.deepEqual(verifyPackProfile(observed.stderr, observed.envelope, entry), item.sourceProfile); if (entry.maxParsed) assert(observed.envelope.result.io.parsedDocuments <= entry.maxParsed); }
    }
    assert.deepEqual(outputs.go, outputs.rust); assert.deepEqual(outputs.go, row.facts.summary); assert.equal(row.rustToGoRatio, row.rust.controllableMillis / row.go.controllableMillis);
    sealedOwner(row.facts.postcardOwner.receipt, 'source-postcard-facts', ['retired'], row.facts.postcardOwner.receiptSHA256);
    summaries.push({id: row.id, goMillis: row.go.controllableMillis, rustMillis: row.rust.controllableMillis, rustToGoRatio: row.rustToGoRatio});
  }
  svc.verifyFreshPostcardReport(report, request, config.allowance);
  svc.verifyCandidateRecord(request.candidate); svc.verifyPostcardExporter(root, exporter, request.candidate);
  assert.deepEqual(svc.verifyGoPackage(request.go.packageRoot, request.go.luaLSArchive, HISTORICAL_GO_BASELINE), go);
  assert.deepEqual(freshPostcardIdentity(config.runnerRoot), config.inputs); assert.deepEqual(postcardReverifyIdentity(runnerRoot), verifierInputs);
  for (const row of snapshots.values()) readPostcardBytes(row.path, row.bytes, row);
  return {schema: 'lycheedev.source-postcard-reverification.v1', state: 'sealed_complete_scenarios_reverified', reference,
    original: {state: report.state, finalIntegrityVerified: report.finalIntegrityVerified, blocker: report.blocker, outerState: outer.receipt.state, inputs: config.inputs},
    verifier: {inputs: verifierInputs, ownerReceiptMaximumBytes: FRESH_OWNER_RECEIPT_BYTES}, candidate: candidateIdentity(request.candidate), layout,
    productInvocations: 0, exporterInvocations: 0, importedSamples: 0, performanceAcceptanceEvaluated: false, originalRecordsUnchanged: true,
    authenticatedFiles: snapshots.size, firstOversizedRetiredReceipt: snapshots.get(join(owners[oversized].directory, 'receipt.json')), scenarios: summaries};
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  assert.equal(process.argv.length, 3, 'usage: node source-pack-scenarios-postcard-reverify.mjs REVERIFY.json');
  const value = JSON.parse(readPostcardBytes(process.argv[2], 65536));
  console.log(JSON.stringify(reverifySealedPostcardFailure(value), null, 2));
}
