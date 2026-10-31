import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync, cpSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {stageSkill} from './rust-skill.mjs';

const commands = JSON.parse(readFileSync(resolve('crates/lycheedev-cli/src/commands.json')));
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-skill-contract-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  return root;
}
const run = (tool, args, cwd = process.cwd()) => spawnSync(process.execPath, [resolve(tool), ...args], {cwd, encoding: 'utf8', shell: false, timeout: 120000});
test('current Skill references resolve to current discovery', () => {
  const result = run('tools/skill-contract.mjs', []);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(JSON.parse(result.stdout).checkKind, 'static-reference-consistency');
});
test('unsupported commands, flags and missing packaged references fail', t => {
  const root = fixture(t);
  writeFileSync(join(root, 'SKILL.md'), '`lycheedev imaginary run`\n`update --delete-everything`\n[Missing](missing.md)\n');
  const result = run('tools/skill-contract.mjs', ['--skill-root', root]);
  assert.equal(result.status, 1);
  assert.equal(JSON.parse(result.stdout).violations.length, 3);
});
test('missing live group in an installed declaration remains visible', t => {
  const root = fixture(t), skill = join(root, 'skill');
  cpSync(resolve('skills/lycheedev'), skill, {recursive: true});
  const selected = commands.filter(command => !command.path.startsWith('live '));
  const path = join(root, 'commands.json');
  writeFileSync(path, JSON.stringify({commands: selected}));
  writeFileSync(join(root, 'describe'), 'console.log(' + JSON.stringify(JSON.stringify({ok: true, result: {commands: selected}})) + ');');
  for (const source of [['--contract-file', path], ['--bin', process.execPath]]) {
    const result = run('tools/skill-contract.mjs', ['--skill-root', skill, ...source], root);
    assert.equal(result.status, 1, result.stdout + result.stderr);
    assert(JSON.parse(result.stdout).violations.some(value => value.includes('live ')));
  }
});
test('staging preserves the whole Skill and reports actual missing capabilities', t => {
  const root = fixture(t), destination = join(root, 'skill');
  stageSkill(destination, commands.filter(command => command.path !== 'doctor'));
  assert.deepEqual(readFileSync(join(destination, 'SKILL.md')), readFileSync(resolve('skills/lycheedev/SKILL.md')));
  for (const name of readdirSync(resolve('skills/lycheedev/references'))) assert.deepEqual(readFileSync(join(destination, 'references', name)), readFileSync(resolve('skills/lycheedev/references', name)));
  const receipt = JSON.parse(readFileSync(destination + '.contract.json'));
  assert.deepEqual(receipt.missingCommands, ['doctor']);
  assert.deepEqual(receipt.missingFlags, []);
  assert.throws(() => stageSkill(destination, commands), /receipt_exists|EEXIST/);
});
test('staging creates missing CI parent directories without overwriting its Skill or receipt', t => {
  const root = fixture(t), destination = join(root, '.tmp', 'ci', 'build-release', 'skill', 'staged');
  stageSkill(destination, commands);
  assert.deepEqual(readFileSync(join(destination, 'SKILL.md')), readFileSync(resolve('skills/lycheedev/SKILL.md')));
  const receiptPath = destination + '.contract.json';
  const receipt = readFileSync(receiptPath);
  assert.equal(JSON.parse(receipt).schema, 'lycheedev.skill-declared-contract.v1');
  assert.deepEqual(JSON.parse(receipt).missingCommands, []);
  assert.deepEqual(JSON.parse(receipt).missingFlags, []);
  assert.throws(() => stageSkill(destination, commands), /receipt_exists/);
  assert.deepEqual(readFileSync(receiptPath), receipt);
});
test('invalid declarations and occupied destinations are never staged', t => {
  const root = fixture(t), destination = join(root, 'missing-parent', 'skill');
  for (const invalid of [[], [commands[0], commands[0]], [{...commands[0], flags: null}]]) assert.throws(() => stageSkill(destination, invalid), /invalid_contract/);
  assert.deepEqual(readdirSync(root), []);
  mkdirSync(destination, {recursive: true});
  writeFileSync(join(destination, 'sentinel'), 'preserve');
  assert.throws(() => stageSkill(destination, commands), {code: 'EEXIST'});
  assert.deepEqual(readdirSync(destination), ['sentinel']);
});
