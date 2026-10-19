// Explicit v13 one-pair formal comparison. Historical/default six-scenario and
// resume controllers remain v11. This entry never builds or runs either CLI.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {dirname, isAbsolute, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {beginScratch, sha256} from '../../tools/rust-disk.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {splitIndexEvidence, INDEX_BYTES} from '../../tools/rust-source-pack-evidence.mjs';
import {GO_COMMIT} from './contract.mjs';
import {verifyGoPackage} from './inputs.mjs';
import {openGoCustody} from './source-pack-custody.mjs';
import {candidateIdentity, verifyCandidateRecord, indexEvidence, evidenceFiles, assertDiskPolicy, RUNNER_FILES} from './source-pack-scenarios.mjs';
import {readFactDelta, comparedRustIndex} from './read-fact-delta.mjs';
import {POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT, preparePostcardIndexExport, runPostcardIndexExport, comparePostcardIndex, capturePostcardIndexExport, assertPostcardOutputBudget, LIMITS, preparePostcardExport, runPostcardExport, capturePostcardExport, verifyPostcardCustody, verifyPostcardExporter, readPostcardBytes} from './read-fact-delta-postcard.mjs';

const exact = (value, keys) => assert.deepEqual(Object.keys(value ?? {}).sort(), [...keys].sort(), 'exact v13 configuration required');
const save = (file, value) => { const raw = Buffer.from(JSON.stringify(value, null, 2) + '\n'); assert(raw.length <= 16 << 20); fs.writeFileSync(file, raw, {flag: 'wx'}); };
export const POSTCARD_RUNNER_FILES = [...RUNNER_FILES, 'tests/rust-performance-paired/source-pack-scenarios-postcard.mjs', 'tests/rust-performance-paired/read-fact-delta-postcard.mjs',
  'tests/rust-parity/formal-lua-3.2.3/assertions.mjs', 'tests/rust-parity/formal-lua-3.2.2/assertions.mjs', 'tests/rust-parity/rust-3.5.0-release-fields.mjs'];
export const postcardRunnerIdentity = root => Object.fromEntries(POSTCARD_RUNNER_FILES.map(file => [file, hashFile(join(root, file))]));
export function validatePostcardRequest(value) {
  exact(value, ['schema', 'layout', 'ownerRoot', 'candidate', 'exporter', 'rustHome', 'pin', 'go']);
  assert.equal(value.schema, 'lycheedev.source-postcard-comparison-config.v1'); assert.equal(value.layout, POSTCARD_LAYOUT);
  exact(value.candidate, ['sourceRoot', 'artifactRoot', 'artifact', 'commit', 'artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']); candidateIdentity(value.candidate);
  exact(value.exporter, ['artifact', 'artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']);
  exact(value.go, ['reference', 'observation', 'observationSHA256', 'packageRoot', 'luaLSArchive', 'packageSHA256']);
  for (const field of [value.go.observationSHA256, value.go.packageSHA256, ...['artifactSHA256', 'binarySHA256', 'sourceTreeSHA256'].map(k => value.exporter[k])]) assert.match(field, /^[a-f0-9]{64}$/);
  for (const file of [value.ownerRoot, value.rustHome, value.candidate.sourceRoot, value.candidate.artifactRoot, value.candidate.artifact, value.exporter.artifact, value.go.observation, value.go.packageRoot, value.go.luaLSArchive]) assert(typeof file === 'string' && isAbsolute(file), 'absolute Postcard paths required');
  assert(value.pin?.source && typeof value.pin.id === 'string'); return structuredClone(value);
}
export function authenticatePostcardGo(input) {
  const observationRaw = readPostcardBytes(input.observation, 16 << 20); assert.equal(sha256(observationRaw), input.observationSHA256, 'Go observation changed'); const observation = JSON.parse(observationRaw);
  assert.equal(observation.schema, 'lycheedev.source-pack-scenarios.v1');
  assert.equal(observation.request?.go?.packageSHA256, input.packageSHA256, 'Go observation producer differs');
  const matching = observation.scenarios?.filter(row => row.facts?.goCustody && JSON.stringify(row.facts.goCustody) === JSON.stringify(input.reference));
  assert(matching?.length >= 1, 'Go custody must be retained by the authenticated independent Go observation');
  const producer = verifyGoPackage(input.packageRoot, input.luaLSArchive); assert.equal(producer.packageSHA256, input.packageSHA256);
  const custody = openGoCustody(input.reference); custody.verify(); custody.reference = input.reference;
  return {custody, provenance: {commit: GO_COMMIT, observation: input.observation, observationSHA256: input.observationSHA256, producer, reference: input.reference}};
}

// This is a complete five-array comparison port, not a new codec or production
// API. Inputs come from independently retained Go output and this Rust index.
export async function comparePostcardFacts({root, candidate, exporter, rustHome, index, go, scratch, evidence, layout = POSTCARD_LAYOUT}, {execute} = {}) {
  assert([POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT].includes(layout));
  let indexToken, indexCustody;
  if (layout === POSTCARD_INDEX_LAYOUT) {
    const preparation = preparePostcardIndexExport(root, rustHome, index, join(scratch, 'postcard-index-input'));
    indexToken = runPostcardIndexExport(root, exporter, candidate, preparation, join(scratch, 'postcard-index-output.jsonl'), evidence, execute ? {execute} : {});
    await comparePostcardIndex(indexToken, go.reference);
    indexCustody = capturePostcardIndexExport(indexToken, evidence);
  }
  const preparation = preparePostcardExport(root, rustHome, index, join(scratch, 'postcard-input'), {layout, indexExport: indexToken});
  const token = runPostcardExport(root, exporter, candidate, preparation, join(scratch, 'postcard-output.jsonl'), evidence, execute ? {execute} : {});
  const delta = await readFactDelta(go.home, rustHome, go.recordsPath, layout, index.path, index.sha256, go, {captureSources: true, retainIndex: true, postcardExport: token, postcardIndexExport: indexToken});
  save(join(evidence, 'facts-comparison.json'), delta);
  if (indexToken) assertPostcardOutputBudget(indexToken.summary.outputBytes, token.summary.outputBytes);
  const custody = capturePostcardExport(token, evidence), manifestPath = join(evidence, 'rust-index-manifest.json');
  if (fs.existsSync(manifestPath)) assert.deepEqual(JSON.parse(readPostcardBytes(manifestPath, 1 << 20)), index.manifest); else save(manifestPath, index.manifest);
  const parsed = comparedRustIndex(delta, index); assert.equal(parsed.summary.documents, delta.documents);
  assert.deepEqual(custody.sources, delta.factSources.rust, 'Postcard compared bytes differ from archived bytes');
  assert(go.reference, 'independent Go custody reference required');
  const goCatalogRaw = readPostcardBytes(go.reference.catalog, 16 << 20); assert.equal(sha256(goCatalogRaw), go.reference.catalogSHA256); const goCatalog = JSON.parse(goCatalogRaw);
  assert.deepEqual(delta.factSources.go, goCatalog.files.map(file => ({file: resolve(goCatalog.home, file.relative), bytes: file.bytes, sha256: file.sha256})), 'Go compared bytes differ from archived bytes');
  go.verify(); verifyPostcardCustody(custody);
  for (const field of ['declarations', 'relationships', 'diagnostics']) assert.equal(delta.totals.rust[field], parsed.summary[field], 'Rust full Facts totals differ from own index');
  assert.equal(delta.differentFiles.length, 0, 'complete five-array fields/order differ');
  return {documents: delta.documents, fullFieldsAndOrderEqual: true, totals: delta.totals, custody: join(evidence, 'postcard-custody.json'), custodySHA256: hashFile(join(evidence, 'postcard-custody.json'), 16 << 20), comparison: join(evidence, 'facts-comparison.json'), exporter: token.exporter, exporterTiming: token.timing, summary: token.summary, ...(indexToken ? {indexExport: {custody: join(evidence, 'postcard-index-custody.json'), custodySHA256: hashFile(join(evidence, 'postcard-index-custody.json'), 16 << 20), summary: indexToken.summary, timing: indexToken.timing, oracle: indexCustody.oracle}} : {})};
}

export async function runPostcardComparison(configuration) {
  assert.equal(process.platform, 'win32', 'formal Postcard comparison requires Windows owned process evidence');
  const request = validatePostcardRequest(JSON.parse(readPostcardBytes(configuration, 65536))), root = fs.realpathSync(request.ownerRoot);
  const runnerRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..'), inputs = postcardRunnerIdentity(runnerRoot), limits = assertDiskPolicy(root);
  verifyCandidateRecord(request.candidate); verifyPostcardExporter(root, request.exporter, request.candidate);
  const independent = authenticatePostcardGo(request.go);
  // Existing custody/index/export bounds, not a change to global disk policy.
  const scratchReserveBytes = LIMITS.custody + LIMITS.total + LIMITS.manifest;
  const evidenceBudget = LIMITS.custody + LIMITS.total + INDEX_BYTES + (64 << 20);
  const job = beginScratch(root, 'source-postcard-comparison', {command: ['source-postcard-comparison', resolve(configuration)], inputs: {configuration: hashFile(configuration, 65536), runner: inputs}, limits,
    admissionOptions: {requiredBytes: scratchReserveBytes + evidenceBudget, scratchReserveBytes}});
  let state = 'failed', handlesReaped = true, failure;
  const report = {schema: 'lycheedev.source-postcard-comparison.v1', layout: POSTCARD_LAYOUT, state, request, inputs, go: independent.provenance, productInvocations: 0, exporterInvocations: 0, performanceAcceptanceEvaluated: false, sixScenarios: 'not_run'};
  try {
    save(join(job.evidence, 'configuration.json'), request);
    // Metadata snapshot proves this exact pin owns the v17 index recipe.
    const index = indexEvidence(request.rustHome, 'rust', request.pin, job.evidence, root, {layout: POSTCARD_LAYOUT});
    handlesReaped = false; report.exporterInvocations = 1;
    report.facts = await comparePostcardFacts({root, candidate: request.candidate, exporter: request.exporter, rustHome: request.rustHome, index, go: independent.custody, scratch: job.directory, evidence: job.evidence});
    handlesReaped = true;
    verifyCandidateRecord(request.candidate); verifyPostcardExporter(root, request.exporter, request.candidate); independent.custody.verify();
    assert.deepEqual(postcardRunnerIdentity(runnerRoot), inputs); assert.equal(hashFile(request.go.observation, 16 << 20), request.go.observationSHA256);
    state = 'succeeded'; report.state = 'complete_full_facts_comparison';
  } catch (error) {
    failure = error; if (error.handlesReaped === true) handlesReaped = true;
    // A semantic failure after a successful process still has a terminal owned
    // invocation; never infer reaping from exit/status alone.
    const invocation = join(job.evidence, 'export-invocation.json');
    if (fs.existsSync(invocation)) handlesReaped = JSON.parse(readPostcardBytes(invocation, 16 << 20)).handlesReaped === true;
    report.state = 'failed'; report.error = error.message;
  } finally {
    const output = join(job.directory, 'postcard-output.jsonl');
    if (state !== 'succeeded' && handlesReaped && fs.existsSync(output) && fs.lstatSync(output).size <= LIMITS.total && !fs.existsSync(join(job.evidence, 'postcard-output-parts'))) {
      const source = {path: output, bytes: fs.lstatSync(output).size, sha256: hashFile(output, LIMITS.total)};
      report.partialOutput = splitIndexEvidence(root, source, join(job.evidence, 'postcard-failed-output-parts'));
    }
    report.handlesReaped = handlesReaped; report.receipt = job.receipt; save(join(job.evidence, 'report.json'), report);
    const logs = evidenceFiles(job.evidence); assert(logs.reduce((n, file) => n + fs.lstatSync(file).size, 0) <= evidenceBudget, 'Postcard evidence total budget');
    job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs}); if (state === 'succeeded' && handlesReaped) job.cleanup();
  }
  if (failure) throw Object.assign(failure, {evidence: join(job.evidence, 'report.json')});
  return {state: report.state, report: join(job.evidence, 'report.json'), receipt: job.receipt};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv.length, 3, 'usage: node source-pack-scenarios-postcard.mjs CONFIG.json');
  console.log(JSON.stringify(await runPostcardComparison(process.argv[2])));
}
