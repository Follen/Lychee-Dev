// Checker/fault tests only. No synthetic envelope is native Skill acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync, execFileSync} from 'node:child_process';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve, win32} from 'node:path';
import os from 'node:os';
import {DatabaseSync} from 'node:sqlite';
import {fileURLToPath} from 'node:url';
import {inventory, inventoryHash, loadCurrentWorkflow, skillTreeIdentity} from './assertions.mjs';
import {assertDataSkillBytes, assertDataSkillDirectory, assertDataSkillInventory, dataSkillIdentity, loadDataSkillContract} from './data-skill-contract.mjs';
import {archiveFixture, decodePartialFrame, digest, goJSON, loadArchivedFixture, loadExporters, loadFixture, npmOfflineFlags, pathStyle, ordinaryPath, FIXED_PIN, SAMPLE_KEY, SAMPLE_KEY_ID, ENCRYPTED_CHUNK, MISSING_KEYS, RESTORED_KEYS} from './data-inputs.mjs';
import {CASES, CHECKS, CSV, CSV_SHA256, PARAMETERS, ROWS, COLUMNS, archiveBytes, assertCapture, assertCaptureProofs, assertDataReport, assertDataValue, assertMemory, captureCanonical, dataArgs, expectedDataCommands, pendingDataTasks, requestFor, sourceReferences} from './data-assertions.mjs';
import {EXPORTER_ARGS} from './data-exporters.mjs';
import {findNpm} from './data-owned.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..'), workflow = loadCurrentWorkflow(root);
const write = (path, bytes) => { mkdirSync(dirname(path), {recursive: true}); writeFileSync(path, bytes); };
const ref = bytes => ({sha256: digest(bytes), bytes: bytes.length});
function temporary(t) { const dir = mkdtempSync(join(os.tmpdir(), 'skill-data-v2-')); t.after(() => rmSync(dir, {recursive: true, force: true})); return dir; }
function frame(blocks) {
  const header = Buffer.alloc(12 + blocks.length * 24); header.write('BLTE'); header.writeUInt32BE(header.length, 4); header[8] = 15; header.writeUIntBE(blocks.length, 9, 3);
  blocks.forEach(({bytes, decoded}, i) => { const at = 12 + i * 24; header.writeUInt32BE(bytes.length, at); header.writeUInt32BE(decoded, at + 4); Buffer.from(digest(bytes, 'md5'), 'hex').copy(header, at + 8); });
  return Buffer.concat([header, ...blocks.map(row => row.bytes)]);
}
function object(raw, encoded = Buffer.concat([Buffer.from('BLTE\0\0\0\0N'), raw])) { const header = encoded.readUInt32BE(4); return {raw, encoded, ckey: digest(raw, 'md5'), ekey: digest(header ? encoded.subarray(0, header) : encoded, 'md5')}; }
function encoding(objects) {
  const ck = [...objects].sort((a, b) => a.ckey.localeCompare(b.ckey)), ek = [...objects].sort((a, b) => a.ekey.localeCompare(b.ekey));
  const cp = Buffer.alloc(1024), ep = Buffer.alloc(1024);
  ck.forEach((o, i) => { const at = i * 38; cp[at] = 1; cp.writeUIntBE(o.raw.length, at + 1, 5); Buffer.from(o.ckey, 'hex').copy(cp, at + 6); Buffer.from(o.ekey, 'hex').copy(cp, at + 22); });
  ek.forEach((o, i) => { const at = i * 25; Buffer.from(o.ekey, 'hex').copy(ep, at); ep.writeUIntBE(o.encoded.length, at + 20, 5); });
  const head = Buffer.alloc(22); Buffer.from('454e011010', 'hex').copy(head); head.writeUInt16BE(1, 5); head.writeUInt16BE(1, 7); head.writeUInt32BE(1, 9); head.writeUInt32BE(1, 13);
  const raw = Buffer.concat([head, Buffer.from(ck[0].ckey, 'hex'), Buffer.from(digest(cp, 'md5'), 'hex'), cp, Buffer.from(ek[0].ekey, 'hex'), Buffer.from(digest(ep, 'md5'), 'hex'), ep]);
  return object(raw, frame([raw.subarray(0, 54), raw.subarray(54)].map(bytes => ({bytes: Buffer.concat([Buffer.from('N'), bytes]), decoded: bytes.length}))));
}
function sample() {
  const raw = Buffer.alloc(224); raw.write('WDC3');
  for (const [at, value] of [[4, 2], [8, 2], [12, 8], [20, 0xf00d00c9], [24, 0xabcd00c9], [28, 1], [32, 2], [44, 2], [56, 48], [68, 2], [80, 208], [84, 1], [120, 216], [124, 1], [208, 1], [212, 42], [216, 2], [220, 99]]) raw.writeUInt32LE(value, at);
  Buffer.from('efcdab9078563412', 'hex').copy(raw, 112);
  const encoded = frame([{bytes: Buffer.concat([Buffer.from('N'), raw.subarray(0, -8)]), decoded: raw.length - 8}, {bytes: Buffer.from(ENCRYPTED_CHUNK, 'hex'), decoded: 8}]);
  const obj = object(raw, encoded);
  return {...obj, identity: {schema: 'lycheedev.skill-partial-fixture.v2', rawBytes: raw.length, rawSHA256: digest(raw), encodedBytes: encoded.length, encodedSHA256: digest(encoded), encodingKey: obj.ekey, encryptedChunkHex: ENCRYPTED_CHUNK, keyID: SAMPLE_KEY_ID, key: SAMPLE_KEY, expectedRows: [[1, 42], [2, 99]]}};
}
function makeFixture(directory, kind, originalRoot = directory) {
  const p = pathStyle(originalRoot), specs = kind === 'catalog' ? [['Item', 401], ['ItemSparse', 402], ['HouseDecor', 403]] : [['Sample', 201]];
  const selected = kind === 'partial' ? sample() : null;
  const tableObjects = specs.map(([name]) => selected ?? object(Buffer.from(`synthetic checker ${name} bytes`)));
  const tables = specs.map(([name, id], i) => ({name, fileDataID: id, layout: 0xabcd0000 + id, ...ref(tableObjects[i].raw), dbdSHA256: digest(`COLUMNS\nint ID\nint Value\n\nBUILD ${FIXED_PIN.fullBuild}\n$id$ID<u32>\nValue<u32>\n`)}));
  const rootRaw = Buffer.alloc(24 + specs.length * 20); rootRaw.write('TSFM'); rootRaw.writeUInt32LE(specs.length, 4); rootRaw.writeUInt32LE(specs.length, 12); rootRaw.writeUInt32LE(0x10000000, 16); rootRaw.writeUInt32LE(0x40, 20);
  let next = 0; specs.forEach(([, id], i) => { rootRaw.writeUInt32LE(id - next, 24 + 4 * i); next = id + 1; Buffer.from(tableObjects[i].ckey, 'hex').copy(rootRaw, 24 + specs.length * 4 + 16 * i); });
  const rootObject = object(rootRaw), enc = encoding([...tableObjects, rootObject]);
  const build = Buffer.from(`build-uid = wow\nroot = ${rootObject.ckey}\nencoding = ${enc.ckey} ${enc.ekey}\nencoding-size = ${enc.raw.length} ${enc.encoded.length}\n`), cdn = Buffer.from('archives =\n');
  const pin = {product: 'retail', region: 'cn', fullBuild: FIXED_PIN.fullBuild, buildConfig: digest(build, 'md5'), cdnConfig: digest(cdn, 'md5'), language: 'zhCN', definitionCommit: FIXED_PIN.definitionCommit};
  const selection = {schema: 'lycheedev.selection.v1', id: '', data: pin}; selection.id = 'PIN-' + digest(goJSON(selection));
  const identity = {schema: 'lycheedev.catalog-navigation-fixture.v1', scope: 'synthetic authenticated local input; not real-client evidence', home: p.join(originalRoot, 'home'), installation: p.join(originalRoot, 'game', '不是身份 client'), snapshot: selection.id, pin, tables};
  for (const [key, raw] of [[pin.buildConfig, build], [pin.cdnConfig, cdn]]) write(join(directory, `game/Data/config/${key.slice(0, 2)}/${key.slice(2, 4)}/${key}`), raw);
  write(join(directory, 'game/.build.info'), `Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|${pin.fullBuild}|${pin.buildConfig}|${pin.cdnConfig}|1\n`);
  write(join(directory, 'game/不是身份 client/.flavor.info'), 'Product Flavor!STRING:0\nwow\n'); write(join(directory, 'game/不是身份 client/version.txt'), pin.fullBuild);
  const objects = [...tableObjects, rootObject, enc], archive = [], entries = []; let offset = 0;
  for (const obj of objects) { entries.push({ekey: obj.ekey, offset, bytes: 30 + obj.encoded.length}); archive.push(Buffer.alloc(30), obj.encoded); offset += 30 + obj.encoded.length; }
  entries.sort((a, b) => a.ekey.localeCompare(b.ekey)); const index = Buffer.alloc(40 + entries.length * 18);
  entries.forEach((row, i) => { const at = 40 + 18 * i; Buffer.from(row.ekey, 'hex').subarray(0, 9).copy(index, at); index.writeUInt32BE(row.offset, at + 10); index.writeUInt32LE(row.bytes, at + 14); });
  write(join(directory, 'game/Data/data/data.000'), Buffer.concat(archive)); write(join(directory, 'game/Data/data/0000000001.idx'), index);
  write(join(directory, 'input-identity.json'), JSON.stringify(identity));
  if (selected) { write(join(directory, 'partial-identity.json'), JSON.stringify(selected.identity)); write(join(directory, 'sample.wdc'), selected.raw); }
  mkdirSync(join(directory, 'home/state'), {recursive: true}); write(join(directory, 'home/workspace.json'), '{}');
  const db = new DatabaseSync(join(directory, 'home/state/toolkit.sqlite'));
  db.exec('CREATE TABLE documents(key TEXT PRIMARY KEY,generation INTEGER,value BLOB);');
  db.prepare('INSERT INTO documents VALUES(?,1,?)').run('pin/' + selection.id, Buffer.from(JSON.stringify(selection)));
  const base = `https://raw.githubusercontent.com/wowdev/WoWDBDefs/${FIXED_PIN.definitionCommit}/`;
  const definitions = [['manifest.json', Buffer.from(JSON.stringify(tables.map(row => ({tableName: row.name, db2FileDataID: row.fileDataID}))))], ...tables.map(row => [`definitions/${row.name}.dbd`, Buffer.from(`COLUMNS\nint ID\nint Value\n\nBUILD ${FIXED_PIN.fullBuild}\n$id$ID<u32>\nValue<u32>\n`)])];
  for (const [name, bytes] of definitions) { const blob = ref(bytes); write(join(directory, 'home/blobs', blob.sha256.slice(0, 2), blob.sha256.slice(2)), bytes); db.prepare('INSERT INTO documents VALUES(?,1,?)').run('definition/' + digest(base + name), Buffer.from(JSON.stringify({url: base + name, blob}))); }
  db.close(); return {directory, files: inventory(directory, {maxFiles: 256, maxBytes: 64 << 20})};
}
function archivedFixture(evidence, kind, originalRoot) {
  const directory = join(dirname(evidence), kind + '-physical'), fixture = makeFixture(directory, kind, originalRoot);
  const reference = archiveFixture(fixture, kind, evidence), path = join(evidence, reference.file), manifest = JSON.parse(readFileSync(path)); manifest.originalRoot = originalRoot;
  writeFileSync(path, JSON.stringify(manifest)); const raw = readFileSync(path); Object.assign(reference, ref(raw));
  return {reference, fixture: loadArchivedFixture(evidence, reference, kind)};
}
const usage = () => ({decodeWork: 50, metadataBytes: 8192, networkBytes: 0, networkRequests: 0, retainedBytes: 65 << 20});
const memory = () => ({mode: 'windows_adaptive', concurrentQueries: 2, ceilingBytes: 1024 << 20, retainedBytes: 512 << 20, metadataBytes: 128 << 20, peakScratchBytes: 1024, growths: 0});
const unavailable = () => [{index: 1, keyID: SAMPLE_KEY_ID, rows: 1, copies: 0, idsComplete: false}];
function dataValue(entry, fixture) {
  const table = fixture.identity.tables.find(row => row.name === (entry.fixture === 'catalog' ? 'HouseDecor' : 'Sample'));
  const partial = entry.partial === true, common = {complete: !partial, partial, resources: usage(), memoryBudget: memory(), resourceAccounting: {wholeQueryAccountingComplete: false, used: usage()}};
  const gap = partial ? {contentVerified: false, partialContent: fixture.partial.partialContent, missing: [{offset: fixture.partial.rawBytes - 8, bytes: 8, keyID: SAMPLE_KEY_ID, chunk: 1}]} : {contentVerified: true};
  if (entry.mode === 'schema') return {...common, snapshot: fixture.identity.snapshot, pin: fixture.identity.pin, source: 'installation', table: table.name, rowCount: partial ? 1 : entry.fixture === 'catalog' ? 3 : 2, truncated: false, truncationReason: 'none', keys: ['ID'], partialReasons: partial ? ['encrypted_sections_unavailable'] : null, fields: (entry.fixture === 'catalog' ? COLUMNS : ['ID', 'Value']).map(name => ({name})), tables: [{...gap, name: table.name, fileDataID: table.fileDataID, contentKey: fixture.objects[table.sha256].ckey, definitionSHA256: table.dbdSHA256, logicalRows: partial ? 1 : entry.fixture === 'catalog' ? 3 : 2, ...(partial ? {unavailablePartitions: unavailable()} : {})}]};
  const root = Object.entries(fixture.rawObjects).find(([, raw]) => raw.subarray(0, 4).toString() === 'TSFM');
  const file = {...gap, pin: fixture.identity.pin, source: 'installation', payloadEncodingKey: fixture.objects[table.sha256].ekey, entry: {fileDataID: table.fileDataID, contentKey: fixture.objects[table.sha256].ckey}, root: ref(root[1]), content: partial ? {sha256: '', bytes: 0} : {sha256: table.sha256, bytes: table.bytes}, catalogSHA256: digest(fixture.catalog), buildConfiguration: {sha256: fixture.configs[0].sha256, bytes: fixture.configs[0].bytes}, cdnConfiguration: {sha256: fixture.configs[1].sha256, bytes: fixture.configs[1].bytes}, verification: {encodingFramingEKey: true, encodingRequestedPagesMD5: true, encodingWholeDecodedCKey: false, rootWholeDecodedCKey: true, contentWholeDecodedCKey: !partial}};
  return {...common, query: requestFor(entry), sources: [{complete: !partial, truncated: false, file, ...(partial ? {unavailablePartitions: unavailable()} : {})}], result: entry.analyze ? {columns: null, rows: null, plan: {outputColumns: COLUMNS, measured: {outputRows: 3, workUnits: 20, chargedBytes: 800, scans: [{table: {catalog: 'static', name: 'housedecor'}, calls: 3, rows: 3, lookups: 3}]}}} : {columns: entry.fixture === 'catalog' ? COLUMNS : ['ID', 'Value'], rows: entry.fixture === 'catalog' ? entry.unordered ? [ROWS[2], ROWS[0], ROWS[1]] : ROWS : partial ? [[1, 42]] : [[1, 42], [2, 99]]}};
}
const envelope = (result, captures = [], context = {}) => ({schema: 'lycheedev.result.v1', ok: true, operationId: '', context, result, captures, warnings: [], error: null});
const historicalSkill = JSON.parse(readFileSync(join(root, 'tests/rust-skill-usage/baselines/rust-candidate-skill.json')));
const simplifiedSkill = JSON.parse(readFileSync(join(root, 'tests/rust-skill-usage/baselines/rust-simplified-skill.json')));
function historicalSkillBytes(selected = historicalSkill) {
  const archives = new Map([[historicalSkill.sourceCommit, 'rust-candidate-0002ce-files.json'], [simplifiedSkill.sourceCommit, 'rust-simplified-6187-files.json']]);
  assert(archives.has(selected.sourceCommit), 'no arbitrary historical source lookup');
  const archive = JSON.parse(readFileSync(join(root, 'tests/rust-skill-usage/baselines', archives.get(selected.sourceCommit))));
  const contract = loadDataSkillContract(dataSkillIdentity(selected));
  assert.equal(archive.sourceCommit, contract.sourceCommit); assert.equal(archive.tree, contract.tree);
  assert.deepEqual(archive.files.map(row => row.path), contract.files.map(row => row.path));
  return new Map(archive.files.map((row, index) => {
    const bytes = Buffer.from(row.text, 'utf8'); assertDataSkillBytes(bytes, contract.files[index]);
    return [row.path, bytes];
  }));
}
function makeReport(t, originalRoot = 'C:\\project', selectedIdentity) {
  const directory = temporary(t), evidence = join(directory, 'relocated-evidence'); mkdirSync(evidence);
  const scratch = win32.join(originalRoot, '.tmp/owner');
  const skill = loadDataSkillContract(selectedIdentity), archived = selectedIdentity ? historicalSkillBytes(skill) : null;
  const config = {root: originalRoot, runnerRoot: 'C:\\runner-only', skillIdentity: dataSkillIdentity(skill), scratch, evidence: 'C:\\project\\evidence\\owner', platform: 'win32', arch: 'x64', node: 'C:\\node24\\node.exe', prefix: win32.join(scratch, 'npm-prefix'), sourcePackage: 'C:\\project\\candidate\\package', version: '3.5.0-rust-test', npm: {path: 'C:\\node24\\node_modules\\npm\\bin\\npm-cli.js'}, exporters: {executables: [{path: 'C:\\project\\target\\debug\\deps\\catalog.exe'}, {path: 'C:\\project\\target\\debug\\deps\\partial.exe'}]}, packageFiles: [{path: 'package.json', bytes: 2, sha256: digest('{}')}, ...skill.files.map(row => ({path: 'payload/skill/' + row.path, bytes: row.bytes, sha256: row.sha256}))]};
  config.launcher = win32.join(config.prefix, 'node_modules/lycheedev/payload/skill/scripts/lycheedev.mjs'); config.tgz = win32.join(scratch, `lycheedev-${config.version}.tgz`); config.packageSHA256 = inventoryHash(config.packageFiles);
  const report = {schema: 'lycheedev.skill-data-usage.v2', state: 'passed_selected_data_workflow', goContract: workflow.goContract, skillTree: 'b9e162ace7abe7ea4a2feecef822283767eb1210', completeSkillAcceptance: false, config, checks: CHECKS.map(caseId => ({caseId, state: 'passed'})), pendingTasks: pendingDataTasks(workflow), fixtures: {}, seeded: {}, workspaces: {}, queries: {}, keys: {}, proofs: {}, commands: [], completedTasks: [{taskId: 'g323-data-query', runtimeVerified: true, basis: 'actual_owned_npm_installed_skill_and_independent_full_capture_replay', captures: 16}]};
  report.skillProof = {...dataSkillIdentity(skill), files: skill.files.map((row, index) => ({path: row.path,
    archive: archiveBytes(evidence, `installed-skill-${index}.bin`, archived ? archived.get(row.path) : readFileSync(join(root, 'skills/lycheedev', row.path)))}))};
  const fixtures = {}, envelopes = {}; let serial = 0;
  function cap(raw, provenance, complete = true, dependencies) { const capture = {schema: 'lycheedev.capture.v1', id: '', blob: ref(raw), mediaType: provenance.kind === 'data-query-csv' ? 'text/csv; charset=utf-8' : 'application/json', provenance: {...provenance, operationId: ''}, complete, truncated: false, capturedAt: new Date(Date.UTC(2026, 9, 18, 0, 0, 0, serial++)).toISOString(), ...(dependencies ? {dependencies} : {})}; capture.id = 'CAP-' + digest(goJSON(captureCanonical(capture))); return capture; }
  function proof(id, result, captures, bodies, fixture, entry) {
    const body = !entry || entry.mode === 'csv' ? JSON.parse(bodies[0]) : result;
    const sources = sourceReferences(entry, body, fixture).map((ref, i) => ({ref, archive: archiveBytes(evidence, `${id}-source-${i}.bin`, entry ? fixture.rawObjects[ref.sha256] : fixture.configs[i].raw)}));
    const capturesProof = captures.map((capture, i) => ({id: capture.id, blob: archiveBytes(evidence, `${id}-cap-${i}.bin`, bodies[i]), metadata: archiveBytes(evidence, `${id}-cap-${i}-metadata.json`, Buffer.from(JSON.stringify({key: 'capture/' + capture.id, generation: 1, valueBase64: Buffer.from(JSON.stringify(capture)).toString('base64')})))}));
    report.proofs[id] = {captures: capturesProof, sources};
    if (entry?.mode === 'csv') { report.proofs[id].output = archiveBytes(evidence, `${id}-output.csv`, bodies[1]); report.proofs[id].sidecar = archiveBytes(evidence, `${id}-sidecar.json`, Buffer.from(JSON.stringify(result))); }
    envelopes[id] = envelope(result, captures, {snapshot: fixture.identity.snapshot});
    captures.forEach((capture, i) => { envelopes[`${id}-verify-${i}`] = envelope(capture); });
  }
  for (const kind of ['catalog', 'partial']) {
    const archived = archivedFixture(evidence, kind, win32.join(scratch, kind + '-fixture')); report.fixtures[kind] = archived.reference; const fixture = fixtures[kind] = archived.fixture;
    report.seeded[kind] = archiveBytes(evidence, `${kind}-seeded.json`, Buffer.from(JSON.stringify(fixture.documents.map(doc => ({key: doc.key, generation: 1, valueBase64: doc.bytes.toString('base64')})))));
    const pin = {schema: 'lycheedev.selection.v1', id: fixture.identity.snapshot, data: fixture.identity.pin};
    const target = {pin, client: {directory: ordinaryPath(fixture.identity.installation), fullBuild: fixture.identity.pin.fullBuild}, installation: ordinaryPath(win32.join(fixture.directory, 'game')), catalogSHA256: digest(fixture.catalog), configurations: fixture.configs.map(({raw, ...ref}) => ref)};
    const raw = Buffer.from(JSON.stringify(target)), capture = cap(raw, {kind: 'local-target', locator: ordinaryPath(fixture.identity.installation), snapshot: pin.id, dataBuild: pin.data.fullBuild});
    proof(`${kind}-target-resolve`, pin, [capture], [raw], fixture);
    envelopes[`${kind}-target-show`] = envelope(pin); const workspace = {schema: 'lycheedev.workspace.v1', workspaceId: 'a'.repeat(32), createdAt: '2026-10-18T00:00:00Z'};
    envelopes[`${kind}-init`] = envelope({root: win32.join(scratch, kind + '-home'), identity: workspace}); report.workspaces[kind] = archiveBytes(evidence, `${kind}-workspace.json`, Buffer.from(JSON.stringify(workspace)));
  }
  envelopes['restored-target-show'] = envelopes['partial-target-show'];
  for (const entry of CASES) {
    const fixture = fixtures[entry.fixture], value = dataValue(entry, fixture), raw = Buffer.from(JSON.stringify(value)), request = requestFor(entry);
    if (entry.mode !== 'schema') report.queries[entry.id] = archiveBytes(evidence, `${entry.id}.query.json`, Buffer.from(JSON.stringify(request)));
    const canonical = {sql: request.sql, ...(request.parameters ? {parameters: Object.fromEntries(Object.keys(PARAMETERS).sort().map(key => [key, PARAMETERS[key]]))} : {})};
    const locator = entry.mode === 'schema' ? `db2:${entry.fixture === 'catalog' ? 'HouseDecor' : 'Sample'}:schema` : 'sql:sha256:' + digest(goJSON(canonical));
    const capture = cap(raw, {kind: entry.mode === 'schema' ? 'data-schema' : 'data-query', locator, snapshot: fixture.identity.snapshot, dataBuild: fixture.identity.pin.fullBuild}, !entry.partial);
    let result = value, captures = [capture], bodies = [raw];
    if (entry.mode === 'csv') {
      const csv = entry.fixture === 'catalog' ? CSV : Buffer.from(entry.partial ? 'ID,Value\n1,42\n' : 'ID,Value\n1,42\n2,99\n');
      const csvCapture = cap(csv, {kind: 'data-query-csv', locator: capture.id, snapshot: fixture.identity.snapshot}, !entry.partial, [capture.id]); captures.push(csvCapture); bodies.push(csv);
      result = {schema: 'lycheedev.query-csv.v1', snapshot: fixture.identity.snapshot, output: ordinaryPath(win32.join(scratch, entry.id + '.csv')), ...ref(csv), columns: entry.fixture === 'catalog' ? COLUMNS : ['ID', 'Value'], rows: entry.fixture === 'catalog' ? 3 : entry.partial ? 1 : 2, queryCapture: capture.id, csvCapture: csvCapture.id, complete: !entry.partial};
    }
    proof(entry.id, result, captures, bodies, fixture, entry);
  }
  envelopes.version = envelope({platform: 'windows/amd64', version: config.version, rust: 'fixture-version'});
  const commands = [['init', []], ['data sql', ['--file', '--encoding', '--output']], ['data db2 schema', []], ['target resolve', ['--installation', '--definitions', '--offline']], ['target show', []], ['evidence verify', []]].map(([path, flags]) => ({path, flags}));
  envelopes.describe = envelope({commands}); envelopes.help = envelope({commands}); envelopes['sql-help'] = envelope({commands: [commands.find(row => row.path === 'data sql')], flags: ['--file', '--encoding', '--output', '--help']});
  report.keys.missing = archiveBytes(evidence, 'missing.keys', Buffer.from(MISSING_KEYS)); report.keys.restored = archiveBytes(evidence, 'restored.keys', Buffer.from(RESTORED_KEYS));
  report.installedInventory = archiveBytes(evidence, 'installed-inventory.json', Buffer.from(JSON.stringify(config.packageFiles))); report.npmShim = archiveBytes(evidence, 'npm-shim.cmd', Buffer.from('@node bin/lycheedev.mjs\r\n'));
  report.commands = expectedDataCommands(config, fixtures, envelopes).map((row, i) => {
    const value = row.kind === 'public' ? envelopes[row.id] : row.id === 'npm-pack' ? [{name: 'lycheedev', version: config.version, filename: win32.basename(config.tgz)}] : {};
    return {...row, exit: 0, signal: null, stdout: archiveBytes(evidence, `command-${i}.stdout`, Buffer.from(JSON.stringify(value))), stderr: archiveBytes(evidence, `command-${i}.stderr`, Buffer.alloc(0))};
  });
  return {report, evidence, fixtures, envelopes, verify: () => assertDataReport(report, workflow, {evidenceDirectory: evidence})};
}

test('Data fixed IN rows, unordered order and CSV bytes remain exact', () => { assert.equal(CSV.length, 76); assert.equal(digest(CSV), CSV_SHA256); assert.equal(CASES.length, 11); assert.equal(CHECKS.length, 26); assert.deepEqual(requestFor(CASES[1]).parameters, PARAMETERS); });
test('candidate authenticates all25 source files separately from unchanged Go323 oracle', () => {
  const contract = assertDataSkillDirectory(join(root, 'skills/lycheedev'));
  assert.equal(contract.files.length, 25); assert.equal(skillTreeIdentity(contract.files), dataSkillIdentity().candidateTree);
  const original = JSON.parse(readFileSync(join(root, 'tests/rust-skill-usage/baselines/go3.2.3-skill.json')));
  assert.equal(original.files.length, 23); assert.equal(skillTreeIdentity(original.files), 'b9e162ace7abe7ea4a2feecef822283767eb1210');
  assert.equal(original.files.find(row => row.path === 'SKILL.md').sha256, 'cbde6b4bb2b598137cfe55c0556765b28890b036e406c1877700a05a33fb6957');
  assert.equal(contract.files.find(row => row.path === 'SKILL.md').sha256, '8b6ba6a956bb06790852a01ea39eb8188b4a991b480287c6d838c6d7d2d7fd8f');
  for (const row of original.files.filter(row => ['scripts/lycheedev.mjs', 'agents/openai.yaml'].includes(row.path))) assert.deepEqual(contract.files.find(current => current.path === row.path), row);
});
test('candidate rejects same-length tampering even with a forged matching SHA256', () => {
  const row = loadDataSkillContract().files[0], raw = readFileSync(join(root, 'skills/lycheedev', row.path));
  raw[0] ^= 1; assert.throws(() => assertDataSkillBytes(raw, row));
  assert.throws(() => assertDataSkillBytes(raw, {...row, sha256: digest(raw)}), /Git blob/);
});
test('candidate inventory rejects missing extra duplicate and changed Skill files', () => {
  const rows = loadDataSkillContract().files.map(({path, bytes, sha256}) => ({path: 'payload/skill/' + path, bytes, sha256}));
  assertDataSkillInventory(rows);
  for (const changed of [rows.slice(1), [...rows, {path: 'payload/skill/extra.md', bytes: 1, sha256: digest('x')}], [...rows, rows[0]], rows.map((row, index) => index ? row : {...row, sha256: '0'.repeat(64)})]) assert.throws(() => assertDataSkillInventory(changed));
});
test('candidate refuses unmodified Go Skill as the adapted packaged candidate', () => {
  const go = JSON.parse(readFileSync(join(root, 'tests/rust-skill-usage/baselines/go3.2.3-skill.json')));
  assert.throws(() => assertDataSkillInventory(go.files.map(({path, bytes, sha256}) => ({path: 'payload/skill/' + path, bytes, sha256}))));
});
test('replay authenticates archived Skill bytes independently of report-supplied hashes', t => {
  const f = makeReport(t), row = f.report.skillProof.files[0], path = join(f.evidence, row.archive.file), raw = readFileSync(path);
  raw[0] ^= 1; writeFileSync(path, raw); Object.assign(row.archive, ref(raw));
  assert.throws(f.verify, /candidate Skill SHA256/);
});
test('replay cannot pass a self-consistent forged package inventory or omit Skill proof', t => {
  const f = makeReport(t), config = f.report.config;
  config.packageFiles.find(row => row.path === 'payload/skill/SKILL.md').sha256 = '0'.repeat(64);
  const raw = Buffer.from(JSON.stringify(config.packageFiles)); writeFileSync(join(f.evidence, f.report.installedInventory.file), raw);
  Object.assign(f.report.installedInventory, ref(raw)); config.packageSHA256 = inventoryHash(config.packageFiles);
  assert.throws(f.verify, /independently fixed candidate/);
  const other = makeReport(t); delete other.report.skillProof; assert.throws(other.verify);
});
test('replay binds full Skill archive ordering and distinct Go/candidate provenance', t => {
  const f = makeReport(t); f.report.skillProof.files.pop(); assert.throws(f.verify, /full archived candidate Skill/);
  const g = makeReport(t); g.report.config.skillIdentity.candidateTree = g.report.skillTree; assert.throws(g.verify);
});
test('separate runner uses sealed product cwd and retains unchanged sourceDigest checks', () => {
  const owner = readFileSync(join(root, 'tests/rust-skill-usage/data-owned.mjs'), 'utf8');
  assert.match(owner, /runnerRoot = resolve\(import\.meta\.dirname, '\.\.\/\.\.'\)/);
  assert.match(owner, /sourceTree = sourceDigest\(root\)/); assert.match(owner, /artifact\.inputHashes\.sourceTree, sourceTree/);
  assert.match(owner, /join\(runnerRoot, 'tests\/rust-skill-usage\/data-run\.mjs'\)/);
  assert.match(owner, /\{cwd: root, env:/); assert.match(owner, /assert\.equal\(sourceDigest\(root\), sourceTree\)/);
  assert.match(owner, /hashFile\(join\(runnerRoot, path\)\), expected/);
});
test('actual npm config parser accepts two independent empty scopes offline', t => {
  const scratch = temporary(t); writeFileSync(join(scratch, 'user.npmrc'), ''); writeFileSync(join(scratch, 'global.npmrc'), '');
  const cli = process.env.npm_execpath || '/usr/lib/node_modules/npm/bin/npm-cli.js';
  let executable = process.execPath, args;
  try { const npm = findNpm(); args = [npm.path, '--version']; } catch { executable = process.platform === 'win32' ? process.execPath : 'npm'; args = process.platform === 'win32' ? [cli, '--version'] : ['--version']; }
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^npm_config_/i.test(key)));
  const result = spawnSync(executable, [...args, ...npmOfflineFlags(scratch)], {cwd: scratch, env, encoding: 'utf8', timeout: 30000, shell: false});
  assert.equal(result.status, 0, result.stderr); assert.match(result.stdout.trim(), /^\d+\.\d+\.\d+/);
  assert.notEqual(npmOfflineFlags(scratch)[6], npmOfflineFlags(scratch)[8]);
});
test('fresh catalog reader reuses authenticated shared fixture seed contract', t => { const dir = join(temporary(t), 'fixture'); makeFixture(dir, 'catalog'); const fixture = loadFixture(dir, 'catalog'); assert.equal(fixture.documents.length, 5); assert.equal(fixture.identity.tables.length, 3); });
test('partial input distinguishes encrypted Sample EKey from ordinary Encoding header60', t => { const dir = join(temporary(t), 'fixture'); makeFixture(dir, 'partial'); const fixture = loadFixture(dir, 'partial'); assert.equal(fixture.partial.partialContent.bytes, 224); assert.equal(Object.values(fixture.objects).length, 4); });
test('partial fixed encrypted vector rejects descriptor/cipher/key tampering', () => { const input = sample(); assert.equal(decodePartialFrame(input.encoded, input.identity, input.raw).raw.readUInt32LE(220), 0); for (const mutate of [i => {i.key = 'f'.repeat(32);}, i => {i.encodingKey = '0'.repeat(32);}, i => {i.encryptedChunkHex = '00';}]) { const identity = structuredClone(input.identity); mutate(identity); assert.throws(() => decodePartialFrame(input.encoded, identity, input.raw)); } });
test('full archived Windows report replays read-only on current host with all16 CAPs', t => { const f = makeReport(t); assert.equal(f.verify(), true); assert.equal(f.report.commands.length, 42); assert.equal(f.report.commands.filter(row => row.kind === 'public').length, 38); });
test('report cannot relabel old Go contract or mark whole Skill/pressure acceptance', t => { const f = makeReport(t); f.report.completeSkillAcceptance = true; assert.throws(f.verify); f.report.completeSkillAcceptance = false; f.report.pendingTasks = []; assert.throws(f.verify); });
test('all commands bind original Node launcher home and argument identity', t => { const f = makeReport(t); for (const id of ['npm-install', 'describe', 'catalog-init', 'catalog-target-resolve-verify-0', 'restored-sql']) { const row = f.report.commands.find(row => row.id === id), old = row.executable; row.executable = 'C:\\wrong\\node.exe'; assert.throws(f.verify); row.executable = old; } });
test('evidence relocation never rewrites original report root or command paths', t => { const f = makeReport(t); assert.equal(f.verify(), true); f.report.config.scratch = '/tmp/pretend-original'; assert.throws(f.verify); });
test('missing or altered fixture archived bytes and manifest cannot replay', t => { const f = makeReport(t), row = f.report.fixtures.partial; const path = join(f.evidence, row.file), raw = readFileSync(path); writeFileSync(path, raw.subarray(1)); assert.throws(f.verify); writeFileSync(path, raw); const manifest = JSON.parse(raw), input = join(f.evidence, manifest.files[0].evidence); writeFileSync(input, 'corrupt'); assert.throws(f.verify); });
test('memoryBudget requires actual adaptive observations and offline network zero', t => { const f = makeReport(t), entry = CASES[1], value = dataValue(entry, f.fixtures.catalog); assertDataValue(entry, value, f.fixtures.catalog); value.resources.networkRequests = 1; assert.throws(() => assertDataValue(entry, value, f.fixtures.catalog)); value.resources.networkRequests = 0; value.memoryBudget.mode = 'fixed'; assert.throws(() => assertDataValue(entry, value, f.fixtures.catalog)); });
test('HouseDecor EXPLAIN uses lowercase static table and actual three lookups', t => { const f = makeReport(t), entry = CASES[3], value = dataValue(entry, f.fixtures.catalog); assertDataValue(entry, value, f.fixtures.catalog); value.result.plan.measured.scans[0].table.name = 'HouseDecor'; assert.throws(() => assertDataValue(entry, value, f.fixtures.catalog)); });
test('samePIN restored schema and SQL reject any stale missing or partial proof', t => { const f = makeReport(t); for (const entry of CASES.filter(e => e.fixture === 'partial' && !e.partial && e.mode !== 'csv')) { const value = dataValue(entry, f.fixtures.partial), target = entry.mode === 'schema' ? value.tables[0] : value.sources[0].file; target.missing = []; assert.throws(() => assertDataValue(entry, value, f.fixtures.partial)); } });
test('partial query proves placeholder bytes and never declares complete content', t => { const f = makeReport(t), entry = CASES.find(e => e.id === 'partial-sql'), value = dataValue(entry, f.fixtures.partial); assertDataValue(entry, value, f.fixtures.partial); value.sources[0].file.content = value.sources[0].file.partialContent; assert.throws(() => assertDataValue(entry, value, f.fixtures.partial)); });
test('capture SQLite metadata bytes are required after successful home cleanup', t => { const f = makeReport(t); const path = join(f.evidence, f.report.proofs['catalog-ordered'].captures[0].metadata.file); writeFileSync(path, '{}'); assert.throws(f.verify); });
test('capture source and CSV dependency/output/sidecar proofs are mandatory', t => { const f = makeReport(t); const p = f.report.proofs['catalog-csv'], old = p.sources; p.sources = []; assert.throws(f.verify); p.sources = old; const path = join(f.evidence, p.sidecar.file); writeFileSync(path, '{}'); assert.throws(f.verify); });
test('verify command result cannot substitute another CAP or only a success flag', t => { const f = makeReport(t), row = f.report.commands.find(row => row.id === 'catalog-schema-verify-0'); const raw = Buffer.from(JSON.stringify(envelope({verified: true}))); writeFileSync(join(f.evidence, row.stdout.file), raw); Object.assign(row.stdout, ref(raw)); assert.throws(f.verify); });
test('exporter manifest rejects unsealed/old owner and different compiler invocation', t => { const dir = temporary(t), path = join(dir, 'receipt.json'); writeFileSync(path, JSON.stringify({schema: 'lycheedev.disk-owner.v1', owner: 'different', state: 'retired', handlesReaped: true})); assert.throws(() => loadExporters(dir, path, 'a'.repeat(64))); assert.deepEqual(EXPORTER_ARGS.slice(0, 6), ['test', '--locked', '--offline', '-j', '1', '-p']); });

test('same-invocation sealed exporter manifest authenticates both binaries and rejects overwritten cache', t => {
  const root = temporary(t), evidence = join(root, 'evidence'); mkdirSync(evidence);
  const target = join(root, 'target'); mkdirSync(target);
  const receiptPath = join(evidence, 'receipt.json'), manifestPath = join(evidence, 'skill-data-exporters.json');
  const executables = ['catalog_navigation_parity', 'skill_data_fixture'].map(name => {
    const path = join(target, name + '.exe'), bytes = Buffer.from('not executable: ' + name); writeFileSync(path, bytes);
    return {name, path, source: join(root, 'crates/lycheedev-data/tests', name + '.rs'), ...ref(bytes)};
  });
  const manifest = {schema: 'lycheedev.skill-data-exporters.v2', sourceTree: 'a'.repeat(64), target, buildEvidence: receiptPath, executables};
  writeFileSync(manifestPath, JSON.stringify(manifest));
  const command = {command: 'cargo', args: EXPORTER_ARGS, cwd: root, channel: target, target, exit: 0, oneShot: true, inputHashes: {sourceTree: manifest.sourceTree}, skillDataExporters: {path: manifestPath, sha256: digest(readFileSync(manifestPath))}};
  writeFileSync(join(evidence, 'command.json'), JSON.stringify(command));
  writeFileSync(join(evidence, 'stdout.bin'), executables.map(row => JSON.stringify({reason: 'compiler-artifact', target: {name: row.name, kind: ['test'], src_path: row.source}, profile: {test: true}, executable: row.path})).join('\n'));
  const receipt = {schema: 'lycheedev.disk-owner.v1', owner: 'skill-data-exporters', state: 'retired', handlesReaped: true, inputHashes: command.inputHashes, command: ['cargo', ...EXPORTER_ARGS], sealedLogs: ['command.json', 'stdout.bin'].map(name => ({path: join(evidence, name), sha256: digest(readFileSync(join(evidence, name)))}))};
  writeFileSync(receiptPath, JSON.stringify(receipt));
  assert.equal(loadExporters(root, receiptPath, manifest.sourceTree).executables.length, 2);
  assert.throws(() => loadExporters(root, receiptPath, 'b'.repeat(64)));
  writeFileSync(executables[0].path, 'later unrelated build'); assert.throws(() => loadExporters(root, receiptPath, manifest.sourceTree));
});
test('public verify rejects internal library wrapper despite matching nested capture', t => {
  const f = makeReport(t), id = 'partial-schema-verify-0', row = f.report.commands.find(row => row.id === id), original = f.envelopes[id];
  const raw = Buffer.from(JSON.stringify(envelope({capture: original.result, verified: true}))); writeFileSync(join(f.evidence, row.stdout.file), raw); Object.assign(row.stdout, ref(raw)); assert.throws(f.verify);
});
test('initialization evidence binds fresh workspace identity and original home', t => {
  const f = makeReport(t), row = f.report.commands.find(row => row.id === 'catalog-init'), changed = structuredClone(f.envelopes['catalog-init']); changed.result.root = 'C:\\different-home';
  const raw = Buffer.from(JSON.stringify(changed)); writeFileSync(join(f.evidence, row.stdout.file), raw); Object.assign(row.stdout, ref(raw)); assert.throws(f.verify);
});
test('schema and SQL independently reject wrong PIN and false complete recovery', t => {
  const f = makeReport(t), entry = CASES.find(row => row.id === 'restored-sql'), value = dataValue(entry, f.fixtures.partial); value.sources[0].file.pin = {...value.sources[0].file.pin, fullBuild: '12.1.0.0'}; assert.throws(() => assertDataValue(entry, value, f.fixtures.partial));
  const schema = CASES.find(row => row.id === 'restored-schema'), restored = dataValue(schema, f.fixtures.partial); restored.unavailablePartitions = []; assert.throws(() => assertDataValue(schema, restored, f.fixtures.partial));
});

test('Root provenance cannot be replaced by selected table or Encoding and FDID must match', t => {
  const f = makeReport(t), entry = CASES[1], value = dataValue(entry, f.fixtures.catalog);
  const objects = Object.entries(f.fixtures.catalog.rawObjects).filter(([, bytes]) => bytes.subarray(0, 4).toString() !== 'TSFM');
  for (const [sha256, raw] of objects) { const changed = structuredClone(value); changed.sources[0].file.root = {sha256, bytes: raw.length}; assert.throws(() => assertDataValue(entry, changed, f.fixtures.catalog)); }
  value.sources[0].file.entry.fileDataID = 401; assert.throws(() => assertDataValue(entry, value, f.fixtures.catalog));
});
test('UNC and extended Windows archived roots retain original identity during relocated replay', t => {
  for (const root of ['\\\\server\\share\\project', '\\\\?\\C:\\project', '\\\\?\\UNC\\server\\share\\project']) {
    assert.equal(pathStyle(root), win32); const f = makeReport(t, root); assert.equal(f.verify(), true);
  }
  assert.equal(pathStyle('/tmp/fixture').sep, '/');
});


test('historical candidate keeps all 25 authenticated original files and unchanged Go Data/Live bytes', () => {
  const contract = loadDataSkillContract(dataSkillIdentity(historicalSkill));
  assert.equal(contract.tree, '5edd025cae923fb2235086f250862696db7a2483');
  assert.equal(contract.files.find(row => row.path === 'SKILL.md').sha256, 'bc214e50a60897a4198fdbedf843e28aede60ef3937cbf9377b8187c81cbc1a8');
  assert.equal(historicalSkillBytes().size, 25);
  const go = JSON.parse(readFileSync(join(root, 'tests/rust-skill-usage/baselines/go3.2.3-skill.json')));
  for (const row of go.files.filter(row => row.path.startsWith('references/data-') || row.path.startsWith('references/live-') || row.path === 'scripts/lycheedev.mjs')) assert.deepEqual(contract.files.find(current => current.path === row.path), row);
});
test('historical archived Data report still replays all checks against its original fixed Skill identity', t => {
  const f = makeReport(t, 'C:\\project', dataSkillIdentity(historicalSkill));
  assert.equal(f.verify(), true); assert.equal(f.report.commands.length, 42); assert.equal(f.report.checks.length, 26);
  assert.notEqual(f.report.config.skillIdentity.candidateTree, dataSkillIdentity().candidateTree);
  f.report.config.skillIdentity = dataSkillIdentity();
  assert.throws(f.verify, /independently fixed candidate/);
});
test('unknown missing null and mixed candidate identities cannot silently select a newer baseline', t => {
  for (const identity of [null, {}, {...dataSkillIdentity(), candidateSourceCommit: 'f'.repeat(40)},
    {...dataSkillIdentity(), candidateTree: historicalSkill.tree}]) assert.throws(() => loadDataSkillContract(identity), /unknown reviewed Skill identity/);
  for (const identity of [undefined, null, {}, {...dataSkillIdentity(), candidateTree: historicalSkill.tree}]) {
    const f = makeReport(t); f.report.config.skillIdentity = identity; assert.throws(f.verify);
  }
  const f = makeReport(t); f.report.config.skillIdentity = dataSkillIdentity(historicalSkill); assert.throws(f.verify, /independently fixed candidate/);
});

test('simplified 6187 Skill keeps its original full bytes and archived report identity', t => {
  const original = loadDataSkillContract(dataSkillIdentity(simplifiedSkill));
  assert.equal(original.tree, '231818eb48f81cf927542bb19d9a8964dfbe4baf');
  assert.equal(historicalSkillBytes(simplifiedSkill).size, 25);
  const f = makeReport(t, 'C:\\project', dataSkillIdentity(simplifiedSkill));
  assert.equal(f.verify(), true);
  assert.equal(f.report.commands.length, 42);
  assert.equal(f.report.checks.length, 26);
  assert.deepEqual(f.report.config.skillIdentity, dataSkillIdentity(simplifiedSkill));
});

test('current Skill identity is a fixed Git source, not learned from candidate bytes', () => {
  const current = loadDataSkillContract();
  const changed = current.files.filter((row, i) => row.gitBlob !== simplifiedSkill.files[i].gitBlob).map(row => row.path);
  assert.deepEqual(changed, ['SKILL.md', 'references/commands.md', 'references/data-investigation.md']);
  assert.equal(execFileSync('git', ['--no-replace-objects', 'rev-parse', `${current.sourceCommit}:skills/lycheedev`], {cwd:root, encoding:'utf8'}).trim(), current.tree);
  for (const row of current.files) {
    const bytes = execFileSync('git', ['--no-replace-objects', 'cat-file', 'blob', `${current.sourceCommit}:skills/lycheedev/${row.path}`], {cwd:root, maxBuffer:1<<20});
    assertDataSkillBytes(bytes, row);
  }
  for (const path of changed) {
    const row = current.files.find(row => row.path === path), bytes = readFileSync(join(root, 'skills/lycheedev', path));
    bytes[0] ^= 1;
    assert.throws(() => assertDataSkillBytes(bytes, row), /SHA256/);
    assert.throws(() => assertDataSkillBytes(bytes, {...row, sha256:digest(bytes)}), /Git blob/);
  }
});
