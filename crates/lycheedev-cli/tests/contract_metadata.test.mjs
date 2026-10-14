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
    .filter(({ path }) => !['live doctor', 'source batch'].includes(path))
    .map(({ parserFlags, ...definition }) => definition);
  assert.equal(expected.length, 82);
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

test('seven legacy paths and two missing handlers are absent from public inventory', () => {
  for (const path of [
    'live ack', 'live bind', 'live finish', 'live hide', 'live probe load',
    'live reset', 'live run', 'live doctor', 'source batch',
  ]) assert.ok(!inventory.some(definition => definition.path === path), path);
});
