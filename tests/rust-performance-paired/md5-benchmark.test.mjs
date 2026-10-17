// Synthetic parser/ownership-contract tests only. No Cargo or benchmark is run;
// generated sample clocks are not performance evidence.
import assert from 'node:assert/strict';
import test from 'node:test';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {BUILD_ARGS, ENCODED_FIXTURE, INPUTS, MODES, SCHEMA, SUPPORT, benchmarkPaths, extractFixtureRoot, loadBenchmarkFixture, orderFor, selectBenchmarkExecutable, verifyBuildEnvironment, verifyLegacyOutput, verifyStreamOutput} from '../../tools/rust-md5-benchmark.mjs';
import {digest} from './data-inputs.mjs';
const root = fileURLToPath(new URL('../..', import.meta.url));
const clone = value => structuredClone(value);
const jsonl = rows => Buffer.from(rows.map(row => JSON.stringify(row)).join('\n') + '\n');
function stream() {
  const rows = [{schema: SCHEMA, kind: 'contract', pairs: 6, chunkBytes: 65536, messages: 2, updatesPerPair: 273,
    inputConstructionAndAuthenticationTimed: false, correctness: {rfcVectors: 7, binaryBoundaryCases: 8, splitPolicies: 4, cancelCases: 10, deadlineCases: 10, cngInitialStopCases: 2}, cngStreaming: false}];
  for (let iteration = 0; iteration < 6; iteration++) for (const mode of orderFor(iteration)) for (const message of ['root', 'payload']) {
    const input = INPUTS[message], cloneNanos = mode === 'md5_clone' ? 2 : 0;
    rows.push({schema: SCHEMA, kind: 'sample', iteration, mode, message, bytes: input.bytes, updates: input.updates, md5: input.md5,
      sha256: mode === 'sha256_md5_consume' ? input.sha256 : null, initNanos: 1, updateNanos: 2, cloneNanos, finalizeNanos: 3, dropNanos: 4, totalNanos: 15 + cloneNanos, unattributedNanos: 5});
  }
  rows.push({schema: SCHEMA, kind: 'complete', samples: 36, correctnessPassed: true}); return rows;
}
test('all six alternating orders preserve separate Root and payload messages and all 36 samples', () => {
  const original = stream(), checked = verifyStreamOutput(jsonl(original));
  assert.deepEqual(checked.samples, original.slice(1, -1)); assert.equal(checked.samples.length, 36);
  assert.deepEqual(orderFor(0), MODES); assert.deepEqual(orderFor(1), [...MODES].reverse());
  assert.equal(INPUTS.payload.updates + INPUTS.root.updates, 273); assert.equal(INPUTS.payload.bytes / 65536, 272);
  assert.match(checked.scope, /No per-sample CPU/);
});
for (const [name, mutate] of [
  ['missing sample', rows => rows.splice(1, 1)], ['duplicate sample', rows => rows[2] = clone(rows[1])],
  ['missing terminal', rows => rows.pop()], ['failed correctness', rows => rows.at(-1).correctnessPassed = false],
  ['extra sample', rows => rows.splice(1, 0, clone(rows[1]))], ['hidden extra field', rows => rows[1].accepted = true],
  ['wrong mode ordering', rows => [rows[1], rows[3]] = [rows[3], rows[1]]], ['wrong message order', rows => [rows[1], rows[2]] = [rows[2], rows[1]]],
  ['wrong pair index', rows => rows[1].iteration = 1], ['combined Root and payload', rows => rows[1].bytes += INPUTS.payload.bytes],
  ['whole-slice payload', rows => rows[2].updates = 1], ['wrong Root MD5', rows => rows[1].md5 = INPUTS.payload.md5],
  ['wrong payload MD5', rows => rows[2].md5 = INPUTS.root.md5], ['missing dual SHA256', rows => rows[5].sha256 = null],
  ['pretended single hash SHA256', rows => rows[1].sha256 = INPUTS.root.sha256], ['negative timer', rows => rows[1].initNanos = -1],
  ['fractional timer', rows => rows[1].initNanos = .5], ['unsafe integer timer', rows => rows[1].initNanos = Number.MAX_SAFE_INTEGER + 1],
  ['clipped residual', rows => rows[1].unattributedNanos = 0], ['overlapping sum', rows => rows[1].totalNanos--],
  ['clone in consuming mode', rows => { rows[3].cloneNanos = 1; rows[3].totalNanos++; }],
  ['construction inside timer', rows => rows[0].inputConstructionAndAuthenticationTimed = true],
  ['missing RFC validation', rows => rows[0].correctness.rfcVectors = 0], ['missing deadline validation', rows => delete rows[0].correctness.deadlineCases],
  ['CNG streaming overclaim', rows => rows[0].cngStreaming = true], ['false message count', rows => rows[0].messages = 1],
]) test(`stream rejects ${name}`, () => { const rows = stream(); mutate(rows); assert.throws(() => verifyStreamOutput(jsonl(rows))); });
test('zero raw clocks stay zero without filtering or fabricated floors', () => {
  const rows = stream(); for (const row of rows.slice(1, -1)) for (const key of Object.keys(row).filter(key => key.endsWith('Nanos'))) row[key] = 0;
  assert.equal(verifyStreamOutput(jsonl(rows)).samples.length, 36);
});
test('stdout rejects empty rows, surplus chatter, invalid JSON and non-UTF8', () => {
  const raw = jsonl(stream());
  for (const value of [Buffer.concat([Buffer.from('\n'), raw]), Buffer.concat([raw, Buffer.from('debug\n')]), Buffer.from('{not json}\n'), Buffer.from([255]), Buffer.alloc((1 << 20) + 1)]) assert.throws(() => verifyStreamOutput(value));
});
const legacy = () => [1, 16, 64].map(mib => ({mib, rustCryptoMillis: 1.123, cngMillis: .456, sameDigest: true, ratio: .4061}));
test('legacy no-argument JSON rows retain the original exact fields and units', () => {
  const result = verifyLegacyOutput(jsonl(legacy())); assert.deepEqual(result.rows, legacy()); assert.match(result.scope, /CNG is not streaming/);
});
for (const [name, mutate] of [
  ['altered sizes', rows => rows[1].mib = 17], ['digest mismatch', rows => rows[0].sameDigest = false],
  ['negative time', rows => rows[1].cngMillis = -1], ['new schema', rows => rows[0].schema = SCHEMA], ['extra row', rows => rows.push(clone(rows[0]))],
]) test(`legacy rejects ${name}`, () => { const rows = legacy(); mutate(rows); assert.throws(() => verifyLegacyOutput(jsonl(rows))); });
const target = resolve('target');
const artifact = {reason: 'compiler-artifact', manifest_path: join(root, 'crates/lycheedev-windows/Cargo.toml'),
  target: {name: 'digest_benchmark', kind: ['example'], crate_types: ['bin'], src_path: join(root, 'crates/lycheedev-windows/examples/digest_benchmark.rs')},
  profile: {test: false, opt_level: '3', debug_assertions: false}, executable: join(target, 'release/examples/digest_benchmark.exe')};
const finished = {reason: 'build-finished', success: true};
test('selects the exact optimized example from current structured Cargo output', () => {
  const selected = selectBenchmarkExecutable(jsonl([artifact, finished]).toString(), target, root); assert.equal(selected.executable, artifact.executable);
  assert.deepEqual(BUILD_ARGS, ['build', '--release', '--locked', '--offline', '--target', 'x86_64-pc-windows-msvc', '-p', 'lycheedev-windows', '--example', 'digest_benchmark', '--message-format=json-render-diagnostics', '-j', '1']);
});
for (const [name, mutate] of [
  ['missing artifact', rows => rows.shift()], ['duplicate artifact', rows => rows.splice(0, 0, clone(rows[0]))],
  ['failed build', rows => rows[1].success = false], ['missing completion', rows => rows.pop()],
  ['CLI instead of example', rows => rows[0].target.kind = ['bin']], ['library example', rows => rows[0].target.crate_types = ['lib']],
  ['wrong example', rows => rows[0].target.name = 'foreign'], ['wrong package', rows => rows[0].manifest_path = join(root, 'crates/lycheedev-cli/Cargo.toml')],
  ['wrong example source', rows => rows[0].target.src_path = join(root, 'crates/foreign.rs')],
  ['debug profile', rows => rows[0].profile.opt_level = '0'], ['debug assertions', rows => rows[0].profile.debug_assertions = true],
  ['test target', rows => rows[0].profile.test = true], ['foreign executable', rows => rows[0].executable = resolve('foreign/digest_benchmark.exe')],
  ['relative executable', rows => rows[0].executable = 'target/release/examples/digest_benchmark.exe'], ['CLI executable', rows => rows[0].executable = join(target, 'release/lycheedev.exe')],
]) test(`Cargo rejects ${name}`, () => { const rows = clone([artifact, finished]); mutate(rows); assert.throws(() => selectBenchmarkExecutable(jsonl(rows).toString(), target, root)); });
function temporary(run) { const directory = mkdtempSync(join(tmpdir(), 'md5-benchmark-unit-')); try { return run(directory); } finally { rmSync(directory, {recursive: true, force: true}); } }
test('path admission requires exactly content fixture and fresh owned artifact directory', () => temporary(directory => {
  mkdirSync(join(directory, 'fixture'));
  assert.equal(benchmarkPaths(['fixture', 'output'], directory).fixtureDirectory, join(directory, 'fixture'));
  for (const args of [[], ['fixture'], ['fixture', 'out', 'extra'], ['fixture', '../out'], ['fixture', 'fixture/output'], ['fixture', 'fixture'], ['--flag', 'output'], ['fixture', '\0']]) assert.throws(() => benchmarkPaths(args, directory));
}));
test('path admission rejects symbolic link ancestors', () => temporary(directory => {
  mkdirSync(join(directory, 'real')); symlinkSync(join(directory, 'real'), join(directory, 'alias'), process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => benchmarkPaths(['alias', 'out'], directory));
}));
function rootFixture(directory) {
  // Reproduce only the fixed 44-byte Root wire bytes for extraction-unit tests.
  // This is never a replacement for loadDataFixture authentication at runtime.
  const raw = Buffer.alloc(44); raw.write('TSFM');
  [1, 0, 1, 0x10000000, 0x40, 501].forEach((value, index) => raw.writeUInt32LE(value, 4 + index * 4));
  Buffer.from(INPUTS.payload.md5, 'hex').copy(raw, 28);
  assert.equal(digest(raw), INPUTS.root.sha256); assert.equal(digest(raw, 'md5'), INPUTS.root.md5);
  const frame = Buffer.concat([Buffer.from('424c5445000000004e', 'hex'), raw]), ekey = digest(frame, 'md5');
  const archive = Buffer.concat([Buffer.alloc(30), frame]), index = Buffer.alloc(58);
  Buffer.from(ekey.slice(0, 18), 'hex').copy(index, 40); index.writeUInt32BE(0, 50); index.writeUInt32LE(archive.length, 54);
  const data = join(directory, 'game/Data/data'); mkdirSync(data, {recursive: true});
  writeFileSync(join(data, 'data.000'), archive); writeFileSync(join(data, '0000000001.idx'), index);
  return {fixture: {directory, identity: {content: {sha256: INPUTS.payload.sha256, contentKey: INPUTS.payload.md5, decodedBytes: INPUTS.payload.bytes, fillByte: 120}},
    objects: {[INPUTS.root.sha256]: {decodedBytes: 44, ckey: INPUTS.root.md5, ekey, encodedBytes: frame.length}}}, raw, archive, index, data};
}
test('extracts exact authenticated Root bytes through bounded archive/index/BLTE decoding', () => temporary(directory => {
  const {fixture, raw} = rootFixture(directory); assert.deepEqual(extractFixtureRoot(fixture), raw);
}));
for (const [name, mutate] of [
  ['unauthenticated Root', value => delete value.fixture.objects[INPUTS.root.sha256]],
  ['wrong content payload', value => value.fixture.identity.content.sha256 = 'f'.repeat(64)],
  ['wrong Root ckey', value => value.fixture.objects[INPUTS.root.sha256].ckey = '0'.repeat(32)],
  ['Root byte corruption', value => { value.archive[value.archive.length - 1] ^= 1; writeFileSync(join(value.data, 'data.000'), value.archive); }],
  ['index truncation', value => writeFileSync(join(value.data, '0000000001.idx'), value.index.subarray(0, -1))],
  ['archive overrun', value => { value.index.writeUInt32LE(value.archive.length + 1, 54); writeFileSync(join(value.data, '0000000001.idx'), value.index); }],
  ['index key mismatch', value => { value.index[40] ^= 1; writeFileSync(join(value.data, '0000000001.idx'), value.index); }],
  ['duplicate matching Root', value => writeFileSync(join(value.data, '0000000001.idx'), Buffer.concat([value.index, value.index.subarray(40)]))],
]) test(`Root extraction rejects ${name}`, () => temporary(directory => { const value = rootFixture(directory); mutate(value); assert.throws(() => extractFixtureRoot(value.fixture)); }));
test('controller statically preserves owned process, raw logs, source and fixture custody, and failure retention', () => {
  const source = readFileSync(join(root, 'tools/rust-md5-benchmark.mjs'), 'utf8');
  assert.match(source, /loadBenchmarkFixture\(paths.fixtureDirectory\)/);
  assert.equal([...source.matchAll(/loadBenchmarkFixture\(paths.fixtureDirectory\)/g)].length, 2);
  assert.match(source, /preflightBenchmarkFixture\(directory\);\n  const fixture = load\(directory, 'content'\);\n  preflightBenchmarkFixture\(directory\)/); assert.match(source, /extractFixtureRoot\(fixture\)/);
  assert.match(source, /runCargo\(root, \[\.\.\.BUILD_ARGS\]/); assert.match(source, /ownedSpawnSync\(/); assert.match(source, /startupDiagnostics\}\)/);
  assert.match(source, /invoke\('legacy-whole-slice', artifact.sourceBinary, \[\], true\)/);
  assert.match(source, /invoke\('fixture-stream', artifact.sourceBinary, \['--fixture-stream', rootFile\], true\)/);
  assert.match(source, /elapsed\(run.proof\)/); assert.match(source, /startupBreakdown\(run.proof, \{required: true\}\)/);
  assert.match(source, /toolchain\('rustc-before-build'\)/); assert.match(source, /toolchain\('rustc-after-benchmarks'\)/);
  assert.match(source, /verifyOwnerEvidence\(root, artifact.buildEvidence, 'md5-benchmark-build'\)/);
  assert.match(source, /job.seal\(/); assert.match(source, /if \(succeeded && handlesReaped\) job.cleanup\(\)/);
  assert(!/spawnSync\(|execFileSync\(|buildRust\(|--bin|--workspace|lycheedev-cli/.test(source));
  assert(SUPPORT.includes('tools/rust-build-auxiliary.json')); assert(SUPPORT.includes('tests/rust-performance-paired/data-inputs.mjs'));
});

test('optimized build rejects inherited optimization/compiler/profile overrides', () => {
  assert.doesNotThrow(() => verifyBuildEnvironment({PATH: 'tools', RUSTUP_TOOLCHAIN: '1.99.0', CARGO_HOME: 'cache', RUSTFLAGS: ''}));
  for (const name of ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_TARGET', 'CARGO_PROFILE_RELEASE_OPT_LEVEL', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS'])
    assert.throws(() => verifyBuildEnvironment({[name]: 'override'}), /uncontrolled_build_environment/);
});

test('stream rejects duplicate object members and ambiguous numeric spellings without filtering', () => {
  const raw = jsonl(stream()).toString();
  for (const changed of [raw.replace('"pairs":6', '"pairs":6,"pairs":6'), raw.replace('"initNanos":1', '"initNanos":1,"initNanos":1'),
    raw.replace('"correctnessPassed":true', '"correctnessPassed":true,"correctnessPassed":true'), raw.replace('"initNanos":1', '"initNanos":1.0'),
    raw.replace('"initNanos":1', '"initNanos":1e0'), raw.replace('"cloneNanos":0', '"cloneNanos":-0')])
    assert.throws(() => verifyStreamOutput(Buffer.from(changed)), /noncanonical_stream_json/);
});
test('legacy preserves fixed-decimal text and rejects duplicate or escaped flat members', () => {
  const raw = [1, 16, 64].map(mib => `{"mib":${mib},"rustCryptoMillis":1.000,"cngMillis":2.000,"sameDigest":true,"ratio":2.0000}`).join('\n') + '\n';
  assert.equal(verifyLegacyOutput(Buffer.from(raw)).rows.length, 3);
  for (const changed of [raw.replace('"mib":1', '"mib":1,"mib":1'), raw.replace('"ratio":2.0000', '"ratio":2.0000,"ratio":2.0000'), raw.replace('"mib"', '"\\u006dib"')])
    assert.throws(() => verifyLegacyOutput(Buffer.from(changed)), /legacy_duplicate/);
});

test('preflight fixes the only admitted content archive to three authenticated index records', () => {
  assert.equal(ENCODED_FIXTURE.index.bytes, 40 + 3 * 18); assert.equal(ENCODED_FIXTURE.archive.bytes, 174716);
  assert.equal(ENCODED_FIXTURE.index.sha256, '315bdd6da2f530e2c094c588be86ccde64723a1946f9a11bd9e7d3851afee800');
  assert.equal(ENCODED_FIXTURE.archive.sha256, 'cf6aae3657915c3ff64a06d01d218aa5841c1633e05a8ef438ef3c09457f4074');
});
for (const [name, size] of [['oversize index', 65536], ['duplicate fourth index record', 112], ['truncated index', 76], ['changed exact-length index', 94]])
  test(`preflight rejects ${name} before calling the fixture decoder`, () => temporary(directory => {
    const data = join(directory, 'game/Data/data'); mkdirSync(data, {recursive: true});
    writeFileSync(join(data, '0000000001.idx'), Buffer.alloc(size));
    // No archive exists: the index must be rejected before even reading it.
    let calls = 0;
    assert.throws(() => loadBenchmarkFixture(directory, () => { calls++; throw new Error('decoder must never run'); }), /bounded regular fixture file|fixed_index/);
    assert.equal(calls, 0);
  }));
