// Independent Data checks and read-only replay. Synthetic tests exercise these
// checks; only actual owned packaged-Skill calls may pass the selected task.
import assert from 'node:assert/strict';
import {join} from 'node:path';
import {writeFileSync} from 'node:fs';
import {DatabaseSync} from 'node:sqlite';
import {CURRENT_GO_CONTRACT, commandFlags, inventoryHash, pendingIncrementTasks, within} from './assertions.mjs';
import {bounded, digest, goJSON, parse, pathStyle, ordinaryPath, npmOfflineFlags, loadArchivedFixture, MISSING_KEYS, RESTORED_KEYS, SAMPLE_KEY_ID} from './data-inputs.mjs';
import {assertDataSkillBytes, assertDataSkillInventory, dataSkillIdentity, loadDataSkillContract} from './data-skill-contract.mjs';

export const SQL = 'SELECT ID, Name_lang, ModelFileDataID FROM HouseDecor WHERE ID IN (:third,:first,:duplicate,:second) ORDER BY ID';
export const PARAMETERS = {third: 30, first: 10, duplicate: 10, second: 20};
export const ROWS = [[10, '无模型🙂', 0], [20, '第二', 501], [30, '第三', 501]];
export const COLUMNS = ['ID', 'Name_lang', 'ModelFileDataID'];
export const CSV = Buffer.from('ID,Name_lang,ModelFileDataID\n10,无模型🙂,0\n20,第二,501\n30,第三,501\n');
export const CSV_SHA256 = 'b304aa35231bb210d013f57cbd9e4e70b18a74f6f5853377db9c704ed3c8bf5e';
export const CASES = [
  {id: 'catalog-schema', fixture: 'catalog', mode: 'schema'},
  {id: 'catalog-ordered', fixture: 'catalog', mode: 'sql'},
  {id: 'catalog-unordered', fixture: 'catalog', mode: 'sql', unordered: true},
  {id: 'catalog-analyze', fixture: 'catalog', mode: 'sql', analyze: true},
  {id: 'catalog-csv', fixture: 'catalog', mode: 'csv'},
  ...['partial', 'restored'].flatMap(phase => ['schema', 'sql', 'csv'].map(mode => ({id: `${phase}-${mode}`, fixture: 'partial', mode, partial: phase === 'partial'}))),
];
export const CHECKS = ['export-catalog', 'export-partial', 'npm-installed-package', 'version', 'describe', 'help', 'sql-help', 'catalog-init-and-seed', 'catalog-target-resolve', 'catalog-target-show', ...CASES.filter(c => c.fixture === 'catalog').map(c => c.id), 'partial-init-and-seed', 'partial-target-resolve', 'partial-target-show', ...CASES.filter(c => c.partial).map(c => c.id), 'restore-keys-same-pin', 'restored-target-show', ...CASES.filter(c => c.fixture === 'partial' && !c.partial).map(c => c.id)];
export function requestFor(entry) {
  return entry.fixture === 'catalog' ? {sql: (entry.analyze ? 'EXPLAIN ANALYZE ' : '') + (entry.unordered ? SQL.replace(' ORDER BY ID', '') : SQL), parameters: PARAMETERS} : {sql: 'SELECT ID, Value FROM Sample ORDER BY ID'};
}
export function pendingDataTasks(workflow) {
  return pendingIncrementTasks(workflow).filter(row => row.taskId !== 'g323-data-query');
}
const unsigned = (n, label) => assert(Number.isSafeInteger(n) && n >= 0, label);
export function assertUsage(value) {
  assert.deepEqual(Object.keys(value).sort(), ['decodeWork', 'metadataBytes', 'networkBytes', 'networkRequests', 'retainedBytes']);
  for (const [key, n] of Object.entries(value)) unsigned(n, key);
  assert.equal(value.networkBytes, 0); assert.equal(value.networkRequests, 0);
}
export function assertMemory(value, resources) {
  assert.deepEqual(Object.keys(value).sort(), ['ceilingBytes', 'concurrentQueries', 'growths', 'metadataBytes', 'mode', 'peakScratchBytes', 'retainedBytes']);
  assert.equal(value.mode, 'windows_adaptive');
  for (const key of Object.keys(value).filter(key => key !== 'mode')) unsigned(value[key], key);
  assert(value.concurrentQueries >= 1); assert(value.ceilingBytes >= value.retainedBytes);
  assert.equal(value.retainedBytes, Math.min(512 << 20, value.ceilingBytes));
  assert.equal(value.metadataBytes, Math.min(128 << 20, value.ceilingBytes));
  assert(resources.retainedBytes + value.peakScratchBytes <= value.retainedBytes);
  assert(resources.metadataBytes <= value.metadataBytes); assert.equal(value.growths, 0);
}
export function assertEnvelope(value) {
  assert.equal(value?.schema, 'lycheedev.result.v1'); assert.equal(value.ok, true); assert.equal(value.error, null);
  assert(Array.isArray(value.captures)); assert(Array.isArray(value.warnings)); assert(value.result !== undefined); return value.result;
}
function noPartial(value) { for (const field of ['partialContent', 'missing', 'unavailablePartitions']) assert(!Object.hasOwn(value, field), `restored/full source retains ${field}`); }
function missing(value, fixture) {
  assert.equal(value.contentVerified, false); assert.deepEqual(value.partialContent, fixture.partial.partialContent);
  assert.equal(value.missing.length, 1); assert.equal(value.missing[0].keyID, SAMPLE_KEY_ID); assert.equal(value.missing[0].bytes, 8);
  assert.equal(value.missing[0].offset, fixture.partial.rawBytes - 8); assert.equal(value.missing[0].chunk, 1);
}
function partitions(value) {
  assert.equal(value?.length, 1); const part = value[0];
  assert.equal(part.index, 1); assert.equal(part.keyID, SAMPLE_KEY_ID); assert.equal(part.rows, 1); assert.equal(part.copies, 0); assert.equal(part.idsComplete, false);
  assert(!Object.hasOwn(part, 'recordIDs'));
}
export function assertDataValue(entry, value, fixture) {
  const partial = entry.partial === true, tableName = entry.fixture === 'catalog' ? 'HouseDecor' : 'Sample';
  const table = fixture.identity.tables.find(row => row.name === tableName); assert(table);
  assert.equal(value.complete, !partial); assert.equal(value.partial, partial);
  assertUsage(value.resources); assertMemory(value.memoryBudget, value.resources);
  assert.equal(value.resourceAccounting.wholeQueryAccountingComplete, false);
  if (value.resourceAccounting.used) assertUsage(value.resourceAccounting.used);
  if (entry.mode === 'schema') {
    assert.equal(value.snapshot, fixture.identity.snapshot); assert.deepEqual(value.pin, fixture.identity.pin);
    assert.equal(value.source, 'installation'); assert.equal(value.table, tableName); assert.equal(value.rowCount, partial ? 1 : entry.fixture === 'catalog' ? 3 : 2);
    assert.equal(value.truncated, false); assert.equal(value.truncationReason, 'none'); assert.deepEqual(value.keys, ['ID']);
    const names = value.fields.map(field => field.name); for (const name of entry.fixture === 'catalog' ? COLUMNS : ['ID', 'Value']) assert(names.includes(name));
    assert.equal(value.tables.length, 1); const proof = value.tables[0];
    assert.equal(proof.name, tableName); assert.equal(proof.fileDataID, table.fileDataID); assert.equal(proof.contentKey, fixture.objects[table.sha256].ckey);
    assert.equal(proof.definitionSHA256, table.dbdSHA256); assert.equal(proof.logicalRows, value.rowCount);
    if (partial) { missing(proof, fixture); partitions(proof.unavailablePartitions); assert.deepEqual(value.partialReasons, ['encrypted_sections_unavailable']); }
    else { assert.equal(proof.contentVerified, true); noPartial(proof); assert.equal(value.partialReasons, null); noPartial(value); }
    return;
  }
  assert.deepEqual(value.query, requestFor(entry)); assert.equal(value.sources.length, 1);
  const source = value.sources[0], file = source.file;
  assert.equal(source.complete, !partial); assert.equal(source.truncated, false); assert.deepEqual(file.pin, fixture.identity.pin); assert.equal(file.source, 'installation');
  assert.equal(file.entry.fileDataID, table.fileDataID);
  assert.equal(file.payloadEncodingKey, fixture.objects[table.sha256].ekey); assert.equal(file.entry.contentKey, fixture.objects[table.sha256].ckey);
  assert.equal(file.catalogSHA256, digest(fixture.catalog));
  assert.deepEqual(file.buildConfiguration, fixture.configs[0] && {sha256: fixture.configs[0].sha256, bytes: fixture.configs[0].bytes});
  assert.deepEqual(file.cdnConfiguration, fixture.configs[1] && {sha256: fixture.configs[1].sha256, bytes: fixture.configs[1].bytes});
  assert.deepEqual(file.verification, {encodingFramingEKey: true, encodingRequestedPagesMD5: true, encodingWholeDecodedCKey: false, rootWholeDecodedCKey: true, contentWholeDecodedCKey: !partial});
  assert.deepEqual(file.root, fixture.rootRef, 'exact authenticated Root object');
  const root = fixture.objects[file.root?.sha256]; assert(root); assert.equal(root.decodedBytes, file.root.bytes);
  if (partial) { missing(file, fixture); partitions(source.unavailablePartitions); assert.deepEqual(file.content, {sha256: '', bytes: 0}); }
  else { assert.equal(file.contentVerified, true); assert.deepEqual(file.content, {sha256: table.sha256, bytes: table.bytes}); noPartial(file); noPartial(source); }
  if (entry.analyze) {
    assert.equal(value.result.columns, null); assert.equal(value.result.rows, null);
    const measured = value.result.plan?.measured; assert(measured); assert.equal(measured.outputRows, 3);
    assert.equal(measured.scans.length, 1); assert.deepEqual(measured.scans[0], {table: {catalog: 'static', name: 'housedecor'}, calls: 3, rows: 3, lookups: 3});
    assert(measured.workUnits > 0); assert(measured.chargedBytes > 0); assert.deepEqual(value.result.plan.outputColumns, COLUMNS);
  } else {
    assert.deepEqual(value.result.columns, entry.fixture === 'catalog' ? COLUMNS : ['ID', 'Value']);
    assert.deepEqual(value.result.rows, entry.fixture === 'catalog' ? entry.unordered ? [ROWS[2], ROWS[0], ROWS[1]] : ROWS : partial ? [[1, 42]] : [[1, 42], [2, 99]]);
  }
}
function ordered(object, keys) { return Object.fromEntries(keys.filter(key => Object.hasOwn(object, key)).map(key => [key, object[key]])); }
export function captureCanonical(capture) {
  return {schema: capture.schema, id: '', blob: ordered(capture.blob, ['sha256', 'bytes']), mediaType: capture.mediaType, provenance: ordered(capture.provenance, ['kind', 'locator', 'snapshot', 'operationId', 'sourceCommit', 'baseSnapshot', 'baseSourceCommit', 'dataBuild', 'session']), complete: capture.complete, truncated: capture.truncated, capturedAt: capture.capturedAt, ...(Object.hasOwn(capture, 'dependencies') ? {dependencies: capture.dependencies} : {})};
}
export function assertCapture(capture, raw, metadata) {
  assert.equal(capture.schema, 'lycheedev.capture.v1'); assert.deepEqual({...capture, id: ''}, captureCanonical(capture));
  assert.equal(capture.id, 'CAP-' + digest(goJSON(captureCanonical(capture))));
  assert.equal(digest(raw), capture.blob.sha256); assert.equal(raw.length, capture.blob.bytes);
  assert.equal(metadata.key, 'capture/' + capture.id); assert(Number.isSafeInteger(metadata.generation) && metadata.generation > 0);
  assert.deepEqual(parse(Buffer.from(metadata.valueBase64, 'base64')), capture); assert.equal(capture.truncated, false);
  assert(Number.isFinite(Date.parse(capture.capturedAt)));
}
export function archiveBytes(evidence, name, bytes) {
  assert(/^[a-zA-Z0-9_.-]+$/.test(name)); writeFileSync(join(evidence, name), bytes, {flag: 'wx'});
  return {file: name, bytes: bytes.length, sha256: digest(bytes)};
}
export function readArchived(evidence, ref) {
  const raw = bounded(within(evidence, ref.file)); assert.equal(raw.length, ref.bytes); assert.equal(digest(raw), ref.sha256); return raw;
}
export function sourceReferences(entry, value, fixture) {
  if (!entry) return value.configurations;
  const table = fixture.identity.tables.find(row => row.name === (entry.fixture === 'catalog' ? 'HouseDecor' : 'Sample'));
  const content = entry.partial ? fixture.partial.partialContent : {sha256: table.sha256, bytes: table.bytes};
  return entry.mode === 'schema' ? [content] : [value.sources[0].file.root, content];
}
export function archiveCaptureProofs({caseId, envelope, entry, fixture, home, evidence, verify, output}) {
  const db = new DatabaseSync(join(home, 'state/toolkit.sqlite'), {readOnly: true});
  const proof = {captures: [], sources: []}; let value = envelope.result;
  try {
    for (const [index, capture] of envelope.captures.entries()) {
      const row = db.prepare('SELECT key,generation,value FROM documents WHERE key=?').get('capture/' + capture.id); assert(row);
      const metadata = {key: row.key, generation: row.generation, valueBase64: Buffer.from(row.value).toString('base64')};
      const raw = bounded(join(home, 'blobs', capture.blob.sha256.slice(0, 2), capture.blob.sha256.slice(2)));
      assertCapture(capture, raw, metadata);
      proof.captures.push({id: capture.id, metadata: archiveBytes(evidence, `${caseId}-cap-${index}-metadata.json`, Buffer.from(JSON.stringify(metadata))), blob: archiveBytes(evidence, `${caseId}-cap-${index}.bin`, raw)});
      if (index === 0 && (!entry || entry.mode === 'csv')) value = parse(raw);
    }
  } finally { db.close(); }
  for (const [index, ref] of sourceReferences(entry, value, fixture).entries()) {
    const raw = bounded(join(home, 'blobs', ref.sha256.slice(0, 2), ref.sha256.slice(2)));
    assert.equal(digest(raw), ref.sha256); assert.equal(raw.length, ref.bytes);
    proof.sources.push({ref, archive: archiveBytes(evidence, `${caseId}-source-${index}.bin`, raw)});
  }
  if (entry?.mode === 'csv') {
    proof.output = archiveBytes(evidence, `${caseId}-output.csv`, bounded(output));
    proof.sidecar = archiveBytes(evidence, `${caseId}-sidecar.json`, bounded(output + '.manifest.json'));
  }
  assertCaptureProofs({caseId, envelope, entry, fixture, evidence, proof, output});
  for (const [index, capture] of envelope.captures.entries()) {
    const verified = verify(`${caseId}-verify-${index}`, capture.id); assert.deepEqual(verified.result, capture); assert.equal(verified.captures.length, 0);
  }
  return proof;
}
export function assertCaptureProofs({caseId, envelope, entry, fixture, evidence, proof, output}) {
  const captures = envelope.captures; assert.equal(captures.length, entry?.mode === 'csv' ? 2 : 1); assert.equal(proof.captures.length, captures.length);
  const bodies = captures.map((capture, index) => {
    const row = proof.captures[index]; assert.equal(row.id, capture.id);
    const raw = readArchived(evidence, row.blob); assertCapture(capture, raw, parse(readArchived(evidence, row.metadata)));
    assert.equal(capture.provenance.snapshot, fixture.identity.snapshot); assert.equal(capture.complete, !entry?.partial);
    assert.deepEqual(capture.dependencies, index === 1 ? [captures[0].id] : undefined);
    return raw;
  });
  let value = envelope.result;
  if (!entry) {
    assert.deepEqual(value.data, fixture.identity.pin); assert.equal(value.id, fixture.identity.snapshot);
    const body = parse(bodies[0]); assert.deepEqual(body.pin, value); assert.equal(body.catalogSHA256, digest(fixture.catalog));
    assert.deepEqual(body.configurations, fixture.configs.map(({raw, ...ref}) => ref));
    assert.equal(captures[0].provenance.kind, 'local-target'); assert.equal(captures[0].provenance.locator, ordinaryPath(fixture.identity.installation));
    assert.equal(body.client.directory, ordinaryPath(fixture.identity.installation)); assert.equal(body.client.fullBuild, fixture.identity.pin.fullBuild);
    assert.equal(body.installation, ordinaryPath(pathStyle(fixture.directory).join(fixture.directory, 'game'))); value = body;
  } else {
    assert.equal(envelope.context.snapshot, fixture.identity.snapshot);
    const expectedKind = entry.mode === 'schema' ? 'data-schema' : 'data-query';
    assert.equal(captures[0].provenance.kind, expectedKind); assert.equal(captures[0].mediaType, 'application/json');
    assert.equal(captures[0].provenance.dataBuild, fixture.identity.pin.fullBuild);
    if (entry.mode !== 'schema') {
      const request = requestFor(entry), canonical = {sql: request.sql, ...(request.parameters ? {parameters: Object.fromEntries(Object.keys(request.parameters).sort().map(key => [key, request.parameters[key]]))} : {})};
      assert.equal(captures[0].provenance.locator, 'sql:sha256:' + digest(goJSON(canonical)));
    } else assert.equal(captures[0].provenance.locator, `db2:${entry.fixture === 'catalog' ? 'HouseDecor' : 'Sample'}:schema`);
    if (entry.mode === 'csv') {
      value = parse(bodies[0]); const expected = entry.fixture === 'catalog' ? CSV : Buffer.from(entry.partial ? 'ID,Value\n1,42\n' : 'ID,Value\n1,42\n2,99\n');
      assert(bodies[1].equals(expected)); assert(readArchived(evidence, proof.output).equals(expected));
      const manifest = envelope.result; assert.deepEqual(parse(readArchived(evidence, proof.sidecar)), manifest);
      assert.deepEqual(manifest, {schema: 'lycheedev.query-csv.v1', snapshot: fixture.identity.snapshot, output: ordinaryPath(output), sha256: digest(expected), bytes: expected.length, columns: entry.fixture === 'catalog' ? COLUMNS : ['ID', 'Value'], rows: entry.fixture === 'catalog' ? 3 : entry.partial ? 1 : 2, queryCapture: captures[0].id, csvCapture: captures[1].id, complete: !entry.partial});
      assert.equal(captures[1].mediaType, 'text/csv; charset=utf-8'); assert.equal(captures[1].provenance.kind, 'data-query-csv'); assert.equal(captures[1].provenance.locator, captures[0].id);
      if (entry.fixture === 'catalog') { assert.equal(expected.length, 76); assert.equal(digest(expected), CSV_SHA256); }
    } else assert.deepEqual(parse(bodies[0]), value);
    assertDataValue(entry, value, fixture);
  }
  const references = sourceReferences(entry, value, fixture); assert.equal(proof.sources.length, references.length);
  for (const [index, ref] of references.entries()) {
    const actual = proof.sources[index]; assert.deepEqual(actual.ref, ref); const raw = readArchived(evidence, actual.archive);
    assert.equal(raw.length, ref.bytes); assert.equal(digest(raw), ref.sha256);
    const expected = entry ? fixture.rawObjects[ref.sha256] : fixture.configs[index].raw; assert(expected && raw.equals(expected));
  }
}
export function dataArgs(entry, config, fixture) {
  const p = pathStyle(config.scratch), args = entry.mode === 'schema' ? ['data', 'db2', 'schema', entry.fixture === 'catalog' ? 'HouseDecor' : 'Sample'] : ['data', 'sql', '--file', p.join(config.scratch, `${entry.id}.query.json`)];
  args.push('--snapshot', fixture.identity.snapshot, '--installation', fixture.identity.installation, '--offline');
  if (entry.fixture === 'partial') args.push('--key-file', p.join(config.scratch, 'sample.keys'));
  if (entry.mode === 'csv') args.push('--encoding', 'csv', '--output', p.join(config.scratch, entry.id + '.csv'));
  return args;
}
export function targetArgs(fixture) {
  return ['target', 'resolve', '--installation', fixture.identity.installation, '--product', 'retail', '--build', '12.1.0.69933', '--region', 'cn', '--locale', 'zhCN', '--definitions', 'b'.repeat(40), '--offline'];
}
export function expectedDataCommands(config, fixtures, envelopes) {
  const p = pathStyle(config.scratch), result = [];
  const publicCall = (id, args, kind = 'catalog') => result.push({id, executable: config.node, args: [config.launcher, ...args, '--home', p.join(config.scratch, kind + '-home'), '--format', 'json'], cwd: config.scratch, kind: 'public'});
  const captures = (id, kind) => { for (const [i, cap] of envelopes[id].captures.entries()) publicCall(`${id}-verify-${i}`, ['evidence', 'verify', cap.id], kind); };
  for (const [i, kind] of ['catalog', 'partial'].entries()) result.push({id: `export-${kind}`, executable: config.exporters.executables[i].path, args: [kind === 'catalog' ? 'prepare_catalog_public_fixture' : 'prepare_skill_partial_fixture', '--exact', '--ignored', '--nocapture'], cwd: config.root, kind: 'exporter'});
  result.push({id: 'npm-pack', executable: config.node, args: [config.npm.path, 'pack', config.sourcePackage, '--json', '--pack-destination', config.scratch, ...npmOfflineFlags(config.scratch)], cwd: config.scratch, kind: 'npm'});
  result.push({id: 'npm-install', executable: config.node, args: [config.npm.path, 'install', '--global', '--prefix', config.prefix, config.tgz, ...npmOfflineFlags(config.scratch)], cwd: config.scratch, kind: 'npm'});
  for (const [id, args] of [['version', ['version']], ['describe', ['describe']], ['help', ['--help']], ['sql-help', ['data', 'sql', '--help']]]) publicCall(id, args);
  for (const kind of ['catalog', 'partial']) {
    publicCall(`${kind}-init`, ['init'], kind);
    publicCall(`${kind}-target-resolve`, targetArgs(fixtures[kind]), kind); captures(`${kind}-target-resolve`, kind);
    publicCall(`${kind}-target-show`, ['target', 'show', fixtures[kind].identity.snapshot], kind);
    for (const entry of CASES.filter(entry => entry.fixture === kind && (kind === 'catalog' || entry.partial))) { publicCall(entry.id, dataArgs(entry, config, fixtures[kind]), kind); captures(entry.id, kind); }
  }
  publicCall('restored-target-show', ['target', 'show', fixtures.partial.identity.snapshot], 'partial');
  for (const entry of CASES.filter(entry => entry.fixture === 'partial' && !entry.partial)) { publicCall(entry.id, dataArgs(entry, config, fixtures.partial), 'partial'); captures(entry.id, 'partial'); }
  return result;
}
export function assertDataReport(report, workflow, {evidenceDirectory = report?.config?.evidence} = {}) {
  assert.equal(report.schema, 'lycheedev.skill-data-usage.v2'); assert.equal(report.state, 'passed_selected_data_workflow');
  assert.equal(report.goContract, CURRENT_GO_CONTRACT); assert.equal(report.skillTree, 'b9e162ace7abe7ea4a2feecef822283767eb1210');
  assert.equal(report.completeSkillAcceptance, false); assert.deepEqual(report.pendingTasks, pendingDataTasks(workflow));
  assert.deepEqual(report.checks.map(row => row.caseId), CHECKS); assert(report.checks.every(row => row.state === 'passed'));
  const config = report.config, p = pathStyle(config.scratch);
  assert(p.isAbsolute(config.runnerRoot), 'separate selected runner identity');
  assert.deepEqual(config.skillIdentity, dataSkillIdentity(loadDataSkillContract(config.skillIdentity)));
  assert.equal(config.platform, 'win32'); assert.equal(config.arch, 'x64'); assert(p.isAbsolute(config.node));
  assert.equal(config.prefix, p.join(config.scratch, 'npm-prefix')); assert.equal(config.launcher, p.join(config.prefix, 'node_modules/lycheedev/payload/skill/scripts/lycheedev.mjs'));
  assert.equal(config.tgz, p.join(config.scratch, `lycheedev-${config.version}.tgz`));
  const fixtures = Object.fromEntries(['catalog', 'partial'].map(kind => [kind, loadArchivedFixture(evidenceDirectory, report.fixtures[kind], kind)]));
  for (const kind of ['catalog', 'partial']) assert.equal(fixtures[kind].directory, p.join(config.scratch, `${kind}-fixture`));
  const envelopes = {}, seen = new Set();
  for (const command of report.commands) {
    assert(!seen.has(command.id)); seen.add(command.id); assert.equal(command.exit, 0); assert.equal(command.signal, null); assert.equal(command.error, undefined);
    const stdout = readArchived(evidenceDirectory, command.stdout), stderr = readArchived(evidenceDirectory, command.stderr); assert(stdout.length + stderr.length <= 24 << 20);
    if (command.kind === 'public') { const envelope = parse(stdout); assertEnvelope(envelope); envelopes[command.id] = envelope; }
  }
  const expected = expectedDataCommands(config, fixtures, envelopes); assert.equal(expected.length, 42); assert.equal(report.commands.length, 42);
  for (const [i, row] of expected.entries()) for (const key of ['id', 'executable', 'args', 'cwd', 'kind']) assert.deepEqual(report.commands[i][key], row[key], `original command ${i}.${key}`);
  assert.equal(envelopes.version.result.platform, 'windows/amd64'); assert.equal(envelopes.version.result.version, config.version); assert.equal(typeof envelopes.version.result.rust, 'string');
  const directory = envelopes.describe.result.commands; assert(Array.isArray(directory)); assert(Array.isArray(envelopes.help.result.commands));
  for (const [path, flags] of [['init', []], ['data sql', ['--file', '--encoding', '--output']], ['data db2 schema', []], ['target resolve', ['--installation', '--definitions', '--offline']], ['target show', []], ['evidence verify', []]]) commandFlags(directory, path, flags);
  assert.deepEqual(envelopes.help.result.commands, directory);
  assert.deepEqual(envelopes['sql-help'].result.commands, [commandFlags(directory, 'data sql', ['--file', '--encoding', '--output'])]);
  assert(envelopes['sql-help'].result.flags.includes('--help'));
  const pack = parse(readArchived(evidenceDirectory, report.commands.find(row => row.id === 'npm-pack').stdout));
  assert.equal(pack.length, 1); assert.equal(pack[0].filename, p.basename(config.tgz)); assert.equal(pack[0].name, 'lycheedev'); assert.equal(pack[0].version, config.version);
  const installed = parse(readArchived(evidenceDirectory, report.installedInventory)); assert.deepEqual(installed, config.packageFiles); assert.equal(inventoryHash(installed), config.packageSHA256);
  const skill = assertDataSkillInventory(installed, config.skillIdentity);
  const {files: skillFiles, ...identity} = report.skillProof;
  assert.deepEqual(identity, config.skillIdentity, 'fixed Go provenance and adapted candidate are separate');
  assert.deepEqual(skillFiles.map(row => row.path), skill.files.map(row => row.path), 'full archived candidate Skill');
  for (const [index, row] of skill.files.entries()) assertDataSkillBytes(readArchived(evidenceDirectory, skillFiles[index].archive), row);
  const shim = readArchived(evidenceDirectory, report.npmShim); assert(shim.length > 0 && shim.toString().includes('lycheedev.mjs'));
  let count = 0;
  for (const kind of ['catalog', 'partial']) {
    const initialized = envelopes[`${kind}-init`].result;
    assert.equal(ordinaryPath(initialized.root), ordinaryPath(p.join(config.scratch, kind + '-home')));
    assert.equal(initialized.identity.schema, 'lycheedev.workspace.v1'); assert.match(initialized.identity.workspaceId, /^[0-9a-f]{32}$/); assert(Number.isFinite(Date.parse(initialized.identity.createdAt)));
    assert.deepEqual(parse(readArchived(evidenceDirectory, report.workspaces[kind])), initialized.identity);
    const seeded = parse(readArchived(evidenceDirectory, report.seeded[kind]));
    assert.deepEqual(seeded, fixtures[kind].documents.map(doc => ({key: doc.key, generation: 1, valueBase64: doc.bytes.toString('base64')})));
    assert.deepEqual(envelopes[`${kind}-target-show`].result, envelopes[`${kind}-target-resolve`].result);
    const id = `${kind}-target-resolve`, envelope = envelopes[id];
    assertCaptureProofs({caseId: id, envelope, fixture: fixtures[kind], evidence: evidenceDirectory, proof: report.proofs[id]});
    count += envelope.captures.length;
  }
  assert.deepEqual(envelopes['restored-target-show'].result, envelopes['partial-target-resolve'].result);
  assert.equal(readArchived(evidenceDirectory, report.keys.missing).toString(), MISSING_KEYS); assert.equal(readArchived(evidenceDirectory, report.keys.restored).toString(), RESTORED_KEYS);
  for (const entry of CASES) {
    const fixture = fixtures[entry.fixture], envelope = envelopes[entry.id];
    if (entry.mode !== 'schema') assert.deepEqual(parse(readArchived(evidenceDirectory, report.queries[entry.id])), requestFor(entry));
    assertCaptureProofs({caseId: entry.id, envelope, entry, fixture, evidence: evidenceDirectory, proof: report.proofs[entry.id], output: p.join(config.scratch, entry.id + '.csv')});
    count += envelope.captures.length;
  }
  assert.equal(count, 16);
  assert.equal(new Set(Object.values(report.proofs).flatMap(proof => proof.captures.map(row => row.id))).size, 16, 'every query owns a fresh CAP');
  for (const [id, envelope] of Object.entries(envelopes).filter(([id]) => !id.includes('-verify-'))) {
    for (const [i, capture] of envelope.captures.entries()) {
      const verified = envelopes[`${id}-verify-${i}`]; assert(verified); assert.deepEqual(verified.result, capture); assert.equal(verified.captures.length, 0);
    }
  }
  assert.deepEqual(report.completedTasks, [{taskId: 'g323-data-query', runtimeVerified: true, basis: 'actual_owned_npm_installed_skill_and_independent_full_capture_replay', captures: 16}]);
  return true;
}
