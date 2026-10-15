// Dependency-free metadata checks. These inspect source; they do not execute
// Rust, establish dispatch correctness, or replace the Windows CLI tests.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = path => readFileSync(new URL(path, import.meta.url), 'utf8');
const fixture = JSON.parse(read('../../../tests/rust-parity/go-3.2.0.manifest.json'));
const inventory = JSON.parse(read('../src/commands.json'));
const source = read('../src/contract.rs');

function explicitArities() {
  const body = source.match(/pub fn takes_value\(&self\) -> bool \{[\s\S]*?match self\.name\(\) \{([\s\S]*?)\n        \}/)?.[1];
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

test('shared definitions equal pinned Go metadata and ordering', () => {
  assert.equal(fixture.oracle.commit, '3057a9666ab078270b63c7f06de748069b0d1f4b');
  const expected = fixture.commands
    .map(({ parserFlags, ...definition }) => definition);
  assert.equal(expected.length, 84);
  // Two approved Source-engine description exceptions; no wildcard summary
  // skips, mutation of the fixed oracle, or relaxation of any other field.
  const doctor = expected.find(({ path }) => path === 'doctor');
  assert.equal(doctor.summary, 'Aggregate read-only workspace, data, LuaLS and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check');
  doctor.summary = 'Aggregate read-only workspace, data, embedded Emmy worker smoke and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check';
  const batch = expected.find(({ path }) => path === 'source batch');
  assert.equal(batch.summary, 'Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped LuaLS session; each item retains its own cursor and coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins');
  batch.summary = 'Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped native Emmy session; each item retains its own cursor and coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins';
  assert.deepEqual(inventory, expected);
});

test('all 115 explicit arities match frozen parser flags', () => {
  const expected = new Map();
  for (const command of fixture.commands) {
    for (const flag of command.parserFlags) {
      if (expected.has(flag.name)) assert.equal(expected.get(flag.name), flag.value);
      expected.set(flag.name, flag.value);
    }
  }
  assert.equal(expected.size, 115);
  assert.deepEqual(explicitArities(), expected);
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
