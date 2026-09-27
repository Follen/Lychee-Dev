import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { writeTarGz } from './tar.mjs';

const repository = resolve(import.meta.dirname, '..');
const version = JSON.parse(readFileSync(join(repository, 'release/version.json'))).version;
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
function write(path, bytes) { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, bytes); }
function run(file, args, env, cwd = repository) {
  return new Promise((resolve, reject) => {
    const child = spawn(file, args, { env, cwd, shell: false, windowsHide: true });
    let stdout = '', stderr = '';
    const timeout = setTimeout(() => child.kill(), 180000);
    child.stdout.on('data', x => { stdout += x; }); child.stderr.on('data', x => { stderr += x; });
    child.on('error', reject);
    child.on('close', status => { clearTimeout(timeout); resolve({ status, stdout, stderr }); });
  });
}

test('update performs real npm replacement in isolated prefixes, keeps both clients and retries safely', { skip: process.platform !== 'win32', timeout: 300000 }, async t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev update 中文 '));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const env = { ...process.env, npm_config_cache: join(root, 'npm-cache'), npm_config_userconfig: join(root, 'empty.npmrc'), npm_config_update_notifier: 'false' };
  delete env.LYCHEEDEV_BIN;
  write(env.npm_config_userconfig, '');
  const binary = join(root, 'new.exe');
  let built = await run('go', ['build', '-o', binary, './cmd/lycheedev'], env);
  assert.equal(built.status, 0, built.stderr);
  const oldBinary = join(root, 'old.exe');
  write(join(root, 'version.go'), 'package buildinfo\nconst Version = "2.5.0"\n');
  write(join(root, 'overlay.json'), JSON.stringify({ Replace: { [join(repository, 'internal/buildinfo/version.go')]: join(root, 'version.go') } }));
  built = await run('go', ['build', '-overlay', join(root, 'overlay.json'), '-o', oldBinary, './cmd/lycheedev'], env);
  assert.equal(built.status, 0, built.stderr);

  function packageFiles(v, exe, revision, old = false) {
    const files = new Map();
    const payload = new Map([
      ['skill/SKILL.md', `---\nname: lycheedev\ndescription: Fixture\n---\n${v}\n`],
      ['addon/Lychee Dev.toc', `## Interface: 120100, 50504, 38002, 16001\n## Version: ${v}\n## SavedVariables: LycheeToolkitDB\nCore/ClientGate.lua\nCore/Start.lua\n`],
      ['addon/Core/ClientGate.lua', 'local name, ns = ...\n'],
      ['addon/Core/Start.lua', `local name, ns = ...\nns.version = "${v}"\n`],
      ['tool/luals/runtime.json', `{"version":"fixture-${v}"}`],
    ]);
    if (old) payload.set('skill/obsolete.md', 'remove me with the old release');
    const resources = [...payload].map(([path, text]) => { const bytes = Buffer.from(text); files.set(`payload/${path}`, bytes); return { path, bytes: bytes.length, sha256: digest(bytes) }; });
    const exeBytes = readFileSync(exe), exePath = 'native/windows-amd64/lycheedev.exe';
    files.set(exePath, exeBytes);
    files.set('release.json', Buffer.from(JSON.stringify({ schema: 'lycheedev.release.v1', version: v, commit: revision.repeat(40), binaries: { 'windows-amd64': { path: exePath, bytes: exeBytes.length, sha256: digest(exeBytes) } }, resources })));
    files.set('package.json', Buffer.from(JSON.stringify({ name: 'lycheedev', version: v, type: 'module', bin: { lycheedev: 'bin/lycheedev.mjs' }, scripts: { postinstall: 'node -e "process.exit(89)"' } })));
    for (const file of ['lycheedev.mjs', 'update.mjs']) files.set(`bin/${file}`, readFileSync(join(repository, 'packages/npm/lycheedev/bin', file)));
    return files;
  }
  const oldFiles = packageFiles('2.5.0', oldBinary, 'a', true);
  const newFiles = packageFiles(version, binary, 'b');
  const tarball = join(root, 'new.tgz');
  await writeTarGz([...newFiles].map(([name, bytes]) => ({ name: `package/${name}`, bytes, mode: 0o644 })), tarball);
  const packed = readFileSync(tarball);
  let metadataRequests = 0;
  const server = createServer((req, res) => {
    if (req.url === '/lycheedev/-/package.tgz') { res.end(packed); return; }
    if (req.url === '/lycheedev') {
      metadataRequests++;
      res.setHeader('content-type', 'application/json');
      res.end(JSON.stringify({ name: 'lycheedev', 'dist-tags': { latest: version }, versions: { [version]: { name: 'lycheedev', version, dist: { tarball: `http://127.0.0.1:${server.address().port}/lycheedev/-/package.tgz`, integrity: `sha512-${createHash('sha512').update(packed).digest('base64')}` } } } }));
      return;
    }
    res.statusCode = 404; res.end('fixture endpoint not found');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  env.npm_config_registry = `http://127.0.0.1:${server.address().port}`;
  const prefix = join(root, 'prefix'), install = join(prefix, 'node_modules/lycheedev');
  for (const [name, bytes] of oldFiles) write(join(install, name), bytes);
  write(join(prefix, 'lycheedev.cmd'), '@echo fixture\n');
  const skill = join(root, 'skills/lycheedev');
  mkdirSync(dirname(skill), { recursive: true });
  let clientRoot = root;
  if (repository.slice(0, 2).toLowerCase() !== root.slice(0, 2).toLowerCase()) {
    mkdirSync(join(repository, '.tmp'), { recursive: true });
    clientRoot = mkdtempSync(join(repository, '.tmp', 'isolated-update-clients-'));
    t.after(() => rmSync(clientRoot, { recursive: true, force: true }));
  }
  const clients = ['client-a', 'client-b'].map(name => join(clientRoot, name));
  for (const client of clients) {
    mkdirSync(join(client, 'Interface/AddOns'), { recursive: true });
    write(join(client, '.flavor.info'), 'wow'); write(join(client, 'version.txt'), '12.1.0.69875');
    write(join(client, 'WTF/untouched.lua'), 'saved variables must survive');
    const setup = await run(oldBinary, ['addon', 'install', '--release', install, '--installation', client, '--format=json'], env);
    assert.equal(setup.status, 0, setup.stdout + setup.stderr);
  }
  const setup = await run(oldBinary, ['skill', 'install', '--release', install, '--path', skill, '--format=json'], env);
  assert.equal(setup.status, 0, setup.stdout + setup.stderr);
  const arguments_ = ['update', '--path', skill, ...clients.flatMap(path => ['--installation', path]), '--home', join(root, 'workspace'), '--format=json'];
  const launch = () => run(process.execPath, [join(install, 'bin/lycheedev.mjs'), ...arguments_], env);

  // Another updater owns the prefix: no registry request or replacement.
  write(join(prefix, '.lycheedev-update/lock.json'), JSON.stringify({ pid: process.pid }));
  let busy = await launch();
  assert.notEqual(busy.status, 0); assert.match(JSON.parse(busy.stdout).error.message, /update.busy/);
  assert.equal(metadataRequests, 0);
  rmSync(join(prefix, '.lycheedev-update/lock.json'));

  // A later conflict must prevent even the earlier CLI/package replacement.
  write(join(clients[1], 'Interface/AddOns/Lychee Dev/Core/Start.lua'), 'local edit');
  let output = await launch();
  assert.notEqual(output.status, 0); assert.equal(metadataRequests, 0, 'conflict must fail before any registry request');
  assert.equal(JSON.parse(output.stdout).ok, false, 'preflight failures retain a structured result');
  assert.equal(JSON.parse(readFileSync(join(install, 'package.json'))).version, '2.5.0');
  write(join(clients[1], 'Interface/AddOns/Lychee Dev/Core/Start.lua'), oldFiles.get('payload/addon/Core/Start.lua'));

  output = await launch();
  assert.equal(output.status, 0, output.stdout + output.stderr);
  let envelope = JSON.parse(output.stdout);
  assert.equal(envelope.result.complete, true); assert.equal(envelope.result.cli.version, version);
  assert.equal(envelope.result.targets.length, 3);
  assert.equal(existsSync(join(skill, 'obsolete.md')), false);
  for (const client of clients) {
    assert.match(readFileSync(join(client, 'Interface/AddOns/Lychee Dev/Core/Start.lua'), 'utf8'), new RegExp(version.replaceAll('.', '\\.')));
    assert.equal(readFileSync(join(client, 'WTF/untouched.lua'), 'utf8'), 'saved variables must survive');
  }
  assert.match(readFileSync(join(install, 'payload/tool/luals/runtime.json'), 'utf8'), /fixture-/);
  output = await launch();
  assert.equal(output.status, 0, output.stdout + output.stderr);
  assert.equal(JSON.parse(output.stdout).result.complete, true);
  assert.equal(readdirSync(join(prefix, '.lycheedev-update')).length, 0, 'completed update retains no backups or transaction');
  assert.deepEqual(readdirSync(`${join(root, 'workspace')}.delivery`).filter(name => name.startsWith('replace-')), []);

  // Emulate an interrupted coordinator after it pinned a package. Restart must
  // reclaim the dead process lock and reuse those exact bytes without resolving
  // latest again, even if the registry is no longer available.
  const dead = await run(process.execPath, ['-e', 'console.log(process.pid)'], env);
  write(join(prefix, '.lycheedev-update/lock.json'), JSON.stringify({ pid: Number(dead.stdout.trim()) }));
  write(join(prefix, '.lycheedev-update', `lycheedev-${version}.tgz`), packed);
  const previousPlan = envelope.result.targets;
  write(join(prefix, '.lycheedev-update/transaction.json'), JSON.stringify({ schema: 'lycheedev.npm-update.v1', prefix, args: arguments_.filter(arg => arg !== '--format=json'), version, integrity: `sha512-${createHash('sha512').update(packed).digest('base64')}`, targets: previousPlan }));
  const requestsBeforeResume = metadataRequests;
  output = await launch();
  assert.equal(output.status, 0, output.stdout + output.stderr);
  assert.equal(metadataRequests, requestsBeforeResume, 'recovery must not resolve latest again');
  assert.equal(JSON.parse(output.stdout).result.complete, true);
  assert.equal(readdirSync(join(prefix, '.lycheedev-update')).length, 0);

  output = await run(process.execPath, [join(install, 'bin/lycheedev.mjs'), ...arguments_.filter(arg => arg !== '--format=json'), '--format=jsonl'], env);
  assert.equal(output.status, 0, output.stdout + output.stderr);
  assert.deepEqual(output.stdout.trim().split('\n').map(line => JSON.parse(line).type), ['begin', 'record', 'end']);

  write(join(root, 'no-targets.json'), '[]');
  const skillBefore = readFileSync(join(skill, 'SKILL.md'));
  output = await run(process.execPath, [join(install, 'bin/lycheedev.mjs'), '--format=json', 'update', '--file', join(root, 'no-targets.json'), '--home', join(root, 'cli-only')], env);
  assert.equal(output.status, 0, output.stdout + output.stderr);
  assert.deepEqual(JSON.parse(output.stdout).result.targets, []);
  assert.deepEqual(readFileSync(join(skill, 'SKILL.md')), skillBefore);
});
