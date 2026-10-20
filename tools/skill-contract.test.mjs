import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync, existsSync, cpSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { stageSkill } from './rust-skill.mjs';
import { loadCliContractOracle } from '../tests/rust-parity/cli-contract-oracle-3.2.3.mjs';

const contractFile = resolve('tests/rust-parity/go-3.2.0.manifest.json');
const oracle = JSON.parse(readFileSync(contractFile, 'utf8'));
const historicalDirectory = resolve('tests/rust-skill-usage/baselines/go3.2.2-commands.md');
const staticCheck = root => spawnSync(process.execPath, [resolve('tools/skill-contract.mjs'), '--skill-root', root, '--contract-file', contractFile], { encoding: 'utf8', shell: false, timeout: 120000 });
// Current docs use independently enumerated approved extensions, never the
// candidate commands.json as its own oracle. Historical fixtures stay frozen.
function currentStaticCheck(root, mutate = () => {}) {
  const temporary = mkdtempSync(join(tmpdir(), 'lycheedev-current-contract-'));
  try {
    const commands = loadCliContractOracle(readFileSync(contractFile)).expected;
    mutate(commands);
    const contract = join(temporary, 'approved.json');
    writeFileSync(contract, JSON.stringify({commands}));
    return spawnSync(process.execPath, [resolve('tools/skill-contract.mjs'), '--skill-root', root, '--contract-file', contract], {encoding: 'utf8', shell: false, timeout: 120000});
  } finally { rmSync(temporary, {recursive: true, force: true}); }
}


test('skill contract rejects unsupported workflow commands and missing packaged references', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-skill-contract-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'references'));
  writeFileSync(join(root, 'references/workflow.md'), '`update --plan`\n`cache status`\n`config show`\n');
  writeFileSync(join(root, 'SKILL.md'), '[Workflow](references/workflow.md)\nComponents: `skill` and `addon`.\n');
  const run = () => staticCheck(root);
  let result = run();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  writeFileSync(join(root, 'references/workflow.md'), '`update --delete-everything`\n`config imaginary`\n');
  writeFileSync(join(root, 'SKILL.md'), '[Missing](references/not-packaged.md)\n');
  result = run();
  assert.equal(result.status, 1);
  assert.equal(JSON.parse(result.stdout).violations.length, 3);
});

test('generated command directory check detects a stale artifact without rewriting it', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-skill-generated-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const output = join(root, 'commands.md');
  const run = extra => spawnSync(process.execPath, [resolve('tools/skill-commands.mjs'), '--target', '--output', output, ...extra], { encoding: 'utf8', shell: false, timeout: 120000 });
  let result = run([]); assert.equal(result.status, 0, result.stderr);
  result = run(['--check']); assert.equal(result.status, 0, result.stderr);
  writeFileSync(output, 'outdated generated commands\n');
  result = run(['--check']); assert.notEqual(result.status, 0);
  assert.equal(readFileSync(output, 'utf8'), 'outdated generated commands\n');
});


test('historical test fixture retains every fixed target command, argument and flag', () => {
  assert.equal(oracle.oracle.commit, '3057a9666ab078270b63c7f06de748069b0d1f4b');
  const markdown = readFileSync(historicalDirectory, 'utf8');
  const actual = [...markdown.matchAll(/^- `([^`]+)` — (mutates|read-only)\.[^\n]*\n  Flags: (.+)$/gm)].map(match => ({
    invocation: match[1], mutates: match[2] === 'mutates',
    flags: [...match[3].matchAll(/`([^`]+)`/g)].map(flag => flag[1]),
  })).sort((a, b) => a.invocation.localeCompare(b.invocation));
  const expected = oracle.commands.map(command => ({
    invocation: [command.path, ...(command.arguments ?? [])].join(' '),
    mutates: command.mutates, flags: command.flags,
  })).sort((a, b) => a.invocation.localeCompare(b.invocation));
  assert.deepEqual(actual, expected);
  const result = currentStaticCheck(resolve('skills/lycheedev'));
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(JSON.parse(result.stdout).checkKind, 'static-reference-consistency');
});

test('Rust staging retains the full workflow even when the candidate lacks target commands', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-full-skill-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const destination = join(root, 'skill');
  const declared = oracle.commands.filter(command => !['live doctor', 'source batch'].includes(command.path));
  stageSkill(destination, declared);
  for (const file of readdirSync(resolve('skills/lycheedev/references'))) {
    assert.deepEqual(readFileSync(join(destination, 'references', file)), readFileSync(resolve('skills/lycheedev/references', file)), file);
  }
  assert.deepEqual(readFileSync(join(destination, 'scripts/lycheedev.mjs')), readFileSync(resolve('skills/lycheedev/scripts/lycheedev.mjs')));
  assert.deepEqual(readFileSync(join(destination, 'agents/openai.yaml')), readFileSync(resolve('skills/lycheedev/agents/openai.yaml')));
  const entry = readFileSync(join(destination, 'SKILL.md'), 'utf8');
  assert.equal(entry, readFileSync(resolve('skills/lycheedev/SKILL.md'), 'utf8'));
  assert.equal(existsSync(join(destination, 'references/candidate-commands.md')), false);
  assert.equal(existsSync(join(destination, 'references/candidate-contract.json')), false);
  const saved = JSON.parse(readFileSync(`${destination}.contract.json`, 'utf8'));
  assert.deepEqual(saved.commands, declared);
  const result = currentStaticCheck(destination);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  // Missing capabilities remain explicit without turning help into instructions.
  assert.deepEqual(saved.missingCommands, ['live doctor', 'source batch']);
  assert.deepEqual(saved.missingFlags, []);
  assert.equal(saved.targetCommit, '0bec60e4406f5d5dc155ee25cef44f9a710e37e8');
  const before = readFileSync(join(destination, 'SKILL.md'));
  assert.throws(() => stageSkill(destination, declared), /receipt_exists|EEXIST/);
  assert.deepEqual(readFileSync(join(destination, 'SKILL.md')), before);
});

test('Rust staging rejects malformed declarations before creating a destination', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-invalid-skill-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const command = oracle.commands[0];
  for (const invalid of [[], [command, command], [{ ...command, flags: null }], [{ ...command, arguments: 'bad' }]]) {
    assert.throws(() => stageSkill(join(root, 'skill'), invalid), /rust-skill.invalid_contract/);
  }
  assert.deepEqual(readdirSync(root), []);
});


test('entire missing command groups and root commands fail both static and binary declaration gates', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-missing-groups-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const skillRoot = join(root, 'skill');
  cpSync(resolve('skills/lycheedev'), skillRoot, {recursive: true});
  // Exhaustive directory parsing belongs to a fixture, not the shipped Skill.
  writeFileSync(join(skillRoot, 'references/commands.md'), readFileSync(historicalDirectory));
  for (const omitted of ['live ', 'source ', 'update']) {
    const commands = oracle.commands.filter(command => !command.path.startsWith(omitted));
    const file = join(root, 'contracts.json');
    writeFileSync(file, JSON.stringify({ commands }));
    // Node is a test-only CLI fixture: its describe script emits declarations.
    // This exercises --bin transport without claiming real candidate execution.
    writeFileSync(join(root, 'describe'), `console.log(${JSON.stringify(JSON.stringify({ ok: true, result: { commands } }))});`);
    for (const input of [['--contract-file', file], ['--bin', process.execPath]]) {
      const result = spawnSync(process.execPath, [resolve('tools/skill-contract.mjs'), '--skill-root', skillRoot, ...input], {
        cwd: root, encoding: 'utf8', shell: false, timeout: 120000,
      });
      assert.equal(result.status, 1, `${omitted}: ${result.stdout} ${result.stderr}`);
      const violations = JSON.parse(result.stdout).violations;
      assert.ok(violations.some(value => value.includes(`command "${omitted.trim()}`)), violations.join('\n'));
    }
  }
});

test('explicit unknown command roots cannot disappear as ordinary prose', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-unknown-command-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  writeFileSync(join(root, 'SKILL.md'), '`lycheedev imaginary run`\n`skill` and `addon` are components.\n');
  const result = staticCheck(root);
  assert.equal(result.status, 1);
  const violations = JSON.parse(result.stdout).violations;
  assert.equal(violations.length, 1);
  assert.ok(violations[0].includes('imaginary run'));
});

test('candidate command generation cannot implicitly overwrite the fixed target', () => {
  const target = historicalDirectory;
  const before = readFileSync(target);
  const result = spawnSync(process.execPath, [resolve('tools/skill-commands.mjs'), '--bin', process.execPath], {
    encoding: 'utf8', shell: false, timeout: 120000,
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /--bin requires an explicit --output/);
  assert.deepEqual(readFileSync(target), before);
});


test('directory-only flags remain bound to their command in both declaration gates', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-missing-flags-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const skillRoot = join(root, 'skill');
  mkdirSync(skillRoot);
  writeFileSync(join(skillRoot, 'SKILL.md'), '- `version` — read-only. Version test.\n  Flags: `--home <root>`, `--format text|json|jsonl`.\n');
  const commands = oracle.commands.map(command => command.path === 'version' ? { ...command, flags: [] } : command);
  const file = join(root, 'contracts.json');
  writeFileSync(file, JSON.stringify({ commands }));
  writeFileSync(join(root, 'describe'), `console.log(${JSON.stringify(JSON.stringify({ ok: true, result: { commands } }))});`);
  for (const input of [['--contract-file', file], ['--bin', process.execPath]]) {
    const result = spawnSync(process.execPath, [resolve('tools/skill-contract.mjs'), '--skill-root', skillRoot, ...input], {
      cwd: root, encoding: 'utf8', shell: false, timeout: 120000,
    });
    assert.equal(result.status, 1, result.stdout + result.stderr);
    const violations = JSON.parse(result.stdout).violations.join('\n');
    assert.ok(violations.includes('flag --home is not accepted by "version"'), violations);
    assert.ok(violations.includes('flag --format is not accepted by "version"'), violations);
  }
});


test('staging records omitted root/group flags without modifying Skill payload', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-stage-gaps-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const commands = oracle.commands.filter(command => !command.path.startsWith('live '))
    .map(command => command.path === 'version' ? {...command, flags: []} : command);
  const destination = join(root, 'skill');
  stageSkill(destination, commands);
  const receipt = JSON.parse(readFileSync(`${destination}.contract.json`, 'utf8'));
  assert.deepEqual(receipt.missingCommands, oracle.commands.filter(command => command.path.startsWith('live ')).map(command => command.path));
  assert.deepEqual(receipt.missingFlags, [{path: 'version', flag: '--home'}, {path: 'version', flag: '--format'}]);
  assert.deepEqual(readdirSync(join(destination, 'references')).sort(), readdirSync(resolve('skills/lycheedev/references')).sort());
  assert.deepEqual(readFileSync(join(destination, 'SKILL.md')), readFileSync(resolve('skills/lycheedev/SKILL.md')));
});

test('default target generation checks the historical fixture without rewriting shipped discovery', () => {
  const path = resolve('skills/lycheedev/references/commands.md'), before = readFileSync(path);
  const result = spawnSync(process.execPath, [resolve('tools/skill-commands.mjs'), '--target', '--check'], {encoding: 'utf8', shell: false, timeout: 120000});
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(result.stdout.trim(), historicalDirectory);
  assert.deepEqual(readFileSync(path), before);
  assert(!before.toString().includes('Flags:'));
});

test('current Agent references require exact approved commands while historical fixture stays frozen', () => {
  const root = resolve('skills/lycheedev');
  assert.equal(currentStaticCheck(root).status, 0);
  const absent = currentStaticCheck(root, commands => commands.splice(commands.findIndex(row => row.path === 'data agent serve'), 1));
  assert.notEqual(absent.status, 0, 'an advertised new command cannot be omitted');
  assert.notEqual(staticCheck(root).status, 0, 'historical Go contract cannot certify new Agent docs');
  assert.equal(JSON.parse(readFileSync(contractFile)).commands.length, 84);
});

test('CI approved Rust static mode is independent and cannot combine contract sources', () => {
  const run = extra => spawnSync(process.execPath, [resolve('tools/skill-contract.mjs'), '--approved-rust', ...extra], {encoding:'utf8',shell:false,timeout:120000});
  const result = run([]);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(JSON.parse(result.stdout).contracts, 87);
  assert.equal(JSON.parse(result.stdout).checkKind, 'static-reference-consistency');
  assert.notEqual(run(['--contract-file', contractFile]).status, 0);
  assert.notEqual(run(['--bin', process.execPath]).status, 0);
});


test('staging refuses pre-existing roots without altering their files', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-stage-existing-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const destination = join(root, 'skill');
  mkdirSync(destination);
  assert.throws(() => stageSkill(destination, oracle.commands), {code: 'EEXIST'});
  writeFileSync(join(destination, 'sentinel'), 'preserve');
  assert.throws(() => stageSkill(destination, oracle.commands), {code: 'EEXIST'});
  assert.equal(readFileSync(join(destination, 'sentinel'), 'utf8'), 'preserve');
  assert.deepEqual(readdirSync(destination), ['sentinel']);
  assert.equal(existsSync(`${destination}.contract.json`), false);
});


test('staging refuses an existing directory link without following it', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-stage-link-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const target = join(root, 'protected'), destination = join(root, 'skill');
  mkdirSync(target); writeFileSync(join(target, 'sentinel'), 'preserve');
  symlinkSync(target, destination, 'junction');
  assert.throws(() => stageSkill(destination, oracle.commands), {code: 'EEXIST'});
  assert.deepEqual(readdirSync(target), ['sentinel']);
  assert.equal(readFileSync(join(target, 'sentinel'), 'utf8'), 'preserve');
  assert.equal(existsSync(`${destination}.contract.json`), false);
});
