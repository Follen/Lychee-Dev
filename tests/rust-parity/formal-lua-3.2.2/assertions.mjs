import {CANDIDATE_RELEASE_FIELDS} from '../rust-3.5.0-release-fields.mjs';
// This checker authenticates inputs and observations. It never substitutes for Lua execution.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative, resolve } from 'node:path';

export const ORACLE = '0bec60e4406f5d5dc155ee25cef44f9a710e37e8';
export const MANIFEST_SHA256 = '9a4ffc0a482b0543bacb3065a56a79657ff7935b4d03b5ab2b9e321bce04f140';
export const BASE = 'tests/rust-parity/formal-lua-3.2.2';
export const PROFILES = Object.freeze(['retail', 'classic', 'titan', 'forever', 'classic-era', 'anniversary']);
export const LIMITS = Object.freeze({ file: 2 << 20, source: 16 << 20, files: 256, cases: 220, caseMs: 20000, totalMs: 20 * 60000, evidence: 64 << 20, nextProcessReserve: 18 << 20, cleanupMs: 65000 });
export const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export const gitBlob = bytes => createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
export function regular(path, maximum = LIMITS.file) {
  // Reject junction/symlink ancestors too, not only a symlink at the leaf.
  let cursor = resolve(path), depth = 0;
  for (;;) {
    assert(++depth <= 128 && !lstatSync(cursor).isSymbolicLink(), 'formal-lua.reparse_boundary');
    const parent = dirname(cursor); if (parent === cursor) break; cursor = parent;
  }
  const stat = lstatSync(path);
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, `formal-lua.invalid_file: ${path}`);
  const bytes = readFileSync(path);
  assert.equal(bytes.length, stat.size, `formal-lua.file_changed: ${path}`);
  return bytes;
}
export function tree(root) {
  const result = []; let total = 0, entries = 0;
  function visit(path, name) {
    assert(++entries <= LIMITS.files * 4, 'formal-lua.entry_budget');
    const info = lstatSync(path);
    assert(!info.isSymbolicLink(), 'formal-lua.reparse_boundary');
    if (info.isDirectory()) for (const child of readdirSync(path).sort()) visit(join(path, child), name ? `${name}/${child}` : child);
    else {
      const bytes = regular(path); total += bytes.length;
      assert(result.length < LIMITS.files && total <= LIMITS.source, 'formal-lua.source_budget');
      result.push({ path: name, bytes: bytes.length, sha256: hash(bytes), gitBlob: gitBlob(bytes) });
    }
  }
  visit(root, ''); return result;
}
export function manifest(root) {
  const bytes = regular(join(root, BASE, 'manifest.json'));
  assert.equal(hash(bytes), MANIFEST_SHA256, 'formal-lua.manifest_identity');
  const value = JSON.parse(bytes);
  assert.equal(value.schema, 'lycheedev.formal-lua-oracle.v1');
  assert.equal(value.oracle.commit, ORACLE);
  assert.deepEqual(value.profiles, PROFILES);
  assert.equal(value.fixtures.length, 58); assert.equal(value.candidateAddon.length, 55);
  // Keep the authenticated frozen manifest bytes; apply the separate approved candidate identity.
  return {...value, retainedReleaseFields: CANDIDATE_RELEASE_FIELDS};
}
export function authenticate(root) {
  const expected = manifest(root);
  assert.deepEqual(tree(join(root, BASE, 'oracle')), expected.fixtures, 'formal-lua.fixture_identity');
  const current = tree(join(root, 'addon')).map(row => ({ ...row, path: `addon/${row.path}` }));
  const lookup = new Map(current.map(row => [row.path, row]));
  const addon = expected.candidateAddon.map(row => {
    const retained = expected.retainedReleaseFields.find(field => field.path === row.path);
    if (!retained) { assert.deepEqual(lookup.get(row.path), row, `formal-lua.candidate_addon_identity: ${row.path}`); return row; }
    assert.deepEqual({bytes: row.bytes, sha256: row.sha256}, retained.expected, 'formal-lua.release_oracle_identity');
    const bytes = regular(join(root, row.path));
    assert.deepEqual({bytes: bytes.length, sha256: hash(bytes)}, retained.retained, 'formal-lua.retained_release_identity');
    const text = bytes.toString('utf8');
    assert.equal(text.split(retained.retainedText).length, 2, 'formal-lua.release_field_cardinality');
    const restored = Buffer.from(text.replace(retained.retainedText, retained.expectedText));
    assert.equal(hash(restored), row.sha256, 'formal-lua.release_only_difference');
    assert.equal(gitBlob(restored), row.gitBlob, 'formal-lua.release_only_git_identity');
    return lookup.get(row.path);
  });
  const required = new Set(expected.candidateAddon.map(row => row.path));
  // Existing historical resources remain visible here, but are neither executed
  // by this entry nor declared removed. Their consumers require another migration.
  return { manifestSHA256: MANIFEST_SHA256, fixtures: expected.fixtures, addon, retainedReleaseFields: expected.retainedReleaseFields,
    historicalAddonExtras: current.filter(row => !required.has(row.path)) };
}
export function materialize(root, target, identity = authenticate(root)) {
  mkdirSync(target); // Exclusive fresh tree. Never adopt or clean another owner.
  for (const [rows, source] of [[identity.fixtures, join(root, BASE, 'oracle')], [identity.addon, root]]) {
    for (const row of rows) {
      const bytes = regular(join(source, row.path));
      assert.equal(hash(bytes), row.sha256); assert.equal(gitBlob(bytes), row.gitBlob);
      const path = join(target, row.path); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, bytes, { flag: 'wx' });
    }
  }
  verifyMaterial(target, identity);
}
export function verifyMaterial(target, identity) {
  assert.deepEqual(tree(target), [...identity.fixtures, ...identity.addon].sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0), 'formal-lua.staged_identity');
}
export function cases(identity) {
  const suites = identity.fixtures.filter(row => /^tests\/addon\/t_[^/]+\.lua$/.test(row.path));
  assert.equal(suites.length, 36);
  const rows = PROFILES.flatMap(profile => suites.map(row => ({ id: `${row.path}/${profile}`, kind: 'addon', profile,
    cwd: 'tests/addon', args: ['run.lua', row.path.split('/').at(-1), profile, '../../addon'],
    completion: `PASS ./${row.path.split('/').at(-1)} [${profile}]\n` })));
  for (const [script, argument, completion, exact] of [
    ['character_state.lua', '../../addon', 'character storage isolation passed', false],
    ['input_state.lua', '../../addon', 'input state: four profiles passed', false],
    ['receiver_bindings.lua', '../../addon', 'receiver bindings: saved actions, deferred lifecycle and assignment rollback', false],
    ['writer.lua', '../../addon/Bridge/CaptureWriter.lua', 'capture-writer: passed\n', true],
  ]) rows.push({ id: `tests/protocol/${script}`, kind: 'protocol', cwd: 'tests/protocol', args: [script, argument], completion, exact });
  assert.equal(rows.length, LIMITS.cases); return rows;
}
export function processReaped(result) {
  const p = result.proof;
  return result.handlesReaped === true && p?.rootReaped === true && p.treeReaped === true
    && p.handlesClosed === true && p.toolIdentityLocksClosed === true && p.outputReadersComplete === true && p.activeProcesses === 0;
}
export function processPassed(result) {
  const p = result.proof;
  return result.status === 0 && !result.error && processReaped(result)
    && !p.forced && !p.error && !p.outputLimit && !p.deadlineHit && !p.cancelled && !p.canceled && !result.signal;
}
export function evaluate(row, result) {
  const output = Buffer.concat([result.stdout ?? Buffer.alloc(0), result.stderr ?? Buffer.alloc(0)]).toString('utf8').replaceAll('\r\n', '\n');
  const completed = row.exact ? output === row.completion : output.includes(row.completion);
  return { id: row.id, kind: row.kind, profile: row.profile, state: processPassed(result) && completed ? 'passed' : 'failed',
    exit: result.status, error: result.error?.message ?? result.error ?? null, completionObserved: completed, handlesReaped: processReaped(result) === true,
    proof: result.proof ?? null, outputSHA256: hash(Buffer.from(output)), logs: result.logs ?? [] };
}
export function cleanLuaEnvironment(environment) {
  const result = Object.fromEntries(Object.entries(environment).filter(([key]) => !/^LUA(?:_|$)/i.test(key)));
  return { ...result, LUA_PATH: '', LUA_CPATH: '' };
}
// Exact translation of the fixed Go TestUnifiedTOCContract, independent of the
// stricter byte identity check. This is static; it is not counted as Lua execution.
export function verifyUnifiedTOC(target) {
  const toc = regular(join(target, 'addon/Lychee Dev.toc')).toString('utf8').replaceAll('\r\n', '\n');
  const lines = toc.split('\n'), loads = lines.filter(line => line && !line.startsWith('#'));
  assert.equal(loads[0].toLowerCase(), 'core\\clientgate.lua');
  const declared = new Set((lines.find(line => line.startsWith('## Interface:')) ?? '').slice('## Interface:'.length).split(',').map(s => s.trim()));
  for (const value of ['120105', '120100', '50504', '38002', '16001', '11509', '20506']) assert(declared.has(value), `formal-lua.interface_missing: ${value}`);
  for (const line of loads) regular(join(target, 'addon', line.replaceAll('\\', '/')));
  assert.equal(loads.filter(line => line.startsWith('Modules\\Events\\CatalogData_')).length, 6);
}
export const BRIDGE_GAPS = Object.freeze([
  { test: 'TestMemoryRecordLuaParity', source: 'tests/protocol/memory_record_test.go', state: 'not_run', reason: 'Go bridge.DecodeMemoryRecord requires an independent Rust host integration; no standalone Lua-only substitute.' },
  { test: 'TestSlotDriverLuaRecoveryWithoutBusinessReplay', source: 'tests/protocol/slot_driver_test.go', state: 'not_run', reason: 'Go channel/memory driver, journal recovery and injected uncertain input require a faithful Rust-host integration with unchanged slot_server.lua.' },
  { test: 'TestSlotDriverLuaReportPreservesFiniteNumbers', source: 'tests/protocol/numeric_report_test.go', state: 'not_run', reason: 'Go driver/report durability, Float64bits, recovered JSON and no-replay assertions require Rust-host integration; writer.lua alone does not cover them.' },
]);
export function runCases(plan, { execute, checkIdentity, now = Date.now, evidenceBytes = () => 0, deadline, onResult = () => {} }) {
  const observations = []; let stop;
  for (const row of plan) {
    if (!stop && now() + LIMITS.cleanupMs + 1000 >= deadline) stop = 'formal-lua.total_deadline';
    if (!stop && evidenceBytes() + LIMITS.nextProcessReserve > LIMITS.evidence) stop = 'formal-lua.evidence_budget';
    if (!stop) { try { checkIdentity(); } catch (error) { stop = error.message; } }
    // Identity checks may traverse/hash many files. Recompute immediately before
    // launching, rather than passing a stale or nonpositive timeout downstream.
    const remaining = deadline - now() - LIMITS.cleanupMs;
    if (!stop && (!Number.isFinite(remaining) || remaining <= 1000)) stop = 'formal-lua.total_deadline';
    let observation;
    if (stop) observation = { id: row.id, kind: row.kind, profile: row.profile, state: 'not_run', reason: stop };
    else {
      try {
        const result = execute(row, Math.min(LIMITS.caseMs, remaining));
        observation = evaluate(row, result);
        if (processReaped(result) !== true) stop = 'formal-lua.tree_reap_unverified';
        else if (result.proof?.deadlineHit || result.proof?.forced || result.proof?.cancelled || result.proof?.canceled || result.signal) stop = 'formal-lua.process_interrupted';
        else if (result.error || result.proof?.error || result.proof?.outputLimit) stop = 'formal-lua.process_failure';
        try { checkIdentity(); } catch (error) { stop = error.message; observation.state = 'failed'; observation.identityError = stop; }
      } catch (error) {
        // An exception before a receipt cannot prove child cleanup. Do not launch
        // any later fixture, even if an earlier fixture passed.
        stop = 'formal-lua.execution_unverified';
        observation = { id: row.id, kind: row.kind, profile: row.profile, state: 'failed', error: error.message, handlesReaped: false };
      }
    }
    observations.push(observation); onResult(observation);
  }
  return observations;
}

// Verify the complete sealed owner directory before trusting any artifact that
// cites it. These two builders seal local logs only: foreign paths, omissions,
// duplicate entries and extra unsealed evidence are all failures.
export function verifyOwnerEvidence(repository, receiptPath, expectedOwner) {
  const path = resolve(receiptPath), bytes = regular(path), receipt = JSON.parse(bytes);
  assert.equal(receipt.schema, 'lycheedev.disk-owner.v1');
  assert(/^[a-f0-9-]{36}$/.test(receipt.id ?? ''), 'formal-lua.owner_id');
  const directory = join(resolve(repository), '.tmp/rust-disk/evidence', receipt.id);
  assert.equal(path, join(directory, 'receipt.json'), 'formal-lua.owner_receipt_path');
  assert.equal(receipt.directory, join(resolve(repository), '.tmp/rust-scratch', receipt.id));
  if (expectedOwner) assert.equal(receipt.owner, expectedOwner);
  assert.equal(receipt.category, 'scratch'); assert.equal(receipt.activePid, null); assert.equal(receipt.handlesReaped, true);
  assert(['succeeded', 'retired'].includes(receipt.state) && typeof receipt.sealedAt === 'string', 'formal-lua.owner_not_sealed');
  assert.equal(hash(Buffer.from(JSON.stringify(receipt.command))), receipt.commandSHA256, 'formal-lua.owner_command_identity');
  assert(Array.isArray(receipt.sealedLogs) && receipt.sealedLogs.length > 0 && receipt.sealedLogs.length <= 2048, 'formal-lua.sealed_log_count');
  const logs = new Map(), hashes = {}; let total = 0;
  for (const row of receipt.sealedLogs) {
    assert(typeof row.path === 'string' && dirname(row.path) === directory && row.original === row.path && basename(row.path) !== 'receipt.json', 'formal-lua.foreign_owner_log');
    assert(!logs.has(row.path), 'formal-lua.duplicate_sealed_log');
    const length = lstatSync(row.path).size; total += length;
    assert(length <= 16 << 20 && total <= LIMITS.evidence, 'formal-lua.sealed_log_budget');
    const content = regular(row.path, 16 << 20); assert.equal(hash(content), row.sha256, 'formal-lua.sealed_log_identity');
    logs.set(row.path, content); hashes[relative(resolve(repository), row.path)] = row.sha256;
  }
  assert.deepEqual(receipt.logHashes, hashes, 'formal-lua.sealed_log_index');
  assert.deepEqual(readdirSync(directory).filter(name => name !== 'receipt.json').sort(), [...logs.keys()].map(path => basename(path)).sort(), 'formal-lua.sealed_log_set');
  const json = name => { const content = logs.get(join(directory, name)); assert(content, `formal-lua.sealed_log_missing: ${name}`); return JSON.parse(content); };
  const processes = [];
  for (const [requestPath, requestBytes] of logs) if (/process-[a-f0-9-]+\.request\.json$/.test(requestPath)) {
    const request = JSON.parse(requestBytes), stem = requestPath.slice(0, -'.request.json'.length);
    for (const [key, suffix] of [['stdout', '.stdout.bin'], ['stderr', '.stderr.bin'], ['receipt', '.receipt.json']]) {
      assert.equal(request[key], stem + suffix, 'formal-lua.process_log_target'); assert(logs.has(request[key]), 'formal-lua.process_log_missing');
    }
    const wrapper = json(basename(requestPath) + '.wrapper.json'), proof = JSON.parse(logs.get(request.receipt));
    assert.equal(wrapper.status, 0); assert(!wrapper.signal && !wrapper.error, 'formal-lua.wrapper_failed');
    assert.equal(proof.schema, 'lycheedev.owned-process.v1');
    assert(processReaped({ handlesReaped: true, proof }), 'formal-lua.build_cleanup_unverified');
    assert.equal(proof.exitCode, 0);
    assert(!proof.error && !proof.deadlineHit && !proof.outputLimit && !proof.cancelled && !proof.canceled, 'formal-lua.build_process_failed');
    // Reuse, without broadening, the owned controller's reviewed compiler-only
    // helper exception. Lua fixtures themselves never get this exception.
    assert(!proof.forced || proof.knownAuxiliaryCleanup === true && (request.msvcCompile === true || request.cargoBuild === true), 'formal-lua.build_process_forced');
    processes.push({ request, proof });
  }
  assert(processes.length > 0, 'formal-lua.no_owned_build_process');
  return { receipt, receiptSHA256: hash(bytes), logs, json, processes };
}
