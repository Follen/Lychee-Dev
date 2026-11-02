import test from 'node:test';
import assert from 'node:assert/strict';
import { appendFileSync, cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { gunzipSync, gzipSync } from 'node:zlib';
import { importLuaToolSupply, verifyLuaToolImport, verifyLuaToolSupply } from './lua-tool-supply.mjs';
import { stageLuac } from './luac.mjs';
import { readTgz } from './tgz.mjs';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const scratch = join(repository, '.tmp', `lua-tool-supply-test-${process.pid}`);
function fixture(fn) {
  rmSync(scratch, { recursive: true, force: true }); mkdirSync(scratch, { recursive: true });
  const root = join(scratch, 'checkout'); mkdirSync(join(root, 'release'), { recursive: true });
  cpSync(join(repository, 'release/tool-supply'), join(root, 'release/tool-supply'), { recursive: true });
  cpSync(join(repository, 'release/tools'), join(root, 'release/tools'), { recursive: true });
  try { return fn(root); } finally { rmSync(scratch, { recursive: true, force: true }); }
}
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
function changeSuppliedFile(root, name, change) {
  const supply = join(root, 'release/tool-supply/lua-5.1.5-windows-amd64');
  const manifestPath = join(supply, 'manifest.json'), manifest = JSON.parse(readFileSync(manifestPath));
  const archivePath = join(supply, 'supply.tgz'), archive = readFileSync(archivePath);
  const entry = readTgz(archive).find(row => row.name === name), changed = Buffer.from(change(entry.bytes.toString()));
  assert.equal(changed.length, entry.bytes.length, 'the fixture mutation preserves tar entry size');
  const tar = gunzipSync(archive), offset = tar.indexOf(entry.bytes); assert(offset >= 0); changed.copy(tar, offset);
  const repacked = gzipSync(tar); writeFileSync(archivePath, repacked);
  Object.assign(manifest.archive, { bytes: repacked.length, sha256: hash(repacked) });
  Object.assign(manifest.files.find(row => row.path === name), { sha256: hash(changed) });
  writeFileSync(manifestPath, JSON.stringify(manifest));
}

test('fixed supply verifies original producer receipts without Git, Rust or current controller files', () => fixture(root => {
  const supply = verifyLuaToolSupply({ root });
  assert.equal(supply.manifest.schema, 'lycheedev.lua-tool-supply.v1');
  assert.equal(supply.manifest.tools.luac.executable, 'luac/luac.exe');
  assert.equal(supply.producers.luac.report.matchesPriorRuntime, true);
  assert.equal(supply.producers.lua51.report.interpreterProbe, 'formal-lua-interpreter-ok');
  for (const producer of Object.values(supply.producers)) {
    assert.equal(producer.receipt.state, 'retired');
    assert.equal(producer.receipt.handlesReaped, true);
    assert.equal(producer.receipt.activePid, null);
  }
}));

test('import preserves the pinned luac staging contract and all original producer bytes', () => fixture(root => {
  const supply = verifyLuaToolSupply({ root }), out = join(root, 'imported');
  const imported = importLuaToolSupply(out, { root });
  assert.equal(imported.lua51.report, join(out, 'lua51/build-report.json'));
  const record = JSON.parse(readFileSync(imported.lua51.report));
  assert.equal(record.schema, 'lycheedev.lua-tool-import.v1');
  assert.equal(JSON.stringify(record).includes(root), false);
  assert.equal(record.manifestSHA256, supply.manifestSHA256);
  for (const [name, bytes] of supply.entries) assert.deepEqual(readFileSync(join(out, name)), bytes, name);
  const stage = join(root, 'package'); mkdirSync(stage); const resources = [];
  stageLuac(stage, resources, imported.luac.directory);
  assert.equal(resources.length, 4);
  assert.equal(imported.luac.executableSHA256, '5aa1605bfea56e6c2961c6b4b0f845a699b43f9b3666dd64f50289c3952d441b');
  assert.throws(() => importLuaToolSupply(out, { root }), /output_exists/);
}));

test('imported interpreter relocates and keeps producer evidence separate from current execution paths', () => fixture(root => {
  const old = join(root, 'old'), moved = join(root, 'moved');
  const imported = importLuaToolSupply(old, { root });
  cpSync(old, moved, { recursive: true }); rmSync(old, { recursive: true });
  const admitted = verifyLuaToolImport(join(moved, 'lua51/build-report.json'), { root });
  assert.equal(admitted.report.executable, join(moved, 'lua51/lua.exe'));
  assert.equal(admitted.report.source.archive, join(moved, 'lua51/lua-5.1.5.tar.gz'));
  assert.equal(admitted.report.license.path, join(moved, 'lua51/LICENSE'));
  assert.equal(admitted.report.executableSHA256, imported.lua51.executableSHA256);
  assert.equal(admitted.report.executableSHA256, '60d6027ffb7e3a2b99bcf5efc0ca67c5266b7c1d397942c8cb02c5c19753768a');
  assert.notEqual(admitted.reportSHA256, admitted.producerReportSHA256);
  assert.equal(relative(moved, admitted.report.buildEvidence), join('lua51', 'producer/evidence/receipt.json'));
}));

test('changed archives, inventory and imports fail instead of adopting new executable bytes', () => fixture(root => {
  const out = join(root, 'out'), imported = importLuaToolSupply(out, { root });
  appendFileSync(join(out, 'lua51/lua.exe'), 'changed');
  assert.throws(() => verifyLuaToolImport(imported.lua51.report, { root }), /import_integrity/);
  const supply = join(root, 'release/tool-supply/lua-5.1.5-windows-amd64');
  appendFileSync(join(supply, 'supply.tgz'), 'changed');
  assert.throws(() => verifyLuaToolSupply({ root }), /archive_integrity/);
}));

test('an import cannot select another manifest or escape its relative tool paths', () => fixture(root => {
  const imported = importLuaToolSupply(join(root, 'out'), { root }), path = imported.lua51.report;
  const record = JSON.parse(readFileSync(path));
  for (const changed of [{ ...record, manifestSHA256: '0'.repeat(64) }, { ...record, executable: '../luac/luac.exe' }, { ...record, producerReceipt: 'C:/foreign/receipt.json' }]) {
    writeFileSync(path, JSON.stringify(changed));
    assert.throws(() => verifyLuaToolImport(path, { root }), /import_identity/);
  }
}));

test('producer evidence remains mandatory even when a changed archive inventory is internally consistent', () => fixture(root => {
  changeSuppliedFile(root, 'lua51/producer/evidence/receipt.json', text => text.replace('"state": "retired"', '"state": "running"'));
  assert.throws(() => verifyLuaToolSupply({ root }), /producer_receipt/);
}));

test('a rewritten producer report cannot borrow an original production receipt', () => fixture(root => {
  changeSuppliedFile(root, 'lua51/producer/build-report.json', text => text.replace('"executableBytes": 489472', '"executableBytes": 489473'));
  assert.throws(() => verifyLuaToolSupply({ root }), /producer_report_not_sealed/);
}));

test('supply inventory refuses traversal and byte changes', () => fixture(root => {
  const path = join(root, 'release/tool-supply/lua-5.1.5-windows-amd64/manifest.json');
  const original = JSON.parse(readFileSync(path)), changed = structuredClone(original);
  changed.files[0].sha256 = '0'.repeat(64); writeFileSync(path, JSON.stringify(changed));
  assert.throws(() => verifyLuaToolSupply({ root }), /inventory_integrity/);
  original.files[0].path = '../foreign.exe'; writeFileSync(path, JSON.stringify(original));
  assert.throws(() => verifyLuaToolSupply({ root }), /invalid_relative_path/);
}));

test('the CLI imports fixed bytes without starting a compiler and rejects ambiguous arguments', () => fixture(() => {
  const script = join(repository, 'tools/lua-tool-supply.mjs');
  const verify = spawnSync(process.execPath, [script, 'verify'], { encoding: 'utf8' });
  assert.equal(verify.status, 0, verify.stderr); assert.equal(JSON.parse(verify.stdout).files, 96);
  for (const args of [[], ['import'], ['verify', 'extra'], ['import', '--unknown'], ['import', 'one', 'two']]) {
    const result = spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });
    assert.equal(result.status, 1); assert.match(result.stderr, /lua-supply.arguments/);
  }
}));
