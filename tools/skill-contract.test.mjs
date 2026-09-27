import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

test('skill contract rejects unsupported workflow commands and missing packaged references', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-skill-contract-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'references'));
  writeFileSync(join(root, 'references/workflow.md'), '`update --plan`\n`cache status`\n`config show`\n');
  writeFileSync(join(root, 'SKILL.md'), '[Workflow](references/workflow.md)\nComponents: `skill` and `addon`.\n');
  const run = () => spawnSync(process.execPath, [resolve('tools/skill-contract.mjs'), '--skill-root', root], { encoding: 'utf8', shell: false, timeout: 120000 });
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
  const run = extra => spawnSync(process.execPath, [resolve('tools/skill-commands.mjs'), '--output', output, ...extra], { encoding: 'utf8', shell: false, timeout: 120000 });
  let result = run([]); assert.equal(result.status, 0, result.stderr);
  result = run(['--check']); assert.equal(result.status, 0, result.stderr);
  writeFileSync(output, 'outdated generated commands\n');
  result = run(['--check']); assert.notEqual(result.status, 0);
  assert.equal(readFileSync(output, 'utf8'), 'outdated generated commands\n');
});
