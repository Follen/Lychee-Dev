import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {syncBuiltinESMExports} from 'node:module';
import childProcess from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {join, dirname, relative, resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {beginScratch, sha256, defaults} from '../../tools/rust-disk.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {elapsed, semantic} from './contract.mjs';
import {PHASE_NAMES, PHASE_TIMING, LOCK_TIMING} from './source-candidates.mjs';
import {verifyPackProfile} from './source-pack-resume.mjs';
import {CASES, candidateIdentity, evidenceFiles} from './source-pack-scenarios.mjs';
import {POSTCARD_LAYOUT} from './read-fact-delta-postcard.mjs';
import {freshPostcardIdentity, FRESH_OWNER_RECEIPT_BYTES} from './source-pack-scenarios-postcard-fresh.mjs';
import {reverifySealedPostcardFailure, validatePostcardReverifyRequest} from './source-pack-scenarios-postcard-reverify.mjs';
const runnerRoot = resolve(import.meta.dirname, '../..');
const put = (path, value) => { fs.mkdirSync(dirname(path), {recursive: true}); fs.writeFileSync(path, typeof value === 'string' ? value : JSON.stringify(value)); };
const load = path => JSON.parse(fs.readFileSync(path));
const nativeProof = (runId, binary, status = 0) => ({schema: 'lycheedev.owned-process.v1', runId, binary, pid: 1234, creationFileTime: '123456789', exitCode: status,
  rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false,
  timing: {schema: 'lycheedev.owned-timing.v1', frequency: 1000000, startTicks: '1000', rootExitTicks: '100900', treeReapedTicks: '101000'}});
function profileFor(entry, summary, pin) {
  const calls = Object.fromEntries(PHASE_NAMES.map(name => [name, 0])); calls.metadata = 1;
  const parsed = entry.mode === 'cold' ? entry.documents : entry.mode === 'warm' ? 0 : 1;
  const io = {cachedFacts: entry.documents - parsed, parsedDocuments: parsed, gitTreeProcesses: entry.mode === 'warm' ? 0 : 1, gitBlobProcesses: entry.mode === 'warm' ? 0 : 1, elapsedMs: 100, deadlineMs: 180000};
  if (entry.mode === 'warm') { calls.cached_index = 1; Object.assign(io, {cachedIndex: true, indexBytesRead: 100, sourceBytesRead: 0}); }
  else { for (const name of ['index_lock', 'git_tree', 'git_blobs_inclusive', 'index_serialize', 'index_publish', 'manifest']) calls[name] = 1;
    Object.assign(calls, {facts_read: entry.documents + Math.ceil(entry.documents / 64), facts_parse: parsed, facts_parse_sequential: parsed, facts_serialize: parsed, facts_publish: parsed, symbols: entry.documents}); io.readBytes = 100; }
  const profile = {schema: 'lycheedev.source-index-phases.v1', timing: PHASE_TIMING, lockTiming: LOCK_TIMING, elapsedMicros: 100000, phases: PHASE_NAMES.map(phase => ({phase, calls: calls[phase], elapsedMicros: calls[phase]}))};
  return {stderr: JSON.stringify(profile) + '\n', envelope: {schema: 'lycheedev.result.v1', ok: true, error: null, context: {snapshot: pin.id}, result: {...summary, io}}};
}
function fixture(t) {
  const root = fs.mkdtempSync(join(tmpdir(), 'postcard-reverify-')); t.after(() => { if (t.passed) fs.rmSync(root, {recursive: true}); else t.diagnostic(`Retained fixture: ${root}`); });
  const inputs = freshPostcardIdentity(runnerRoot), limits = {...defaults};
  for (const path of ['source', 'go', 'artifact', 'exporter']) fs.mkdirSync(join(root, path));
  const request = {schema: 'lycheedev.source-postcard-fresh-config.v1', layout: POSTCARD_LAYOUT, ownerRoot: root,
    candidate: {sourceRoot: join(root, 'source'), artifactRoot: join(root, 'artifact'), artifact: join(root, 'artifact/artifact.json'), commit: '1'.repeat(40), artifactSHA256: 'a'.repeat(64), binarySHA256: 'b'.repeat(64), sourceTreeSHA256: 'c'.repeat(64)},
    go: {sourceRoot: join(root, 'go'), packageRoot: join(root, 'go'), luaLSArchive: join(root, 'luals.zip'), packageSHA256: 'd'.repeat(64)},
    exporter: {mode: 'existing', artifact: join(root, 'exporter/artifact.json'), artifactSHA256: 'e'.repeat(64), binarySHA256: 'f'.repeat(64), sourceTreeSHA256: 'c'.repeat(64)}};
  const outer = beginScratch(root, 'source-postcard-fresh-diagnostics', {command: ['source-postcard-fresh-diagnostics', join(root, 'initial.json'), join(root, 'conditions.json')], inputs: {runner: inputs}, limits});
  const jobs = [{role: 'healthy-base'}, ...CASES.slice(2).flatMap(entry => ['go', 'rust'].map(engine => ({role: entry.id, engine})))].map((row, i) => ({...row,
    job: beginScratch(root, i ? `source-pack2-${row.role}-${row.engine}` : 'source-pack2-healthy-base', {command: ['source', 'index'], inputs: {...inputs, ...(i ? {healthyBase: 'a'.repeat(64)} : {})}, limits})}));
  const conditions = {machine: 'synthetic', powerAndThermal: 'unknown', competingLoad: 'unknown', disk: 'synthetic', operator: 'Node tests'};
  const config = {request, layout: POSTCARD_LAYOUT, conditions, runnerRoot, inputs, scratch: outer.directory, evidence: outer.evidence};
  const path = {configuration: join(outer.evidence, 'fresh-configuration.json'), observation: join(outer.evidence, 'report.json'), ownedReport: join(outer.evidence, 'postcard-owned-report.json'), outerReceipt: outer.receipt};
  const report = {schema: 'lycheedev.source-pack-scenarios.v1', state: 'source_pack_scenarios_failed', blocker: 'Postcard regular file budget', finalIntegrityVerified: false, requiresOuterRetirement: false,
    performanceAcceptanceEvaluated: false, fullB01B07Acceptance: false, importedSamples: 0, notRunInvocations: [], notRunCases: [], request, inputs, runnerRoot, conditions, diskPolicy: limits,
    productCommit: request.candidate.commit, layout: POSTCARD_LAYOUT, machine: {platform: 'win32', powerBefore: 'same', powerAfter: 'same'}, owners: jobs.map(({job, ...row}) => ({...row, receipt: job.receipt})),
    invocationAttempts: CASES.flatMap((entry, i) => (i === 1 ? ['rust', 'go'] : ['go', 'rust']).map(engine => ({caseId: entry.id, engine}))), scenarios: []};
  const binaries = {rust: join(root, 'artifact/rust.exe'), go: join(root, 'go/go.exe')};
  const factsJobs = [];
  CASES.forEach((entry, i) => {
    const pin = {id: 'PIN-' + (i < 2 ? 'base' : entry.id)}, row = {id: entry.id, mode: entry.mode, state: 'verified', factsEquivalent: true, order: i === 1 ? ['rust', 'go'] : ['go', 'rust'], pin};
    const result = {schema: 'lycheedev.source-map.v2', storage: 'file-cache', parser: 'source-v1', repository: 'fixture', product: 'retail', commit: '1'.repeat(40), documents: entry.documents, declarations: 0, relationships: 0, diagnostics: 0, assets: 0, complete: true};
    const observed = profileFor(entry, result, pin), summary = semantic('B02-retail-index', observed.envelope, 'go', result, pin.id).value;
    for (const engine of ['go', 'rust']) {
      const job = (i < 2 ? jobs[0] : jobs[1 + (i - 2) * 2 + (engine === 'rust' ? 1 : 0)]).job, home = join(job.directory, `${engine}-home`), runId = randomUUID(), stem = join(i < 2 ? join(job.evidence, entry.id) : job.evidence, `process-${runId}`);
      const project = i > 1 ? join(job.directory, 'variant-project') : undefined;
      const args = ['source', 'index', ...(project ? ['--project', project] : ['--snapshot', pin.id]), '--home', home, '--format', 'json'];
      const req = {schema: 'lycheedev.owned-request.v2', runId, command: binaries[engine], args, cwd: request[engine === 'rust' ? 'candidate' : 'go'].sourceRoot, timeoutMs: 210000, orphanMs: 30000, cargoBuild: false, msvcCompile: false};
      for (const [key, suffix] of [['stdout', '.stdout.bin'], ['stderr', '.stderr.bin'], ['receipt', '.receipt.json'], ['identity', '.identity.json'], ['rootIdentity', '.root.json'], ['memoryPrefix', '.memory'], ['memoryMarker', '.phase']]) req[key] = stem + suffix;
      put(stem + '.request.json', req); put(stem + '.request.json.wrapper.json', {status: 0});
      const proof = {...nativeProof(runId, binaries[engine]), startupIdentity: req.identity, rootIdentity: req.rootIdentity};
      put(req.receipt, proof); put(req.stdout, JSON.stringify(observed.envelope)); put(req.stderr, engine === 'rust' ? observed.stderr : '');
      put(req.identity, {schema: 'lycheedev.owned-start.v1', runId, requestSHA256: hashFile(stem + '.request.json'), wrapperObservedSHA256: inputs['tools/rust-owned-process.ps1'], rootAssignment: 'PROC_THREAD_ATTRIBUTE_JOB_LIST', historicalReapProof: false});
      put(req.rootIdentity, {schema: 'lycheedev.owned-root.v1', runId, jobAssignedAtCreation: true, historicalReapProof: false, pid: proof.pid, creationFileTime: proof.creationFileTime});
      row[engine] = {binary: binaries[engine], cwd: req.cwd, args, home, proof, exit: 0, valid: true, handlesReaped: true, memory: {state: 'sampled_lower_bound'}, stdoutSHA256: hashFile(req.stdout), stderrSHA256: hashFile(req.stderr), ...elapsed(proof)};
      if (i === 1) row[engine].warmReuse = {verified: true};
      if (project) { row[engine].fixture = join(job.evidence, 'mutation.json'); put(row[engine].fixture, {pin, project}); }
      if (engine === 'rust') row.rust.sourceProfile = verifyPackProfile(Buffer.from(observed.stderr), observed.envelope, entry);
    }
    const factsJob = beginScratch(root, 'source-postcard-facts', {command: ['source-postcard-facts', pin.id], limits}); factsJobs.push(factsJob); put(join(factsJob.evidence, 'synthetic-custody.json'), {fixture: true});
    factsJob.seal({state: 'succeeded', handlesReaped: true, logs: evidenceFiles(factsJob.evidence)}); factsJob.cleanup();
    row.facts = {documents: entry.documents, fullFieldsAndOrderEqual: true, summary, rustCustody: {directory: factsJob.evidence, catalog: join(factsJob.evidence, 'postcard-custody.json'), catalogSHA256: 'a'.repeat(64), candidate: candidateIdentity(request.candidate)},
      goCustody: {directory: jobs[0].job.evidence, catalog: join(jobs[0].job.evidence, 'go-custody.json'), catalogSHA256: 'b'.repeat(64)}, postcardOwner: {receipt: factsJob.receipt, receiptSHA256: hashFile(factsJob.receipt)}};
    row.rustToGoRatio = 1; report.scenarios.push(row);
  });
  for (let i = 0; i < 2200; i++) put(join(jobs[1].job.evidence, `${String(i).padStart(4, '0')}-${'e'.repeat(160)}.json`), {ordinal: i});
  jobs.forEach(({job}, i) => { job.seal({state: i ? 'succeeded' : 'failed', handlesReaped: true, logs: evidenceFiles(job.evidence)}); if (i) job.cleanup(); });
  const childId = randomUUID(), childStem = join(outer.evidence, `process-${childId}`), childProof = {...nativeProof(childId, 'C:\\node.exe', 1), startupIdentity: childStem + '.identity.json', rootIdentity: childStem + '.root.json'};
  put(childStem + '.receipt.json', childProof); put(childStem + '.request.json.wrapper.json', {status: 0});
  const childRequest = {schema: 'lycheedev.owned-request.v2', runId: childId, command: childProof.binary, cwd: root, timeoutMs: 3600000, orphanMs: 30000, cargoBuild: false, msvcCompile: false,
    args: [join(runnerRoot, 'tests/rust-performance-paired/source-pack-scenarios-postcard-fresh.mjs'), '--inner', path.configuration]};
  for (const [key, suffix] of [['stdout', '.stdout.bin'], ['stderr', '.stderr.bin'], ['receipt', '.receipt.json'], ['identity', '.identity.json'], ['rootIdentity', '.root.json']]) childRequest[key] = childStem + suffix;
  put(childStem + '.request.json', childRequest); put(childRequest.stdout, 'synthetic failed controller stdout'); put(childRequest.stderr, '');
  put(childRequest.identity, {schema: 'lycheedev.owned-start.v1', runId: childId, requestSHA256: hashFile(childStem + '.request.json'), wrapperObservedSHA256: inputs['tools/rust-owned-process.ps1'], rootAssignment: 'PROC_THREAD_ATTRIBUTE_JOB_LIST', historicalReapProof: false});
  put(childRequest.rootIdentity, {schema: 'lycheedev.owned-root.v1', runId: childId, jobAssignedAtCreation: true, historicalReapProof: false, pid: childProof.pid, creationFileTime: childProof.creationFileTime});
  const owned = {schema: 'lycheedev.source-postcard-fresh-owned.v1', state: 'failed', error: 'fresh Postcard controller incomplete', handlesReaped: true, measuredIndexInvocations: 12, exporterInvocations: 6,
    performanceAcceptanceEvaluated: false, importedSamples: 0, request, inputs, allowance: null, diskPolicy: limits, observation: path.observation, process: {exit: 1, error: null, handlesReaped: true, proof: childProof}};
  put(path.configuration, config); put(path.observation, report); put(path.ownedReport, owned); outer.seal({state: 'failed', handlesReaped: true, logs: evidenceFiles(outer.evidence)});
  const reference = {schema: 'lycheedev.source-postcard-reverify-config.v1', ownerRoot: root, originalRunnerSHA256: sha256(Buffer.from(JSON.stringify(inputs))), ...Object.fromEntries(Object.entries(path).map(([key, path]) => [key, {path, sha256: hashFile(path)}]))};
  const calls = {candidate: 0, exporter: 0, go: 0, facts: 0};
  // Synthetic process/owner graph uses real disk records and native validators.
  // Artifact/build and full-custody validators have independent real Node suites;
  // no fixture claims these synthetic executables or Facts are genuine products.
  const services = {verifyCandidateRecord(input) { assert.deepEqual(input, request.candidate); calls.candidate++; return {sourceBinary: binaries.rust}; },
    verifyPostcardExporter(_root, input) { assert.equal(input.artifactSHA256, request.exporter.artifactSHA256); calls.exporter++; },
    verifyGoPackage() { calls.go++; return {binary: binaries.go, packageSHA256: request.go.packageSHA256}; },
    verifyFreshPostcardReport(input, selected) { assert.deepEqual(input, report); assert.deepEqual(selected, request); calls.facts++; }};
  function reseal(receiptPath) {
    const receipt = load(receiptPath); receipt.sealedLogs = evidenceFiles(dirname(receiptPath)).map(path => ({path, original: path, sha256: hashFile(path)}));
    receipt.logHashes = Object.fromEntries(receipt.sealedLogs.map(row => [relative(root, row.path), row.sha256])); put(receiptPath, receipt);
    for (const ref of Object.values(reference)) if (ref?.path) ref.sha256 = hashFile(ref.path);
  }
  return {root, reference, report, owned, config, path, jobs, factsJobs, calls, services, reseal, run: () => reverifySealedPostcardFailure(reference, services)};
}
test('read-only reverify authenticates real sealed owner/native-call graph and preserves the failed report bytes', t => {
  const f = fixture(t), before = Object.values(f.path).map(path => fs.readFileSync(path)); const result = f.run();
  assert.equal(result.state, 'sealed_complete_scenarios_reverified'); assert.equal(result.productInvocations, 0); assert.equal(result.exporterInvocations, 0); assert.equal(result.original.state, 'source_pack_scenarios_failed');
  assert.equal(result.original.finalIntegrityVerified, false); assert.equal(result.original.outerState, 'failed'); assert.equal(result.originalRecordsUnchanged, true); assert.equal(result.performanceAcceptanceEvaluated, false);
  assert.equal(result.scenarios.length, 6); assert(result.firstOversizedRetiredReceipt.bytes > 1 << 20); assert.equal(result.verifier.ownerReceiptMaximumBytes, 16 << 20);
  assert.deepEqual(f.calls, {candidate: 2, exporter: 2, go: 2, facts: 1}); Object.values(f.path).forEach((path, i) => assert.deepEqual(fs.readFileSync(path), before[i]));
});
test('reverify rejects substituted reference paths, digests, original runner and unexpected fields', t => {
  const f = fixture(t);
  for (const change of [v => v.schema = 'unknown', v => v.extra = true, v => v.originalRunnerSHA256 = '0'.repeat(64), v => v.observation.sha256 = '0'.repeat(64), v => v.observation.path = f.path.ownedReport]) {
    const value = structuredClone(f.reference); change(value); assert.throws(() => reverifySealedPostcardFailure(value, f.services));
  }
  assert.deepEqual(validatePostcardReverifyRequest(f.reference), f.reference);
});
test('reverify never changes a partial, different failure or cleanup failure into a complete observation', t => {
  const f = fixture(t), original = structuredClone(f.report);
  for (const change of [v => v.blocker = 'product failed', v => v.state = 'source_pack_scenarios_observed', v => v.finalIntegrityVerified = true, v => v.cleanupFailure = 'blocked', v => v.requiresOuterRetirement = true, v => v.scenarios.pop(), v => v.scenarios[0].state = 'failed', v => v.invocationAttempts.pop(), v => v.notRunInvocations.push({caseId: 'cold', engine: 'go'})]) {
    const report = structuredClone(original); change(report); put(f.path.observation, report); f.reseal(f.path.outerReceipt); assert.throws(f.run);
  }
});
test('reverify rejects oversized and foreign owners, changed sealed logs and native process identity failures', t => {
  const f = fixture(t), receiptPath = f.jobs[1].job.receipt, original = fs.readFileSync(receiptPath);
  fs.truncateSync(receiptPath, FRESH_OWNER_RECEIPT_BYTES + 1); assert.throws(f.run, /Postcard regular file budget/); fs.writeFileSync(receiptPath, original);
  for (const change of [v => v.owner = 'foreign', v => v.state = 'failed', v => v.handlesReaped = false, v => v.admission.limits.scratchBytes++, v => v.logHashes = {}, v => v.sealedLogs.push(v.sealedLogs[0])]) {
    const receipt = JSON.parse(original); change(receipt); put(receiptPath, receipt); assert.throws(f.run); fs.writeFileSync(receiptPath, original);
  }
  const row = f.report.scenarios[2].go, proofPath = join(f.jobs[1].job.evidence, `process-${row.proof.runId}.receipt.json`), before = fs.readFileSync(proofPath);
  put(proofPath, {...row.proof, binary: 'foreign.exe'}); assert.throws(f.run, /sealed log changed/);
  f.reseal(receiptPath); assert.throws(f.run); fs.writeFileSync(proofPath, before); f.reseal(receiptPath);
  const requestPath = proofPath.replace('.receipt.json', '.request.json'), request = load(requestPath); request.args[0] = 'other'; put(requestPath, request); f.reseal(receiptPath); assert.throws(f.run);
});
test('reverify rejects an owner receipt reparse boundary', t => {
  const f = fixture(t), path = f.jobs[1].job.receipt, retained = path + '.original'; fs.renameSync(path, retained);
  try { fs.symlinkSync(retained, path, 'file'); } catch (error) { if (['EPERM', 'EACCES'].includes(error.code)) { fs.renameSync(retained, path); t.skip('Windows symbolic-link privilege unavailable'); return; } throw error; }
  assert.throws(f.run, /reparse_boundary/);
});

test('base-only terminal growth cannot stand in for a receipt already oversized during final verification', t => {
  const f = fixture(t), base = f.jobs[0].job, changed = f.jobs[1].job;
  for (const name of fs.readdirSync(changed.evidence).filter(name => /^\d{4}-e+\.json$/.test(name))) fs.renameSync(join(changed.evidence, name), join(base.evidence, name));
  f.reseal(base.receipt); f.reseal(changed.receipt);
  assert(fs.statSync(base.receipt).size > 1 << 20); assert(fs.statSync(changed.receipt).size < 1 << 20);
  assert.throws(f.run, /already-retired owner/);
});
test('reverify performs no filesystem mutation or process invocation', t => {
  const f = fixture(t), mocks = [];
  for (const method of ['writeFileSync', 'appendFileSync', 'mkdirSync', 'renameSync', 'rmSync', 'unlinkSync', 'copyFileSync']) mocks.push(t.mock.method(fs, method, () => assert.fail(`read-only verifier called ${method}`)));
  for (const method of ['spawnSync', 'spawn', 'execSync', 'execFileSync']) mocks.push(t.mock.method(childProcess, method, () => assert.fail(`read-only verifier called ${method}`)));
  syncBuiltinESMExports();
  try { assert.equal(f.run().originalRecordsUnchanged, true); } finally { for (const mock of mocks) mock.mock.restore(); syncBuiltinESMExports(); }
});
