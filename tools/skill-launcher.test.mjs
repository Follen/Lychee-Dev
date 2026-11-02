import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, copyFileSync } from 'node:fs';
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

test('mixed PATH preserves directory order between npm shim and native exe', { skip: process.platform !== 'win32' }, t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev mixed PATH '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const npm = join(root, 'npm'), native = join(root, 'native');
  mkdirSync(join(npm, 'node_modules/lycheedev/bin'), { recursive: true }); mkdirSync(native);
  writeFileSync(join(npm, 'lycheedev.cmd'), 'exit /b 99');
  writeFileSync(join(npm, 'node_modules/lycheedev/bin/lycheedev.mjs'), 'console.log(JSON.stringify({selected:"npm",args:process.argv.slice(2)}));process.exitCode=7;');
  copyFileSync(process.execPath, join(native, 'lycheedev.exe'));
  const args = ['-e', 'console.log(JSON.stringify({selected:"native",args:process.argv.slice(1)}));process.exitCode=8;', '中文 & spaces'];
  for (const [first, second, selected, status] of [[npm,native,'npm',7], [native,npm,'native',8]]) {
    const env = {...process.env, PATH: `${first};${second}`}; delete env.LYCHEEDEV_BIN;
    const result=spawnSync(process.execPath,[resolve('skills/lycheedev/scripts/lycheedev.mjs'),...args],{env,encoding:'utf8',windowsHide:true});
    assert.equal(result.status,status,result.stderr);
    assert.deepEqual(JSON.parse(result.stdout),{selected,args:selected==='npm'?args:['中文 & spaces']});
  }
});


test('Windows skill finds a custom global prefix outside PATH through the npm CLI', { skip: process.platform !== 'win32' }, t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev global 中文 & '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const npm = join(root, 'node on PATH');
  const prefix = join(root, 'custom prefix outside PATH');
  mkdirSync(join(npm, 'node_modules/npm/bin'), { recursive: true });
  mkdirSync(join(prefix, 'node_modules/lycheedev/bin'), { recursive: true });
  writeFileSync(join(npm, 'npm.cmd'), '@echo shim must not execute\r\nexit /b 99\r\n');
  writeFileSync(join(npm, 'node_modules/npm/bin/npm-cli.js'), `
    require('node:assert/strict').deepEqual(process.argv.slice(2), ['prefix', '-g']);
    console.log(${JSON.stringify(prefix)});
  `);
  writeFileSync(join(prefix, 'node_modules/lycheedev/bin/lycheedev.mjs'), 'console.log(JSON.stringify(process.argv.slice(2))); process.exitCode = 7;');
  const env = { ...process.env, PATH: npm };
  delete env.LYCHEEDEV_BIN;
  const args = ['source', 'query', '中文 & spaces', '$(echo must-not-run)', '--format', 'json'];
  const result = spawnSync(process.execPath, [resolve('skills/lycheedev/scripts/lycheedev.mjs'), ...args], { env, encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 7, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), args);
});

test('Skill launcher forwards exact arguments and exit code to the selected binary', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev explicit 中文 '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const program = join(root, 'echo arguments.mjs');
  writeFileSync(program, 'console.log(JSON.stringify(process.argv.slice(2))); process.exitCode = 7;');
  const args = [program, '中文 & spaces', '"quoted"', '--format=json', '$(not-a-shell)', ''];
  const result = spawnSync(process.execPath, [resolve('skills/lycheedev/scripts/lycheedev.mjs'), ...args], {
    env: { ...process.env, LYCHEEDEV_BIN: process.execPath }, encoding: 'utf8', windowsHide: true, shell: false,
  });
  assert.equal(result.status, 7, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), args.slice(1));
});

test('Missing explicitly selected binary fails without PATH fallback', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev missing selected '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const result = spawnSync(process.execPath, [resolve('skills/lycheedev/scripts/lycheedev.mjs'), 'describe'], {
    env: { ...process.env, LYCHEEDEV_BIN: join(root, 'missing') }, encoding: 'utf8', windowsHide: true, shell: false,
  });
  assert.equal(result.status, 3);
  assert.match(result.stderr, /LYCHEEDEV_BIN does not exist/);
  assert.equal(result.stdout, '');
});
