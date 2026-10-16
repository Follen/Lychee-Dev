import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync, readdirSync} from 'node:fs';
import {resolve, dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const read = path => readFileSync(join(root, path));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const record = JSON.parse(read('vendor/tls-name-adaptation.json'));
const patch = read(record.patch).toString();
const lines = text => text.match(/[^\n]*\n|[^\n]+$/g) ?? [];

// Reverse exactly these unified text hunks in memory. This neither executes a
// dependency nor changes a source file, registry cache, trust store or lock.
function originals() {
  const result = new Map();
  for (const block of patch.split(/(?=^--- a\/)/m).filter(Boolean)) {
    const rows = lines(block);
    const path = rows.shift().trimEnd().slice('--- a/'.length);
    assert.equal(rows.shift().trimEnd(), `+++ b/${path}`);
    const candidate = lines(read(path).toString());
    let consumed = 0, output = [];
    while (rows.length) {
      const header = rows.shift().match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
      assert(header, `invalid patch hunk: ${path}`);
      const start = Number(header[3]) - 1;
      assert(start >= consumed);
      output.push(...candidate.slice(consumed, start)); consumed = start;
      let oldCount = 0, newCount = 0;
      while (rows.length && !rows[0].startsWith('@@ ')) {
        const line = rows.shift(), tag = line[0], bytes = line.slice(1);
        assert([' ', '+', '-'].includes(tag));
        if (tag !== '-') { assert.equal(candidate[consumed++], bytes); newCount++; }
        if (tag !== '+') { output.push(bytes); oldCount++; }
      }
      assert.equal(oldCount, Number(header[2] ?? 1));
      assert.equal(newCount, Number(header[4] ?? 1));
    }
    output.push(...candidate.slice(consumed));
    assert(!result.has(path)); result.set(path, output.join(''));
  }
  return result;
}

test('fixed TLS archives and every retained official package file have exact identities', () => {
  assert.equal(record.schema, 'lycheedev.tls-name-adaptation.v1');
  assert.equal(hash(Buffer.from(patch)), record.patchSHA256);
  assert.deepEqual(record.packages.map(p => [p.name, p.version, p.archiveSHA256]), [
    ['schannel', '0.1.29', '91c1b7e4904c873ef0710c1f407dde2e6287de2bebc1bbbf7d430bb7cbffd939'],
    ['native-tls', '0.2.18', '465500e14ea162429d264d44189adc38b199b62b1c21eea9f69e4b73cb03bbf2'],
  ]);
  for (const p of record.packages) {
    const base = `vendor/${p.name}-${p.version}`;
    const actual = readdirSync(join(root, base), {recursive: true, withFileTypes: true})
      .filter(e => e.isFile()).map(e => join(e.parentPath, e.name).slice(join(root, base).length + 1).replaceAll('\\', '/')).sort();
    assert.deepEqual(actual, p.files.map(f => f.path).sort());
    for (const f of p.files) assert.equal(hash(read(`${base}/${f.path}`)), f.candidateSHA256, `${base}/${f.path}`);
  }
});
test('three-file patch reverses to official bytes with no other upstream changes', () => {
  const before = originals();
  assert.deepEqual([...before.keys()].sort(), ['vendor/native-tls-0.2.18/src/imp/schannel.rs',
    'vendor/native-tls-0.2.18/src/lib.rs', 'vendor/schannel-0.1.29/src/tls_stream.rs']);
  for (const p of record.packages) for (const f of p.files) {
    const path = `vendor/${p.name}-${p.version}/${f.path}`;
    assert.equal(hash(before.has(path) ? Buffer.from(before.get(path)) : read(path)), f.sha256, path);
  }
  assert(!patch.split('\n').filter(l => l.startsWith('+') && !l.startsWith('+++')).some(l => /unsafe|accept_invalid_hostnames\(true|accept_invalid_certs\(true/.test(l)));
});
test('pinned TLS dependencies select the exact local source identities in the ordinary lock', () => {
  const cargo = read('Cargo.toml').toString(), lock = read('Cargo.lock').toString();
  for (const p of record.packages) {
    assert(cargo.includes(`${p.name} = { path = "vendor/${p.name}-${p.version}" }`));
    const entries = lock.split('[[package]]').filter(s => s.startsWith(`\nname = "${p.name}"\n`));
    assert.equal(entries.length, 1); assert(entries[0].includes(`version = "${p.version}"`));
    assert(!/^source = |^checksum = /m.test(entries[0]));
  }
  assert(cargo.includes('[patch.crates-io]'));
});

import {cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {inspectTlsNameAdaptation, stageTlsNameSources} from '../../../tools/rust-tls-identity.mjs';
import {licensedDependencies} from '../../../tools/rust-license-inventory.mjs';
function temporary() {
  const directory = mkdtempSync(join(tmpdir(), 'lychee-tls-custody-'));
  cpSync(join(root, 'vendor'), join(directory, 'vendor'), {recursive: true});
  return {directory, close() { rmSync(directory, {recursive: true, force: true}); }};
}
function metadata(directory) {
  const packages = record.packages.map(p => ({id: p.name, name:p.name, version:p.version, source:null,
    license:p.license, repository:p.repository, manifest_path:join(directory, 'vendor', `${p.name}-${p.version}`, 'Cargo.toml')}));
  packages.push({id:'owned', name:'lycheedev-core', source:null, manifest_path:join(directory, 'crates/lycheedev-core/Cargo.toml')});
  return {packages, resolve:{nodes:packages.map(p => ({id:p.id}))}};
}
test('actual license consumer includes both path packages and their original full notices', () => {
  const packages = licensedDependencies(root, metadata(root), []);
  assert.deepEqual(packages.map(p => p.name).sort(), ['native-tls', 'schannel']);
  for (const p of packages) {
    const original = record.packages.find(x => x.name === p.name);
    assert.equal(p.correspondingSource.archiveSHA256, original.archiveSHA256);
    assert(p.licenseFiles.some(f => f.text.includes('Permission is hereby granted')));
    assert(p.licenseFiles.every(f => original.files.some(x => x.path === f.name && x.sha256 === f.sha256)));
  }
});
test('license consumer rejects a foreign manifest, version, repository or changed exact source', () => {
  for (const alter of [p => p.manifest_path += '.foreign', p => p.version = '0.0.0',
    p => p.repository = 'https://example.invalid/fork', p => p.license = 'unknown']) {
    const m = metadata(root); alter(m.packages[0]);
    assert.throws(() => licensedDependencies(root, m, []), /tls_source_identity/);
  }
  const f = temporary(); try {
    const path = 'vendor/schannel-0.1.29/src/tls_stream.rs';
    writeFileSync(join(f.directory,path), read(path).toString() + '// changed\n');
    assert.throws(() => licensedDependencies(f.directory, metadata(f.directory), []), /tls.source_identity/);
  } finally { f.close(); }
});
test('corresponding-source staging retains path packages beside an isolated registry directory', () => {
  const directory = mkdtempSync(join(tmpdir(), 'lychee-tls-stage-'));
  try {
    const staged = stageTlsNameSources(root, directory);
    const registry = join(directory, 'vendor/registry/synthetic-0.0.0');
    mkdirSync(registry, {recursive:true});
    writeFileSync(join(registry, 'Cargo.toml'), 'synthetic registry fixture; never compiled');
    writeFileSync(join(registry, '.cargo-checksum.json'), '{}');
    assert.equal(inspectTlsNameAdaptation(directory).manifestSHA256, staged.manifestSHA256);
    assert.equal(licensedDependencies(directory, metadata(directory), []).length, 2);
    assert.throws(() => stageTlsNameSources(root, directory), /tls.stage_exists/);
    const rebuild = read('tools/rust-source-rebuild.mjs').toString();
    assert(rebuild.includes('const tls=stageTlsNameSources(root,source)'));
    assert(rebuild.includes("join(source,'vendor/registry')"));
    assert(rebuild.includes('directory="vendor/registry"'));
    assert(!rebuild.includes('directory="vendor"'));
  } finally { rmSync(directory, {recursive:true,force:true}); }
});
test('source authentication refuses extra files and changed adaptation metadata', () => {
  for (const change of [directory => writeFileSync(join(directory,'vendor/native-tls-0.2.18/extra'), 'extra'),
    directory => writeFileSync(join(directory,'vendor/tls-name-adaptation.json'), '{}')]) {
    const f = temporary(); try {
      change(f.directory); assert.throws(() => inspectTlsNameAdaptation(f.directory), /tls\./);
    } finally { f.close(); }
  }
});
