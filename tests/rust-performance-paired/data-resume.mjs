// One bounded continuation of an interrupted full observation, never a sample
// merger or an acceptance shortcut. All previous files are read-only.
import assert from 'node:assert/strict';
import {basename, dirname, join, relative, resolve} from 'node:path';
import {contained} from '../../tools/rust-disk.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {elapsed} from './contract.mjs';
import {summarizeData} from './data-contract.mjs';
import {bounded, digest, parse} from './data-inputs.mjs';

export function dataMode(extra) {
  if (!extra.length) return {acceptance: false, resumeFrom: null};
  if (extra.length === 1 && extra[0] === '--acceptance') return {acceptance: true, resumeFrom: null};
  assert(extra.length === 2 && extra[0] === '--resume-unattempted' && extra[1], 'expected --acceptance OR --resume-unattempted <prior-owned-report.json>');
  return {acceptance: false, resumeFrom: resolve(extra[1])};
}

export function remainingDataCases(entries, previous) {
  assert.equal(previous.performanceAcceptanceEvaluated, false, 'cannot resume/merge acceptance samples');
  assert.equal(previous.preflightPairs.length, 0); assert.equal(previous.requiresOuterRetirement, false);
  assert.equal(previous.state, 'failed'); assert(!previous.continuation, 'one-hop continuation only');
  const attempted = [...new Set(previous.pairs.map(row => row.caseId))];
  assert(attempted.length > 0 && attempted.length < entries.length, 'partial nonempty observation required');
  assert.deepEqual(attempted, entries.slice(0, attempted.length).map(entry => entry.id), 'only an uninterrupted completed path prefix can be continued');
  const completed = entries.slice(0, attempted.length);
  // Historical rows predate pairTerminal. Their sealed pair.json valid=true
  // is authenticated independently below; new runs never enable this option.
  assert(summarizeData(completed, previous.pairs, false, {allowLegacyTerminal: true}).every(group => group.state === 'equivalent_observation_only' && group.timings), 'every attempted path must already be complete/equivalent');
  assert.equal(previous.pairs.length, completed.length * 2);
  for (const row of previous.pairs) for (const engine of ['go', 'rust']) {
    const sample = row[engine]; assert.equal(sample.exit, 0); assert(!sample.error && !sample.failure); assert.equal(sample.handlesReaped, true);
    const timing = elapsed(sample.proof); assert.equal(sample.controllableMillis, timing.controllableMillis, 'original owned timing required');
  }
  return {completed, selected: entries.slice(attempted.length)};
}

export function verifyDataContinuation(root, priorPath, current) {
  root = resolve(root); priorPath = contained(root, resolve(priorPath)); assert.equal(basename(priorPath), 'owned-report.json');
  const evidence = dirname(priorPath), id = basename(evidence), receiptPath = join(evidence, 'receipt.json');
  assert.match(id, /^[0-9a-f-]{36}$/); assert.equal(evidence, join(root, '.tmp/rust-disk/evidence', id));
  function receipt(path, expectedOwner) {
    const bytes = bounded(contained(root, path), 2 << 20), value = parse(bytes);
    assert.equal(value.schema, 'lycheedev.disk-owner.v1'); assert.equal(value.owner, expectedOwner); assert.equal(value.handlesReaped, true); assert.equal(value.activePid, null);
    assert.equal(path, join(root, '.tmp/rust-disk/evidence', value.id, 'receipt.json'));
    assert.equal(value.directory, join(root, '.tmp/rust-scratch', value.id));
    assert(Array.isArray(value.sealedLogs) && value.sealedLogs.length > 0 && value.sealedLogs.length <= 4096);
    const logs = [], seen = new Set(); let total = 0;
    for (const log of value.sealedLogs) {
      const preserved = contained(dirname(path), log.path), key = relative(root, preserved); assert(!seen.has(key), 'duplicate sealed log'); seen.add(key);
      const body = bounded(preserved, 16 << 20); assert((total += body.length) <= 128 << 20, 'prior evidence byte budget');
      const sha256 = digest(body); assert.equal(log.sha256, sha256, 'prior sealed raw log changed'); assert.equal(value.logHashes[key], sha256);
      logs.push({path: preserved, bytes: body.length, sha256});
    }
    assert.deepEqual(Object.keys(value.logHashes).sort(), [...seen].sort(), 'complete sealed log index required');
    return {value, sha256: digest(bytes), logs: {files: logs.length, bytes: total, sha256: digest(JSON.stringify(logs))}};
  }
  function sealed(path, owner) {
    const bytes = bounded(contained(root, path), 16 << 20), sha256 = digest(bytes);
    const matches = owner.value.sealedLogs.filter(log => log.path === path); assert.equal(matches.length, 1);
    assert.equal(matches[0].sha256, sha256); assert.equal(owner.value.logHashes[relative(root, path)], sha256, 'prior sealed evidence changed');
    return {value: parse(bytes), sha256};
  }
  const owner = receipt(receiptPath, 'paired-data'); assert.equal(owner.value.state, 'failed');
  const owned = sealed(priorPath, owner), original = sealed(join(evidence, 'report.json'), owner), configuration = sealed(join(evidence, 'configuration.json'), owner);
  const old = configuration.value, report = original.value, outer = owned.value;
  assert.equal(outer.schema, 'lycheedev.paired-data-owned.v1'); assert.equal(report.schema, 'lycheedev.paired-data.v1');
  assert.equal(outer.state, 'failed'); assert.equal(outer.performanceAcceptanceEvaluated, false); assert.equal(outer.artifactAndRunnerStable, true);
  assert.equal(outer.process.exit, 1); assert.equal(outer.process.handlesReaped, true); assert(!outer.process.error);
  assert.equal(outer.observation, join(evidence, 'report.json'));
  assert.equal(old.root, root); assert.equal(old.evidence, evidence); assert.equal(old.scratch, owner.value.directory);
  assert.equal(old.acceptance, false); assert(!old.resumeFrom && !old.runnerRoot, 'original full single-root observation required');
  assert.deepEqual(outer.inputs, old.inputs);
  // Authenticate the old runner against the unchanged artifact checkout, while
  // the new executing runner has its own separately sealed identity.
  for (const [path, sha] of Object.entries(old.inputs)) assert.equal(hashFile(contained(root, join(root, path))), sha, 'original runner checkout changed');
  const now = current.config;
  assert.deepEqual(Object.keys(old.inputs).sort(), Object.keys(now.inputs).filter(path => path !== 'tests/rust-performance-paired/data-resume.mjs').sort(), 'complete original runner inventory required');
  assert.deepEqual(old.rust, now.rust); assert.deepEqual(outer.rust, now.rust);
  for (const key of ['root', 'binary', 'binarySHA256', 'packageSHA256']) { assert.equal(old.go[key], now.go[key]); assert.equal(outer.go[key], now.go[key]); }
  for (const key of ['goSource', 'luaLSArchive', 'rustArtifact', 'sharpModule']) assert.equal(old[key], now[key], `continuation ${key} identity changed`);
  assert.deepEqual(old.fixtures, now.fixtures); assert.deepEqual(old.conditions, now.conditions);
  assert.deepEqual(report.inputs.fixtures, current.report.inputs.fixtures, 'full original input inventory required');
  for (const key of ['hostname', 'platform', 'arch', 'release', 'node', 'totalMemory', 'powerBefore', 'toolchains']) assert.deepEqual(report.machine[key], current.report.machine[key], `continuation machine ${key} changed`);
  assert.deepEqual(report.machine.cpus.map(cpu => cpu.model), current.report.machine.cpus.map(cpu => cpu.model));
  for (const key of ['entryPath', 'versions', 'loadedModuleSHA256']) assert.deepEqual(report.decoder[key], current.report.decoder[key], 'independent decoder identity changed');
  const plan = remainingDataCases(current.entries, report); assert.equal(report.pairOwners.length, plan.completed.length);
  const pairReceipts = [];
  for (const [index, entry] of plan.completed.entries()) {
    const row = report.pairOwners[index]; assert.equal(row.caseId, entry.id); assert.equal(row.iteration, 0); assert.equal(row.preflight, true);
    const pairOwner = receipt(row.receipt, `paired-data-${entry.id}-preflight`), last = index === plan.completed.length - 1;
    assert.equal(pairOwner.value.state, last ? 'succeeded' : 'retired');
    assert.deepEqual(pairOwner.value.inputHashes, {go: now.go.binarySHA256, rust: now.rust.binarySHA256, fixture: current.report.inputs.fixtures[entry.fixture].inventory.sha256});
    const pair = sealed(join(dirname(row.receipt), 'pair.json'), pairOwner);
    assert.equal(pair.value.valid, true); assert.equal(pair.value.caseId, entry.id); assert.equal(pair.value.iteration, 0); assert.equal(pair.value.preflight, true);
    assert.deepEqual(pair.value.rows, report.pairs.filter(sample => sample.caseId === entry.id));
    if (last) { assert.match(report.blocker, /^EPERM\b/); assert(report.blocker.includes(pairOwner.value.id), 'cleanup blocker must name the final sealed owner'); }
    pairReceipts.push({path: row.receipt, sha256: pairOwner.sha256, state: pairOwner.value.state, pairSHA256: pair.sha256, sealedLogs: pairOwner.logs});
  }
  return {selected: plan.selected, evidence: {schema: 'lycheedev.data-unattempted-continuation.v1', priorOwnedReport: priorPath, priorOwnedReportSHA256: owned.sha256,
    priorReportSHA256: original.sha256, priorConfigurationSHA256: configuration.sha256, priorReceiptSHA256: owner.sha256, priorSealedLogs: owner.logs, priorRunnerInputs: old.inputs,
    priorState: outer.state, priorBlocker: report.blocker, pairReceipts, skippedCompletedCases: plan.completed.map(entry => entry.id), selectedCases: plan.selected.map(entry => entry.id),
    importedSamples: 0, priorFailurePreserved: true, performanceAcceptanceEvaluated: false, completeOriginalRun: false}};
}
