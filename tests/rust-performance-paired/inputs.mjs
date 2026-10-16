import assert from 'node:assert/strict';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {DatabaseSync} from 'node:sqlite';
import {hashFile} from '../../tools/rust-build.mjs';
import {readZip} from '../../tools/zip.mjs';
import {assertCorpus, sha256} from '../rust-source-full-corpus/assertions.mjs';
import {GO_COMMIT} from './contract.mjs';

export const LUALS = Object.freeze({name: 'lua-language-server', version: '3.19.1', platform: 'windows-amd64', url: 'https://github.com/LuaLS/lua-language-server/releases/download/3.19.1/lua-language-server-3.19.1-win32-x64.zip', bytes: 4453980, sha256: 'fdb9a59108cf62517813c97fa5549b0e16d1ef0688306bac728b08434db7e4cd'});
export function safeRelative(path) {
  assert(typeof path === 'string' && path.length > 0 && !path.includes('\\'));
  assert(path.split('/').every(p => p && p !== '.' && p !== '..' && !/[:\x00-\x1f]/.test(p)), 'unsafe package path');
  return path;
}
export function inventory(directory, {maxFiles = 20000, maxBytes = 512 << 20} = {}) {
  const rows = []; let total = 0;
  function visit(path, name) {
    const stat = lstatSync(path); assert(!stat.isSymbolicLink(), 'input symlink/reparse rejected');
    if (stat.isDirectory()) for (const child of readdirSync(path).sort()) visit(join(path, child), name ? `${name}/${child}` : child);
    else {
      assert(stat.isFile() && rows.length < maxFiles && (total += stat.size) <= maxBytes, 'input inventory budget/type');
      rows.push({path: name, bytes: stat.size, sha256: hashFile(path)});
    }
  }
  visit(directory, ''); return {rows, bytes: total, sha256: sha256(Buffer.from(JSON.stringify(rows)))};
}
export function cloneSeed(source, destination, expected) {
  assert(!existsSync(destination), 'cold home must not exist');
  assert.equal(inventory(source).sha256, expected.sha256, 'seed changed'); mkdirSync(destination);
  for (const row of expected.rows) {
    const path = join(destination, safeRelative(row.path)); mkdirSync(dirname(path), {recursive: true}); copyFileSync(join(source, row.path), path);
  }
  // Empty workspace directories are meaningful even when no file is present.
  for (const name of ['state', 'pins', 'mirrors', 'blobs', 'indexes', 'cache', 'runs', 'captures', 'locks', 'tmp']) mkdirSync(join(destination, name), {recursive: true});
  assert.equal(inventory(destination).sha256, expected.sha256, 'independent cold input byte identity');
}
export function assertSeedCold(home) {
  const roots = ['workspace.json', 'state', 'pins', 'mirrors', 'blobs', 'indexes', 'cache', 'runs', 'captures', 'locks', 'tmp'];
  assert(readdirSync(home).every(name => roots.includes(name)), 'unknown seed root, including Source derived facts, rejected');
  for (const name of ['indexes', 'cache', 'blobs', 'runs', 'captures', 'tmp']) {
    assert(existsSync(join(home, name)), `missing initialized directory ${name}`);
    assert.equal(inventory(join(home, name)).rows.length, 0, `preseeded derived/evidence files in ${name}`);
  }
  // LuaLS/Emmy worktrees, semantic state and derived indexes can only be created
  // by the timed index/query. Nothing is copied from a previously queried home.
  return inventory(home);
}
export function verifySeedMetadata(home, pin, evidenceCopy) {
  const path = join(home, 'state/toolkit.sqlite');
  assert(lstatSync(path).isFile() && lstatSync(path).size <= 16 << 20);
  assert.deepEqual(readdirSync(join(home, 'state')).sort(), ['toolkit.sqlite'], 'seed must be checkpointed and closed: no WAL, SHM, journal or unknown state');
  assert.equal(inventory(join(home, 'pins')).rows.length, 0, 'pin must be only the public metadata document');
  const expected = hashFile(path); copyFileSync(path, evidenceCopy); assert.equal(hashFile(evidenceCopy), expected);
  // Inspect an immutable byte copy after requiring absence of WAL/journal;
  // SQLite never touches the fixture that will be cloned or measured.
  const db = new DatabaseSync(evidenceCopy, {readOnly: true, enableForeignKeyConstraints: false, enableDoubleQuotedStringLiterals: false});
  try {
    assert.equal(db.prepare('PRAGMA user_version').get().user_version, 1);
    assert.deepEqual(db.prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name").all().map(row => ({...row})), [{type: 'table', name: 'deleted_generations'}, {type: 'table', name: 'documents'}], 'sync-only metadata must have the complete fixed Go/Rust writable schema');
    assert.equal(db.prepare('SELECT count(*) AS count FROM deleted_generations').get().count, 0, 'fresh seed must have no deleted generations');
    const rows = db.prepare('SELECT key, generation, value FROM documents').all(); assert.equal(rows.length, 1, 'sync-only metadata must contain exactly one pin, no cached analysis/validation');
    assert.equal(rows[0].key, `pin/${pin.id}`); assert.equal(rows[0].generation, 1);
    assert.deepEqual(JSON.parse(Buffer.from(rows[0].value)), pin, 'real CLI-created pin document');
  } finally { db.close(); }
  assert.equal(hashFile(path), expected); assert.deepEqual(readdirSync(join(home, 'state')).sort(), ['toolkit.sqlite']);
  return {sha256: expected, checkpointed: true, documents: 1, deletedGenerations: 0, documentKey: `pin/${pin.id}`, evidenceCopy};
}
export function derivedIdentity(home, engine) {
  // Go fixed source index uses this directory. Rust's serialized index/query
  // image is in cache/objects and also reports cachedIndex/parsedDocuments.
  const directory = engine === 'go' ? join(home, 'source/v1/facts/snapshots') : join(home, 'cache/objects');
  assert(existsSync(directory), 'cold operation did not leave its derived index');
  const result = inventory(directory, {maxBytes: 1 << 30}); assert(result.rows.length > 0, 'empty derived index');
  const files = result.rows.map(row => {
    const stat = statSync(join(directory, row.path), {bigint: true});
    return {...row, device: stat.dev.toString(), fileId: stat.ino.toString(), modifiedNs: stat.mtimeNs.toString(), createdNs: stat.birthtimeNs.toString()};
  });
  return {directory, files, sha256: sha256(Buffer.from(JSON.stringify(files)))};
}
export function proveWarmReuse(engine, before, after, envelope) {
  assert.equal(before.sha256, after.sha256, 'derived content/file identity changed across warm command');
  const compact = value => ({directory: value.directory, files: value.files.length, sha256: value.sha256, evidence: value.evidence});
  if (engine === 'rust') {
    const io = envelope?.result?.io ?? envelope?.result?.coverage?.io;
    assert.equal(io?.cachedIndex, true, 'Rust must report actual cachedIndex reuse'); assert.equal(io.parsedDocuments, 0);
    return {verified: true, basis: 'same derived file hashes/IDs/timestamps plus actual public cachedIndex=true and parsedDocuments=0', before: compact(before), after: compact(after), io};
  }
  return {verified: true, basis: 'fixed Go openIndex path and unchanged nonempty snapshot records/manifest hashes, file IDs and timestamps before/after warm command; no public hit counter is invented', before: compact(before), after: compact(after)};
}
export function verifyGoMetadata(text) {
  assert.match(text, /:\s+go1\.27\.1\r?\n/, 'fixed Go toolchain');
  for (const [key, value] of [['vcs.revision', GO_COMMIT], ['vcs.modified', 'false'], ['CGO_ENABLED', '0'], ['GOOS', 'windows'], ['GOARCH', 'amd64'], ['-trimpath', 'true']]) {
    assert(text.split(/\r?\n/).some(line => line.trim() === `build\t${key}=${value}`), `missing Go build identity ${key}=${value}`);
  }
  assert(!/^\s*build\t-gcflags=/m.test(text), 'nonstandard Go optimization flags');
  return {commit: GO_COMMIT, modified: false, toolchain: 'go1.27.1', cgo: 0, optimized: true, metadataSHA256: sha256(Buffer.from(text))};
}
export function verifyGoPackage(packageRoot, archivePath) {
  packageRoot = resolve(packageRoot);
  const manifest = JSON.parse(readFileSync(join(packageRoot, 'release.json')));
  assert.equal(manifest.schema, 'lycheedev.release.v1'); assert.equal(manifest.version, '3.2.3'); assert.equal(manifest.commit, GO_COMMIT);
  const binary = manifest.binaries?.['windows-amd64']; assert.equal(binary?.path, 'native/windows-amd64/lycheedev.exe');
  const all = inventory(packageRoot), byName = new Map(all.rows.map(row => [row.path, row]));
  const check = (row, prefix = '') => { const actual = byName.get(prefix + safeRelative(row.path)); assert(actual && actual.bytes === row.bytes && actual.sha256 === row.sha256, `package integrity ${prefix}${row.path}`); };
  check(binary); assert(Array.isArray(manifest.resources) && manifest.resources.length > 0);
  const seen = new Set();
  for (const row of manifest.resources) { assert(!seen.has(row.path), 'duplicate resource'); seen.add(row.path); check(row, 'payload/'); }
  for (const row of all.rows.filter(row => row.path.startsWith('payload/'))) assert(seen.has(row.path.slice(8)), 'unlisted payload file');
  const archive = readFileSync(archivePath); assert.equal(archive.length, LUALS.bytes); assert.equal(sha256(archive), LUALS.sha256);
  const runtimeBytes = readFileSync(join(packageRoot, 'payload/tool/luals/runtime.json'), 'utf8');
  assert.equal(runtimeBytes.trim(), JSON.stringify(LUALS), 'fixed Go Open compares exact canonical runtime bytes');
  const runtime = JSON.parse(runtimeBytes);
  for (const [key, value] of Object.entries(LUALS)) assert.equal(runtime[key], value, `LuaLS ${key}`);
  const files = readZip(archive), expectedRuntime = new Set(['tool/luals/runtime.json']);
  for (const [name, bytes] of files) {
    if (name.endsWith('/') && bytes.length === 0) continue;
    if (/^meta\/3rd\/[^/]+\/\.git$/.test(name)) { assert.equal(bytes.toString(), 'gitdir: ../../../.git/modules/meta/3rd/' + name.split('/')[2] + '\n'); continue; }
    safeRelative(name); expectedRuntime.add(`tool/luals/${name}`);
    const actual = byName.get(`payload/tool/luals/${name}`); assert(actual && actual.bytes === bytes.length && actual.sha256 === sha256(bytes), `pinned runtime bytes ${name}`);
  }
  assert.deepEqual([...seen].filter(name => name.startsWith('tool/luals/')).sort(), [...expectedRuntime].sort(), 'complete exact LuaLS runtime inventory');
  return {root: packageRoot, binary: join(packageRoot, binary.path), binarySHA256: binary.sha256, manifestSHA256: hashFile(join(packageRoot, 'release.json')), packageSHA256: all.sha256, runtime, archiveSHA256: LUALS.sha256};
}

// Same byte-level authentication as the original full-corpus runner. The
// supplied Git executor runs inside the outer Job and logs every response.
export function auditCorpus(mirror, oracle, git, evidence, prefix = '') {
  const commit = git(['--git-dir', mirror, 'cat-file', 'commit', oracle.commit]);
  assert.equal(createHash('sha1').update(`commit ${commit.length}\0`).update(commit).digest('hex'), oracle.commit);
  const tree = git(['--git-dir', mirror, 'ls-tree', '-r', '-l', '-z', '--full-tree', oracle.commit]);
  const rows = tree.toString('utf8').split('\0').filter(Boolean).map(entry => {
    const m = /^(100644|100755) blob ([0-9a-f]{40})\s+(\d+)\t([^\0]+)$/.exec(entry); assert(m, 'ordinary tracked files only');
    safeRelative(m[4]); return {path: m[4], bytes: Number(m[3]), gitSHA1: m[2]};
  });
  assert.equal(rows.length, oracle.allFiles);
  for (let offset = 0; offset < rows.length; offset += 32) {
    const batch = rows.slice(offset, offset + 32);
    const output = git(['--git-dir', mirror, 'cat-file', '--batch'], batch.map(row => `${row.gitSHA1}\n`).join(''));
    let cursor = 0;
    for (const row of batch) {
      const end = output.indexOf(10, cursor); assert(end >= cursor, 'Git batch header');
      assert.equal(output.subarray(cursor, end).toString(), `${row.gitSHA1} blob ${row.bytes}`);
      const raw = output.subarray(end + 1, end + 1 + row.bytes);
      assert.equal(raw.length, row.bytes); assert.equal(output[end + 1 + row.bytes], 10);
      assert.equal(createHash('sha1').update(`blob ${raw.length}\0`).update(raw).digest('hex'), row.gitSHA1);
      row.sha256 = sha256(raw); cursor = end + row.bytes + 2;
    }
    assert.equal(cursor, output.length, 'surplus Git bytes');
  }
  assertCorpus(rows, oracle);
  writeFileSync(join(evidence, `${prefix}verified-corpus-manifest.json`), JSON.stringify({commit: oracle.commit, files: rows}, null, 2) + '\n');
  writeFileSync(join(evidence, `${prefix}git-commit.bin`), commit); writeFileSync(join(evidence, `${prefix}git-tree.bin`), tree);
  return {allFiles: rows.length, documents: oracle.documents, documentBytes: oracle.documentBytes, allPathSHA256SizeDigest: oracle.allPathSHA256SizeDigest};
}
