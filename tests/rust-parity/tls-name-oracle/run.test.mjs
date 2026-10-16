import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {beginScratch, sha256} from '../../../tools/rust-disk.mjs';
import {runCargo, sourceDigest} from '../../../tools/rust-build.mjs';
import {CARGO_ARGS, runGo, runRust, sealedRust} from './run.mjs';
import {compare, MARKER, NAMES, rustObservations} from './compare.mjs';

function observations() {
  const go = NAMES.map((input, i) => ({input, sni: i === 4 ? input : 'example.test', accepted: i < 2,
    phase: i < 2 ? 'complete' : 'tls', goVersion: 'go1.27.1', goos: 'windows', trust: 'private test root only'}));
  const rust = go.map(row => ({input: row.input, sni: row.sni, accepted: row.accepted, os: 'windows',
    trust: row.trust, stop: null, error: row.accepted ? null : {details: {phase: 'tls'}}}));
  return {go, rust};
}
const output = rows => rows.map(row => `${MARKER}${JSON.stringify(row)}\n`).join('');
function fixture(exit = 0) {
  const root = mkdtempSync(join(tmpdir(), 'lychee-tls-node-'));
  function write(path, value) { const full = join(root, path); mkdirSync(dirname(full), {recursive: true}); writeFileSync(full, value); }
  for (const directory of ['crates', 'release', '.cargo']) mkdirSync(join(root, directory));
  for (const name of ['Cargo.toml', 'Cargo.lock', '.cargo/config.toml',
    'tests/rust-parity/tls-name-oracle/main.go', 'tests/rust-parity/tls-name-oracle/run.mjs', 'tests/rust-parity/tls-name-oracle/compare.mjs',
    ...['root.pem', 'server.pem', 'server-key.pem', 'server.p12'].map(n => `crates/lycheedev-data/src/remote/transport/raw_uri/tls_fixtures/${n}`)]) write(name, 'synthetic Node fixture; never executable');
  write('go.exe', 'not an executable; injected executor only');
  // Synthetic-only Cargo execution below needs no build space reserve beyond
  // runCargo's ordinary admission; do not depend on the host's free reserve.
  write('release/rust-disk-policy.json', JSON.stringify({schema: 'lycheedev.disk-policy.v1', freeReserveBytes: 0}));
  const job = beginScratch(root, 'ci-data-remote', {command: ['cargo', ...CARGO_ARGS], inputs: {sourceTree: sourceDigest(root)}});
  const stdout = join(job.evidence, 'stdout.bin'), stderr = join(job.evidence, 'stderr.bin'), command = join(job.evidence, 'command.json');
  writeFileSync(stdout, `running synthetic fixture\n${output(observations().rust)}test result: synthetic\n`);
  writeFileSync(stderr, '');
  writeFileSync(command, JSON.stringify({command: 'cargo', args: CARGO_ARGS, cwd: root, oneShot: true, exit, inputHashes: {sourceTree: sourceDigest(root)}}));
  job.seal({state: exit ? 'failed' : 'succeeded', handlesReaped: true, logs: [command, stdout, stderr]});
  job.cleanup();
  return {root, job, stdout, command, cleanup() { rmSync(root, {recursive: true, force: true}); }};
}
function mutateReceipt(f, update) {
  const value = JSON.parse(readFileSync(f.job.receipt)); update(value); writeFileSync(f.job.receipt, JSON.stringify(value));
}
function executor(f, run = {}) {
  const calls = [];
  const execute = (command, args, options) => {
    calls.push({command, args, options});
    assert.equal(options.env.GOTOOLCHAIN, 'local'); assert.equal(options.env.GOPROXY, 'off');
    assert.equal(options.env.GOSUMDB, 'off'); assert.equal(options.env.GOWORK, 'off');
    assert.equal(options.env.CGO_ENABLED, '0'); assert.equal(options.env.GOENV, 'off');
    assert.equal(options.env.GOFLAGS, ''); assert.equal(options.env.GO111MODULE, 'off');
    assert.equal(dirname(options.env.GOCACHE), dirname(options.env.GOTMPDIR));
    assert.equal(dirname(dirname(options.env.GOCACHE)), join(f.root, '.tmp/rust-scratch'));
    assert(existsSync(options.env.GOCACHE));
    const version = args[0] === 'version';
    assert.equal(options.timeout, version ? 15000 : 120000);
    assert.equal(options.orphanMs, 5000);
    const record = join(options.evidence, `fake-process-${calls.length}.json`);
    writeFileSync(record, JSON.stringify({synthetic: true, command, args}));
    return {status: 0, handlesReaped: true, proof: {binary: join(f.root, 'go.exe')}, stderr: Buffer.alloc(0),
      stdout: Buffer.from(version ? 'go version go1.27.1 windows/amd64\r\n' : JSON.stringify(observations().go)), logs: [record],
      ...(version ? {} : run)};
  };
  return {execute, calls};
}

test('comparison admits all five actual records and reports each observed difference', () => {
  const {go, rust} = observations();
  assert.equal(compare(go, rustObservations(`prefix noise\n${output(rust)}`)).differences.length, 0);
  rust[2].sni = 'example.test.'; rust[2].accepted = true; rust[3].error.details.phase = 'connect';
  rust[4].stop = {code: 'command.cancelled'};
  assert.deepEqual(compare(go, rust).differences.map(d => d.field ?? d.stage), ['sni', 'accepted', 'rust', 'rust-control']);
});
test('comparison refuses missing, duplicate, foreign platform/version/trust and malformed records', () => {
  for (const mutate of [x => x.go.pop(), x => x.rust[1] = x.rust[0], x => x.go[0].goVersion = 'go1.28',
    x => x.go[0].goos = 'linux', x => x.rust[0].os = 'linux', x => x.rust[0].trust = 'system', x => x.go[0].accepted = 'true']) {
    const x = observations(); mutate(x); assert.throws(() => compare(x.go, x.rust));
  }
  assert.throws(() => rustObservations(`${MARKER}{broken}`));
});
test('exact receipt is selected without scanning or adopting another owner', () => {
  const f = fixture(); try {
    const got = sealedRust(f.root, f.job.id);
    assert.equal(got.stdout, f.stdout); assert.equal(got.exit, 0);
    assert.throws(() => sealedRust(f.root, '../other'), /one exact UUID/);
    mutateReceipt(f, r => r.owner = 'ci-another-test'); assert.throws(() => sealedRust(f.root, f.job.id));
  } finally { f.cleanup(); }
});
test('active, unreaped, wrong command, changed source and unsealed stdout are rejected', () => {
  const edits = [r => r.state = 'active', r => r.handlesReaped = false,
    r => r.command = ['cargo', 'test'], r => r.inputHashes.sourceTree = '0'.repeat(64),
    r => r.sealedLogs = r.sealedLogs.filter(l => !l.path.endsWith('stdout.bin'))];
  for (const edit of edits) { const f = fixture(); try { mutateReceipt(f, edit); assert.throws(() => sealedRust(f.root, f.job.id)); } finally { f.cleanup(); } }
});
test('sealed log corruption and a path outside that exact owner are rejected', () => {
  const f = fixture(); try {
    writeFileSync(f.stdout, 'corrupt'); assert.throws(() => sealedRust(f.root, f.job.id), /integrity/);
    writeFileSync(f.stdout, 'outside');
    mutateReceipt(f, r => { r.sealedLogs[0].path = join(f.root, 'Cargo.toml'); r.sealedLogs[0].sha256 = sha256(readFileSync(join(f.root, 'Cargo.toml'))); });
    assert.throws(() => sealedRust(f.root, f.job.id), /outside_owned_root/);
  } finally { f.cleanup(); }
});
test('sealed command metadata must describe the same one-shot invocation', () => {
  const f = fixture(); try {
    const command = JSON.parse(readFileSync(f.command)); command.oneShot = false; writeFileSync(f.command, JSON.stringify(command));
    mutateReceipt(f, r => r.sealedLogs.find(l => l.path === f.command).sha256 = sha256(readFileSync(f.command)));
    assert.throws(() => sealedRust(f.root, f.job.id));
  } finally { f.cleanup(); }
});
test('original Rust group is invoked exactly once and a sealed failed group remains failed', () => {
  const f = fixture(101); let calls = 0;
  try {
    const result = runRust(f.root, (root, args, options) => {
      ++calls; assert.equal(root, f.root); assert.deepEqual(args, CARGO_ARGS);
      assert.deepEqual(options, {owner: 'ci-data-remote', oneShot: true});
      throw Object.assign(new Error('build.cargo_failed:101'), {buildEvidence: f.job.receipt});
    });
    assert.equal(calls, 1); assert.equal(result.passed, false); assert.equal(result.rust.id, f.job.id);
    const absent = runRust(f.root, () => { throw new Error('disk.admission'); });
    assert.equal(absent.rust, null); assert.equal(absent.passed, false);
  } finally { f.cleanup(); }
});
test('actual runCargo receipt and command metadata are accepted on success and failure', () => {
  for (const status of [0, 101]) {
    const f = fixture(); let calls = 0;
    try {
      const result = runRust(f.root, (root, args, options) => runCargo(root, args, {...options,
        execute(command, actualArgs) {
          ++calls; assert.equal(command, 'cargo'); assert.deepEqual(actualArgs, CARGO_ARGS);
          return {status, handlesReaped: true, stdout: Buffer.from(output(observations().rust)), stderr: Buffer.from('synthetic compiler diagnostic')};
        }}));
      assert.equal(calls, 1); assert.equal(result.passed, status === 0);
      assert.notEqual(result.rust.id, f.job.id); assert.equal(result.rust.exit, status);
      assert.equal(JSON.parse(readFileSync(result.rust.path)).state, 'retired');
      assert.equal(rustObservations(readFileSync(result.rust.stdout, 'utf8')).length, 5);
    } finally { f.cleanup(); }
  }
});
test('owned Go uses exact sealed Rust output, private registered cache, and retires only its scratch', () => {
  const f = fixture(101); try {
    const fake = executor(f); const result = runGo(f.root, f.job.id, fake.execute);
    assert.equal(result.state, 'succeeded'); assert.equal(result.rust.exit, 101); // Whole Rust group failure is never rewritten.
    assert.equal(fake.calls.length, 2); assert.equal(fake.calls[1].command, join(f.root, 'go.exe'));
    assert.equal(result.comparison.cases, 5);
    const receipt = JSON.parse(readFileSync(result.receipt));
    assert.equal(receipt.state, 'retired'); assert.equal(receipt.handlesReaped, true);
    assert.equal(existsSync(receipt.directory), false); assert(existsSync(f.stdout));
    assert(receipt.sealedLogs.some(l => l.path.endsWith('go-results.json')));
    assert(receipt.sealedLogs.some(l => l.path.endsWith('comparison.json')));
    for (const log of receipt.sealedLogs) assert.equal(sha256(readFileSync(log.path)), log.sha256);
  } finally { f.cleanup(); }
});
test('Go mismatches and nonzero oracle exits preserve all five observations and fail', () => {
  const f = fixture(); try {
    const go = observations().go; go[2].accepted = true;
    const fake = executor(f, {status: 1, stdout: Buffer.from(JSON.stringify(go))});
    const result = runGo(f.root, f.job.id, fake.execute);
    assert.equal(result.state, 'failed'); assert.equal(result.comparison.differences.length, 1);
    assert.equal(result.errors.length, 2);
    const receipt = JSON.parse(readFileSync(result.receipt));
    assert.equal(receipt.state, 'failed'); assert(existsSync(receipt.directory));
    assert(existsSync(join(receipt.directory, 'go-cache')));
    assert.equal(JSON.parse(readFileSync(join(dirname(result.receipt), 'go-results.json'))).length, 5);
  } finally { f.cleanup(); }
});
test('wrong Go version prevents build; missing Rust observations fail rather than skip', () => {
  const f = fixture(); try {
    const fake = executor(f);
    const version = runGo(f.root, f.job.id, (command, args, options) => ({...fake.execute(command, args, options), stdout: Buffer.from('go version go1.28 windows/amd64')}));
    assert.equal(version.state, 'failed'); assert.equal(fake.calls.length, 1);
    writeFileSync(f.stdout, 'no TLS output');
    mutateReceipt(f, r => r.sealedLogs.find(l => l.path === f.stdout).sha256 = sha256(readFileSync(f.stdout)));
    const missing = runGo(f.root, f.job.id, executor(f).execute);
    assert.equal(missing.state, 'failed'); assert(missing.errors.some(e => e.includes('finite cases')));
  } finally { f.cleanup(); }
});
test('unreaped or thrown Go execution keeps owned scratch and records interruption', () => {
  for (const throwing of [false, true]) {
    const f = fixture(); try {
      const fake = executor(f, {handlesReaped: false, status: null});
      const result = runGo(f.root, f.job.id, throwing ? () => { throw Error('executor lost'); } : fake.execute);
      assert.equal(result.state, 'interrupted'); assert.equal(result.handlesReaped, false);
      const receipt = JSON.parse(readFileSync(result.receipt)); assert.equal(receipt.state, 'interrupted'); assert(existsSync(receipt.directory));
    } finally { f.cleanup(); }
  }
});
test('CI pins official Go, reuses the remote owner output, and never adds a second Rust test run', () => {
  const root = resolve(import.meta.dirname, '../../..');
  const workflow = readFileSync(join(root, '.github/workflows/rust-parity.yml'), 'utf8');
  assert(workflow.includes('actions/setup-go@40f1582b2485089dde7abd97c1529aa768e1baff'));
  assert(workflow.includes("go-version: '1.27.1'\n          cache: false"));
  assert.equal(workflow.split('node tests/rust-parity/tls-name-oracle/run.mjs rust').length - 1, 1);
  assert.equal(workflow.split('node tests/rust-parity/tls-name-oracle/run.mjs go').length - 1, 1);
  assert(workflow.includes('LYCHEEDEV_TLS_RUST_OWNER: ${{ steps.data_remote.outputs.tls_rust_owner }}'));
  assert(workflow.includes('tests/rust-parity/tls-name-oracle/run.test.mjs'));
  assert(!workflow.includes('rust-cargo.mjs --owner ci-data-remote '));
  assert(CARGO_ARGS.includes('--nocapture'));
  assert(!workflow.includes("steps.data_remote.outcome == 'success'"));
  assert(!workflow.includes("steps.data_remote.outputs.tls_rust_owner != ''"));
});

// Pinned dependency custody is checked by the existing runner-test group.
import './adaptation.test.mjs';
