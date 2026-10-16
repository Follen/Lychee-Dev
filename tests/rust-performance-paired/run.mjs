// Run only under tools/rust-performance-paired.mjs's outer owned Windows Job.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import os from 'node:os';
import {beginScratch} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {collectOwnedMemory} from '../../tools/rust-owned-memory.mjs';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {inspectEmmy} from '../../tools/rust-emmy-identity.mjs';
import {assertEnvelope, sha256} from '../rust-source-full-corpus/assertions.mjs';
import {CASES, GO_COMMIT, REPETITIONS, TARGET_RATIO, compareSamples, elapsed, orderFor, semantic, summarize} from './contract.mjs';
import {assertSeedCold, auditCorpus, cloneSeed, derivedIdentity, proveWarmReuse, verifyGoMetadata, verifyGoPackage, verifySeedMetadata} from './inputs.mjs';

const [configuration, ...extra] = process.argv.slice(2);
assert(configuration && extra.length === 0 && process.platform === 'win32', 'owned Windows controller requires configuration');
const config = JSON.parse(readFileSync(configuration)), root = resolve(config.root), out = resolve(config.evidence), start = performance.now();
const oracle = JSON.parse(readFileSync(join(root, 'tests/rust-source-full-corpus/retail-oracle.json')));
let fatal = false, sequence = 0;
const report = {
  schema: 'lycheedev.paired-performance.v1', state: 'running', startedAt: new Date().toISOString(),
  scope: ['B01 describe only', 'B02 fixed full-Retail local index and precise query only'],
  completeB01B07Acceptance: false, targetRatio: TARGET_RATIO, repetitionsPerColdWarmGroup: REPETITIONS,
  remaining: ['B01 parser/selection/project/persistence', 'B02 local sync preparation, refs/context/flow/batch and full semantic checks', 'B03 retained session/invalidation/error recovery/close', 'B04-B07', 'full F/P correctness qualification'],
  cacheDefinition: 'Application-derived cache cold: independent new home per engine/path/pair copied only from that engine’s independently initialized, synced and authenticated seed; no cross-engine cache or metadata sharing; warm: exact same home, pin, binary and command after its own successful cold invocation. OS page/file caches are not flushed or claimed cold. Describe has no derived cache; cold/warm names denote first/repeated invocation only.',
  timingDefinition: 'Native monotonic clock immediately before CLI CreateProcessW until root and every owned descendant reaped and output drained. Includes all CLI local validation, worktree/index/cache reads/writes, worker startup and cleanup. PowerShell/compiler startup and observer shutdown excluded; observer startup, scheduling and pipe-drain overhead within the native interval remain included without subtraction. Wrapper wall time separately retained.',
  machine: {hostname: os.hostname(), platform: process.platform, release: os.release(), arch: process.arch, node: process.version, cpus: os.cpus(), totalMemory: os.totalmem(), freeMemory: os.freemem(), loadAverage: os.loadavg(), conditions: config.conditions},
  inputs: {oracle, rust: config.rust, go: config.go}, preparation: [], pairs: [], pairOwners: [],
  metrics: {cpu: 'not_measured', io: 'not_measured', rss: 'not_measured; Windows working set/private usage recorded per CLI owned Job with identity deduplication; never sum child and outer Job counters'},
  excludedDownloadMillis: null, excludedExternalWaitMillis: null,
};
const save = () => writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
const files = directory => readdirSync(directory).filter(name => name !== 'receipt.json' && lstatSync(join(directory, name)).isFile()).map(name => join(directory, name));
function controlled(binary, args, evidence, {timeout = 210000, env = process.env, measure = false} = {}) {
  assert(!fatal, 'previous unverified process failure requires outer retirement');
  const startedAt = new Date().toISOString(), begin = performance.now();
  fatal = true; // An exception before a verified receipt must retain ownership.
  const run = ownedSpawnSync(binary, args, {cwd: root, env, evidence, timeout, orphanMs: 30000, memoryObservation: measure});
  const row = {binary, args, startedAt, endedAt: new Date().toISOString(), wallMillis: performance.now() - begin, pid: run.proof?.pid, exit: run.status, signal: run.signal, error: run.error?.message, handlesReaped: run.handlesReaped, proof: run.proof, logs: run.logs, stdoutSHA256: sha256(run.stdout), stderrSHA256: sha256(run.stderr), stdoutBytes: run.stdout.length, stderrBytes: run.stderr.length};
  fatal = run.handlesReaped !== true;
  if (measure) row.memory = collectOwnedMemory(run, {evidence, sourceBinary: binary, runState: run.status === 0 && !run.error ? 'passed_invocation' : 'failed_invocation'});
  return {run, row};
}
function prepare(binary, args, label, options) {
  const {run, row} = controlled(binary, args, out, options);
  report.preparation.push({...row, label, phase: 'preparation_not_subtracted_from_a_measured_command'}); save();
  assert(run.handlesReaped && !run.error && run.status === 0, `${label}: ${run.error?.message ?? run.stderr.toString()}`);
  return run.stdout;
}
const gitEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE'));
Object.assign(gitEnv, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: 'NUL', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1'});
function git(args, input) {
  assert(!fatal); const id = sequence++, begin = performance.now();
  const run = spawnSync('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', ...args], {input, cwd: root, env: gitEnv, timeout: 60000, maxBuffer: 16 << 20, windowsHide: true, shell: false});
  const stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0), prefix = join(out, `git-audit-${id}`);
  writeFileSync(`${prefix}.stdout.bin`, stdout); writeFileSync(`${prefix}.stderr.bin`, stderr);
  report.preparation.push({label: 'authenticated-source-read', args, inputSHA256: input === undefined ? null : sha256(Buffer.from(input)), wallMillis: performance.now() - begin, exit: run.status, signal: run.signal, error: run.error?.message, stdoutSHA256: sha256(stdout), stderrSHA256: sha256(stderr), logs: [`${prefix}.stdout.bin`, `${prefix}.stderr.bin`], ownership: 'outer Job; terminal proof required'}); save();
  if (run.error || run.signal) fatal = true;
  assert(!run.error && run.status === 0, 'authenticated Git read failed'); return stdout;
}
function identity() {
  const source = config.goSource;
  assert.equal(git(['-C', source, 'rev-parse', 'HEAD']).toString().trim(), GO_COMMIT);
  assert.equal(git(['-C', source, 'status', '--porcelain', '--untracked-files=all']).toString().trim(), '', 'Go checkout must be exact and clean');
  const go = verifyGoPackage(config.go.root, config.luaLSArchive);
  assert.equal(go.packageSHA256, config.go.packageSHA256, 'Go package changed');
  const rust = verifyArtifact(root, config.rustArtifact);
  assert(['release', 'symbols'].includes(rust.profile), 'optimized Rust required');
  assert.equal(rust.binarySHA256, config.rust.binarySHA256); assert.equal(sourceDigest(root), rust.inputHashes.sourceTree, 'Rust source/build identity changed');
  const emmy = inspectEmmy(root); assert.equal(emmy.identity, 'public-upstream-unmodified-v1');
  assert.equal(emmy.patchedTreeSHA256, rust.inputHashes.emmy?.patchedTreeSHA256);
  const channel = JSON.parse(readFileSync(join(root, 'release/npm-channel.json')));
  assert.equal(channel.version, '3.5.0'); assert.equal(channel.distTag, 'beta');
  report.inputs.rust.checkout = {commit: git(['rev-parse', 'HEAD']).toString().trim(), status: git(['status', '--porcelain', '--untracked-files=all']).toString()};
  report.inputs.rust.emmyIdentity = {identity: emmy.identity, baseCommit: emmy.base.commit, treeSHA256: emmy.patchedTreeSHA256};
  return {go, rust};
}
function invoke(entry, engine, home, pin, evidence, iteration, cacheMode) {
  const binary = engine === 'go' ? config.go.binary : config.rust.sourceBinary;
  const args = [...entry.args, ...(pin ? ['--snapshot', pin] : []), '--home', home, '--format', 'json'];
  const {run, row} = controlled(binary, args, evidence, {measure: true});
  Object.assign(row, {caseId: entry.id, engine, iteration, cacheMode, home, snapshot: pin, excludedDownloadMillis: 0, excludedExternalWaitMillis: 0, valid: false});
  let envelope;
  try {
    assert.equal(run.status, 0); assert(!run.error && run.handlesReaped); Object.assign(row, elapsed(run.proof));
    envelope = JSON.parse(run.stdout); semantic(entry.id, envelope, engine, oracle, pin); row.valid = true;
  } catch (error) { row.failure = error.message.slice(0, 4096); }
  return {row, envelope};
}

save();
try {
  identity();
  report.inputs.go.build = verifyGoMetadata(prepare('go', ['version', '-m', config.go.binary], 'fixed-Go-build-metadata').toString());
  report.machine.powerBefore = prepare('powercfg.exe', ['/getactivescheme'], 'power-policy-before').toString().trim();
  assert(!process.env.RUSTFLAGS && !process.env.CARGO_ENCODED_RUSTFLAGS, 'benchmark compiler override environment must be empty');
  report.machine.toolchains = {nodeBinarySHA256: hashFile(process.execPath), rustc: prepare('rustc', ['-Vv'], 'Rust-toolchain-metadata').toString(), go: prepare('go', ['version'], 'Go-toolchain-metadata').toString()};
  assert.match(report.machine.toolchains.rustc, /^rustc 1\.99\.0\b/);
  const seeds = {}, seedIdentities = {}, pins = {};
  function prepareSeeds() {
    for (const engine of ['go', 'rust']) {
      const seed = join(config.scratch, `${engine}-sync-only-seed`), binary = engine === 'go' ? config.go.binary : config.rust.sourceBinary;
      assert(!existsSync(seed));
      assertEnvelope({exit: 0, envelope: JSON.parse(prepare(binary, ['init', '--home', seed, '--format', 'json'], `${engine}-init-sync-only-seed`))});
      const sync = prepare(binary, ['source', 'sync', '--source', oracle.repository, '--product', oracle.product, '--ref', oracle.commit, '--home', seed, '--format', 'json'], `${engine}-fixed-source-sync-network-and-local-inseparable`, {timeout: 240000});
      const pinned = assertEnvelope({exit: 0, envelope: JSON.parse(sync)});
      assert.equal(pinned.source?.exactCommit, oracle.commit); assert.equal(pinned.source.repository, oracle.repository); assert.equal(pinned.source.product, oracle.product); assert.equal(pinned.source.requestedRef, oracle.commit); assert.match(pinned.id, /^PIN-/);
      const corpus = auditCorpus(join(seed, 'mirrors', `${oracle.repository}.git`), oracle, git, out, `${engine}-`);
      const metadata = verifySeedMetadata(seed, pinned, join(out, `${engine}-seed-toolkit.sqlite`));
      seeds[engine] = seed; pins[engine] = pinned; seedIdentities[engine] = assertSeedCold(seed);
      report.inputs[`${engine}Seed`] = {sha256: seedIdentities[engine].sha256, bytes: seedIdentities[engine].bytes, pin: pinned, corpus, metadata}; save();
    }
    assert.deepEqual(pins.rust, pins.go, 'independently created public fixed pins must agree');
  }
  for (const entry of CASES) {
    // Describe remains independently useful when later input acquisition fails.
    if (entry.id !== 'B01-describe' && !seedIdentities.go) prepareSeeds();
    for (let iteration = 0; iteration < REPETITIONS; iteration++) {
      const pair = beginScratch(root, `paired-${entry.id}-${iteration}`, {command: entry.args, inputs: {go: config.go.binarySHA256, rust: config.rust.binarySHA256, seeds: Object.fromEntries(Object.entries(seedIdentities).map(([engine, value]) => [engine, value.sha256]))}, admissionOptions: {requiredBytes: 512 << 20, scratchReserveBytes: 512 << 20}});
      report.pairOwners.push({caseId: entry.id, iteration, receipt: pair.receipt}); save();
      let valid = false;
      try {
        const homes = {go: join(pair.directory, 'go-home'), rust: join(pair.directory, 'rust-home')};
        const copying = performance.now();
        if (entry.id !== 'B01-describe') for (const engine of ['go', 'rust']) cloneSeed(seeds[engine], homes[engine], seedIdentities[engine]);
        report.preparation.push({label: 'independent-home-copy', caseId: entry.id, iteration, wallMillis: performance.now() - copying, seedSHA256: entry.id === 'B01-describe' ? null : Object.fromEntries(Object.entries(seedIdentities).map(([engine, value]) => [engine, value.sha256])), classification: 'fixture construction; not product work removed from timing'});
        for (const cacheMode of ['cold', 'warm']) {
          const row = {caseId: entry.id, iteration, cacheMode, order: orderFor(iteration), resultEquivalent: false, inputSHA256: entry.id === 'B01-describe' ? null : oracle.allPathSHA256SizeDigest};
          report.pairs.push(row); save();
          const outputs = {};
          for (const engine of row.order) {
            const captureDerived = label => {
              const value = derivedIdentity(homes[engine], engine);
              value.evidence = join(pair.evidence, `${engine}-${label}-derived.json`);
              writeFileSync(value.evidence, JSON.stringify(value, null, 2) + '\n'); return value;
            };
            const needsReuse = cacheMode === 'warm' && entry.id !== 'B01-describe';
            let before = null, beforeError;
            if (needsReuse) try { before = captureDerived('before-warm'); } catch (error) { beforeError = error.message; }
            const output = invoke(entry, engine, homes[engine], entry.id === 'B01-describe' ? null : pins[engine].id, pair.evidence, iteration, cacheMode);
            if (needsReuse && output.row.valid && !before) {
              output.row.warmReuse = {verified: false, state: 'same_home_repeated_reuse_unproved', reason: beforeError}; output.row.valid = false;
            }
            if (before && output.row.valid) {
              try { output.row.warmReuse = proveWarmReuse(engine, before, captureDerived('after-warm'), output.envelope); }
              catch (error) { output.row.warmReuse = {verified: false, state: 'same_home_repeated_reuse_unproved', reason: error.message, before: {sha256: before.sha256, evidence: before.evidence}}; output.row.valid = false; }
            }
            row[engine] = output.row; outputs[engine] = output.envelope; save();
            if (fatal) throw new Error('unverified process reap; outer retirement required');
          }
          try { assert(row.go.valid && row.rust.valid, 'failed sample retained'); Object.assign(row, compareSamples(entry.id, outputs.go, outputs.rust, oracle, pins.go?.id)); }
          catch (error) { row.failure = error.message.slice(0, 4096); }
          save();
          if (!row.resultEquivalent) break; // No warm label after a failed cold.
        }
        const attempted = report.pairs.filter(row => row.caseId === entry.id && row.iteration === iteration);
        valid = attempted.length === 2 && attempted.every(row => row.resultEquivalent);
        writeFileSync(join(pair.evidence, 'pair.json'), JSON.stringify({caseId: entry.id, iteration, valid, rows: attempted}, null, 2) + '\n');
      } finally {
        pair.seal({state: fatal ? 'interrupted' : valid ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: files(pair.evidence)});
        if (!fatal && valid) pair.cleanup();
      }
      // Honest fail-fast: no deleting failures or replacing slow samples. The
      // remaining pairs stay missing and the 20-pair gate cannot pass.
      if (!valid) break;
    }
  }
  identity();
  report.machine.powerAfter = prepare('powercfg.exe', ['/getactivescheme'], 'power-policy-after').toString().trim();
  assert.equal(report.machine.powerAfter, report.machine.powerBefore, 'power policy changed');
  report.groups = summarize(report.pairs);
  report.state = report.groups.every(row => row.state === 'passed_selected_path') ? 'passed_selected_paths_full_acceptance_open' : 'failed';
} catch (error) { report.blocker = error.message.slice(0, 4096); report.groups = summarize(report.pairs); report.state = 'failed'; }
finally { report.wallMillis = performance.now() - start; report.finishedAt = new Date().toISOString(); report.requiresOuterRetirement = fatal; save(); }
console.log(JSON.stringify({state: report.state, report: join(out, 'report.json'), groups: report.groups}));
if (report.state !== 'passed_selected_paths_full_acceptance_open') process.exitCode = 1;
