// Only launched inside the existing non-breakaway outer owned Windows Job.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {createRequire} from 'node:module';
import {performance} from 'node:perf_hooks';
import os from 'node:os';
import {beginScratch} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {collectOwnedMemory} from '../../tools/rust-owned-memory.mjs';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {inspectEmmy} from '../../tools/rust-emmy-identity.mjs';
import {elapsed} from './contract.mjs';
import {cloneSeed, inventory, verifyGoMetadata, verifyGoPackage} from './inputs.mjs';
import {GO_COMMIT, REPETITIONS, TARGET_RATIO, compareData, dataCases, orderFor, summarizeData} from './data-contract.mjs';
import {decodeDataImage, decodedIdentity, digest, loadDataFixture, parse, seedDataHome, validateDataOutput} from './data-inputs.mjs';
import {verifyDataContinuation} from './data-resume.mjs';

assert(process.argv.length === 3 && process.platform === 'win32');
const config = parse(readFileSync(process.argv[2])), root = resolve(config.root), runnerRoot = resolve(config.runnerRoot), out = resolve(config.evidence), start = performance.now();
assert(!config.resumeFrom || !config.acceptance);
let fatal = false;
const report = {schema: 'lycheedev.paired-data.v1', state: 'running', startedAt: new Date().toISOString(), performanceAcceptanceEvaluated: config.acceptance,
  fullB04Acceptance: false, targetRatio: TARGET_RATIO, scope: ['tiny synthetic SQL correctness only', '17 MiB three-chunk BLTE inspect', 'five 1031x519 BLP2 storage modes, PNG/WebP default export'],
  cacheDefinition: 'Independent real CLI init per engine/fixture, then imported identical authenticated synthetic pin/DBD documents and blobs. No discovery/selection/sync performance claim. New application decoded-cache-cold home per engine/path/pair; warm repeats its own successful cold, retaining actual decoded entries and checks. OS caches are not flushed.',
  timingDefinition: 'Existing native monotonic CreateProcess-to-root/tree-reap-and-output-drain clock. All product validation/hash/decode/encode/capture/output/cleanup work stays included. Fixture construction, independent audits and decoder verification are separately recorded, never subtracted from a product command.',
  metrics: {cpu: 'not_measured', io: 'not_measured', memory: 'existing identity-deduplicated sampled owned Job working set/private bytes with gaps; not unique physical RSS; do not sum outer and inner Jobs'},
  remaining: ['representative SQL throughput', 'real-client inputs', 'raw export', 'Hotfix/effective/remote paths', 'full accounting and B04 acceptance'],
  machine: {hostname: os.hostname(), platform: process.platform, arch: process.arch, release: os.release(), node: process.version, cpus: os.cpus(), totalMemory: os.totalmem(), conditions: config.conditions},
  inputs: {go: config.go, rust: config.rust}, preparation: [], preflightPairs: [], pairs: [], pairOwners: [], groups: []};
const save = () => writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
const files = path => readdirSync(path).filter(name => name !== 'receipt.json' && lstatSync(join(path, name)).isFile()).map(name => join(path, name));
function controlled(binary, args, evidence, measured = false, env = process.env) {
  assert(!fatal, 'unresolved prior owned process'); fatal = true;
  const began = performance.now(), run = ownedSpawnSync(binary, args, {cwd: root, env, evidence, timeout: 330000, orphanMs: 30000, memoryObservation: measured});
  fatal = run.handlesReaped !== true;
  const row = {binary, args, wallMillis: performance.now() - began, exit: run.status, error: run.error?.message, handlesReaped: run.handlesReaped, proof: run.proof,
    logs: run.logs, stdoutSHA256: digest(run.stdout), stderrSHA256: digest(run.stderr), stdoutBytes: run.stdout.length, stderrBytes: run.stderr.length};
  if (measured) row.memory = collectOwnedMemory(run, {evidence, sourceBinary: binary, runState: run.status === 0 && !run.error ? 'passed_invocation' : 'failed_invocation'});
  return {run, row};
}
function prepare(binary, args, label, env) {
  const {run, row} = controlled(binary, args, out, false, env); report.preparation.push({label, ...row}); save();
  assert(run.handlesReaped && !run.error && run.status === 0, `${label}: failed owned preparation`); return run.stdout;
}
const gitEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE'));
Object.assign(gitEnv, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: 'NUL', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1'});
function identities() {
  assert.equal(prepare('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-C', config.goSource, 'rev-parse', 'HEAD'], 'Go323 commit', gitEnv).toString().trim(), GO_COMMIT);
  assert.equal(prepare('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', '-C', config.goSource, 'status', '--porcelain', '--untracked-files=all'], 'Go323 clean checkout', gitEnv).toString().trim(), '');
  assert.equal(verifyGoPackage(config.go.root, config.luaLSArchive).packageSHA256, config.go.packageSHA256);
  const rust = verifyArtifact(root, config.rustArtifact); assert(['release', 'symbols'].includes(rust.profile)); assert.equal(rust.binarySHA256, config.rust.binarySHA256); assert.equal(sourceDigest(root), rust.inputHashes.sourceTree);
  const emmy = inspectEmmy(root); assert.equal(emmy.identity, 'public-upstream-unmodified-v1'); assert.equal(emmy.patchedTreeSHA256, rust.inputHashes.emmy.patchedTreeSHA256);
  const channel = parse(readFileSync(join(root, 'release/npm-channel.json'))); assert.equal(channel.version, '3.5.0'); assert.equal(channel.distTag, 'beta');
  for (const [path, sha] of Object.entries(config.inputs)) assert.equal(hashFile(join(runnerRoot, path)), sha, 'executing runner changed');
}

let entries = [], fixtures = {}, sharp, decoderModules;
save();
try {
  identities(); report.inputs.go.build = verifyGoMetadata(prepare('go', ['version', '-m', config.go.binary], 'Go323 executable metadata').toString());
  report.machine.powerBefore = prepare('powercfg.exe', ['/getactivescheme'], 'power before').toString().trim();
  assert(!process.env.RUSTFLAGS && !process.env.CARGO_ENCODED_RUSTFLAGS);
  report.machine.toolchains = {nodeBinarySHA256: hashFile(process.execPath), go: prepare('go', ['version'], 'Go toolchain').toString(), rustc: prepare('rustc', ['-Vv'], 'Rust toolchain').toString()};
  assert.match(report.machine.toolchains.rustc, /^rustc 1\.99\.0\b/);
  const require = createRequire(import.meta.url), entryPath = require.resolve(config.sharpModule);
  sharp = require(entryPath); assert(typeof sharp === 'function' && sharp.versions?.sharp && sharp.versions?.vips);
  const decoderCache = sharp.cache(false);
  for (const key of ['memory', 'files', 'items']) assert.equal(decoderCache[key].max, 0, 'independent decoder cache must be disabled');
  const seen = new Set(); function loaded(module) { if (!module || seen.has(module.filename)) return; seen.add(module.filename); for (const child of module.children) loaded(child); }
  loaded(require.cache[entryPath]); decoderModules = Object.fromEntries([...seen].sort().map(path => [path, hashFile(path)]));
  report.decoder = {entryPath, versions: sharp.versions, loadedModuleSHA256: decoderModules, cache: decoderCache,
    inputPolicy: 'already authenticated bounded encoded Buffer, no scratch filename; pipeline destroyed after completed decode, operation/file cache disabled',
    scope: 'existing supplied sharp; no installation; module and loaded native-addon files hashed; operating-system shared libraries not claimed fully inventoried'};
  const begun = performance.now();
  fixtures = Object.fromEntries(Object.entries(config.fixtures).map(([kind, path]) => [kind, loadDataFixture(path, kind)]));
  entries = dataCases(fixtures.images.images); report.inputs.fixtures = Object.fromEntries(Object.entries(fixtures).map(([kind, fixture]) => [kind, fixture.receipt]));
  report.preparation.push({label: 'independent fixture/CKey/EKey/BLTE/DBD/pixel audit', wallMillis: performance.now() - begun});
  if (config.resumeFrom) {
    const continuation = verifyDataContinuation(root, config.resumeFrom, {config, report, entries});
    report.continuation = continuation.evidence; entries = continuation.selected; save();
  }
  const seeds = {};
  for (const [kind, fixture] of Object.entries(fixtures)) for (const engine of ['go', 'rust']) {
    const home = join(config.scratch, `${kind}-${engine}-seed`), binary = engine === 'go' ? config.go.binary : config.rust.sourceBinary;
    assert(!existsSync(home)); const init = parse(prepare(binary, ['init', '--home', home, '--format', 'json'], `${kind}-${engine} init`)); assert.equal(init.ok, true);
    seeds[`${kind}-${engine}`] = {home, identity: seedDataHome(fixture, home)};
    assert.deepEqual(decodedIdentity(home), []);
  }
  report.inputs.seeds = Object.fromEntries(Object.entries(seeds).map(([key, seed]) => [key, {home: seed.home, sha256: seed.identity.sha256}])); save();

  async function pair(entry, iteration, preflight) {
    const fixture = fixtures[entry.fixture], job = beginScratch(root, `paired-data-${entry.id}-${preflight ? 'preflight' : iteration}`, {command: entry.args,
      inputs: {go: config.go.binarySHA256, rust: config.rust.binarySHA256, fixture: fixture.receipt.inventory.sha256}, admissionOptions: {requiredBytes: 256 << 20, scratchReserveBytes: 256 << 20}});
    const ownership = {caseId: entry.id, iteration, preflight, receipt: job.receipt}; report.pairOwners.push(ownership);
    const destination = preflight && config.acceptance && entry.performance ? report.preflightPairs : report.pairs;
    const rows = []; let valid = false, phase = 'preparation', terminalFailure;
    try {
      assert.equal(inventory(fixture.directory, {maxFiles: 256, maxBytes: 64 << 20}).sha256, fixture.receipt.inventory.sha256);
      const homes = Object.fromEntries(['go', 'rust'].map(engine => [engine, join(job.directory, engine)]));
      for (const engine of ['go', 'rust']) { const seed = seeds[`${entry.fixture}-${engine}`]; cloneSeed(seed.home, homes[engine], seed.identity); assert.deepEqual(decodedIdentity(homes[engine]), []); }
      phase = 'samples';
      for (const cacheMode of ['cold', 'warm']) {
        const row = {caseId: entry.id, iteration, cacheMode, preflight, order: orderFor(iteration), resultEquivalent: false, fixtureSHA256: fixture.receipt.inventory.sha256}; rows.push(row); destination.push(row); save();
        const values = {};
        for (const engine of row.order) {
          const home = homes[engine], binary = engine === 'go' ? config.go.binary : config.rust.sourceBinary;
          const output = entry.kind === 'image' ? join(job.directory, `${engine}-${cacheMode}.${entry.encoding}`) : null;
          const before = decodedIdentity(home); assert.equal(before.length, cacheMode === 'cold' ? 0 : 2);
          const args = [...entry.args, ...(output ? ['--output', output] : []), '--snapshot', fixture.identity.snapshot, '--installation', fixture.identity.installation, '--home', home, '--format', 'json', '--offline'];
          const {run, row: sample} = controlled(binary, args, job.evidence, true);
          row[engine] = {...sample, home, snapshot: fixture.identity.snapshot, valid: false, excludedDownloadMillis: 0, excludedExternalWaitMillis: 0}; save();
          try {
            assert(run.handlesReaped && !run.error && run.status === 0); assert.equal(run.stderr.toString(), ''); Object.assign(row[engine], elapsed(run.proof));
            const auditStarted = performance.now(), result = await validateDataOutput({entry, fixture, engine, cacheMode, envelope: parse(run.stdout), home, output,
              decodeImage: bytes => decodeDataImage(sharp, bytes), evidence: job.evidence});
            const after = decodedIdentity(home); assert.equal(after.length, 2); if (cacheMode === 'warm') assert.deepEqual(after, before, 'warm cache metadata/source identities changed');
            Object.assign(row[engine], {valid: true, reuse: result.observations.reuse, observations: result.observations, captures: result.proofs, auditMillis: performance.now() - auditStarted});
            values[engine] = result.value;
          } catch (error) { row[engine].failure = error.message.slice(0, 4096); }
          save(); if (fatal) throw new Error('unverified process retirement');
        }
        try { assert(row.go.valid && row.rust.valid); row.resultEquivalent = compareData(values.go, values.rust); }
        catch (error) { row.failure = error.message.slice(0, 4096); }
        save(); if (!row.resultEquivalent) break;
      }
      phase = 'final_integrity';
      assert.equal(inventory(fixture.directory, {maxFiles: 256, maxBytes: 64 << 20}).sha256, fixture.receipt.inventory.sha256);
      valid = rows.length === 2 && rows.every(row => row.resultEquivalent);
    } catch (error) { terminalFailure = {phase, message: error.message.slice(0, 4096)}; report.preparation.push({caseId: entry.id, iteration, preflight, failure: terminalFailure.message}); }
    finally {
      const pairTerminal = {state: valid ? 'verified' : terminalFailure?.phase === 'final_integrity' ? 'failed_final_integrity' : 'failed_pair', ...(terminalFailure ? {failure: terminalFailure} : {})};
      ownership.pairTerminal = pairTerminal;
      for (const row of rows) row.pairTerminal = pairTerminal;
      writeFileSync(join(job.evidence, 'pair.json'), JSON.stringify({caseId: entry.id, iteration, preflight, valid, rows}, null, 2) + '\n');
      job.seal({state: fatal ? 'interrupted' : valid ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: files(job.evidence)});
      if (!fatal && valid) {
        try { job.cleanup(); ownership.cleanup = {state: 'retired'}; }
        catch (error) { ownership.cleanup = {state: 'failed_retained', code: error.code, syscall: error.syscall, path: error.path, message: error.message}; save(); throw error; }
      }
      save();
    }
    return valid;
  }
  for (const entry of entries) {
    const verified = await pair(entry, 0, true); assert(!fatal, 'outer retirement required');
    if (!verified || !config.acceptance || !entry.performance) continue;
    for (let iteration = 0; iteration < REPETITIONS; iteration++) { const complete = await pair(entry, iteration, false); assert(!fatal, 'outer retirement required'); if (!complete) break; }
  }
  identities();
  for (const [path, hash] of Object.entries(decoderModules)) assert.equal(hashFile(path), hash, 'decoder module changed');
  if (config.resumeFrom) {
    const again = verifyDataContinuation(root, config.resumeFrom, {config, report, entries: dataCases(fixtures.images.images)});
    assert.deepEqual(again.evidence, report.continuation, 'prior evidence changed during continuation'); report.continuation.priorEvidenceStable = true;
  }
  report.machine.powerAfter = prepare('powercfg.exe', ['/getactivescheme'], 'power after').toString().trim(); assert.equal(report.machine.powerAfter, report.machine.powerBefore);
  report.groups = summarizeData(entries, report.pairs, config.acceptance);
  report.state = report.groups.length === entries.length * 2 && report.groups.every(group => ['passed_selected_path', 'equivalent_observation_only'].includes(group.state))
    ? config.resumeFrom ? 'equivalent_data_continuation_only' : config.acceptance ? 'passed_selected_data_paths_full_acceptance_open' : 'equivalent_data_observation_only' : 'failed';
} catch (error) { report.blocker = error.message.slice(0, 4096); report.groups = summarizeData(entries, report.pairs, config.acceptance); report.state = 'failed'; }
finally { report.wallMillis = performance.now() - start; report.finishedAt = new Date().toISOString(); report.requiresOuterRetirement = fatal; save(); }
console.log(JSON.stringify({state: report.state, report: join(out, 'report.json'), groups: report.groups}, null, 2));
if (report.state === 'failed') process.exitCode = 1;
