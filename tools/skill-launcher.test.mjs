import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

test('Windows skill resolves an npm prefix on PATH without executing cmd shims', { skip: process.platform !== 'win32' }, () => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev skill 中文 '));
  try {
    const bin = join(root, 'node_modules/lycheedev/bin');
    mkdirSync(bin, { recursive: true });
    writeFileSync(join(root, 'lycheedev.cmd'), '@echo shim must not execute\r\nexit /b 99\r\n');
    writeFileSync(join(bin, 'lycheedev.mjs'), 'console.log(JSON.stringify(process.argv.slice(2))); process.exitCode = 7;');
    const env = { ...process.env, PATH: root };
    delete env.LYCHEEDEV_BIN;
    const args = ['source', 'query', '中文 & spaces', '--format', 'json'];
    const result = spawnSync(process.execPath, [resolve('skills/lycheedev/scripts/lycheedev.mjs'), ...args], { env, encoding: 'utf8', windowsHide: true });
    assert.equal(result.status, 7, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), args);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
