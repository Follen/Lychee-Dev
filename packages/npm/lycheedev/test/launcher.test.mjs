import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, symlinkSync, unlinkSync, copyFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { nativePath, launch } from '../bin/lycheedev.mjs';

function fixture(t, platform = process.platform, arch = process.arch) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-package-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const target = `${platform === 'win32' ? 'windows' : platform}-${arch === 'x64' ? 'amd64' : arch}`;
  const path = `native/${target}/lycheedev${platform === 'win32' ? '.exe' : ''}`;
  const bytes = Buffer.from('launcher-test-binary');
  mkdirSync(join(root, 'native', target), { recursive: true });
  writeFileSync(join(root, path), bytes);
  writeFileSync(join(root, 'package.json'), JSON.stringify({ version: '2.0.0-dev' }));
  const manifest = { schema: 'lycheedev.release.v1', version: '2.0.0-dev', binaries: { [target]: { path, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') } } };
  const save = () => writeFileSync(join(root, 'release.json'), JSON.stringify(manifest));
  save();
  return { root, path, target, manifest, save };
}

test('selects the shipped windows-amd64 target and fails closed everywhere else', t => {
  const f = fixture(t, 'win32', 'x64');
  assert.equal(nativePath(f.root, 'win32', 'x64'), join(f.root, f.path));
  // windows-amd64 is the only shipped target; removed and unknown platforms
  // must all fail closed without touching the filesystem.
  for (const [platform, arch] of [['win32', 'arm64'], ['linux', 'x64'], ['linux', 'arm64'], ['darwin', 'x64'], ['darwin', 'arm64']]) {
    assert.throws(() => nativePath('unused', platform, arch), /unsupported_platform/, `${platform}/${arch} must not resolve`);
  }
});

test('rejects changed bytes, manifest paths and version mismatches', t => {
  const f = fixture(t);
  f.manifest.version = '1.2.0'; f.save();
  assert.throws(() => nativePath(f.root), /version_mismatch/);
  f.manifest.version = '2.0.0-dev';
  f.manifest.binaries[f.target].path = '../outside'; f.save();
  assert.throws(() => nativePath(f.root), /invalid_binary_record/);
  f.manifest.binaries[f.target].path = f.path; f.save();
  writeFileSync(join(f.root, f.path), 'launcher-test-binarY');
  assert.throws(() => nativePath(f.root), /binary_integrity/);
});

test('passes argv without shell interpretation and propagates status', t => {
  const f = fixture(t);
  const args = ['source', 'query', '空 格; $(not-a-command)', '--format=json'];
  assert.deepEqual(launch(f.root, args, (binary, actual, options) => {
    assert.equal(binary, join(f.root, f.path));
    assert.deepEqual(actual, args);
    assert.equal(options.shell, false);
    assert.equal(options.stdio, 'inherit');
    return { status: 7 };
  }), { status: 7 });
  assert.deepEqual(launch(f.root, [], () => ({ signal: 'SIGTERM' })), { signal: 'SIGTERM' });
});

test('rejects a native directory link outside the package even with matching bytes', t => {
  const f = fixture(t);
  const outside = fixture(t);
  const directory = join(f.root, 'native', f.target);
  rmSync(directory, { recursive: true });
  symlinkSync(join(outside.root, 'native', outside.target), directory, process.platform === 'win32' ? 'junction' : 'dir');
  try {
    assert.throws(() => nativePath(f.root), /binary_outside_package/);
  } finally {
    unlinkSync(directory);
  }
});

test('executes the entry point through a linked installation path', t => {
  const f = fixture(t);
  const bin = join(f.root, 'bin');
  const alias = join(f.root, 'linked-bin');
  mkdirSync(bin);
  copyFileSync(new URL('../bin/lycheedev.mjs', import.meta.url), join(bin, 'lycheedev.mjs'));
  // A missing manifest must fail, not silently exit before invoking the launcher.
  unlinkSync(join(f.root, 'release.json'));
  symlinkSync(bin, alias, process.platform === 'win32' ? 'junction' : 'dir');
  try {
    const result = spawnSync(process.execPath, [join(alias, 'lycheedev.mjs'), 'version'], { encoding: 'utf8' });
    assert.equal(result.error, undefined);
    assert.equal(result.status, 3);
    assert.match(result.stderr, /lycheedev:.*ENOENT/);
  } finally {
    unlinkSync(alias);
  }
});
