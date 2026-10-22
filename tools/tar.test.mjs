import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gunzipSync, gzipSync } from 'node:zlib';
import { readTarGz, readTarGzFile, writeTarGz } from './tar.mjs';
import { readTgz } from './tgz.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-tar-'));
  t.after(() => rmSync(root, { recursive: true }));
  return root;
}

test('round trip preserves spaces, CJK names, binary and empty content', async t => {
  const root = fixture(t);
  const out = join(root, 'round.tar.gz');
  const binary = Buffer.from(Array.from({ length: 257 }, (_, i) => (i * 37 + 11) % 256));
  const entries = [
    { name: 'notes/read me.txt', bytes: 'hello world' },
    { name: '文档/说明.txt', bytes: '荔枝开发工具' },
    { name: 'bin/data.bin', bytes: binary },
    { name: 'empty.txt', bytes: '' },
  ];
  assert.equal(await writeTarGz(entries, out), out);
  const read = await readTarGzFile(out);
  assert.deepEqual([...read.keys()], ['bin/data.bin', 'empty.txt', 'notes/read me.txt', '文档/说明.txt']);
  assert.equal(read.get('notes/read me.txt').toString('utf8'), 'hello world');
  assert.equal(read.get('文档/说明.txt').toString('utf8'), '荔枝开发工具');
  assert.deepEqual(read.get('bin/data.bin'), binary);
  assert.equal(read.get('empty.txt').length, 0);
});

test('identical input yields byte-identical archives', async t => {
  const root = fixture(t);
  const entries = new Map([['b.txt', 'two'], ['a.txt', 'one'], ['dir/c.bin', Buffer.from([0, 255, 128])]]);
  await writeTarGz(entries, join(root, 'one.tar.gz'));
  await writeTarGz(entries, join(root, 'two.tar.gz'));
  const one = readFileSync(join(root, 'one.tar.gz'));
  assert.deepEqual(one, readFileSync(join(root, 'two.tar.gz')));
  await writeTarGz([...entries], join(root, 'three.tar.gz'));
  assert.deepEqual(one, readFileSync(join(root, 'three.tar.gz')));
});

test('per-entry mode is recorded and content reads back unchanged', async t => {
  const root = fixture(t);
  const out = join(root, 'modes.tar.gz');
  await writeTarGz([{ name: 'b.txt', bytes: 'bbb' }, { name: 'a.txt', bytes: 'aaa', mode: 0o600 }], out);
  const entries = readTgz(readFileSync(out));
  assert.deepEqual(entries.map(entry => [entry.name, entry.mode, entry.type]), [['a.txt', 0o600, '0'], ['b.txt', 0o644, '0']]);
  const read = await readTarGzFile(out);
  assert.equal(read.get('a.txt').toString('utf8'), 'aaa');
  assert.equal(read.get('b.txt').toString('utf8'), 'bbb');
});

test('invalid entry names are rejected', async t => {
  const root = fixture(t);
  for (const name of ['/etc/passwd', 'a/../../b.txt', 'a\\b.txt', '..', 'deep/x/../..']) {
    await assert.rejects(writeTarGz([[name, 'x']], join(root, 'bad.tar.gz')), { message: `tar.invalid_entry_name: ${name}` });
  }
});

test('long names split at a slash boundary and round trip', async t => {
  const root = fixture(t);
  const ascii = `${'a'.repeat(80)}/${'b'.repeat(80)}`;
  const cjk = `文档目录/${'文件名'.repeat(10)}`;
  assert.ok(Buffer.byteLength(ascii) > 100 && Buffer.byteLength(cjk) > 100);
  const out = join(root, 'long.tar.gz');
  await writeTarGz([[ascii, 'ascii'], [cjk, 'cjk']], out);
  const read = await readTarGzFile(out);
  assert.deepEqual([...read.keys()], [ascii, cjk]);
  assert.equal(read.get(ascii).toString('utf8'), 'ascii');
  assert.equal(read.get(cjk).toString('utf8'), 'cjk');
});

test('long names without a usable split are rejected', async t => {
  const root = fixture(t);
  for (const name of ['z'.repeat(150), `dir/${'w'.repeat(150)}`, `${'a'.repeat(256)}/b`]) {
    await assert.rejects(writeTarGz([[name, 'x']], join(root, 'bad.tar.gz')), { message: `tar.name_too_long: ${name}` });
  }
});

test('duplicate names throw at write time and on crafted archives', async t => {
  const root = fixture(t);
  await assert.rejects(writeTarGz([['dup.txt', 'a'], ['dup.txt', 'b']], join(root, 'dup.tar.gz')), { message: 'tar.duplicate_entry: dup.txt' });
  const pa = join(root, 'a.tar.gz');
  const pb = join(root, 'b.tar.gz');
  await writeTarGz([['dup.txt', 'first']], pa);
  await writeTarGz([['dup.txt', 'second']], pb);
  // Merge the two entry streams (minus end padding) to craft one duplicate archive.
  const first = gunzipSync(readFileSync(pa));
  const second = gunzipSync(readFileSync(pb));
  let end = first.length;
  while (end >= 512 && first.subarray(end - 512, end).every(byte => byte === 0)) end -= 512;
  const merged = gzipSync(Buffer.concat([first.subarray(0, end), second]));
  assert.throws(() => readTarGz(merged), { message: 'tar.duplicate_entry: dup.txt' });
});

test('empty entry list round trips to an empty map', async t => {
  const root = fixture(t);
  const out = join(root, 'empty.tar.gz');
  await writeTarGz([], out);
  const read = await readTarGzFile(out);
  assert.equal(read.size, 0);
  assert.deepEqual(read, new Map());
});
