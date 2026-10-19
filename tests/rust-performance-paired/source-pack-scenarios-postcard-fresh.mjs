// Explicit v13 fresh six-scenario route. Reuses the established controller,
// process owners, timing boundaries and Go oracle. No historical continuation.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {dirname, isAbsolute, join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch, contained, sha256} from '../../tools/rust-disk.mjs';
import {hashFile, runCargo} from '../../tools/rust-build.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {processPassed, processReaped} from '../rust-parity/formal-lua-3.2.3/assertions.mjs';
import {splitIndexEvidence, verifyParts} from '../../tools/rust-source-pack-evidence.mjs';
import {semantic} from './contract.mjs';
import {sourcePhaseEnvironment, validateConditions} from './source-candidates.mjs';
import {openGoCustody} from './source-pack-custody.mjs';
import {CASES, FRESH_ALLOWANCE, candidateIdentity, captureComparedGo, indexEvidence, evidenceFiles, assertDiskPolicy, validateRequest, verifyCandidateRecord, runController, verifyObservedReport} from './source-pack-scenarios.mjs';
import {comparePostcardFacts, POSTCARD_RUNNER_FILES} from './source-pack-scenarios-postcard.mjs';
import {POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT, verifyPostcardIndexCustody, assertPostcardOutputBudget, INDEX_EXPORT_ARGS, EXPORT_ARGS, LIMITS, EXPORT_BUILD_ARGS, TOOLCHAIN_OWNER, postcardCompilerEnvironment, postcardEnvironmentSHA, postcardToolchainPolicy, postcardCompilerObservation, readPostcardBytes, sealPostcardExporter, verifyPostcardExporter, verifyPostcardCustody} from './read-fact-delta-postcard.mjs';

const exact = (v, keys, label = 'exact fresh v13 fields required') => assert.deepEqual(Object.keys(v ?? {}).sort(), [...keys].sort(), label);
const json = (file, maximum = 16 << 20) => JSON.parse(readPostcardBytes(file, maximum));
const save = (file, value) => { const raw = Buffer.from(JSON.stringify(value, null, 2) + '\n'); assert(raw.length <= 16 << 20); fs.writeFileSync(file, raw, {flag: 'wx'}); };
export const FRESH_POSTCARD_FILES = Object.freeze([...POSTCARD_RUNNER_FILES, 'tests/rust-performance-paired/source-pack-scenarios-postcard-fresh.mjs']);
export const freshPostcardIdentity = root => Object.fromEntries(FRESH_POSTCARD_FILES.map(file => [file, hashFile(join(root, file))]));
const expectedExporter = value => { const {mode, ...identity} = value; assert.equal(mode, 'existing'); return identity; };

export function validatePostcardFreshRequest(value) {
  exact(value, ['schema', 'layout', 'ownerRoot', 'candidate', 'go', 'exporter']);
  assert.equal(value.schema, 'lycheedev.source-postcard-fresh-config.v1'); assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(value.layout));
  const common = validateRequest({schema: 'lycheedev.source-pack-scenarios-config.v1', ownerRoot: value.ownerRoot, candidate: value.candidate, go: value.go});
  assert(['build', 'existing'].includes(value.exporter?.mode), 'explicit exporter preparation mode required');
  if (value.exporter.mode === 'build') {
    exact(value.exporter, ['mode', 'artifactDirectory']); assert(isAbsolute(value.exporter.artifactDirectory));
    contained(common.ownerRoot, value.exporter.artifactDirectory); assert(!fs.existsSync(value.exporter.artifactDirectory), 'new exporter artifact directory required');
  } else {
    exact(value.exporter, ['mode', 'artifact', 'artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']);
    assert(isAbsolute(value.exporter.artifact)); contained(common.ownerRoot, value.exporter.artifact);
    for (const key of ['artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']) assert.match(value.exporter[key], /^[a-f0-9]{64}$/);
    assert.equal(value.exporter.sourceTreeSHA256, common.candidate.sourceTreeSHA256, 'same-source exporter required');
  }
  return {...common, schema: value.schema, layout: value.layout, exporter: structuredClone(value.exporter)};
}

// Build once, before any measured product invocation, or authenticate an
// existing same-source artifact. Only runCargo owns compilation and targets.
export function prepareFreshPostcardExporter(request, overrides = {}) {
  const {verify = verifyPostcardExporter, allowance, environment = process.env} = overrides;
  freshPostcardDiskPolicy(request.ownerRoot, environment, allowance);
  const started = performance.now(); let identity, built = false;
  if (request.exporter.mode === 'build') {
    const directory = request.exporter.artifactDirectory;
    const artifact = buildPostcardExporter(request, overrides);
    const path = join(directory, 'artifact.json');
    identity = {artifact: path, artifactSHA256: hashFile(path, 65536), binarySHA256: artifact.binarySHA256, sourceTreeSHA256: artifact.inputHashes.sourceTree}; built = true;
  } else identity = expectedExporter(request.exporter);
  const verified = verify(request.ownerRoot, identity, request.candidate);
  return {request: {...request, exporter: {mode: 'existing', ...identity}}, preparation: {built, wrapperWallMillis: performance.now() - started,
    scope: 'Exporter build/authentication before product measurement; separate from every index elapsed', identity: verified}};
}

// New exporter route only. Compiler observations have their own sealed owner;
// The same explicit task-local limits reach both compiler and Cargo owners;
// default policy, build identity and environment construction stay unchanged.
export function buildPostcardExporter(request, {build = runCargo, execute = ownedSpawnSync, owner = beginScratch, seal = sealPostcardExporter, environment = process.env, allowance} = {}) {
  const {ownerRoot: root, candidate} = request, limits = freshPostcardDiskPolicy(root, environment, allowance), env = postcardCompilerEnvironment(environment), policy = postcardToolchainPolicy(candidate.sourceRoot);
  const environmentSHA256 = postcardEnvironmentSHA(env), job = owner(root, TOOLCHAIN_OWNER, {command: ['rustc', '-Vv', 'before-and-after-source-exporter-build'],
    inputs: {sourceTree: candidate.sourceTreeSHA256, toolchainPolicy: policy.sha256, environment: environmentSHA256}, limits,
    admissionOptions: {requiredBytes: 1 << 20, scratchReserveBytes: 1 << 20}});
  const record = {schema: 'lycheedev.source-postcard-toolchain.v1', state: 'running', sourceRoot: candidate.sourceRoot, sourceTreeSHA256: candidate.sourceTreeSHA256, policy, environmentSHA256, observations: []};
  const path = join(job.evidence, 'toolchain.json'); let handlesReaped = true, terminal = false;
  function observe(label) {
    assert(handlesReaped); handlesReaped = false;
    const run = execute('rustc', ['-Vv'], {cwd: candidate.sourceRoot, env: {...env}, evidence: job.evidence, timeout: 180000, cargoBuild: false});
    handlesReaped = processReaped(run) === true;
    const identity = postcardCompilerObservation(run, policy); assert.match(run.proof.runId, /^[a-f0-9-]{36}$/);
    record.observations.push({label, runId: run.proof.runId, identity}); return identity;
  }
  try {
    const before = observe('before-build');
    handlesReaped = false; // Unknown Cargo throws must not imply tree retirement.
    return build(root, [...EXPORT_BUILD_ARGS], {sourceRoot: candidate.sourceRoot, owner: 'source-postcard-exporter-build', oneShot: true, environment: {...env}, limits,
      onSuccess(invocation) {
        handlesReaped = true; // runCargo calls this only after its owned process completes.
        assert.deepEqual(json(invocation.buildEvidence, 1 << 20).admission.limits, limits, 'exporter Cargo admission differs');
        assert.deepEqual(json(job.receipt, 1 << 20).admission.limits, limits, 'exporter compiler admission differs');
        // These are runCargo's existing additions, not compiler-selection
        // overrides. Bind its effective environment to this same snapshot.
        const cargoEnv = {...env, CARGO_TARGET_DIR: invocation.target, CARGO_INCREMENTAL: '0'};
        if (invocation.inputHashes.emmyPatch) cargoEnv.LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256 = invocation.inputHashes.emmyPatch;
        assert.equal(postcardEnvironmentSHA(cargoEnv), invocation.environmentSHA256, 'exporter build environment differs from compiler observations');
        assert.equal(invocation.inputHashes.sourceTree, candidate.sourceTreeSHA256);
        assert.deepEqual(postcardToolchainPolicy(candidate.sourceRoot), policy, 'compiler policy changed during build');
        assert.deepEqual(observe('after-build'), before, 'compiler changed across exporter build');
        record.build = {receipt: invocation.buildEvidence, command: ['cargo', ...EXPORT_BUILD_ARGS], inputHashes: invocation.inputHashes, environmentSHA256: invocation.environmentSHA256, target: invocation.target};
        record.state = 'verified'; record.handlesReaped = true; save(path, record);
        job.seal({state: 'succeeded', handlesReaped: true, logs: evidenceFiles(job.evidence)}); job.cleanup(); terminal = true;
        const toolchain = {record: path, recordSHA256: hashFile(path, 65536), receipt: job.receipt, receiptSHA256: hashFile(job.receipt, 1 << 20)};
        return seal(root, candidate.sourceRoot, request.exporter.artifactDirectory, invocation, toolchain);
      }});
  } catch (error) {
    if (error.buildEvidence) { try { if (json(error.buildEvidence, 1 << 20).handlesReaped !== true) handlesReaped = false; } catch { handlesReaped = false; } }
    record.error = error.message;
    throw Object.assign(error, {handlesReaped, toolchainEvidence: {record: path, receipt: job.receipt}});
  } finally {
    if (!terminal) {
      record.state = handlesReaped ? 'failed' : 'interrupted'; record.handlesReaped = handlesReaped;
      if (!fs.existsSync(path)) save(path, record);
      job.seal({state: record.state, handlesReaped, logs: evidenceFiles(job.evidence)});
      // Failed compiler/build observations retain their own scratch/evidence.
    }
  }
}

// Original limits are kept: one 32 MiB container, 128 MiB summed records/output,
// 512 MiB binary custody, 16 MiB per sealed file, unchanged global disk policy.
export function postcardComparisonFootprint() {
  const scratchReserveBytes = LIMITS.custody + LIMITS.total + LIMITS.manifest;
  const evidenceBytes = LIMITS.custody + LIMITS.total + (64 << 20);
  return {requiredBytes: scratchReserveBytes + evidenceBytes, scratchReserveBytes, evidenceBytes};
}
function archivedRustIndex(index, directory) {
  const manifest = join(directory, 'rust-index-parts/manifest.json'), identity = {path: index.path, bytes: index.bytes, sha256: index.sha256};
  const parts = json(manifest, 65536); assert.deepEqual(parts.source, identity); verifyParts(dirname(manifest), parts);
  const original = join(directory, 'rust-index-manifest.json'); assert.deepEqual(json(original, 1 << 20), index.manifest);
  return {directory: dirname(manifest), manifest, manifestSHA256: hashFile(manifest, 65536), source: identity,
    indexManifest: {path: original, bytes: fs.lstatSync(original).size, sha256: hashFile(original, 1 << 20)}};
}

export async function comparePostcardScenario(homes, outputs, pin, oracle, directory, root, retained, custodyOptions, exporter, overrides = {}) {
  const svc = {beginScratch, indexEvidence, captureComparedGo, openGoCustody, comparePostcardFacts, ownedSpawnSync, ...overrides};
  const layout = custodyOptions.layout ?? POSTCARD_LAYOUT, expectedCalls = layout === POSTCARD_INDEX_LAYOUT ? 2 : 1;
  assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(layout));
  const candidate = custodyOptions.candidate, limits = custodyOptions.limits ?? assertDiskPolicy(root);
  const indexes = retained ? {go: retained.index, rust: svc.indexEvidence(homes.rust, 'rust', pin, directory, root, {layout})} :
    Object.fromEntries(['go', 'rust'].map(engine => [engine, svc.indexEvidence(homes[engine], engine, pin, directory, root, {layout})]));
  const goCustody = retained?.reference ?? await svc.captureComparedGo(homes.go, indexes.go, directory, root, custodyOptions);
  const estimate = postcardComparisonFootprint();
  const job = svc.beginScratch(root, 'source-postcard-facts', {command: ['source-postcard-facts', pin.id], inputs: {candidate: candidateIdentity(candidate), exporter, goCustody, rustIndex: indexes.rust.sha256}, limits,
    admissionOptions: {requiredBytes: estimate.requiredBytes, scratchReserveBytes: estimate.scratchReserveBytes}});
  let handlesReaped = true, succeeded = false, failure, result, go; const started = performance.now();
  const report = {schema: 'lycheedev.source-postcard-scenario-facts.v1', state: 'running', productInvocations: 0, exporterInvocations: 0, pin, exporter, estimate};
  const execute = (...args) => {
    assert(report.exporterInvocations < expectedCalls, 'bounded exporter invocations per comparison');
    assert.deepEqual(args[1], expectedCalls === 2 && report.exporterInvocations === 0 ? INDEX_EXPORT_ARGS : EXPORT_ARGS, 'exact Index then Facts exporter order'); report.exporterInvocations++;
    handlesReaped = false; const run = svc.ownedSpawnSync(...args); handlesReaped = processReaped(run) === true; return run;
  };
  try {
    go = svc.openGoCustody(goCustody); go.reference = goCustody;
    const facts = await svc.comparePostcardFacts({root, candidate, exporter, rustHome: homes.rust, index: indexes.rust, go, scratch: job.directory, evidence: job.evidence, layout}, {execute});
    assert(handlesReaped && report.exporterInvocations === expectedCalls, 'complete owned exporters required');
    const values = Object.fromEntries(['go', 'rust'].map(engine => [engine, semantic('B02-retail-index', outputs[engine], engine, oracle, pin.id).value]));
    assert.deepEqual(values.go, values.rust, 'complete shared summary differs'); assert.equal(facts.documents, oracle.documents);
    for (const engine of ['go', 'rust']) for (const field of ['declarations', 'relationships', 'diagnostics']) assert.equal(facts.totals[engine][field], values[engine][field], `${engine} full Facts totals`);
    result = {goCustody, rustCustody: {directory: job.evidence, catalog: facts.custody, catalogSHA256: facts.custodySHA256, candidate: candidateIdentity(candidate)},
      documents: facts.documents, fullFieldsAndOrderEqual: true, totals: facts.totals,
      indexes: Object.fromEntries(Object.entries(indexes).map(([key, value]) => [key, {path: value.path, bytes: value.bytes, sha256: value.sha256}])),
      summary: values.go, evidence: facts.comparison, comparisonSHA256: hashFile(facts.comparison, 16 << 20), indexCustody: archivedRustIndex(indexes.rust, directory),
      exporter: facts.exporter, exporterTiming: facts.exporterTiming, exportSummary: facts.summary, ...(facts.indexExport ? {indexExport: facts.indexExport} : {}), validationWallMillis: performance.now() - started,
      validationScope: 'Full post-measurement comparison/custody owner; product elapsed remains unchanged'};
    report.result = result; report.state = 'verified'; succeeded = true;
  } catch (error) { failure = error; report.state = handlesReaped ? 'failed' : 'interrupted'; report.error = error.message; }
  finally { try {
    go?.recordsStream.destroy();
    let archivedOutputBytes = 0;
    for (const name of ['postcard-index-output', 'postcard-output']) {
      const manifest = join(job.evidence, `${name}-parts/manifest.json`);
      if (fs.existsSync(manifest)) archivedOutputBytes += json(manifest, 65536).source.bytes;
    }
    for (const name of ['postcard-index-output', 'postcard-output']) {
      const output = join(job.directory, `${name}.jsonl`);
      if (!succeeded && handlesReaped && fs.existsSync(output) && !fs.existsSync(join(job.evidence, `${name}-parts`))) {
        const bytes = fs.lstatSync(output).size;
        if (bytes <= LIMITS.total - archivedOutputBytes) {
          const parts = splitIndexEvidence(root, {path: output, bytes, sha256: hashFile(output, LIMITS.total)}, join(job.evidence, name === 'postcard-output' ? 'postcard-failed-output-parts' : 'postcard-index-failed-output-parts'));
          if (layout === POSTCARD_LAYOUT) report.partialOutput = parts; else (report.partialOutputs ??= []).push(parts); archivedOutputBytes += bytes;
        } else (report.retainedRawOutputs ??= []).push({path: output, bytes, reason: 'unchanged shared 128 MiB output archive cap; failed owner retains scratch'});
      }
    }
    report.handlesReaped = handlesReaped; save(join(job.evidence, 'postcard-scenario-report.json'), report);
    const logs = evidenceFiles(job.evidence); assert(logs.reduce((n, file) => n + fs.lstatSync(file).size, 0) <= estimate.evidenceBytes, 'Postcard comparison evidence budget');
    job.seal({state: handlesReaped ? succeeded ? 'succeeded' : 'failed' : 'interrupted', handlesReaped, logs});
    if (succeeded && handlesReaped) job.cleanup();
  } catch (error) { failure ??= error; succeeded = false; } }
  const owner = {receipt: job.receipt, report: join(job.evidence, 'postcard-scenario-report.json')};
  try { owner.receiptSHA256 = hashFile(owner.receipt, 1 << 20); owner.reportSHA256 = hashFile(owner.report, 16 << 20); } catch (error) { failure ??= error; }
  if (failure) throw Object.assign(failure, {handlesReaped, postcardEvidence: owner});
  return {...result, postcardOwner: owner};
}

export function verifyFreshPostcardFacts(facts, candidate, exporter, layout = POSTCARD_LAYOUT, limits) {
  assert.deepEqual(facts.rustCustody.candidate, candidateIdentity(candidate));
  const raw = readPostcardBytes(facts.rustCustody.catalog, 16 << 20); assert.equal(sha256(raw), facts.rustCustody.catalogSHA256); const custody = JSON.parse(raw); verifyPostcardCustody(custody);
  assert.equal(custody.exporter.artifactSHA256, exporter.artifactSHA256); assert.equal(custody.exporter.artifact.binarySHA256, exporter.binarySHA256); assert.equal(custody.exporter.artifact.inputHashes.sourceTree, candidate.sourceTreeSHA256);
  assert.deepEqual(custody.summary, facts.exportSummary); assert.equal(custody.summary.records, facts.documents);
  const comparisonRaw = readPostcardBytes(facts.evidence, 16 << 20); assert.equal(sha256(comparisonRaw), facts.comparisonSHA256); const comparison = JSON.parse(comparisonRaw);
  assert.equal(custody.layout, layout); assert.equal(comparison.inputs.rustLayout, layout); assert.equal(comparison.documents, facts.documents); assert.deepEqual(comparison.differentFiles, []); assert.deepEqual(comparison.totals, facts.totals);
  assert.deepEqual(comparison.factSources.rust, custody.sources); assert.equal(comparison.inputs.rustIndexSHA256, facts.indexes.rust.sha256);
  const go = openGoCustody(facts.goCustody); go.recordsStream.destroy(); go.verify();
  const goRaw = readPostcardBytes(facts.goCustody.catalog, 16 << 20); assert.equal(sha256(goRaw), facts.goCustody.catalogSHA256); const goCatalog = JSON.parse(goRaw);
  assert.deepEqual(comparison.factSources.go, goCatalog.files.map(row => ({file: resolve(goCatalog.home, row.relative), bytes: row.bytes, sha256: row.sha256})));
  assert.equal(facts.indexes.go.sha256, goCatalog.records.sha256);
  const index = facts.indexCustody; assert.equal(hashFile(index.manifest, 65536), index.manifestSHA256); const parts = json(index.manifest, 65536); assert.deepEqual(parts.source, index.source); assert.deepEqual(index.source, facts.indexes.rust); verifyParts(index.directory, parts);
  assert.equal(hashFile(index.indexManifest.path, 1 << 20), index.indexManifest.sha256); const manifest = json(index.indexManifest.path, 1 << 20); assert.deepEqual(manifest, custody.index.manifest); assert.equal(manifest.blob.sha256, index.source.sha256);
  if (layout === POSTCARD_INDEX_LAYOUT) {
    const ref = facts.indexExport; assert(ref, 'complete Index export custody required');
    assert.equal(hashFile(ref.custody, 16 << 20), ref.custodySHA256); const exported = json(ref.custody); verifyPostcardIndexCustody(exported);
    assert.deepEqual(exported.index, custody.index); assert.deepEqual(exported.exporter, custody.exporter); assert.deepEqual(exported.summary, ref.summary);
    assert.deepEqual(exported.oracle, ref.oracle); assert.deepEqual(exported.oracle.go, facts.goCustody); assert.equal(exported.oracle.goRecordsSHA256, facts.indexes.go.sha256);
    assert.equal(exported.oracle.goManifestSHA256, comparison.inputs.goManifestSHA256); assert.equal(exported.oracle.documents, facts.documents);
    for (const field of ['repository', 'product', 'commit', 'parser']) assert.equal(exported.oracle.source[field], facts.summary[field], 'public Index source differs from measured result');
    assert.equal(exported.oracle.assets, facts.summary.assets); assert.equal(exported.oracle.diagnostics, facts.summary.diagnostics);
    assertPostcardOutputBudget(exported.summary.outputBytes, custody.summary.outputBytes);
  } else assert.equal(facts.indexExport, undefined, 'historical JSON Index has no binary Index export');
  const owner = facts.postcardOwner; assert.equal(hashFile(owner.receipt, 1 << 20), owner.receiptSHA256); assert.equal(hashFile(owner.report, 16 << 20), owner.reportSHA256);
  const report = json(owner.report), receipt = json(owner.receipt, 1 << 20); assert.equal(report.state, 'verified'); assert.equal(report.handlesReaped, true); assert.equal(report.exporterInvocations, layout === POSTCARD_INDEX_LAYOUT ? 2 : 1); assert.equal(report.productInvocations, 0);
  assert.equal(receipt.owner, 'source-postcard-facts'); assert(['succeeded', 'retired'].includes(receipt.state)); assert.equal(receipt.handlesReaped, true); assert.equal(receipt.activePid, null);
  if (limits) assert.deepEqual(receipt.admission.limits, limits, 'Postcard custody admission differs');
  const {postcardOwner, ...expected} = facts; assert.deepEqual(report.result, expected, 'archived scenario result differs'); return true;
}
export function verifyFreshPostcardReport(report, request, allowance) {
  assert.equal(report.layout, request.layout ?? POSTCARD_LAYOUT); assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(report.layout)); assert.equal(report.invocationAttempts.length, 12, 'exactly twelve fresh measured index invocations');
  const expected = CASES.flatMap((entry, i) => (i === 1 ? ['rust', 'go'] : ['go', 'rust']).map(engine => ({caseId: entry.id, engine})));
  assert.deepEqual(report.invocationAttempts, expected); assert.equal(report.continuation, undefined); assert.equal(report.importedSamples, 0);
  const limits = freshPostcardDiskPolicy(request.ownerRoot, process.env, allowance); assert.deepEqual(report.diskPolicy, limits, 'fresh controller disk policy differs');
  assert.equal(report.owners.length, 9, 'one healthy base and eight changed scenario owners required');
  for (const owner of report.owners) assert.deepEqual(json(contained(request.ownerRoot, owner.receipt), 1 << 20).admission.limits, limits, 'scenario owner admission differs');
  assert.equal(report.scenarios.length, 6); for (const row of report.scenarios) verifyFreshPostcardFacts(row.facts, request.candidate, expectedExporter(request.exporter), request.layout, limits);
  return true;
}

export async function runPostcardFreshController(config, overrides = {}) {
  assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(config.layout)); assert.equal(config.layout, config.request.layout); assert.equal(config.request.exporter.mode, 'existing');
  const exporter = expectedExporter(config.request.exporter);
  const services = {runnerIdentity: freshPostcardIdentity,
    verifyAdditionalIdentity: request => verifyPostcardExporter(request.ownerRoot, exporter, request.candidate),
    compareFacts: (...args) => comparePostcardScenario(...args, exporter),
    verifyAdditionalEvidence: report => verifyFreshPostcardReport(report, config.request, config.allowance), ...overrides};
  return runController(config, services);
}

export function parsePostcardFreshArguments(args) {
  const [configuration, conditions, flag] = args;
  assert((args.length === 2 || args.length === 3 && flag === '--scratch-6gib') &&
    [configuration, conditions].every(value => typeof value === 'string' && value && !value.startsWith('--')),
    'usage: node source-pack-scenarios-postcard-fresh.mjs CONFIG.json CONDITIONS.json [--scratch-6gib]');
  return {configuration, conditions, scratch6GiB: flag === '--scratch-6gib'};
}
function freshPostcardDiskPolicy(root, environment, allowance) {
  if (allowance !== undefined) assert.deepEqual(allowance, FRESH_ALLOWANCE, 'only explicitly authorized task-local 6 GiB allowance');
  return assertDiskPolicy(root, environment, allowance);
}
export function validateFreshPostcardOwner(config, environment = process.env) {
  const limits = freshPostcardDiskPolicy(config.request.ownerRoot, environment, config.allowance);
  const owner = json(join(config.evidence, 'receipt.json'), 1 << 20);
  assert.equal(owner.schema, 'lycheedev.disk-owner.v1'); assert.equal(owner.owner, 'source-postcard-fresh-diagnostics'); assert.equal(owner.state, 'active');
  assert.equal(resolve(config.scratch), join(config.request.ownerRoot, '.tmp/rust-scratch', owner.id)); assert.equal(resolve(owner.directory), resolve(config.scratch)); assert.equal(resolve(config.evidence), join(config.request.ownerRoot, '.tmp/rust-disk/evidence', owner.id));
  assert.deepEqual(owner.inputHashes.runner, config.inputs, 'fresh runner receipt differs');
  assert.deepEqual(owner.inputHashes.allowance, config.allowance, 'fresh allowance receipt differs');
  assert.deepEqual(owner.admission.limits, limits, 'fresh admission receipt differs');
  assert.equal(owner.command[0], 'source-postcard-fresh-diagnostics');
  assert.equal(owner.command.length, config.allowance ? 4 : 3);
  if (config.allowance) assert.equal(owner.command[3], '--scratch-6gib');
  assert.equal(owner.commandSHA256, sha256(Buffer.from(JSON.stringify(owner.command))));
  return limits;
}

export async function runPostcardFreshOwned(configuration, conditionsPath, {scratch6GiB = false} = {}) {
  assert.equal(process.platform, 'win32', 'formal fresh v13 scenarios require Windows');
  assert.equal(typeof scratch6GiB, 'boolean'); const allowance = scratch6GiB ? FRESH_ALLOWANCE : undefined;
  let request = validatePostcardFreshRequest(json(configuration, 65536)); const conditions = validateConditions(json(conditionsPath, 16384));
  const root = request.ownerRoot, runnerRoot = resolve(import.meta.dirname, '../..'), inputs = freshPostcardIdentity(runnerRoot), limits = freshPostcardDiskPolicy(root, process.env, allowance), environment = sourcePhaseEnvironment(process.env);
  verifyCandidateRecord(request.candidate);
  const job = beginScratch(root, 'source-postcard-fresh-diagnostics', {command: ['source-postcard-fresh-diagnostics', resolve(configuration), resolve(conditionsPath), ...(scratch6GiB ? ['--scratch-6gib'] : [])], inputs: {configuration: hashFile(configuration, 65536), conditions: hashFile(conditionsPath, 16384), runner: inputs, ...(allowance ? {allowance} : {})}, limits,
    admissionOptions: {requiredBytes: 16 << 20, scratchReserveBytes: 1 << 20}});
  let state = 'failed', handlesReaped = true, failure, exporterPreparation;
  const report = {schema: 'lycheedev.source-postcard-fresh-owned.v1', state, measuredIndexInvocations: 0, exporterInvocations: 0, performanceAcceptanceEvaluated: false, importedSamples: 0, inputs, diskPolicy: limits, allowance: allowance ?? null};
  try {
    const prepared = prepareFreshPostcardExporter(request, {allowance}); request = prepared.request; exporterPreparation = prepared.preparation; report.exporterPreparation = exporterPreparation;
    assert.deepEqual(freshPostcardIdentity(runnerRoot), inputs, 'fresh runner changed during preparation');
    const config = {request, layout: request.layout, conditions, runnerRoot, inputs, scratch: job.directory, evidence: job.evidence, ...(allowance ? {allowance} : {})};
    validateFreshPostcardOwner(config);
    const configPath = join(job.evidence, 'fresh-configuration.json'); save(configPath, config);
    handlesReaped = false;
    const run = ownedSpawnSync(process.execPath, [join(runnerRoot, 'tests/rust-performance-paired/source-pack-scenarios-postcard-fresh.mjs'), '--inner', configPath], {cwd: root, env: environment, evidence: job.evidence, timeout: 3600000});
    handlesReaped = processReaped(run) === true; report.process = {exit: run.status, error: run.error?.message ?? null, handlesReaped, proof: run.proof};
    const observedPath = join(job.evidence, 'report.json'), observed = fs.existsSync(observedPath) ? json(observedPath) : null;
    report.observation = observedPath; report.measuredIndexInvocations = observed?.invocationAttempts?.length ?? 0;
    for (const row of observed?.scenarios ?? []) {
      if (row.facts?.exportSummary) report.exporterInvocations += row.facts.indexExport ? 2 : 1;
      else if (row.factsFailure?.report) { try { const failed = json(row.factsFailure.report); assert(Number.isInteger(failed.exporterInvocations) && failed.exporterInvocations >= 0 && failed.exporterInvocations <= (request.layout === POSTCARD_INDEX_LAYOUT ? 2 : 1)); report.exporterInvocations += failed.exporterInvocations; } catch { report.exporterInvocationsUnknown = true; } }
    }
    assert(processPassed(run), 'fresh Postcard controller incomplete');
    verifyObservedReport(run, observed, request, inputs, undefined, undefined, undefined, {layout: request.layout});
    verifyFreshPostcardReport(observed, request, allowance); verifyCandidateRecord(request.candidate); verifyPostcardExporter(root, expectedExporter(request.exporter), request.candidate);
    assert.deepEqual(freshPostcardIdentity(runnerRoot), inputs); assert.deepEqual(observed.diskPolicy, limits);
    state = 'succeeded'; report.state = 'source_postcard_fresh_scenarios_observed';
  } catch (error) {
    failure = error; report.error = error.message;
    if (error.handlesReaped === false) handlesReaped = false;
    if (error.toolchainEvidence) report.toolchainEvidence = error.toolchainEvidence;
    if (error.buildEvidence) { report.buildEvidence = error.buildEvidence; try { if (json(error.buildEvidence, 1 << 20).handlesReaped !== true) handlesReaped = false; } catch { handlesReaped = false; } }
  } finally {
    report.request = request; report.handlesReaped = handlesReaped; save(join(job.evidence, 'postcard-owned-report.json'), report);
    job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs: evidenceFiles(job.evidence)}); if (state === 'succeeded' && handlesReaped) job.cleanup();
  }
  return {state: report.state, success: !failure, report: join(job.evidence, 'postcard-owned-report.json'), receipt: job.receipt, error: failure?.message};
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  if (process.argv[2] === '--inner') {
    assert.equal(process.platform, 'win32'); assert.equal(process.argv.length, 4);
    const config = json(process.argv[3], 1 << 20); config.request = validatePostcardFreshRequest(config.request); validateConditions(config.conditions);
    validateFreshPostcardOwner(config);
    const report = await runPostcardFreshController(config); console.log(JSON.stringify({state: report.state, report: join(config.evidence, 'report.json')})); if (report.state !== 'source_pack_scenarios_observed') process.exitCode = 1;
  } else {
    const {configuration, conditions, scratch6GiB} = parsePostcardFreshArguments(process.argv.slice(2));
    const result = await runPostcardFreshOwned(configuration, conditions, {scratch6GiB}); console.log(JSON.stringify(result)); if (!result.success) process.exitCode = 1;
  }
}
