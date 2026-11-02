// Fixed Lua tools have their own producer identity. Importing verifies bytes;
// it never builds tools, probes Rust, or relabels a historical build receipt.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep, win32 } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readTgz } from './tgz.mjs';
import { verifySystemImports } from './luac.mjs';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const SUPPLY_DIRECTORY = 'release/tool-supply/lua-5.1.5-windows-amd64';
export const IMPORT_SCHEMA = 'lycheedev.lua-tool-import.v1';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const fail = code => { throw new Error(`lua-supply.${code}`); };
function check(condition, code) { if (!condition) fail(code); }
function same(actual, expected, code) {
  try { assert.deepEqual(actual, expected); } catch { fail(code); }
}
function within(root, path) {
  const base = resolve(root), absolute = resolve(path), rel = relative(base, absolute);
  check(rel && rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel), 'outside_root');
  let current = base;
  check(!lstatSync(base).isSymbolicLink(), 'reparse_point');
  for (const part of rel.split(sep)) {
    current = join(current, part);
    if (existsSync(current)) check(!lstatSync(current).isSymbolicLink(), 'reparse_point');
  }
  return absolute;
}
function portable(path) {
  check(typeof path === 'string' && path.length <= 512 && /^[A-Za-z0-9][A-Za-z0-9._/-]*$/.test(path)
    && !path.split('/').some(part => !part || part === '.' || part === '..'), 'invalid_relative_path');
  return path;
}
function regular(path, maximum = 1 << 20) {
  const stat = lstatSync(path);
  check(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, 'invalid_file');
  const bytes = readFileSync(path); check(bytes.length === stat.size, 'file_changed'); return bytes;
}
function toolPaths(kind) {
  return { executable: `${kind}/${kind === 'luac' ? 'luac' : 'lua'}.exe`, sourceArchive: `${kind}/lua-5.1.5.tar.gz`,
    license: `${kind}/LICENSE`, ...(kind === 'luac' ? { runtime: 'luac/runtime.json' } : {}),
    producerReport: `${kind}/producer/build-report.json`, producerReceipt: `${kind}/producer/evidence/receipt.json`,
    recipe: `${kind}/producer/recipe.mjs` };
}
function producer(kind, entries, paths, pinned, pinnedSHA256) {
  const reportBytes = entries.get(paths.producerReport), receiptBytes = entries.get(paths.producerReceipt);
  check(reportBytes && receiptBytes, 'producer_missing');
  const report = JSON.parse(reportBytes), receipt = JSON.parse(receiptBytes);
  check(receipt.schema === 'lycheedev.disk-owner.v1' && receipt.category === 'scratch'
    && receipt.owner === (kind === 'luac' ? 'luac-build' : 'formal-lua-build')
    && ['succeeded', 'retired'].includes(receipt.state) && receipt.handlesReaped === true
    && receipt.activePid === null, 'producer_receipt');
  check(Array.isArray(receipt.sealedLogs) && receipt.sealedLogs.length > 0 && receipt.sealedLogs.length <= 100, 'producer_logs');
  const seen = new Set(); let sealedReport = false;
  for (const log of receipt.sealedLogs) {
    // Original Windows paths remain evidence. Never follow them on this host.
    const name = win32.basename(log.path), key = `${kind}/producer/evidence/${portable(name)}`;
    check(!seen.has(key), 'producer_log_duplicate'); seen.add(key);
    const bytes = entries.get(key);
    check(bytes && hash(bytes) === log.sha256, 'producer_log_integrity');
    if (hash(bytes) === hash(reportBytes)) sealedReport = true;
  }
  check(sealedReport, 'producer_report_not_sealed');
  const executable = entries.get(paths.executable), source = entries.get(paths.sourceArchive), license = entries.get(paths.license);
  check(executable && source && license, 'tool_missing');
  check(hash(source) === pinned.source.sha256 && source.length === pinned.source.bytes, 'source_integrity');
  check(hash(license) === pinned.runtime.licenseSHA256, 'license_integrity');
  const imports = verifySystemImports(executable);
  same(receipt.inputHashes?.toolchain, pinned.toolchain, 'producer_toolchain');
  check(receipt.inputHashes?.sourceSHA256 === pinned.source.sha256, 'producer_source');
  check(Array.isArray(report.toolFiles) && report.toolFiles.length === 2
    && report.toolFiles.every(file => /^[a-f0-9]{64}$/.test(file.sha256)), 'producer_compiler_identity');
  check(entries.has(paths.recipe), 'producer_recipe_missing');
  if (kind === 'luac') {
    same(report.manifest, pinned, 'luac_producer_identity');
    same(JSON.parse(entries.get(paths.runtime)), pinned.runtime, 'luac_runtime_identity');
    check(hash(executable) === pinned.runtime.sha256 && executable.length === pinned.runtime.bytes, 'luac_integrity');
    check(report.priorIdentitySHA256 === pinnedSHA256 && report.matchesPriorRuntime === true, 'luac_prior_identity');
    check(receipt.inputHashes.priorIdentitySHA256 === pinnedSHA256, 'luac_producer_pin');
  } else {
    check(report.schema === 'lycheedev.formal-lua-interpreter.v1' && report.version === '5.1.5'
      && report.platform === 'windows-amd64' && report.interpreterProbe === 'formal-lua-interpreter-ok', 'interpreter_identity');
    check(hash(executable) === report.executableSHA256 && executable.length === report.executableBytes, 'interpreter_integrity');
    check(report.source?.sha256 === pinned.source.sha256 && report.source?.url === pinned.source.url
      && report.source?.bytes === source.length && report.license?.sha256 === pinned.runtime.licenseSHA256, 'interpreter_source');
    same(report.toolchain, pinned.toolchain, 'interpreter_toolchain');
    same(report.support, receipt.inputHashes.support, 'interpreter_producer_support');
    check(hash(entries.get(paths.recipe)) === report.support['tools/rust-formal-lua.mjs'], 'interpreter_recipe');
    same(report.imports, imports, 'interpreter_imports');
  }
  return { report, receipt, reportSHA256: hash(reportBytes), receiptSHA256: hash(receiptBytes) };
}

export function verifyLuaToolSupply({ root = repository, supplyDirectory = SUPPLY_DIRECTORY } = {}) {
  const directory = within(root, resolve(root, supplyDirectory));
  const manifestBytes = regular(within(root, join(directory, 'manifest.json')), 256 << 10), manifest = JSON.parse(manifestBytes);
  check(manifest.schema === 'lycheedev.lua-tool-supply.v1' && manifest.version === '5.1.5'
    && manifest.platform === 'windows-amd64', 'manifest_identity');
  check(manifest.archive?.path === 'supply.tgz', 'archive_path');
  const archive = regular(within(root, join(directory, manifest.archive.path)), 2 << 20);
  check(archive.length === manifest.archive.bytes && hash(archive) === manifest.archive.sha256, 'archive_integrity');
  const unpacked = readTgz(archive);
  check(unpacked.length > 0 && unpacked.length <= 200 && unpacked.reduce((sum, row) => sum + row.bytes.length, 0) <= 8 << 20, 'inventory_budget');
  const entries = new Map();
  for (const row of unpacked) { portable(row.name); check(!entries.has(row.name), 'duplicate_file'); entries.set(row.name, row.bytes); }
  check(Array.isArray(manifest.files) && manifest.files.length === entries.size, 'inventory_identity');
  const seen = new Set();
  for (const row of manifest.files) {
    portable(row.path); check(!seen.has(row.path), 'duplicate_file'); seen.add(row.path);
    const bytes = entries.get(row.path);
    check(bytes && bytes.length === row.bytes && hash(bytes) === row.sha256, 'inventory_integrity');
  }
  same(manifest.tools, { luac: toolPaths('luac'), lua51: toolPaths('lua51') }, 'tool_paths');
  const pinnedBytes = regular(within(root, join(root, 'release/tools/luac.json'))), pinned = JSON.parse(pinnedBytes);
  const producers = Object.fromEntries(['luac', 'lua51'].map(kind => [kind,
    producer(kind, entries, manifest.tools[kind], pinned, hash(pinnedBytes))]));
  return { manifest, manifestSHA256: hash(manifestBytes), entries, producers };
}

function importRecord(supply) {
  const producer = supply.producers.lua51;
  return { schema: IMPORT_SCHEMA, version: '5.1.5', platform: 'windows-amd64', tool: 'lua51',
    manifestSHA256: supply.manifestSHA256, executable: 'lua.exe', sourceArchive: 'lua-5.1.5.tar.gz', license: 'LICENSE',
    producerReport: 'producer/build-report.json', producerReceipt: 'producer/evidence/receipt.json',
    producerReportSHA256: producer.reportSHA256, producerReceiptSHA256: producer.receiptSHA256 };
}

export function importLuaToolSupply(output, { root = repository, supplyDirectory = SUPPLY_DIRECTORY } = {}) {
  const supply = verifyLuaToolSupply({ root, supplyDirectory }), directory = within(root, resolve(root, output));
  check(!existsSync(directory), 'output_exists');
  mkdirSync(directory, { recursive: true });
  for (const [name, bytes] of supply.entries) {
    const target = within(directory, join(directory, name)); mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, bytes, { flag: 'wx' });
  }
  const report = join(directory, 'lua51/build-report.json');
  writeFileSync(report, `${JSON.stringify(importRecord(supply), null, 2)}\n`, { flag: 'wx' });
  return { manifestSHA256: supply.manifestSHA256,
    luac: { directory: join(directory, 'luac'), runtimeSHA256: hash(supply.entries.get('luac/runtime.json')),
      executableSHA256: hash(supply.entries.get('luac/luac.exe')) },
    lua51: { directory: join(directory, 'lua51'), report, executableSHA256: hash(supply.entries.get('lua51/lua.exe')) },
    importRecord: report };
}

export function verifyLuaToolImport(reportPath, { root = repository } = {}) {
  const path = within(root, resolve(root, reportPath)), directory = dirname(path), recordBytes = regular(path), record = JSON.parse(recordBytes);
  const supply = verifyLuaToolSupply({ root });
  same(record, importRecord(supply), 'import_identity');
  for (const [name, bytes] of supply.entries) {
    if (!name.startsWith('lua51/')) continue;
    const actual = regular(within(root, join(directory, name.slice('lua51/'.length))));
    check(actual.equals(bytes), 'import_integrity');
  }
  const original = supply.producers.lua51.report;
  const report = { ...original, executable: join(directory, record.executable),
    source: { ...original.source, archive: join(directory, record.sourceArchive) },
    license: { ...original.license, path: join(directory, record.license) },
    buildEvidence: join(directory, record.producerReceipt) };
  return { report, reportSHA256: hash(recordBytes), producerReportSHA256: supply.producers.lua51.reportSHA256,
    buildReceiptSHA256: supply.producers.lua51.receiptSHA256, supplyManifestSHA256: supply.manifestSHA256 };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, output, ...extra] = process.argv.slice(2);
    check(!extra.length && ((command === 'verify' && !output) || (command === 'import' && output && !output.startsWith('--'))), 'arguments');
    if (command === 'verify') {
      const supply = verifyLuaToolSupply();
      console.log(JSON.stringify({ manifestSHA256: supply.manifestSHA256, files: supply.entries.size,
        luacSHA256: supply.producers.luac.report.manifest.runtime.sha256,
        interpreterSHA256: supply.producers.lua51.report.executableSHA256 }, null, 2));
    } else console.log(JSON.stringify(importLuaToolSupply(output), null, 2));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
