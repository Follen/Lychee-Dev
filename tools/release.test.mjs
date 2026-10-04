// Unit coverage for the release assembly logic that is testable offline:
// version policy, dist-tag routing (REL-11), strict tag parsing, registry-state
// classification (REL-09) and the sealed tarball whitelist audit (PKG-05).
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  auditTgz, classifyRegistry, distTagFor, parseOptions, parseTag, parseVcsIdentity, policyFor, registryStateCommand, REPOSITORY_URL, TARGETS, verifySourceInputs, validateNpmChannel, verifyNpmChannelTags, exportSourceIndex, isRetiredSlotSource,
} from './release.mjs';
import { readTarGz } from './tar.mjs';
import { luaRuntime, generateGoIdentity, runtimeFiles, stageLuaLS } from './luals.mjs';

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

test('retired receiver and slot modules are excluded from release payloads', () => {
  for (const path of [
    'addon/Bridge/Session.lua', 'addon/Bridge/MemoryProtocol.lua',
    'addon/Bridge/InputProtocol.lua', 'addon/Bridge/MatrixSymbol.lua',
    'addon/Bridge/ReceiptView.lua', 'addon/Bridge/ReportStore.lua',
    'addon/Bridge/Investigation.lua', 'addon/Bridge/ProbeRunner.lua',
    'addon/Bridge/ProbeQueue.lua', 'addon/Bridge/Reentry.lua',
    'addon/Bridge/Identity.lua', 'addon/Bridge/Receiver.lua',
    'addon/Bridge/InputSignal.lua', 'addon/Bridge/FaultRunner.lua',
    'addon/Bridge/SlotProtocol.lua', 'addon/Bridge/SlotRuntime.lua',
    'addon/Bridge/InputState.lua', 'addon/Bridge/StartupBeacon.lua',
    'addon/Bridge/ReceiverBindings.lua', 'addon/Bridge/Definitions.lua',
    'addon/Modules/AutomationHistory.lua',
  ]) assert.equal(isRetiredSlotSource(path), true, path);
  assert.equal(isRetiredSlotSource('addon/Bridge/DuplexProtocol.lua'), false);
});

test('current release channel never moves latest and rejects stale policy',()=>{
  const channel=JSON.parse(readFileSync(new URL('../release/npm-channel.json',import.meta.url),'utf8'));
  const {version}=JSON.parse(readFileSync(new URL('../release/version.json',import.meta.url),'utf8'));
  assert.equal(channel.version,version);
  assert.equal(channel.preserveLatest,'2.5.1');
  assert.equal(distTagFor(version),'next');
  assert.equal(distTagFor('2.5.1'),'latest');
  assert.equal(validateNpmChannel(channel,version),channel);
  for(const bad of [null,[],{...channel,extra:true},{...channel,distTag:'latest'},
    {...channel,preserveLatest:version},{...channel,version:version+'-dev'},
    {...channel,schema:'unknown'},{...channel,preserveLatest:251}]) {
    assert.throws(()=>validateNpmChannel(bad,version),/invalid_npm_channel/);
  }
  assert.throws(()=>validateNpmChannel(channel,'0.0.0'),/invalid_npm_channel/);
  assert.ok(verifyNpmChannelTags({latest:'2.5.1',next:'0.0.0'},channel,version,'next','before').ok);
  assert.ok(verifyNpmChannelTags({latest:'2.5.1',next:version},channel,version,'next','after').ok);
  for(const [tags,tag,phase] of [
    [{latest:version,next:version},'next','before'],
    [{latest:version,next:version},'next','after'],
    [{latest:'2.5.1',next:'0.0.0'},'next','after'],
    [{latest:'2.5.1',next:version},'latest','before'],
    [{},'next','before'],[null,'next','before'],
    [{latest:'2.5.1'},'next','invalid'],
  ]) assert.throws(()=>verifyNpmChannelTags(tags,channel,version,tag,phase),/npm_channel_mismatch/);
});

test('LuaLS Go identity matches the sole release manifest', () => {
  generateGoIdentity({ check: true });
  assert.throws(() => runtimeFiles(Buffer.from('not the pinned archive')), /luals.archive_integrity/);
});

test('offline LuaLS archive stages a complete inventoried runtime', async t => {
  const archivePath = process.env.LYCHEEDEV_LUALS_ARCHIVE;
  if (!archivePath) { t.skip('set LYCHEEDEV_LUALS_ARCHIVE for pinned archive test'); return; }
  const root=mkdtempSync(join(tmpdir(),'lycheedev-luals-stage-'));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  const resources=[];
  await stageLuaLS(root,resources,{archivePath});
  const files=runtimeFiles(readFileSync(archivePath));
  const pointers=[...files].filter(([name])=>name.endsWith('/.git'));
  assert.equal(pointers.length,14);
  for (const [name,bytes] of pointers) {
    const match=/^meta\/3rd\/([^/]+)\/\.git$/.exec(name);
    assert.ok(match,`unexpected omitted path: ${name}`);
    assert.equal(bytes.toString('utf8'),`gitdir: ../../../.git/modules/meta/3rd/${match[1]}\n`);
    assert.ok(!resources.some(entry=>entry.path===`tool/luals/${name}`));
  }
  assert.equal(resources.length,files.size-pointers.length);
  for (const [name,bytes] of files) {
    if (name.endsWith('/.git')) continue;
    const path=`tool/luals/${name}`;
    const staged=readFileSync(join(root,'payload',...path.split('/')));
    assert.deepEqual(staged,bytes);
    assert.ok(resources.some(entry=>entry.path===path && entry.sha256===sha256(bytes)));
  }
});

test('release source inputs accept the committed flat addon manifest and reject missing blobs', () => {
  const result = verifySourceInputs();
  assert.ok(result.requiredPaths.includes('addon/Lychee Dev.toc'));
  assert.throws(() => verifySourceInputs('HEAD', ['addon/nonexistent-release-input.toc']), /release.source_archive_incomplete/);
});

test('source index export preserves all LF blobs beyond Windows MAX_PATH without changing Git configuration', { skip: process.platform !== 'win32' }, t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-export-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const repo = join(root, 'repo');
  mkdirSync(repo);
  const git = args => {
    const result = spawnSync('git', ['-c', 'core.longpaths=true', ...args], { cwd: repo, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout;
  };
  git(['init', '--quiet']);
  git(['config', 'core.longpaths', 'false']);
  git(['config', 'core.autocrlf', 'true']);
  const longName = `research/${'journal-'.repeat(10)}/${'history-'.repeat(8)}/snapshot.jsonl`;
  const blobs = new Map([
    ['short.txt', Buffer.from('short\nsecond\n')],
    [longName, Buffer.from('retained journal\n中文 evidence\n')],
  ]);
  for (const [name, blob] of blobs) {
    const file = join(repo, ...name.split('/'));
    mkdirSync(join(file, '..'), { recursive: true });
    writeFileSync(file, blob.toString('utf8').replaceAll('\n', '\r\n'));
  }
  git(['add', '.']);
  for (const [name, blob] of blobs) assert.deepEqual(Buffer.from(git(['show', `:${name}`])), blob);
  const configBefore = readFileSync(join(repo, '.git', 'config'));
  const destination = join(root, 'export-' + 'x'.repeat(110), 'index-tree');
  mkdirSync(destination, { recursive: true });
  assert.ok(join(destination, ...longName.split('/')).length > 260);
  const failed = spawnSync('git', ['-c', 'core.longpaths=false', '-c', 'core.autocrlf=false', 'checkout-index', '-a', '-f', `--prefix=${destination}/`], { cwd: repo, encoding: 'utf8' });
  assert.notEqual(failed.status, 0, 'fixture did not reproduce the Windows export failure');
  assert.match(failed.stderr, /Filename too long/i);
  exportSourceIndex(destination, repo);
  const exported = [];
  const walk = (path, prefix = '') => {
    for (const entry of readdirSync(path, { withFileTypes: true })) {
      const name = prefix + entry.name;
      if (entry.isDirectory()) walk(join(path, entry.name), name + '/');
      else exported.push(name);
    }
  };
  walk(destination);
  assert.deepEqual(exported.sort(), [...blobs.keys()].sort(), 'index export omitted or added files');
  for (const [name, blob] of blobs) assert.deepEqual(readFileSync(join(destination, ...name.split('/'))), blob, name);
  assert.deepEqual(readFileSync(join(repo, '.git', 'config')), configBefore);
  assert.equal(git(['config', '--local', '--get', 'core.longpaths']).trim(), 'false');
  assert.equal(git(['config', '--local', '--get', 'core.autocrlf']).trim(), 'true');
});

test('source index export matches git archive attributes and keeps research evidence only in Git', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-source-boundary-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const repo = join(root, 'repo');
  mkdirSync(repo);
  const git = (args, binary = false) => {
    const result = spawnSync('git', ['-c', 'core.longpaths=true', ...args], { cwd: repo, encoding: binary ? undefined : 'utf8' });
    assert.equal(result.status, 0, result.stderr?.toString());
    return result.stdout;
  };
  git(['init', '--quiet']);
  git(['config', 'core.longpaths', 'false']);
  git(['config', 'core.autocrlf', 'true']);
  const journal = 'docs/toolkit/research/lua-mailbox-titan-69874/current-candidate/pressure/.lycheedev/live/connections/CON-9108c86a801af51807c3a031a123846f.jsonl.history/1a9906078a5cdc7f768a7e1bf737150e72534c57402b942cd9087282711dfe68.jsonl';
  const directoryOnlyEvidence = 'directory-only/nested/evidence.json';
  const evidence = Buffer.from('original raw journal\r\nretained in Git\r\n');
  const files = new Map([
    ['.gitattributes', Buffer.concat([readFileSync(new URL('../.gitattributes', import.meta.url)),
      Buffer.from('\ndirectory-only export-ignore\ndirectory-only/nested/evidence.json -export-ignore\n')])],
    ['cmd/lycheedev/main.go', Buffer.from('package main\n// product source\n')],
    ['addon/Lychee Dev.toc', Buffer.from('## Title: Lychee Dev\n')],
    ['skills/lycheedev/SKILL.md', Buffer.from('# Lychee Dev\n')],
    ['skills/lycheedev/references/说明 文档.md', Buffer.from('中文 product reference\n')],
    ['LICENSE', Buffer.from('project license\n')],
    ['THIRD_PARTY_NOTICES.md', Buffer.from('source notices\n')],
    ['packages/npm/lycheedev/LICENSE', Buffer.from('package license\n')],
    ['packages/npm/lycheedev/THIRD_PARTY_NOTICES', Buffer.from('package notices\n')],
    ['docs/toolkit/release-3.1.0.md', Buffer.from('release acceptance summary\n')],
    [journal, evidence],
    [directoryOnlyEvidence, Buffer.from('directory-pruned investigation\n')],
  ]);
  for (const [name, bytes] of files) {
    const file = join(repo, ...name.split('/'));
    mkdirSync(join(file, '..'), { recursive: true });
    writeFileSync(file, bytes);
  }
  git(['add', '.']);
  git(['-c', 'user.name=Source boundary fixture', '-c', 'user.email=fixture@example.invalid',
    '-c', 'commit.gpgSign=false', 'commit', '--quiet', '-m', 'product source and retained investigation']);
  assert.deepEqual(git(['show', `HEAD:${journal}`], true), evidence, 'research evidence was removed from Git');
  const archive = readTarGz(git(['archive', '--format=tar.gz', 'HEAD'], true));
  assert.equal(archive.has(journal), false, 'project attributes shipped raw evidence');
  assert.equal(archive.has(directoryOnlyEvidence), false, 'directory export-ignore did not prune descendants');
  for (const name of files.keys()) if (name !== journal && name !== directoryOnlyEvidence) assert.ok(archive.has(name), `product source missing: ${name}`);
  const configBefore = readFileSync(join(repo, '.git', 'config'));
  // The comparison side must use indexed attributes and bytes, even if the
  // working tree contains opposite attributes, altered code and an extra file.
  writeFileSync(join(repo, '.gitattributes'), '*.go export-ignore\n');
  writeFileSync(join(repo, 'cmd/lycheedev/main.go'), 'uncommitted code must not enter source export');
  writeFileSync(join(repo, 'untracked.txt'), 'not indexed');
  const destination = join(root, 'index-tree');
  mkdirSync(destination);
  exportSourceIndex(destination, repo);
  const exported = new Map();
  const walk = (path, prefix = '') => {
    for (const entry of readdirSync(path, { withFileTypes: true })) {
      const name = prefix + entry.name;
      if (entry.isDirectory()) walk(join(path, entry.name), name + '/');
      else exported.set(name, readFileSync(join(path, entry.name)));
    }
  };
  walk(destination);
  assert.deepEqual([...exported.keys()].sort(), [...archive.keys()].sort(), 'comparison export and git archive source sets differ');
  for (const [name, bytes] of archive) assert.deepEqual(exported.get(name), bytes, name);
  assert.deepEqual(exported.get('skills/lycheedev/references/说明 文档.md'), Buffer.from('中文 product reference\n'));
  assert.deepEqual(git(['show', `HEAD:${journal}`], true), evidence);
  assert.deepEqual(readFileSync(join(repo, '.git', 'config')), configBefore);
  assert.equal(git(['config', '--local', '--get', 'core.longpaths']).trim(), 'false');
  assert.equal(git(['config', '--local', '--get', 'core.autocrlf']).trim(), 'true');
});

test('registry-state returns one complete document and --out persists the same state', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'lycheedev-registry-'));
  const previousExit = process.exitCode;
  const fetchImpl = async () => ({ status: 200, text: async () => JSON.stringify({ dist: { integrity: 'sha512-match' } }) });
  try {
    const args = ['--name', 'lycheedev', '--version', '2.0.0', '--integrity', 'sha512-match'];
    const record = await registryStateCommand(args, fetchImpl);
    assert.deepEqual(record, {
      name: 'lycheedev', version: '2.0.0', url: 'https://registry.npmjs.org/lycheedev/2.0.0',
      state: 'exists-matching', integrity: 'sha512-match',
    });
    const path = join(directory, 'registry.json');
    assert.deepEqual(await registryStateCommand([...args, '--out', path], fetchImpl), {
      state: 'exists-matching', integrity: 'sha512-match',
    });
    assert.deepEqual(JSON.parse(readFileSync(path, 'utf8')), record);
  } finally {
    process.exitCode = previousExit;
    rmSync(directory, { recursive: true, force: true });
  }
});

test('registry-state CLI prints one JSON document without --out', () => {
  const directory = mkdtempSync(join(tmpdir(), 'lycheedev-registry-cli-'));
  try {
    const preload = join(directory, 'fetch.mjs');
    writeFileSync(preload, 'globalThis.fetch = async () => ({ status: 404, text: async () => "" });\n');
    const script = fileURLToPath(new URL('./release.mjs', import.meta.url));
    const run = spawnSync(process.execPath, ['--import', pathToFileURL(preload).href, script, 'registry-state',
      '--name', 'lycheedev', '--version', '2.0.1'], { encoding: 'utf8' });
    assert.equal(run.status, 0, run.stderr);
    assert.deepEqual(JSON.parse(run.stdout), {
      name: 'lycheedev', version: '2.0.1',
      url: 'https://registry.npmjs.org/lycheedev/2.0.1', state: 'absent',
    });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('VCS identity parsing accepts real `go version -m` output with indented build lines (REL-01)', () => {
  // Real output shape: every build line starts with a tab before the "build" key.
  const output = [
    '/out/lycheedev.exe: executable',
    '\tpath\tgithub.com/follenfang/lycheedev/cmd/lycheedev',
    '\tmod\tgithub.com/follenfang/lycheedev\t(development)',
    '\tdep\tgithub.com/chai2010/gettext-go@v1.0.3',
    '\tbuild\t-compiler=gc',
    '\tbuild\tvcs=git',
    `\tbuild\tvcs.revision=${'8'.repeat(40)}`,
    '\tbuild\tvcs.time=2026-09-21T01:49:46Z',
    '\tbuild\tvcs.modified=true',
  ].join('\n');
  assert.deepEqual(
    parseVcsIdentity(output),
    { revision: '8'.repeat(40), modified: 'true' },
  );
  // A binary without a stamp parses to undefined so the gate still rejects it.
  assert.deepEqual(parseVcsIdentity('\tbuild\t-compiler=gc\n'), { revision: undefined, modified: undefined });
});

test('repeated options accumulate so seal can fold multiple test reports (§6)', () => {
  assert.deepEqual(parseOptions(['--out', 'x']), { '--out': 'x' });
  assert.deepEqual(parseOptions(['--flag']), { '--flag': true });
  assert.deepEqual(
    parseOptions(['--out', 'd', '--report', 'a.json', '--report', 'b.json', '--report', 'c.json']),
    { '--out': 'd', '--report': ['a.json', 'b.json', 'c.json'] },
  );
});

test('development versions are private and unpublished; release versions publish', () => {
  const dev = policyFor('2.0.0-dev');
  assert.equal(dev.privateMustBeTrue, true);
  assert.equal(dev.development, true);
  assert.equal(dev.npmDistTag, null);
  assert.equal(policyFor('2.0.0').privateMustBeTrue, false);
  assert.equal(policyFor('2.0.0-rc.2').privateMustBeTrue, false);
  assert.equal(policyFor('2.0.0-rc.2').npmDistTag, 'next');
});

test('REL-11: release candidates only enter next, finals enter latest', () => {
  assert.equal(distTagFor('2.0.0-rc.1'), 'next');
  assert.equal(distTagFor('2.0.0'), 'latest');
  assert.equal(distTagFor('2.0.1'), 'latest');
  assert.throws(() => distTagFor('2.0.0-beta.1'), /invalid_version/);
  assert.throws(() => distTagFor('2.0.0-dev'), /invalid_version/);
});

test('REL-01: the tag is exactly v<semver> including -rc.N', () => {
  assert.equal(parseTag('v2.0.0'), '2.0.0');
  assert.equal(parseTag('v2.0.0-rc.12'), '2.0.0-rc.12');
  for (const tag of ['2.0.0', 'v2.0', 'v2.0.0-rc', 'v2.0.0-beta.1', 'v2.0.0 ', 'v2.0.0\n', 'refs/tags/v2.0.0', undefined]) {
    assert.throws(() => parseTag(tag), /invalid_tag/, `accepted ${JSON.stringify(tag)}`);
  }
});

test('REL-09: unknown registry responses are never treated as absent', () => {
  assert.deepEqual(classifyRegistry({ status: 404 }).state, 'absent');
  assert.deepEqual(classifyRegistry({ status: 200, body: JSON.stringify({ dist: { integrity: 'sha512-a' } }), expectedIntegrity: 'sha512-a' }).state, 'exists-matching');
  assert.deepEqual(classifyRegistry({ status: 200, body: JSON.stringify({ dist: { integrity: 'sha512-b' } }), expectedIntegrity: 'sha512-a' }).state, 'exists-conflicting');
  for (const attempt of [
    { status: 403 }, { status: 500 }, { status: 200, body: '<html>gateway</html>' },
    { status: 200, body: JSON.stringify({ dist: {} }) }, { status: 0, body: '' },
  ]) assert.equal(classifyRegistry(attempt).state, 'unknown', JSON.stringify(attempt));
});

function fixturePackage(version = '2.0.0') {
  const commit = 'a'.repeat(40);
  const files = new Map();
  const binaries = {};
  for (const entry of TARGETS) {
    const bytes = Buffer.from(`binary-${entry.target}`);
    binaries[entry.target] = { path: entry.binary, bytes: bytes.length, sha256: sha256(bytes) };
    files.set(`package/${entry.binary}`, bytes);
  }
  const resources = [];
  for (const path of ['skill/SKILL.md', 'addon/Lychee Dev.toc', 'addon/Core/Runtime.lua', 'tool/luals/runtime.json', 'tool/luals/LICENSE', 'tool/luals/bin/lua-language-server.exe', 'tool/luals/bin/main.lua', 'tool/luals/main.lua']) {
    const bytes = path === 'tool/luals/runtime.json' ? Buffer.from(JSON.stringify(luaRuntime)) : Buffer.from(`content of ${path}\n`);
    resources.push({ path, bytes: bytes.length, sha256: sha256(bytes) });
    files.set(`package/payload/${path}`, bytes);
  }
  const correspondingSource = {
    repository: REPOSITORY_URL, commit, tag: `v${version}`,
    archive: `lycheedev-${version}-corresponding-source.tar.gz`, sha256: 'c'.repeat(64),
  };
  const releaseJson = { schema: 'lycheedev.release.v1', version, commit, binaries, resources, correspondingSource };
  const packageJson = {
    name: 'lycheedev', version, private: version.endsWith('-dev') || undefined,
    type: 'module', bin: { lycheedev: 'bin/lycheedev.mjs' },
    files: ['bin/', 'native/', 'payload/', 'release.json', 'README.md', 'LICENSE', 'THIRD_PARTY_NOTICES'],
    engines: { node: '>=22.14.0' },
    publishConfig: { access: 'public', registry: 'https://registry.npmjs.org/' },
    repository: { type: 'git', url: `git+${REPOSITORY_URL}.git`, directory: 'packages/npm/lycheedev' },
  };
  files.set('package/package.json', Buffer.from(JSON.stringify(packageJson)));
  files.set('package/release.json', Buffer.from(JSON.stringify(releaseJson)));
  files.set('package/bin/lycheedev.mjs', Buffer.from('#!/usr/bin/env node\n'));
  files.set('package/bin/update.mjs', Buffer.from('// updater\n'));
  files.set('package/README.md', Buffer.from('# lycheedev\n'));
  files.set('package/LICENSE', Buffer.from('license text\n'));
  files.set('package/THIRD_PARTY_NOTICES', Buffer.from('notices\n'));
  return { files, releaseJson };
}

test('PKG-05: the sealed tarball whitelist accepts the release shape', () => {
  const { files } = fixturePackage();
  const entries = [...files].map(([name, bytes]) => ({ name, bytes, mode: 0o644 }));
  const audit = auditTgz(entries, { version: '2.0.0', expectedCommit: 'a'.repeat(40) });
  assert.deepEqual(audit.violations, []);
  assert.equal(audit.ok, true);
});

test('local package is private, marked development only, and rejected by production audit', () => {
  const {files}=fixturePackage();
  const pkg=JSON.parse(files.get('package/package.json'));
  pkg.private=true;
  pkg.lycheedevDevelopmentOnly={commit:'a'.repeat(40),workspaceDirty:true};
  files.set('package/package.json',Buffer.from(JSON.stringify(pkg)));
  const release=JSON.parse(files.get('package/release.json'));
  delete release.correspondingSource;
  files.set('package/release.json',Buffer.from(JSON.stringify(release)));
  const entries=[...files].map(([name,bytes])=>({name,bytes}));
  assert.equal(auditTgz(entries,{version:'2.0.0',expectedCommit:'a'.repeat(40),developmentOnly:true}).ok,true);
  assert.match(auditTgz(entries,{version:'2.0.0',expectedCommit:'a'.repeat(40)}).violations.join(),/development-only package|private=true|correspondingSource/);
});

test('PKG-05/REL-08: extra, banned, missing and tampered content all fail closed', () => {
  const base = () => [...fixturePackage().files].map(([name, bytes]) => ({ name, bytes, mode: 0o644 }));
  const mutate = (entries, fn) => { fn(entries); return auditTgz(entries, { version: '2.0.0', expectedCommit: 'a'.repeat(40) }); };

  assert.match(mutate(base(), entries => entries.push({ name: 'package/tests/investigation.md', bytes: Buffer.from('x') })).violations.join(), /forbidden entry|banned content/);
  assert.match(mutate(base(), entries => entries.push({ name: 'package/payload/addon/automation.py', bytes: Buffer.from('x') })).violations.join(), /forbidden entry|banned content/);
  assert.match(mutate(base(), entries => { entries.find(entry => entry.name === 'package/LICENSE').name = 'package/LICENSE.moved'; }).violations.join(), /missing entry: package\/LICENSE/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/payload/addon/Core/Runtime.lua');
    entry.bytes = Buffer.from('tampered\n');
  }).violations.join(), /resource record mismatch/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/release.json');
    const manifest = JSON.parse(entry.bytes.toString('utf8'));
    manifest.resources.push({ path: 'addon/Evil.lua', bytes: 1, sha256: 'd'.repeat(64) });
    entry.bytes = Buffer.from(JSON.stringify(manifest));
  }).violations.join(), /resource missing from payload/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/release.json');
    const manifest = JSON.parse(entry.bytes.toString('utf8'));
    delete manifest.correspondingSource;
    entry.bytes = Buffer.from(JSON.stringify(manifest));
  }).violations.join(), /correspondingSource/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/release.json');
    const manifest = JSON.parse(entry.bytes.toString('utf8'));
    manifest.correspondingSource.commit = 'b'.repeat(40);
    entry.bytes = Buffer.from(JSON.stringify(manifest));
  }).violations.join(), /correspondingSource\.commit/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/package.json');
    const pkg = JSON.parse(entry.bytes.toString('utf8'));
    pkg.dependencies = { leftpad: '1.0.0' };
    entry.bytes = Buffer.from(JSON.stringify(pkg));
  }).violations.join(), /dependencies must be empty/);
});

test('the audit binds commit and version of the assembly (REL-01/05)', () => {
  const entries = [...fixturePackage().files].map(([name, bytes]) => ({ name, bytes, mode: 0o644 }));
  assert.match(auditTgz(entries, { version: '9.9.9' }).violations.join(), /package version/);
  assert.match(auditTgz(entries, { version: '2.0.0', expectedCommit: 'b'.repeat(40) }).violations.join(), /release\.json commit/);
});

test('a missing LICENSE fails with the license decision pending error', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-nolicense-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const name of ['README.md', 'THIRD_PARTY_NOTICES']) writeFileSync(join(root, name), `${name}\n`);
  const { files } = fixturePackage();
  const entries = [...files].map(([name, bytes]) => ({ name, bytes, mode: 0o644 }));
  const audit = auditTgz(entries, { version: '2.0.0', expectedCommit: 'a'.repeat(40), sourceRoot: root });
  assert.equal(audit.ok, false);
  assert.match(audit.violations.join(), /license decision pending/);
});
