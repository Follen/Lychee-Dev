// Dependency-free metadata checks. These inspect source; they do not execute
// Rust, establish dispatch correctness, or replace the Windows CLI tests.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { loadCliContractOracle, verifyGo324CommandInheritance } from '../../../tests/rust-parity/cli-contract-oracle-3.2.4.mjs';

const read = path => readFileSync(new URL(path, import.meta.url), 'utf8');
const historicalBytes = read('../../../tests/rust-parity/go-3.2.0.manifest.json');
const fixture = loadCliContractOracle(historicalBytes);
const inventory = JSON.parse(read('../src/commands.json'));
const source = read('../src/contract.rs');

function explicitArities(contract = source) {
  const body = contract.match(/pub fn takes_value\(&self\) -> bool \{[\s\S]*?match self\.name\(\) \{([\s\S]*?)\n        \}/)?.[1];
  assert.ok(body, 'expected explicit name-based arity match');
  const armPattern = /((?:\s*"--[a-z-]+"\s*\|?)+)\s*=>\s*(?:\{\s*(false|true)\s*\}|(false|true)),?/g;
  const arms = [...body.matchAll(armPattern)];
  assert.equal(arms.length, 2, 'review changed arity implementation');
  const values = new Map();
  for (const [, names, braced, plain] of arms) {
    const value = braced ?? plain;
    for (const [, name] of names.matchAll(/"(--[a-z-]+)"/g)) {
      assert.ok(!values.has(name), `duplicate explicit arity ${name}`);
      values.set(name, value === 'true');
    }
  }
  const remaining = body.replace(armPattern, '').trim();
  assert.equal(remaining, 'name => panic!("flag missing explicit contract arity: {name}"),');
  return values;
}

function assertMetadata(actual) {
  assert.deepEqual(actual, fixture.expected);
}

test('shared definitions equal fixed Go 3.2.4 plus exact approved differences and ordering', () => {
  assert.equal(fixture.oracle.commit, '29bdbdaff5f064ec212ff2064380b1ccac652eda');
  assert.equal(fixture.oracle.release, 'Go v3.2.4');
  assert.equal(fixture.commands.length, 84);
  assert.equal(fixture.expected.length, 71);
  assertMetadata(inventory);
});

test('all 115 frozen arities plus Data prune, Agent TTL and protected inventory match parser flags', () => {
  const expected = new Map();
  for (const command of fixture.commands) {
    for (const flag of command.parserFlags) {
      if (expected.has(flag.name)) assert.equal(expected.get(flag.name), flag.value);
      expected.set(flag.name, flag.value);
    }
  }
  assert.equal(expected.size, 115);
  assert.ok(!expected.has('--data-derived'));
  expected.set('--data-derived', false);
  expected.set('--result-ttl-seconds', true);
  expected.set('--data', false);
  assert.deepEqual(explicitArities(), expected);
});

test('Data retirement is one opt-in prune flag with an exact bounded-scope description', () => {
  const frozen = fixture.commands.find(command => command.path === 'cache prune');
  const actual = inventory.find(command => command.path === 'cache prune');
  assert.deepEqual(actual.flags, [...frozen.flags, '--data-derived']);
  assert.equal(actual.summary, frozen.summary + '; --data-derived selects rebuildable Data objects and generations only (not downloads, shared blobs or CAP), with a remaining-byte target and distinct object-group attempt cap; staging cannot be combined');
  assert.deepEqual(inventory.filter(command => command.flags.includes('--data-derived')).map(command => command.path), ['cache prune']);
  assert.equal(explicitArities().get('--data-derived'), false);
  for (const mutate of [
    rows => rows.find(command => command.path === 'cache prune').flags.pop(),
    rows => rows.find(command => command.path === 'cache prune').flags.splice(-1, 1, '--data-derived <scope>'),
    rows => rows.find(command => command.path === 'cache status').flags.push('--data-derived'),
    rows => { rows.find(command => command.path === 'cache prune').summary = frozen.summary; },
  ]) {
    const rows = structuredClone(inventory);
    mutate(rows);
    assert.throws(() => assertMetadata(rows), {code: 'ERR_ASSERTION'});
  }
});

test('every current flag has a reviewed arity; enum and prose regressions', () => {
  const arities = explicitArities();
  for (const definition of inventory) {
    for (const display of definition.flags) {
      assert.ok(arities.has(display.split(/\s+/)[0]), `${definition.path}: ${display}`);
    }
  }
  assert.equal(arities.get('--encoding'), true);
  assert.equal(arities.get('--scan'), false);
  assert.equal(arities.get('--session-reuse'), false);
});

test('seven legacy paths are absent and formal doctor routes are registered', () => {
  for (const path of [
    'live ack', 'live bind', 'live finish', 'live hide', 'live probe load',
    'live reset', 'live run',
  ]) assert.ok(!inventory.some(definition => definition.path === path), path);
  for (const path of ['live doctor', 'live reload fallback']) {
    assert.ok(inventory.some(definition => definition.path === path), path);
  }
});

test('batch is registered only with its concrete source handler and exact flags', () => {
  const source = read('../../lycheedev-source/src/lib.rs');
  assert.match(source, /pub mod batch;/);
  assert.match(source, /"source batch" => return batch::dispatch\(inv, tools\)/);
  const entry = inventory.find(({path}) => path === 'source batch');
  const frozen = fixture.commands.find(({path}) => path === 'source batch');
  assert.deepEqual(entry.flags, frozen.flags);
  assert.equal(entry.mutates, true);
});


test('Source registry exposes the existing refs context and session handlers to describe', () => {
  const implementation = read('../../lycheedev-source/src/lib.rs');
  const registry = implementation.match(/pub fn commands\(\) -> &'static \[&'static str\] \{([\s\S]*?)\n\}/)?.[1];
  assert.ok(registry, 'recognizable Source registry');
  const actual = [...registry.matchAll(/"(source [^"]+)"/g)].map(match => match[1]);
  const expected = fixture.commands.filter(({path}) => path.startsWith('source ')).map(({path}) => path);
  assert.equal(new Set(actual).size, actual.length);
  assert.deepEqual([...actual].sort(), expected.sort());
  for (const name of ['source refs', 'source context', 'source session status', 'source session close']) {
    assert.ok(implementation.slice(implementation.indexOf('pub fn dispatch_with_services')).includes(`"${name}"`));
  }
});


test('oracle preserves historical bytes and rejects corrupt or relabeled fixtures', () => {
  const original = JSON.parse(historicalBytes);
  assert.deepEqual(fixture.commands, original.commands);
  assert.equal(fixture.historicalReference.commit, '3057a9666ab078270b63c7f06de748069b0d1f4b');
  assert.equal(read('../../../tests/rust-parity/go-3.2.0.manifest.json'), historicalBytes);
  for (const corrupt of [
    bytes => bytes + ' ',
    bytes => bytes.replace('3057a9666ab078270b63c7f06de748069b0d1f4b', fixture.oracle.commit),
    bytes => bytes.replace('"value": true', '"value": false'),
    bytes => bytes.replace('"mutates": true', '"mutates": false'),
  ]) assert.throws(() => loadCliContractOracle(corrupt(historicalBytes)), /historical CLI fixture identity/);
  assert.throws(() => verifyGo324CommandInheritance({commands: 'not fixed Go source', products: '', historicalProducts: ''}, historicalBytes), /Go 3.2.4 commands source identity/);
});

test('every command rejects missing duplicate and corrupted metadata, including the approved addition', () => {
  for (let index = 0; index < fixture.expected.length; index++) {
    const path = fixture.expected[index].path;
    const mutations = [
      ['missing', rows => rows.splice(index, 1)],
      ['duplicate', rows => rows.splice(index, 0, structuredClone(rows[index]))],
      ['path', rows => { rows[index].path += ' unapproved'; }],
      ['summary', rows => { rows[index].summary += ' '; }],
      ['mutates', rows => { rows[index].mutates = !rows[index].mutates; }],
      ['arguments', rows => { rows[index].arguments.push('<unapproved>'); }],
      ['missing flag', rows => { rows[index].flags.pop(); }],
      ['extra flag', rows => { rows[index].flags.push('--unapproved'); }],
      ['unknown field', rows => { rows[index].unapproved = true; }],
    ];
    if (fixture.expected[index].flags.length > 1) {
      mutations.push(['flag order', rows => { rows[index].flags.reverse(); }]);
    }
    for (const [field, mutate] of mutations) {
      const rows = structuredClone(fixture.expected);
      mutate(rows);
      assert.throws(() => assertMetadata(rows), {code: 'ERR_ASSERTION'}, `${path}: ${field}`);
    }
  }
});

test('command order, positional arguments, exact approved wording and addition placement remain mandatory', () => {
  const reversed = structuredClone(fixture.expected).reverse();
  assert.throws(() => assertMetadata(reversed), {code: 'ERR_ASSERTION'});
  const misplaced = structuredClone(fixture.expected);
  misplaced.push(...misplaced.splice(misplaced.findIndex(command => command.path === 'data cache prepare'), 1));
  assert.throws(() => assertMetadata(misplaced), {code: 'ERR_ASSERTION'});
  const extra = structuredClone(fixture.expected);
  extra.push({...structuredClone(extra[0]), path: 'data cache unapproved'});
  assert.throws(() => assertMetadata(extra), {code: 'ERR_ASSERTION'});
  for (const path of ['doctor', 'cache prune', 'source batch', 'source session status', 'source session close', 'source validate']) {
    const rows = structuredClone(fixture.expected);
    rows.find(command => command.path === path).summary = fixture.commands.find(command => command.path === path).summary;
    assert.throws(() => assertMetadata(rows), {code: 'ERR_ASSERTION'}, path);
  }
  for (let index = 0; index < fixture.expected.length; index++) {
    if (!fixture.expected[index].arguments.length) continue;
    const rows = structuredClone(fixture.expected);
    rows[index].arguments = [];
    assert.throws(() => assertMetadata(rows), {code: 'ERR_ASSERTION'}, rows[index].path);
  }
});

test('arity corruption, unknown flags and removal of fail-closed fallback are rejected', () => {
  const expected = explicitArities();
  for (const [name, value] of expected) {
    // Move this literal into the opposite Rust match arm, then parse the
    // corrupted source through the same checker as the real implementation.
    let corrupted = source.replace(new RegExp(`"${name}"\\s*\\|`), '');
    if (corrupted === source) corrupted = source.replace(new RegExp(`\\|\\s*"${name}"`), '');
    assert.notEqual(corrupted, source, `${name}: removed original literal`);
    const anchor = value ? '--allow-partial' : '--after-id';
    corrupted = corrupted.replace(`"${anchor}"`, `"${name}" | "${anchor}"`);
    const actual = explicitArities(corrupted);
    assert.equal(actual.size, 118);
    assert.equal(actual.get(name), !value, `${name}: opposite source arm`);
    assert.throws(() => assert.deepEqual(actual, expected), {code: 'ERR_ASSERTION'}, name);
  }
  const extra = source.replace('"--allow-partial"', '"--unreviewed" | "--allow-partial"');
  assert.throws(() => assert.deepEqual(explicitArities(extra), expected), {code: 'ERR_ASSERTION'});
  const missing = source.replace('"--allow-partial" | ', '');
  assert.throws(() => assert.deepEqual(explicitArities(missing), expected), {code: 'ERR_ASSERTION'});
  const duplicate = source.replace('"--allow-partial"', '"--allow-partial" | "--allow-partial"');
  assert.throws(() => explicitArities(duplicate), /duplicate explicit arity/);
  assert.throws(() => explicitArities(source.replace('name => panic!("flag missing explicit contract arity: {name}"),', '_ => false,')), {code: 'ERR_ASSERTION'});
});

test('protected Data inventory is only a boolean cache status flag', () => {
  const frozen = fixture.commands.find(command => command.path === 'cache status');
  const actual = inventory.find(command => command.path === 'cache status');
  assert.deepEqual(actual.flags, ['--data', ...frozen.flags]);
  assert.equal(actual.summary, frozen.summary);
  assert.deepEqual(inventory.filter(command => command.flags.includes('--data')).map(command => command.path), ['cache status']);
  assert.equal(explicitArities().get('--data'), false);
});
