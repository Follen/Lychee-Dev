import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {syncBuiltinESMExports} from 'node:module';
import {join, dirname, resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {DatabaseSync} from 'node:sqlite';
import {sha256, defaults, beginScratch} from '../../tools/rust-disk.mjs';
import {splitIndexEvidence} from '../../tools/rust-source-pack-evidence.mjs';
import {hashFile, sourceDigest, runCargo} from '../../tools/rust-build.mjs';
import {CASES, BASE_COMMIT, FRESH_ALLOWANCE, assertDiskPolicy, captureComparedGo, candidateIdentity, runController, verifyObservedReport} from './source-pack-scenarios.mjs';
import {POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT, POSTCARD_PREPARATION_POLICY, LIMITS, EXPORT_BUILD_ARGS, TOOLCHAIN_OWNER, postcardEnvironmentSHA, postcardCompilerEnvironment} from './read-fact-delta-postcard.mjs';
import {fixture as binaryFixture, put, putJSON, compilerPolicy, compilerText} from './read-fact-delta-postcard.fixtures.mjs';
import {FRESH_OWNER_RECEIPT_BYTES, readFreshPostcardReceipt, freshPostcardReceiptSHA256, parsePostcardFreshArguments, validateFreshPostcardOwner, validatePostcardFreshRequest, prepareFreshPostcardExporter, postcardComparisonFootprint, comparePostcardScenario, verifyFreshPostcardFacts, verifyFreshPostcardReport, runPostcardFreshController, freshPostcardIdentity, FRESH_POSTCARD_FILES} from './source-pack-scenarios-postcard-fresh.mjs';

const root = resolve(import.meta.dirname, '../..'), conditions = {machine: 'fixture', powerAndThermal: 'unknown', competingLoad: 'unknown', disk: 'fixture', operator: 'Node tests'};
const proof = ms => ({schema: 'lycheedev.owned-process.v1', rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false,
  timing: {schema: 'lycheedev.owned-timing.v1', frequency: 1000000, startTicks: '1000000', rootExitTicks: String(1000000 + ms * 1000 - 100), treeReapedTicks: String(1000000 + ms * 1000)}});
function requestAt(directory) {
  for (const path of ['source', 'artifacts', 'go']) fs.mkdirSync(join(directory, path));
  const artifact = join(directory, 'artifacts/artifact.json'); fs.writeFileSync(artifact, '{}');
  return {schema: 'lycheedev.source-postcard-fresh-config.v1', layout: POSTCARD_LAYOUT, ownerRoot: directory,
    candidate: {sourceRoot: join(directory, 'source'), artifactRoot: join(directory, 'artifacts'), artifact, commit: '8c9a1bccd543b8e7d605d1f791a34f7d3aa713f8', artifactSHA256: 'a'.repeat(64), binarySHA256: 'b'.repeat(64), sourceTreeSHA256: 'c'.repeat(64)},
    go: {referenceRoot: join(directory, 'go'), packageSHA256: 'd'.repeat(64)},
    exporter: {mode: 'existing', artifact: join(directory, 'exporter/artifact.json'), artifactSHA256: 'e'.repeat(64), binarySHA256: 'f'.repeat(64), sourceTreeSHA256: 'c'.repeat(64)}};
}
function temporary(t) { const directory = fs.mkdtempSync(join(tmpdir(), 'postcard-fresh-')); t.after(() => { if (t.passed) fs.rmSync(directory, {recursive: true, force: true}); else t.diagnostic(`Retained fixture: ${directory}`); }); return directory; }
function ownerFactory(directory, owners, events, fault = {}) {
  return (_root, name, options) => {
    if (fault.quota && name === 'source-postcard-facts') throw new Error('disk.scratch_quota');
    const id = owners.length, scratch = join(directory, `scratch-${id}`), evidence = join(directory, `evidence-${id}`); fs.mkdirSync(scratch); fs.mkdirSync(evidence);
    const receipt = join(evidence, 'receipt.json'), record = {schema: 'lycheedev.disk-owner.v1', id: String(id), owner: name, category: 'scratch', directory: scratch, activePid: null, state: 'active', admission: {limits: options.limits}};
    putJSON(receipt, record);
    const owner = {directory: scratch, evidence, receipt, name, options, cleaned: false, seal(value) { events.push(`seal:${name}:${value.state}`); Object.assign(record, value); putJSON(receipt, record); this.sealed = value; }, cleanup() { assert.equal(record.state, 'succeeded'); assert.equal(record.handlesReaped, true); events.push(`cleanup:${name}`); this.cleaned = true; fs.rmSync(scratch, {recursive: true}); record.state = 'retired'; putJSON(receipt, record); }};
    owners.push(owner); events.push(`begin:${name}`); return owner;
  };
}
async function controller(t, fault = {}) {
  const directory = temporary(t), request = requestAt(directory), evidence = join(directory, 'outer'), scratch = join(directory, 'outer-scratch'); fs.mkdirSync(evidence); fs.mkdirSync(scratch);
  request.layout = fault.layout ?? POSTCARD_LAYOUT;
  const inputs = {runner: 'fixture'}, owners = [], events = [], samples = [], exportCalls = [], admissions = [];
  const oracle = JSON.parse(fs.readFileSync(join(root, 'tests/rust-source-full-corpus/retail-oracle.json'))), basePin = {id: 'PIN-base', source: {exactCommit: BASE_COMMIT}};
  let calls = 0, identities = 0;
  const services = {
    runnerIdentity: () => inputs, verifyCandidateRecord: () => ({sourceBinary: 'rust.exe'}), verifyFixedGoPackage: () => ({binary: 'go.exe', packageSHA256: request.go.packageSHA256}),
    verifyAdditionalIdentity() { identities++; if (fault.identityAt === identities) throw new Error('same-source exporter changed'); },
    beginScratch: ownerFactory(directory, owners, events), admission(_root, options) { admissions.push(options); }, metadataPageSize: () => 4096,
    prepareSeeds(homes) { for (const home of Object.values(homes)) fs.mkdirSync(home); return {pin: basePin, rows: []}; },
    inventory() { return {rows: [{path: 'fixture', bytes: 100, sha256: 'a'.repeat(64)}], bytes: 100, sha256: 'healthy'}; },
    cloneSeed(_source, dest) { fs.mkdirSync(dest); events.push('clone'); },
    prepareVariant({homes, directory, id}) { const entry = CASES.find(row => row.id === id); events.push(`variant:${id}:${Object.keys(homes)[0]}`); return {pin: {id: `PIN-${id}`, source: {exactCommit: id}}, oracle: {...oracle, documents: entry.documents, commit: id}, project: join(directory, 'project'), mutation: {id}}; },
    ownedSpawnSync(binary, args, options) {
      if (binary === 'powercfg.exe') return {status: 0, handlesReaped: true, proof: proof(10), stdout: Buffer.from('power'), stderr: Buffer.alloc(0), logs: []};
      const at = calls++, entry = CASES[Math.floor(at / 2)], engine = binary === 'go.exe' ? 'go' : 'rust'; samples.push({engine, entry, args, options}); events.push(`product:${entry.id}:${engine}`);
      if (fault.throwAt === at) throw new Error('product spawn failed');
      const result = {schema: 'lycheedev.source-map.v2', storage: 'file-cache', parser: 'source-v1', repository: oracle.repository, product: oracle.product, commit: entry.mode === 'changed' ? entry.id : BASE_COMMIT,
        documents: entry.documents, declarations: 1, relationships: 0, diagnostics: 0, assets: 0, complete: true, io: {parsedDocuments: entry.mode === 'cold' ? 4036 : entry.mode === 'warm' ? 0 : 1}};
      return {status: fault.failAt === at ? 4 : 0, handlesReaped: fault.unreapedAt !== at, proof: {...proof(engine === 'go' ? 100 : 20), ...(fault.unreapedAt === at ? {treeReaped: false} : {})},
        stdout: Buffer.from(JSON.stringify({schema: 'lycheedev.result.v1', ok: true, error: null, context: {snapshot: entry.mode === 'changed' ? `PIN-${entry.id}` : basePin.id}, result})), stderr: Buffer.from('profile'), logs: []};
    },
    collectOwnedMemory: () => ({state: 'sampled_lower_bound'}), verifyPackProfile: () => ({}), derivedIdentity: () => ({sha256: 'same'}), proveWarmReuse: () => ({verified: true}),
    indexEvidence: () => ({path: 'records', bytes: 100, sha256: 'a'.repeat(64)}), captureGoCustody: (_home, _index, evidence) => ({directory: evidence, catalog: join(evidence, 'go-custody.json'), catalogSHA256: 'd'.repeat(64)}), openGoCustody() {},
    compareFacts(_homes, _outputs, pin, selected, evidence, _root, retained, custodyOptions) {
      assert.equal(custodyOptions.layout, request.layout);
      const index = exportCalls.length; exportCalls.push({pin, retained, custodyOptions}); events.push(`export:${index}`);
      if (fault.exportAt === index) throw Object.assign(new Error('exporter failure'), {handlesReaped: !fault.exportUnreaped, postcardEvidence: {receipt: 'retained-exporter-receipt'}});
      return {goCustody: {directory: evidence, catalog: join(evidence, 'go-custody.json'), catalogSHA256: 'd'.repeat(64)}, rustCustody: {directory: evidence, catalog: join(evidence, 'postcard-custody.json'), catalogSHA256: 'e'.repeat(64), candidate: candidateIdentity(request.candidate)},
        documents: selected.documents, fullFieldsAndOrderEqual: true, totals: {}, indexes: {go: {bytes: 100}, rust: {bytes: 100}}, exporterTiming: {exporterWrapperWallMillis: 9000}, validationWallMillis: 12000};
    },
    verifyAdditionalEvidence(report) { events.push('final-custody'); assert.equal(report.scenarios.length, 6); if (fault.finalCustody) throw new Error('late archived pack changed'); },
  };
  const config = {request, layout: request.layout, runnerRoot: root, inputs, conditions, scratch, evidence, ...fault.config};
  const report = await runPostcardFreshController(config, services); return {report, request, inputs, samples, exportCalls, events, owners, admissions, config, services};
}

test('explicit fresh v13 config rejects continuations, policy extras and wrong exporter source', t => {
  const directory = temporary(t), request = requestAt(directory); assert.equal(validatePostcardFreshRequest(request).layout, POSTCARD_LAYOUT);
  for (const change of [r => r.layout = 'v11-pack2', r => r.resumeGo = {}, r => r.allowance = {}, r => r.exporter.sourceTreeSHA256 = '0'.repeat(64), r => r.exporter.mode = 'automatic']) { const copy = structuredClone(request); change(copy); assert.throws(() => validatePostcardFreshRequest(copy)); }
  const build = {...request, exporter: {mode: 'build', artifactDirectory: join(directory, 'new-exporter')}}; assert.equal(validatePostcardFreshRequest(build).exporter.mode, 'build');
});
function compilerBuildBoundary(t, fault = {}) {
  const directory = temporary(t), request = requestAt(directory); request.exporter = {mode: 'build', artifactDirectory: join(directory, 'new-exporter')};
  putJSON(join(request.candidate.sourceRoot, 'release/tools/rust.json'), compilerPolicy);
  const binary = join(directory, 'rustc.exe'); put(binary, 'synthetic compiler bytes');
  const owners = [], events = [], environment = {PATH: 'synthetic controlled path'}, owner = ownerFactory(directory, owners, events);
  let builds = 0, observations = 0, verifies = 0, artifacts = 0;
  const services = {environment, owner,
    execute(command, args, options) {
      const i = observations++; events.push(`compiler:${i}`); assert.equal(command, 'rustc'); assert.deepEqual(args, ['-Vv']); assert.equal(options.cwd, request.candidate.sourceRoot); assert.deepEqual(options.env, environment); assert.equal(options.cargoBuild, false);
      let stdout = compilerText; if (fault.releaseAt === i) stdout = stdout.replaceAll('1.99.0', '1.98.0'); if (fault.changedAt === i) stdout = stdout.replace('1'.repeat(40), '2'.repeat(40));
      if (fault.changedBinaryAt === i) put(binary, 'different compiler bytes');
      if (fault.throwAt === i) throw new Error('compiler spawn failed');
      return {status: 0, handlesReaped: true, proof: {...proof(10), runId: `${i ? 'bbbbbbbb' : 'aaaaaaaa'}-bbbb-4ccc-8ddd-eeeeeeeeeeee`, binary, ...(fault.unreapedAt === i ? {treeReaped: false} : {}), ...(fault.flagsAt === i ? fault.flags : {})}, stdout: Buffer.from(stdout), stderr: Buffer.alloc(0)};
    },
    build(root, args, options) {
      builds++; events.push('cargo'); assert.equal(root, directory); assert.deepEqual(args, EXPORT_BUILD_ARGS); assert.equal(options.owner, 'source-postcard-exporter-build'); assert.equal(options.oneShot, false); assert.deepEqual(options.environment, environment);
      if (fault.buildThrows) throw new Error('unknown build retirement');
      assert.deepEqual(options.limits, assertDiskPolicy(directory, {}, services.allowance));
      putJSON(join(directory, 'build-receipt.json'), {admission: {limits: fault.buildLimits ? defaults : options.limits}});
      const target = join(directory, 'target'), effective = {...environment, CARGO_TARGET_DIR: target};
      return options.onSuccess({target, inputHashes: {sourceTree: request.candidate.sourceTreeSHA256}, environmentSHA256: fault.environment ? '0'.repeat(64) : postcardEnvironmentSHA(effective), buildEvidence: join(directory, 'build-receipt.json')});
    },
    seal(root, sourceRoot, directory, invocation, toolchain, options) {
      assert.deepEqual(options, {preparationPolicy: POSTCARD_PREPARATION_POLICY});
      artifacts++; events.push('artifact'); assert.equal(owners[0].name, TOOLCHAIN_OWNER); assert(owners[0].cleaned); assert.equal(hashFile(toolchain.record), toolchain.recordSHA256); assert.equal(hashFile(toolchain.receipt), toolchain.receiptSHA256);
      const record = JSON.parse(fs.readFileSync(toolchain.record)); assert.equal(record.state, 'verified'); assert.equal(record.observations.length, 2);
      const artifact = {binarySHA256: 'e'.repeat(64), inputHashes: invocation.inputHashes, toolchain, preparationPolicy: options.preparationPolicy}; fs.mkdirSync(directory); putJSON(join(directory, 'artifact.json'), artifact); return artifact;
    },
    verify(_root, identity, candidate, options) { verifies++; assert.equal(identity.sourceTreeSHA256, candidate.sourceTreeSHA256); assert.deepEqual(options, {requireCurrentPolicy: true, limits: assertDiskPolicy(directory, {}, services.allowance)}); return {artifactSHA256: identity.artifactSHA256}; },
  };
  return {request, services, owners, events, run: () => prepareFreshPostcardExporter(request, services), counts: () => ({builds, observations, verifies, artifacts})};
}
test('same-source exporter observes the fixed compiler before and after its one owned build, outside measurement', t => {
  const f = compilerBuildBoundary(t), prepared = f.run(); assert.deepEqual(f.counts(), {builds: 1, observations: 2, verifies: 1, artifacts: 1});
  assert(f.events.indexOf('compiler:0') < f.events.indexOf('cargo')); assert(f.events.indexOf('cargo') < f.events.indexOf('compiler:1')); assert(f.events.indexOf('compiler:1') < f.events.indexOf('artifact'));
  assert.equal(prepared.request.exporter.mode, 'existing'); assert.equal(prepared.preparation.built, true);
  prepareFreshPostcardExporter(prepared.request, {build() { assert.fail('existing artifact must not compile'); }, execute() { assert.fail('existing artifact uses sealed observations'); }, verify() { return {}; }});
});

test('default exporter preparation keeps inherited incremental setting and the default disk policy', t => {
  const f = compilerBuildBoundary(t); f.services.environment.CARGO_INCREMENTAL = '1';
  const prepared = f.run();
  assert.equal(prepared.preparation.built, true);
  assert.deepEqual(f.owners[0].options.limits, defaults);
});
test('compiler version, identity, environment, spawn and retirement failures cannot seal an exporter or clean failed owners', t => {
  for (const fault of [{releaseAt: 0}, {changedAt: 1}, {changedBinaryAt: 1}, {unreapedAt: 0}, {unreapedAt: 1}, {throwAt: 0}, {environment: true}, {buildThrows: true}, {flagsAt: 1, flags: {deadlineHit: true}}, {flagsAt: 1, flags: {cancelled: true}}, {flagsAt: 1, flags: {outputLimit: true}}]) {
    const f = compilerBuildBoundary(t, fault); assert.throws(f.run, error => {
      assert(error.toolchainEvidence); assert.equal(error.handlesReaped, !(fault.unreapedAt !== undefined || fault.throwAt !== undefined || fault.buildThrows)); return true;
    });
    assert.equal(f.counts().artifacts, 0); assert.equal(f.counts().builds, fault.releaseAt === 0 || fault.unreapedAt === 0 || fault.throwAt === 0 ? 0 : 1);
    assert.equal(f.owners[0].cleaned, false); assert.notEqual(f.owners[0].sealed.state, 'succeeded');
  }
});
test('compiler overrides are rejected before any owner, including case variants and Cargo wrapper aliases', () => {
  for (const key of ['RUSTC', 'rustc_wrapper', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER']) assert.throws(() => postcardCompilerEnvironment({[key]: 'replacement'}), /compiler override/);
  assert.deepEqual(postcardCompilerEnvironment({PATH: 'same', RUSTC: ''}), {PATH: 'same', RUSTC: ''});
});
test('missing pinned compiler policy or inherited compiler override fails before ownership and execution', t => {
  for (const kind of ['missing-policy', 'override']) {
    const f = compilerBuildBoundary(t);
    if (kind === 'missing-policy') fs.unlinkSync(join(f.request.candidate.sourceRoot, 'release/tools/rust.json'));
    else f.services.environment.RUSTC_WRAPPER = 'unapproved';
    assert.throws(f.run); assert.equal(f.owners.length, 0); assert.deepEqual(f.counts(), {builds: 0, observations: 0, verifies: 0, artifacts: 0});
  }
});
test('fresh v13 executes twelve product index calls in six scenarios and keeps exporter wall separate', async t => {
  const f = await controller(t); assert.equal(f.report.state, 'source_pack_scenarios_observed', f.report.blocker); assert.equal(f.samples.length, 12); assert.equal(f.exportCalls.length, 6);
  assert.deepEqual(f.report.scenarios.map(r => r.id), CASES.map(r => r.id)); assert.equal(f.report.layout, POSTCARD_LAYOUT); assert.equal(f.report.continuation, undefined);
  for (const row of f.report.scenarios) { assert.equal(row.go.controllableMillis, 100); assert.equal(row.rust.controllableMillis, 20); assert.equal(row.rustToGoRatio, 0.2); assert.equal(row.facts.exporterTiming.exporterWrapperWallMillis, 9000); }
  assert(f.samples.every(s => s.options.timeout === 210000 && s.options.memoryObservation)); assert(f.samples.filter(s => s.engine === 'rust').every(s => s.options.env.LYCHEEDEV_SOURCE_PHASE_PROFILE === '1'));
  for (let i = 0; i < 6; i++) assert(f.events.indexOf(`export:${i}`) > f.events.indexOf(`product:${CASES[i].id}:rust`));
  assert(f.events.indexOf('final-custody') < f.events.indexOf('cleanup:source-pack2-healthy-base')); assert(f.owners.every(owner => owner.cleaned));
  verifyObservedReport({status: 0, handlesReaped: true, proof: proof(10)}, f.report, f.request, f.inputs, undefined, undefined, undefined, {layout: POSTCARD_LAYOUT});
  assert.throws(() => verifyObservedReport({status: 0, handlesReaped: true, proof: proof(10)}, f.report, f.request, f.inputs));
});
for (const [name, fault, products] of [
  ['cold exporter failure', {exportAt: 0}, 2], ['warm exporter failure', {exportAt: 1}, 4], ['mutation exporter failure', {exportAt: 2}, 6],
  ['unreaped cold exporter', {exportAt: 0, exportUnreaped: true}, 2], ['unreaped mutation exporter', {exportAt: 2, exportUnreaped: true}, 6],
  ['changed product failure', {failAt: 4}, 5], ['unreaped product', {unreapedAt: 4}, 5], ['late changed archive', {finalCustody: true}, 12],
]) test(`${name} retains failed owners and stops every later product call`, async t => {
  const f = await controller(t, fault); assert.equal(f.report.state, 'source_pack_scenarios_failed'); assert.equal(f.samples.length, products); assert.equal(f.owners[0].cleaned, false);
  assert.equal(f.report.requiresOuterRetirement, Boolean(fault.exportUnreaped || fault.unreapedAt !== undefined));
  if (fault.exportUnreaped) assert(f.owners.filter(o => !o.cleaned).every(o => o.sealed.state === 'interrupted'));
});
test('fresh v13 rejects all historical resume modes before launching work', async t => {
  for (const config of [{resumeCold: {}}, {resumeRemaining: {}}, {resumeGo: {}}]) await assert.rejects(controller(t, {config}), /fresh scenarios/);
});
test('historical controller cannot relabel itself v13 without every authenticated comparison hook', async t => {
  const f = await controller(t);
  for (const key of ['runnerIdentity', 'compareFacts', 'verifyAdditionalIdentity', 'verifyAdditionalEvidence']) {
    const services = {...f.services, ownedSpawnSync() { assert.fail('no launch before explicit v13 hooks'); }}; delete services[key];
    await assert.rejects(runController(f.config, services), /authenticated exporter controller/);
  }
});
test('fresh report gate requires exact twelve calls and original complete scenario order', () => {
  const base = {layout: POSTCARD_LAYOUT, invocationAttempts: [], scenarios: [], importedSamples: 0}; assert.throws(() => verifyFreshPostcardReport(base, {}), /twelve/);
  const attempts = CASES.flatMap((row, i) => (i === 1 ? ['rust', 'go'] : ['go', 'rust']).map(engine => ({caseId: row.id, engine})));
  assert.throws(() => verifyFreshPostcardReport({...base, invocationAttempts: attempts.reverse()}, {}));
});
test('fresh extra comparison work keeps original explicit byte and runner identity bounds', () => {
  const estimate = postcardComparisonFootprint(); assert.equal(estimate.scratchReserveBytes, (512 + 128 + 8) * 2 ** 20); assert.equal(LIMITS.total, 128 << 20);
  assert.equal(Object.keys(freshPostcardIdentity(root)).length, FRESH_POSTCARD_FILES.length); assert(FRESH_POSTCARD_FILES.includes('tests/rust-performance-paired/read-fact-delta-postcard.mjs'));
});

async function nativeBoundary(t, callback, configure = () => {}) {
  await binaryFixture(t, async f => {
    Object.assign(f.build.candidate, {commit: '1'.repeat(40), artifactSHA256: f.build.expected.artifactSHA256, binarySHA256: f.build.expected.binarySHA256});
    configure(f);
    const go = await f.custody(), goManifest = JSON.parse(fs.readFileSync(join(dirname(f.records), 'manifest.json')));
    const retained = {reference: go.reference, index: {path: f.records, bytes: fs.lstatSync(f.records).size, sha256: hashFile(f.records), manifest: goManifest}}; go.recordsStream.destroy();
    const summary = goManifest.summary, pin = {schema: 'lycheedev.selection.v1', id: 'PIN-native', source: {repository: summary.repository, product: summary.product, exactCommit: summary.commit, requestedRef: summary.commit, parserRevision: summary.parser}};
    fs.mkdirSync(join(f.home, 'state')); const db = new DatabaseSync(join(f.home, 'state/toolkit.sqlite'));
    db.exec('PRAGMA user_version = 1; CREATE TABLE documents(key TEXT PRIMARY KEY, generation INTEGER, value BLOB); CREATE TABLE deleted_generations(key TEXT PRIMARY KEY, generation INTEGER);');
    db.prepare('INSERT INTO documents VALUES (?,1,?)').run(`pin/${pin.id}`, Buffer.from(JSON.stringify(pin))); db.prepare('INSERT INTO documents VALUES (?,1,?)').run(`source/rust-index/${f.index.manifest.identity}`, Buffer.from(JSON.stringify(f.index.manifest))); db.close();
    const envelope = {schema: 'lycheedev.result.v1', ok: true, error: null, context: {snapshot: pin.id}, result: {schema: 'lycheedev.source-map.v2', storage: 'file-cache', ...summary, assets: 0, complete: true}};
    const owners = [], events = [], begin = ownerFactory(f.root, owners, events);
    const run = () => comparePostcardScenario({go: f.goHome, rust: f.home}, {go: envelope, rust: envelope}, pin, summary, f.evidence, f.root, retained, {candidate: f.build.candidate, limits: f.limits ?? {}}, f.build.expected, {beginScratch: begin, ownedSpawnSync: f.execute});
    await callback(Object.assign(f, {owners, events, run}));
  });
}
test('real Node six-scenario comparison port archives complete native-format input/output and survives source cleanup', t => nativeBoundary(t, async f => {
  // Changed scenarios retire the completed Go owner before the Rust call.
  fs.rmSync(f.goHome, {recursive: true});
  const result = await f.run(); assert.equal(f.calls(), 1); assert.equal(result.documents, 3); assert.equal(f.owners.length, 1); assert(f.owners[0].cleaned);
  fs.rmSync(f.home, {recursive: true}); verifyFreshPostcardFacts(result, f.build.candidate, f.build.expected);
  const catalog = JSON.parse(fs.readFileSync(result.rustCustody.catalog)); assert.equal(hashFile(catalog.manifestArchive.path), catalog.summary.manifestSHA256);
}));
test('full JSONL above 16 MiB remains streamed in existing parts and every sealed file stays bounded', t => nativeBoundary(t, async f => {
  const result = await f.run(), catalog = JSON.parse(fs.readFileSync(result.rustCustody.catalog)), output = catalog.references.at(-1);
  assert(result.exportSummary.outputBytes > 16 << 20); assert(result.exportSummary.outputBytes <= LIMITS.total);
  assert.equal(output.manifest.parts.length, 3); assert.equal(output.manifest.partBytes, 8 << 20);
  assert(f.owners[0].sealed.logs.every(path => fs.lstatSync(path).size <= 16 << 20));
  fs.rmSync(f.home, {recursive: true}); fs.rmSync(f.goHome, {recursive: true}); verifyFreshPostcardFacts(result, f.build.candidate, f.build.expected);
}, f => { for (const facts of f.facts) facts.headers[0].value = 'x'.repeat(6 << 20); f.saveGoFacts(); }));
test('real comparison port retains partial exporter output and propagates unreaped lifecycle', async t => {
  await nativeBoundary(t, async f => { f.proofOverrides = {treeReaped: false}; await assert.rejects(f.run(), error => error.handlesReaped === false && Boolean(error.postcardEvidence)); assert.equal(f.owners[0].cleaned, false); assert.equal(f.owners[0].sealed.state, 'interrupted'); });
  await nativeBoundary(t, async f => { f.mutateRows = rows => rows.pop(); await assert.rejects(f.run(), error => error.handlesReaped === true); assert.equal(f.owners[0].sealed.state, 'failed'); assert.equal(f.owners[0].cleaned, false); assert(fs.existsSync(join(f.owners[0].evidence, 'postcard-failed-output-parts/manifest.json'))); });
});
test('final archived Facts verifier rejects a changed pack, JSONL, original manifest, index, comparison, owner or exporter identity', async t => {
  for (const change of ['pack', 'output', 'manifest', 'index', 'comparison', 'owner', 'exporter']) await nativeBoundary(t, async f => {
    const result = await f.run(), catalog = JSON.parse(fs.readFileSync(result.rustCustody.catalog));
    if (change === 'exporter') { assert.throws(() => verifyFreshPostcardFacts(result, f.build.candidate, {...f.build.expected, binarySHA256: '0'.repeat(64)})); return; }
    const path = change === 'comparison' ? result.evidence : change === 'owner' ? result.postcardOwner.receipt : change === 'manifest' ? catalog.manifestArchive.path : change === 'index' ? result.indexCustody.manifest : join(catalog.references[change === 'output' ? catalog.references.length - 1 : 0].directory, 'index.part000.bin');
    fs.appendFileSync(path, 'changed'); assert.throws(() => verifyFreshPostcardFacts(result, f.build.candidate, f.build.expected));
  });
});

test('explicit binary Index18 route reaches all six fresh comparisons with twelve unchanged measured product calls', async t => {
  const f = await controller(t, {layout: POSTCARD_INDEX_LAYOUT}); assert.equal(f.report.state, 'source_pack_scenarios_observed');
  assert.equal(f.samples.length, 12); assert.equal(f.exportCalls.length, 6); assert.equal(f.report.layout, POSTCARD_INDEX_LAYOUT);
  assert.equal(validatePostcardFreshRequest(f.request).layout, POSTCARD_INDEX_LAYOUT);
  assert.deepEqual(f.samples.map(s => [s.entry.id, s.engine]), CASES.flatMap((c, i) => (i === 1 ? ['rust', 'go'] : ['go', 'rust']).map(e => [c.id, e])));
  for (const row of f.report.scenarios) assert.equal(row.rustToGoRatio, 0.2);
});


test('fresh CLI accepts one trailing exact 6 GiB flag and rejects every other argument shape', () => {
  assert.deepEqual(parsePostcardFreshArguments(['config', 'conditions']), {configuration: 'config', conditions: 'conditions', scratch6GiB: false});
  assert.deepEqual(parsePostcardFreshArguments(['config', 'conditions', '--scratch-6gib']), {configuration: 'config', conditions: 'conditions', scratch6GiB: true});
  for (const args of [[], ['config'], ['config', 'conditions', '--scratch-7gib'], ['config', 'conditions', '--scratch-6gib', '--scratch-6gib'], ['--scratch-6gib', 'config', 'conditions'], ['config', 'conditions', 'extra'], ['config', 'conditions', '--scratch-6gib', 'extra']]) assert.throws(() => parsePostcardFreshArguments(args), /usage/);
});
for (const layout of [POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT]) for (const explicit of [false, true]) test(`${layout} ${explicit ? '6' : '5'} GiB propagates through every scenario owner, post-admission and custody`, async t => {
  const f = await controller(t, {layout, config: explicit ? {allowance: FRESH_ALLOWANCE} : {}});
  assert.equal(f.report.state, 'source_pack_scenarios_observed', f.report.blocker);
  const expected = {...defaults, scratchBytes: (explicit ? 6 : 5) * 1024 ** 3}; assert.deepEqual(f.report.diskPolicy, expected);
  assert.equal(f.owners.length, 9); assert.equal(f.admissions.length, 9); assert.equal(f.exportCalls.length, 6);
  for (const owner of f.owners) { assert.deepEqual(owner.options.limits, expected); assert.deepEqual(JSON.parse(fs.readFileSync(owner.receipt)).admission.limits, expected); }
  for (const options of f.admissions) assert.deepEqual(options.limits, expected);
  for (const call of f.exportCalls) assert.deepEqual(call.custodyOptions.limits, expected);
  assert.equal(f.config.runnerRoot, root); assert.notEqual(f.config.runnerRoot, f.request.candidate.sourceRoot);
});
test('fresh controller and exporter preparation reject altered or falsy allowance and disk environment overrides before ownership', async t => {
  const variants = [null, false, 0, {}, {...FRESH_ALLOWANCE, scratchBytes: 7 * 1024 ** 3}, {...FRESH_ALLOWANCE, scope: 'other'}, {...FRESH_ALLOWANCE, authorizationReference: 'forged'}, {...FRESH_ALLOWANCE, freeReserveBytes: 0}];
  for (const allowance of variants) {
    await assert.rejects(controller(t, {config: {allowance}}), /authorized task-local/);
    const f = compilerBuildBoundary(t); f.services.allowance = allowance; assert.throws(f.run, /authorized task-local/); assert.equal(f.owners.length, 0);
  }
  for (const key of ['LYCHEEDEV_DISK_SCRATCH_GIB', 'lycheedev_disk_free_gib', 'LYCHEEDEV_DISK_UNKNOWN']) {
    const f = compilerBuildBoundary(t); f.services.environment[key] = '6'; f.services.allowance = FRESH_ALLOWANCE;
    assert.throws(f.run, /disk policy overrides/); assert.equal(f.owners.length, 0);
  }
});
test('fresh explicit allowance reaches compiler and Cargo owners; mismatched Cargo receipt cannot seal exporter', t => {
  const f = compilerBuildBoundary(t); f.services.allowance = FRESH_ALLOWANCE; const prepared = f.run();
  assert.equal(prepared.preparation.built, true); assert.deepEqual(f.owners[0].options.limits, {...defaults, scratchBytes: 6 * 1024 ** 3});
  const broken = compilerBuildBoundary(t, {buildLimits: true}); broken.services.allowance = FRESH_ALLOWANCE;
  assert.throws(broken.run, /Cargo admission differs/); assert.equal(broken.counts().artifacts, 0); assert.equal(broken.owners[0].cleaned, false);
});
test('inner receipt gate binds exact allowance, policy, runner and command without modifying the default policy', t => {
  for (const explicit of [false, true]) {
    const directory = temporary(t), inputs = {runner: 'fixture'}, allowance = explicit ? FRESH_ALLOWANCE : undefined, limits = assertDiskPolicy(directory, {}, allowance);
    const command = ['source-postcard-fresh-diagnostics', join(directory, 'configuration'), join(directory, 'conditions'), ...(explicit ? ['--scratch-6gib'] : [])];
    const job = beginScratch(directory, 'source-postcard-fresh-diagnostics', {command, inputs: {runner: inputs, ...(allowance ? {allowance} : {})}, limits});
    const config = {request: {ownerRoot: directory}, inputs, scratch: job.directory, evidence: job.evidence, ...(allowance ? {allowance} : {})};
    assert.deepEqual(validateFreshPostcardOwner(config, {}), limits);
    const original = JSON.parse(fs.readFileSync(job.receipt));
    for (const mutate of [r => r.inputHashes.allowance = {...FRESH_ALLOWANCE, scope: 'forged'}, r => r.admission.limits.scratchBytes = 7 * 1024 ** 3, r => r.admission.limits.freeReserveBytes = 0, r => r.inputHashes.runner = {}, r => r.command.push('--scratch-6gib')]) {
      const changed = structuredClone(original); mutate(changed); putJSON(job.receipt, changed); assert.throws(() => validateFreshPostcardOwner(config, {}));
    }
    putJSON(job.receipt, original);
    const mismatch = {...config, allowance: explicit ? undefined : FRESH_ALLOWANCE}; assert.throws(() => validateFreshPostcardOwner(mismatch, {}), /allowance receipt/);
    assert.throws(() => validateFreshPostcardOwner(config, {LYCHEEDEV_DISK_SCRATCH_GIB: '6'}), /overrides/);
    job.seal({state: 'succeeded', handlesReaped: true}); job.cleanup(); assert.deepEqual(assertDiskPolicy(directory, {}), {...defaults});
  }
});


test('actual exporter preparation crosses compiler and Cargo admissions above 5 GiB and reuses same-source artifact without building', t => {
  const directory = temporary(t), request = requestAt(directory), source = request.candidate.sourceRoot;
  for (const name of ['crates', '.cargo']) fs.mkdirSync(join(source, name));
  put(join(source, 'Cargo.toml'), '[workspace]'); put(join(source, 'Cargo.lock'), '# synthetic'); putJSON(join(source, 'release/tools/rust.json'), compilerPolicy);
  request.candidate.sourceTreeSHA256 = sourceDigest(source); request.exporter = {mode: 'build', artifactDirectory: join(directory, 'new-exporter')};
  const compiler = join(directory, 'rustc.exe'); put(compiler, 'synthetic compiler, never launched');
  const protectedPath = join(directory, '.tmp/rust-scratch/protected/logical-size'); put(protectedPath, 'x');
  // A one-byte file with a synthetic inventory size avoids Windows allocation.
  const originalStat = fs.lstatSync, statMock = t.mock.method(fs, 'lstatSync', (...args) => { const stat = originalStat(...args); if (args[0] === protectedPath) stat.size = defaults.scratchBytes + 1; return stat; }); syncBuiltinESMExports();
  try {
  let compilerCalls = 0, cargoCalls = 0; const environment = {...process.env};
  const services = {environment,
    execute(command, args, options) { assert.equal(command, 'rustc'); assert.deepEqual(args, ['-Vv']); assert.equal(options.cwd, source); compilerCalls++;
      return {status: 0, handlesReaped: true, stdout: Buffer.from(compilerText), stderr: Buffer.alloc(0), proof: {...proof(10), binary: compiler, runId: `${compilerCalls === 1 ? 'aaaaaaaa' : 'bbbbbbbb'}-bbbb-4ccc-8ddd-eeeeeeeeeeee`}}; },
    build(root, args, options) { return runCargo(root, args, {...options, execute(command, cargoArgs, invocation) { cargoCalls++; assert.equal(command, 'cargo'); assert.deepEqual(cargoArgs, EXPORT_BUILD_ARGS); assert.equal(invocation.cwd, source); return {status: 0, handlesReaped: true, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0)}; }}); },
    seal(_root, sourceRoot, destination, invocation, toolchain) { assert.equal(sourceRoot, source); const artifact = {binarySHA256: 'e'.repeat(64), inputHashes: invocation.inputHashes, buildEvidence: invocation.buildEvidence, toolchain}; fs.mkdirSync(destination); putJSON(join(destination, 'artifact.json'), artifact); return artifact; },
    verify(_root, identity, candidate) { assert.equal(identity.sourceTreeSHA256, candidate.sourceTreeSHA256); return {}; },
  };
  assert.throws(() => prepareFreshPostcardExporter(request, services), /disk.scratch_quota/); assert.equal(compilerCalls + cargoCalls, 0);
  const prepared = prepareFreshPostcardExporter(request, {...services, allowance: FRESH_ALLOWANCE}), artifact = JSON.parse(fs.readFileSync(prepared.request.exporter.artifact));
  assert.equal(compilerCalls, 2); assert.equal(cargoCalls, 1);
  for (const path of [artifact.buildEvidence, artifact.toolchain.receipt]) { const receipt = JSON.parse(fs.readFileSync(path)); assert.equal(receipt.state, 'retired'); assert.deepEqual(receipt.admission.limits, {...defaults, scratchBytes: 6 * 1024 ** 3}); assert.equal(receipt.inputHashes.sourceTree, request.candidate.sourceTreeSHA256); }
  assert.equal(fs.statSync(protectedPath).size, 1); assert.equal(sourceDigest(source), request.candidate.sourceTreeSHA256);
  assert.notEqual(source, root); assert(!fs.existsSync(join(directory, '.tmp/rust-disk/channels/ordinary.lease')));
  prepareFreshPostcardExporter(prepared.request, {...services, allowance: FRESH_ALLOWANCE, build() { assert.fail('sealed exporter reuse must not compile'); }, execute() { assert.fail('sealed exporter reuse must not observe compiler'); }});
  assert.equal(compilerCalls, 2); assert.equal(cargoCalls, 1);
  } finally { statMock.mock.restore(); syncBuiltinESMExports(); }
});
test('actual Go custody admission records the unchanged exact 6 GiB policy', t => binaryFixture(t, async f => {
  const directory = join(f.root, 'new-go-custody'); fs.mkdirSync(directory); const limits = {...defaults, scratchBytes: 6 * 1024 ** 3};
  const index = {path: f.records, bytes: fs.statSync(f.records).size, sha256: hashFile(f.records), manifest: JSON.parse(fs.readFileSync(join(dirname(f.records), 'manifest.json')))};
  splitIndexEvidence(f.root, {path: index.path, bytes: index.bytes, sha256: index.sha256}, join(directory, 'go-index-parts'));
  const reference = await captureComparedGo(f.goHome, index, directory, f.root, {limits}); assert.equal(reference.directory, directory);
  const receipt = JSON.parse(fs.readFileSync(join(directory, 'go-custody-admission.json'))); assert.deepEqual(receipt.admission.limits, limits); assert.equal(receipt.admission.scratchReserveBytes, 0);
}));
test('actual Postcard custody owner records 6 GiB and rejects a rebound mismatched policy receipt', t => nativeBoundary(t, async f => {
  const result = await f.run(); assert.deepEqual(JSON.parse(fs.readFileSync(result.postcardOwner.receipt)).admission.limits, f.limits);
  verifyFreshPostcardFacts(result, f.build.candidate, f.build.expected, POSTCARD_LAYOUT, f.limits);
  const receipt = JSON.parse(fs.readFileSync(result.postcardOwner.receipt)); receipt.admission.limits.scratchBytes = defaults.scratchBytes; putJSON(result.postcardOwner.receipt, receipt); result.postcardOwner.receiptSHA256 = hashFile(result.postcardOwner.receipt);
  assert.throws(() => verifyFreshPostcardFacts(result, f.build.candidate, f.build.expected, POSTCARD_LAYOUT, f.limits), /custody admission differs/);
}, f => { f.limits = {...defaults, scratchBytes: 6 * 1024 ** 3}; }));


test('final fresh report rejects a changed scenario-owner admission before accepting custody', async t => {
  const f = await controller(t, {config: {allowance: FRESH_ALLOWANCE}}), receipt = JSON.parse(fs.readFileSync(f.owners[0].receipt));
  receipt.admission.limits.scratchBytes = defaults.scratchBytes; putJSON(f.owners[0].receipt, receipt);
  await assert.rejects(async () => verifyFreshPostcardReport(f.report, f.request, FRESH_ALLOWANCE), /scenario owner admission differs/);
});


function actualReceiptOwners(directory, limits) {
  return [{role: 'healthy-base'}, ...CASES.slice(2).flatMap(row => ['go', 'rust'].map(engine => ({role: row.id, engine})))].map((row, i) => {
    const job = beginScratch(directory, i === 0 ? 'source-pack2-healthy-base' : `source-pack2-${row.role}-${row.engine}`, {command: ['source', 'index'], limits});
    const logs = [];
    // Actual rust-disk writer, unique real files and both generated log indexes.
    // This matches the fan-out that made the Windows Go owner exceed 1 MiB.
    for (let n = 0; n < (i === 1 ? 2200 : 1); n++) { const path = join(job.evidence, `${String(n).padStart(4, '0')}-${'e'.repeat(160)}.json`); putJSON(path, {ordinal: n}); logs.push(path); }
    job.seal({state: 'succeeded', handlesReaped: true, logs}); job.cleanup();
    return {...row, receipt: job.receipt};
  });
}
test('real rust-disk receipt above 1 MiB passes the complete fresh final custody gate at the bounded 16 MiB ceiling', t => nativeBoundary(t, async f => {
  const result = await f.run(), owners = actualReceiptOwners(f.root, f.limits), path = owners[1].receipt;
  assert(fs.statSync(path).size > 1 << 20); assert(fs.statSync(path).size <= FRESH_OWNER_RECEIPT_BYTES);
  const report = {layout: POSTCARD_LAYOUT, importedSamples: 0, diskPolicy: f.limits, owners,
    invocationAttempts: CASES.flatMap((row, i) => (i === 1 ? ['rust', 'go'] : ['go', 'rust']).map(engine => ({caseId: row.id, engine}))),
    scenarios: CASES.map(row => ({id: row.id, facts: result}))};
  const request = {ownerRoot: f.root, candidate: f.build.candidate, exporter: {mode: 'existing', ...f.build.expected}, layout: POSTCARD_LAYOUT};
  const digest = freshPostcardReceiptSHA256(f.root, path), original = fs.readFileSync(path);
  assert.equal(readFreshPostcardReceipt(f.root, path, digest).state, 'retired');
  assert.equal(verifyFreshPostcardReport(report, request), true);
  for (const [mutate, error] of [
    [v => v.owner = 'foreign', /scenario owner identity/],
    [v => v.schema = 'unknown', /lycheedev.disk-owner/],
    [v => v.admission.limits.scratchBytes++, /scenario owner admission/],
  ]) { const value = JSON.parse(original); mutate(value); putJSON(path, value); assert.throws(() => verifyFreshPostcardReport(report, request), error); }
  fs.writeFileSync(path, original); assert.throws(() => readFreshPostcardReceipt(f.root, path, '0'.repeat(64)), /receipt SHA changed/);
  fs.writeFileSync(path, Buffer.concat([original, Buffer.alloc(FRESH_OWNER_RECEIPT_BYTES - original.length, 32)]));
  assert.equal(readFreshPostcardReceipt(f.root, path).state, 'retired'); assert.equal(verifyFreshPostcardReport(report, request), true);
  fs.truncateSync(path, FRESH_OWNER_RECEIPT_BYTES + 1);
  assert.throws(() => verifyFreshPostcardReport(report, request), error => error.message.includes(path) && error.message.includes('16777216 byte ceiling'));
  fs.writeFileSync(path, original); assert.equal(freshPostcardReceiptSHA256(f.root, path), digest);
}, f => { f.limits = {...defaults}; }));
test('fresh receipt reader rejects reparse leaves, reparse ancestors and nonregular files', t => {
  const directory = temporary(t), actual = join(directory, 'actual'), receipt = join(actual, 'receipt.json'); fs.mkdirSync(actual); putJSON(receipt, {schema: 'lycheedev.disk-owner.v1'});
  assert.throws(() => readFreshPostcardReceipt(directory, actual), /receipt.*ceiling/);
  try { fs.symlinkSync(actual, join(directory, 'alias'), 'junction'); fs.symlinkSync(receipt, join(directory, 'linked.json'), 'file'); }
  catch (error) { if (['EPERM', 'EACCES'].includes(error.code)) { t.skip('Windows symbolic-link privilege is unavailable'); return; } throw error; }
  for (const path of [join(directory, 'alias/receipt.json'), join(directory, 'linked.json')]) assert.throws(() => readFreshPostcardReceipt(directory, path), /reparse_boundary/);
});
