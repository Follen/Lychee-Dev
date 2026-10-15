// Static/checker regressions only; these successes do not claim Lua was run.
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { BASE, BRIDGE_GAPS, LIMITS, MANIFEST_SHA256, ORACLE, PROFILES, authenticate, cases, cleanLuaEnvironment, evaluate, gitBlob, hash, manifest, materialize, processPassed, runCases, verifyMaterial, verifyOwnerEvidence, verifyUnifiedTOC } from '../tests/rust-parity/formal-lua-3.2.2/assertions.mjs';
import { sourceDigest } from './rust-build.mjs';
import { verifyCandidateArtifact } from './rust-formal-lua.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const fixed = authenticate(root), plan = cases(fixed);
const proof = () => ({ rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false, outputLimit: false });
const result = row => ({ status: 0, handlesReaped: true, proof: proof(), stdout: Buffer.from(row.completion), stderr: Buffer.alloc(0), logs: ['owned-process.receipt.json'] });
function scratch(fn) { const dir = mkdtempSync(join(tmpdir(), 'lycheedev-formal-lua-checker-')); try { return fn(dir); } finally { rmSync(dir, { recursive: true, force: true }); } }
function copied(fn) { return scratch(dir => { cpSync(join(root, BASE), join(dir, BASE), { recursive: true }); cpSync(join(root, 'addon'), join(dir, 'addon'), { recursive: true }); return fn(dir); }); }

test('manifest and all 113 source records match immutable Git objects byte for byte', () => {
  const m = manifest(root); assert.equal(hash(readFileSync(join(root, BASE, 'manifest.json'))), MANIFEST_SHA256);
  for (const row of [...m.fixtures, ...m.candidateAddon]) {
    const bytes = execFileSync('git', ['--no-pager', '--no-optional-locks', '-c', 'core.fsmonitor=false', 'cat-file', 'blob', `${ORACLE}:${row.path}`], { cwd: root, timeout: 10000, maxBuffer: LIMITS.file });
    assert.equal(bytes.length, row.bytes, row.path); assert.equal(hash(bytes), row.sha256, row.path); assert.equal(gitBlob(bytes), row.gitBlob, row.path);
  }
});
test('original 39 addon and 13 protocol sets are complete, not selected old test names', () => {
  const m = manifest(root);
  for (const [prefix, expected] of [['tests/addon/', 39], ['tests/protocol/', 13]]) {
    const actual = m.fixtures.filter(row => row.path.startsWith(prefix)).map(row => row.path);
    const original = execFileSync('git', ['ls-tree', '--full-tree', '-r', '--name-only', ORACLE, `:(top)${prefix}`], { cwd: root, timeout: 10000, maxBuffer: 65536 }).toString().trim().split('\n');
    assert.equal(actual.length, expected); assert.deepEqual(actual, original);
  }
});
test('every addon fixture runs independently under all six exact profiles', () => {
  assert.equal(plan.length, 220); assert.equal(new Set(plan.map(row => row.id)).size, 220);
  for (const profile of PROFILES) {
    const rows = plan.filter(row => row.profile === profile); assert.equal(rows.length, 36);
    for (const row of rows) assert.deepEqual(row.args, ['run.lua', row.id.split('/')[2], profile, '../../addon']);
  }
  assert.equal(plan.filter(row => row.kind === 'protocol').length, 4);
  assert(!plan.some(row => row.args.includes('slot_server.lua')));
});
test('staging contains current authenticated addon plus exact frozen fixture dependencies', () => scratch(dir => {
  const target = join(dir, 'staged'); materialize(root, target, fixed); verifyMaterial(target, fixed); verifyUnifiedTOC(target);
  assert.equal(readFileSync(join(target, 'tests/baseline/probes/sync.lua')).toString(), readFileSync(join(root, BASE, 'oracle/tests/baseline/probes/sync.lua')).toString());
  // The current delivery retired the 21 historical addon extras; their frozen
  // source evidence remains separate from the exact staged addon inventory.
  assert.deepEqual(fixed.historicalAddonExtras, []);
}));
test('fixture mutation, current addon mutation, missing source and unexpected fixture fail authentication', () => {
  for (const mutate of [
    dir => writeFileSync(join(dir, BASE, 'oracle/tests/addon/t_bootstrap.lua'), '-- old legacy test\n'),
    dir => writeFileSync(join(dir, 'addon/Bridge/SlotRuntime.lua'), '-- wrong protocol\n'),
    dir => rmSync(join(dir, BASE, 'oracle/tests/addon/t_input_mailbox.lua')),
    dir => writeFileSync(join(dir, BASE, 'oracle/tests/addon/t_unreviewed.lua'), 'assert(true)\n'),
  ]) copied(dir => { mutate(dir); assert.throws(() => authenticate(dir)); });
});
test('self-consistent rewritten manifest cannot authorize a substituted fixture', () => copied(dir => {
  const file = join(dir, BASE, 'manifest.json'), m = JSON.parse(readFileSync(file)); m.fixtures[0].sha256 = '0'.repeat(64); writeFileSync(file, JSON.stringify(m));
  assert.throws(() => authenticate(dir), /manifest_identity/);
}));
test('symlink fixture cannot escape the authenticated tree', () => copied(dir => {
  const path = join(dir, BASE, 'oracle/tests/addon'); rmSync(path, { recursive: true }); symlinkSync(join(root, BASE, 'oracle/tests/addon'), path, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => authenticate(dir), /reparse/);
}));
test('symlink ancestors cannot substitute the manifest or full oracle directory', () => copied(dir => {
  const path = join(dir, BASE, 'oracle'); rmSync(path, { recursive: true }); symlinkSync(join(root, BASE, 'oracle'), path, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => authenticate(dir), /reparse/);
}));
test('staged modification and unexpected extra are rejected after execution', () => scratch(dir => {
  const target = join(dir, 'stage'); materialize(root, target, fixed);
  writeFileSync(join(target, 'addon/Bridge/SlotRuntime.lua'), 'assert(true)'); assert.throws(() => verifyMaterial(target, fixed));
}));
test('completion text alone and zero exit alone cannot pass a Lua case', () => {
  const row = plan[0], good = result(row); assert.equal(evaluate(row, good).state, 'passed');
  for (const bad of [{ ...good, status: 1 }, { ...good, error: new Error('failed') }, { ...good, handlesReaped: false }, { ...good, proof: undefined }, { ...good, stdout: Buffer.from('PASS unrelated\n') }]) assert.equal(evaluate(row, bad).state, 'failed');
});
test('every required owned cleanup proof is necessary; forced/output/active cases fail', () => {
  for (const key of ['rootReaped', 'treeReaped', 'handlesClosed', 'toolIdentityLocksClosed', 'outputReadersComplete']) {
    const r = result(plan[0]); r.proof[key] = false; assert.equal(processPassed(r), false, key);
  }
  for (const [key, value] of [['forced', true], ['outputLimit', true], ['activeProcesses', 1], ['deadlineHit', true], ['error', 'fault']]) {
    const r = result(plan[0]); r.proof[key] = value; assert.equal(processPassed(r), false, key);
  }
});
test('writer has exact original output condition; CRLF is accepted without weakening bytes', () => {
  const row = plan.at(-1); assert.equal(evaluate(row, result(row)).state, 'passed');
  assert.equal(evaluate(row, { ...result(row), stdout: Buffer.from(row.completion.replaceAll('\n', '\r\n')) }).state, 'passed');
  assert.equal(evaluate(row, { ...result(row), stdout: Buffer.from(`prefix ${row.completion}`) }).state, 'failed');
});
test('ordinary failed fixture does not prevent any of the remaining 219 executions', () => {
  let launches = 0;
  const rows = runCases(plan, { deadline: 1000000, now: () => 0, checkIdentity() {}, execute(row) { launches++; return { ...result(row), status: launches === 1 ? 1 : 0 }; } });
  assert.equal(launches, 220); assert.equal(rows[0].state, 'failed'); assert(rows.slice(1).every(row => row.state === 'passed'));
});
test('unverified process cleanup blocks all later launches and preserves 220 explicit outcomes', () => {
  let launches = 0;
  const rows = runCases(plan, { deadline: 1000000, now: () => 0, checkIdentity() {}, execute(row) { launches++; return { ...result(row), handlesReaped: false }; } });
  assert.equal(launches, 1); assert.equal(rows.length, 220); assert.equal(rows[0].state, 'failed'); assert(rows.slice(1).every(row => row.state === 'not_run'));
});
test('contradictory handlesReaped boolean cannot override failed process proof', () => {
  let launches = 0;
  const rows = runCases(plan, { deadline: 1000000, now: () => 0, checkIdentity() {}, execute(row) { launches++; return { ...result(row), proof: { ...proof(), rootReaped: false } }; } });
  assert.equal(launches, 1); assert.equal(rows[0].handlesReaped, false); assert(rows.slice(1).every(row => row.state === 'not_run'));
});
test('controller throw without receipt blocks later cases instead of pretending cleanup', () => {
  let launches = 0;
  const rows = runCases(plan, { deadline: 1000000, now: () => 0, checkIdentity() {}, execute() { launches++; throw new Error('launch transport lost'); } });
  assert.equal(launches, 1); assert.equal(rows[0].handlesReaped, false); assert(rows.slice(1).every(row => row.reason === 'formal-lua.execution_unverified'));
});
test('identity mutation after one case fails that case and records all subsequent not_run', () => {
  let checks = 0, launches = 0;
  const rows = runCases(plan, { deadline: 1000000, now: () => 0, checkIdentity() { if (++checks === 2) throw new Error('changed'); }, execute(row) { launches++; return result(row); } });
  assert.equal(launches, 1); assert.equal(rows[0].state, 'failed'); assert.equal(rows[0].identityError, 'changed'); assert(rows.slice(1).every(row => row.state === 'not_run'));
});
test('total time reserve and cumulative output budget reject launch rather than reset budgets', () => {
  for (const config of [{ deadline: LIMITS.cleanupMs + 1000, now: () => 0 }, { deadline: 1000000, now: () => 0, evidenceBytes: () => LIMITS.evidence - LIMITS.nextProcessReserve + 1 }]) {
    let launches = 0;
    const rows = runCases(plan, { checkIdentity() {}, execute(row) { launches++; return result(row); }, ...config });
    assert.equal(launches, 0); assert(rows.every(row => row.state === 'not_run'));
  }
});
test('case timeout is bounded by remaining aggregate time minus cleanup reserve', () => {
  const observed = [];
  runCases(plan.slice(0, 1), { deadline: 70000, now: () => 0, checkIdentity() {}, execute(row, timeout) { observed.push(timeout); return result(row); } });
  assert.deepEqual(observed, [5000]);
});
test('Lua init and search hook environment never enters interpreter processes', () => {
  assert.deepEqual(cleanLuaEnvironment({ PATH: 'system', LUA_INIT: 'evil()', lua_init_5_1: 'evil()', LUA_PATH: 'external/?.lua', LUA_CPATH: 'external/?.dll', NORMAL: 'keep' }), { PATH: 'system', NORMAL: 'keep', LUA_PATH: '', LUA_CPATH: '' });
});
test('all original Go functions are explicitly mapped; three bridge gaps cannot count as passed', () => {
  const map = JSON.parse(readFileSync(join(root, BASE, 'go-test-map.json')));
  const originals = fixed.fixtures.filter(row => row.path.endsWith('_test.go')).flatMap(row => [...readFileSync(join(root, BASE, 'oracle', row.path)).toString().matchAll(/^func (Test\w+)\(t \*testing.T\)/gm)].map(match => match[1])).sort();
  assert.deepEqual(map.tests.map(row => row.goTest).sort(), originals); assert.equal(map.tests.length, 9);
  assert.deepEqual(map.tests.filter(row => row.mode === 'deferred-rust-host-integration').map(row => row.goTest).sort(), BRIDGE_GAPS.map(row => row.test).sort());
  assert(BRIDGE_GAPS.every(row => row.state === 'not_run')); assert.equal(map.executionCases, plan.length);
});
test('build uses official MSVC interpreter sources and reuses owned controller, not Cargo or raw spawn', () => {
  const source = readFileSync(join(root, 'tools/rust-formal-lua.mjs')).toString();
  assert.match(source, /from '\.\/rust-owned-process\.mjs'/); assert.match(source, /from '\.\/luac\.mjs'/);
  assert.match(source, /msvcCompile/); assert.match(source, /cargoBuild: false/);
  assert(!source.includes('node:child_process')); assert(!source.includes('buildLuac(')); assert(!source.includes('writeIdentity'));
  assert.match(source, /'lua\.c'/); assert.match(source, /!\['luac\.c', 'print\.c'\]\.includes\(name\)/);
});

test('slow identity validation never launches after exhausting the durable deadline', () => {
  let time = 0, launches = 0;
  const rows = runCases(plan, { deadline: 100000, now: () => time, checkIdentity() { time = 110000; }, execute(row, timeout) { launches++; assert(timeout > 0); return result(row); } });
  assert.equal(launches, 0); assert(rows.every(row => row.state === 'not_run' && row.reason === 'formal-lua.total_deadline'));
});
test('real deadlineHit, forced retirement and cancellation stop all subsequent fixtures', () => {
  for (const flags of [{ deadlineHit: true }, { forced: true }, { forced: true, knownAuxiliaryCleanup: true }, { cancelled: true }, { canceled: true }]) {
    let launches = 0;
    const rows = runCases(plan, { deadline: 1000000, now: () => 0, checkIdentity() {}, execute(row) { launches++; return { ...result(row), proof: { ...proof(), ...flags } }; } });
    assert.equal(launches, 1); assert.equal(rows[0].state, 'failed'); assert(rows.slice(1).every(row => row.state === 'not_run' && row.reason === 'formal-lua.process_interrupted'));
  }
  const rows = runCases(plan, { deadline: 1000000, now: () => 0, checkIdentity() {}, execute(row) { return { ...result(row), signal: 'SIGTERM' }; } });
  assert.equal(rows[0].state, 'failed'); assert(rows.slice(1).every(row => row.state === 'not_run'));
});
function sealedFixture(directory, owner = 'formal-lua-build') {
  const id = '11111111-2222-4333-8444-555555555555', evidence = join(directory, '.tmp/rust-disk/evidence', id);
  mkdirSync(evidence, { recursive: true });
  const stem = join(evidence, 'process-aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee');
  const request = { command: owner === 'rust-build' ? 'cargo' : 'lua.exe', args: ['build', '--locked'], cwd: directory, cargoBuild: owner === 'rust-build', msvcCompile: false,
    stdout: stem + '.stdout.bin', stderr: stem + '.stderr.bin', receipt: stem + '.receipt.json' };
  const json = (path, value) => writeFileSync(path, JSON.stringify(value) + '\n');
  json(stem + '.request.json', request); json(stem + '.request.json.wrapper.json', { status: 0, signal: null });
  json(request.receipt, { ...proof(), schema: 'lycheedev.owned-process.v1', exitCode: 0, deadlineHit: false });
  writeFileSync(request.stdout, 'build output'); writeFileSync(request.stderr, '');
  const receipt = { schema: 'lycheedev.disk-owner.v1', id, owner, category: 'scratch', directory: join(directory, '.tmp/rust-scratch', id), state: 'retired', activePid: null, handlesReaped: true,
    sealedAt: '2026-10-14T00:00:00Z', command: [request.command, ...request.args], inputHashes: {} };
  const receiptPath = join(evidence, 'receipt.json');
  function seal() {
    const names = ['.request.json', '.request.json.wrapper.json', '.stdout.bin', '.stderr.bin', '.receipt.json'].map(suffix => stem + suffix);
    if (owner === 'rust-build') names.push(join(evidence, 'command.json'));
    receipt.commandSHA256 = hash(Buffer.from(JSON.stringify(receipt.command)));
    receipt.sealedLogs = names.map(path => ({ path, original: path, sha256: hash(readFileSync(path)) }));
    receipt.logHashes = Object.fromEntries(receipt.sealedLogs.map(row => [relative(directory, row.path), row.sha256])); json(receiptPath, receipt);
  }
  if (owner !== 'rust-build') seal();
  return { evidence, stem, request, json, receipt, receiptPath, seal };
}
test('all sealed build evidence, not just the report, is hash authenticated', () => scratch(directory => {
  const f = sealedFixture(directory); assert.equal(verifyOwnerEvidence(directory, f.receiptPath, 'formal-lua-build').processes.length, 1);
  writeFileSync(f.request.stdout, 'modified compiler log'); assert.throws(() => verifyOwnerEvidence(directory, f.receiptPath, 'formal-lua-build'), /sealed_log_identity/);
}));
test('missing, omitted, duplicate, extra and cross-owner build evidence is rejected', () => {
  for (const mutate of [
    f => rmSync(f.request.stderr),
    f => { f.receipt.sealedLogs = f.receipt.sealedLogs.filter(row => row.path !== f.request.receipt); },
    f => { f.receipt.sealedLogs.push(f.receipt.sealedLogs[0]); },
    f => writeFileSync(join(f.evidence, 'unsealed.txt'), 'not sealed'),
    f => { f.receipt.sealedLogs[0].path = join(dirname(f.evidence), 'another-owner', 'borrowed.json'); },
  ]) scratch(directory => { const f = sealedFixture(directory); mutate(f); f.json(f.receiptPath, f.receipt); assert.throws(() => verifyOwnerEvidence(directory, f.receiptPath, 'formal-lua-build')); });
});
test('sealed process outputs and receipts must remain within their exact request group', () => scratch(directory => {
  const f = sealedFixture(directory); f.request.stdout = f.request.stderr; f.json(f.stem + '.request.json', f.request); f.seal();
  assert.throws(() => verifyOwnerEvidence(directory, f.receiptPath, 'formal-lua-build'), /process_log_target/);
}));
test('even rehashed failed or unreaped build receipts cannot authenticate an artifact', () => {
  for (const bad of [{ exitCode: 1 }, { deadlineHit: true }, { rootReaped: false }, { forced: true }, { outputLimit: true }]) scratch(directory => {
    const f = sealedFixture(directory); f.json(f.request.receipt, { ...proof(), schema: 'lycheedev.owned-process.v1', exitCode: 0, ...bad }); f.seal();
    assert.throws(() => verifyOwnerEvidence(directory, f.receiptPath, 'formal-lua-build'));
  });
});
function candidateFixture(directory) {
  for (const name of ['crates', 'release', '.cargo']) mkdirSync(join(directory, name));
  writeFileSync(join(directory, 'Cargo.toml'), '[workspace]\n'); writeFileSync(join(directory, 'Cargo.lock'), '# fixture\n');
  const f = sealedFixture(directory, 'rust-build'), sourceTree = sourceDigest(directory), environmentSHA256 = 'a'.repeat(64);
  f.receipt.inputHashes = { sourceTree };
  f.json(join(f.evidence, 'command.json'), { command: 'cargo', args: f.request.args, cwd: directory, exit: 0, inputHashes: { sourceTree }, environmentSHA256 }); f.seal();
  const sourceBinary = join(directory, 'candidate.exe'); writeFileSync(sourceBinary, 'synthetic Node identity fixture, never executable');
  const artifact = { schema: 'lycheedev.rust-build-artifact.v1', sourceRoot: directory, sourceBinary, binarySHA256: hash(readFileSync(sourceBinary)), sameInvocation: true,
    command: ['cargo', ...f.request.args], inputHashes: { sourceTree }, environmentSHA256, buildEvidence: f.receiptPath };
  const artifactPath = join(directory, 'artifact.json'); f.json(artifactPath, artifact);
  return { ...f, artifact, artifactPath };
}
test('candidate source tree and exact successful build evidence authenticate together', () => scratch(directory => {
  const f = candidateFixture(directory); assert.equal(verifyCandidateArtifact(directory, f.artifactPath).sourceTree, sourceDigest(directory));
  writeFileSync(join(directory, 'Cargo.toml'), '[workspace]\n# changed after build\n');
  assert.throws(() => verifyCandidateArtifact(directory, f.artifactPath), /candidate_source_changed/);
}));
test('matching executable hash cannot bypass missing receipt or a different sourceRoot', () => {
  for (const mutate of [
    f => { f.artifact.buildEvidence = join(dirname(f.evidence), 'nonexistent', 'receipt.json'); },
    f => { f.artifact.sourceRoot = dirname(f.artifact.sourceRoot); },
    f => { f.artifact.inputHashes.sourceTree = '0'.repeat(64); },
    f => { f.artifact.command = ['build', '--different']; },
  ]) scratch(directory => {
    const f = candidateFixture(directory); mutate(f); f.json(f.artifactPath, f.artifact);
    assert.throws(() => verifyCandidateArtifact(directory, f.artifactPath));
  });
});

test('artifact command uses the actual producer cargo prefix exactly once', () => {
  const producer = readFileSync(join(root, 'tools/rust-build.mjs'), 'utf8');
  assert.match(producer, /sameInvocation:true,command:\['cargo',\.\.\.args\],buildEvidence:invocation\.buildEvidence/);
  scratch(directory => {
    const f = candidateFixture(directory);
    assert.deepEqual(f.artifact.command, ['cargo', ...f.request.args]);
    assert.deepEqual(f.receipt.command, f.artifact.command);
    verifyCandidateArtifact(directory, f.artifactPath);
  });
  for (const command of [['build', '--locked'], ['cargo', 'cargo', 'build', '--locked'], ['rustc', 'build', '--locked']]) scratch(directory => {
    const f = candidateFixture(directory); f.artifact.command = command; f.json(f.artifactPath, f.artifact);
    assert.throws(() => verifyCandidateArtifact(directory, f.artifactPath));
  });
});

// Current release authentication never weakens source-byte custody.
test('3.2.2 has only two exact retained release literals and unchanged historical oracle', () => {
  const m = manifest(root);
  assert.equal(m.oracle.commit, '0bec60e4406f5d5dc155ee25cef44f9a710e37e8');
  assert.deepEqual(m.retainedReleaseFields.map(row => row.path), ['addon/Core/Runtime.lua', 'addon/Lychee Dev.toc']);
  assert.equal(hash(readFileSync(join(root, 'tests/rust-parity/formal-lua/manifest.json'))), m.historicalOracle.manifestSHA256);
  assert.deepEqual(fixed.retainedReleaseFields, m.retainedReleaseFields);
});
test('retained release allowance rejects every additional source edit', () => copied(dir => {
  for (const row of manifest(dir).retainedReleaseFields) {
    const path = join(dir, row.path), original = readFileSync(path);
    writeFileSync(path, Buffer.concat([original, Buffer.from('\n-- changed business bytes\n')]));
    assert.throws(() => authenticate(dir), /retained_release_identity/);
    writeFileSync(path, original);
  }
  authenticate(dir);
}));
test('current static TOC requires both Retail interfaces and original client gate covers both', () => copied(dir => {
  const path = join(dir, 'addon/Lychee Dev.toc');
  writeFileSync(path, readFileSync(path, 'utf8').replace('120105, ', ''));
  assert.throws(() => verifyUnifiedTOC(dir), /interface_missing: 120105/);
  const gate = readFileSync(join(root, BASE, 'oracle/tests/addon/t_client_gate.lua'), 'utf8');
  assert(gate.includes('Env.testBuilds.retail'));
  assert(gate.includes('12.1.0'));
  assert(readFileSync(join(root, BASE, 'oracle/tests/addon/env.lua'), 'utf8').includes('120105'));
}));
