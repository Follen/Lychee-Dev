// Linux-capable checker/fault tests. These are never real CLI/Skill acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import os from 'node:os';
import {assertApiAnswer, assertBoundedCoverage, assertNativeEvidence, commandFlags, inventory, inventoryHash, jsonFile, sha256, verifyCandidate, within} from './assertions.mjs';

const workflow = jsonFile(new URL('./workflow.json', import.meta.url));
const oracle = jsonFile(new URL('../rust-source-full-corpus/retail-oracle.json', import.meta.url));
const write = (path, text) => { mkdirSync(dirname(path), {recursive: true}); writeFileSync(path, text); };
const json = (path, value) => write(path, JSON.stringify(value, null, 2) + '\n');
function fixture(t) {
  const root = mkdtempSync(join(os.tmpdir(), 'lycheedev-skill-checker-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const packageRoot = join(root, 'candidate/package'), reportPath = join(root, 'candidate/report.json');
  const binary = Buffer.from('synthetic checker bytes; not an EXE'), launcher = '// synthetic checker launcher; never executed\n';
  write(join(root, 'packages/npm/lycheedev/bin/lycheedev-rust.mjs'), launcher);
  write(join(packageRoot, 'bin/lycheedev.mjs'), launcher);
  for (const path of workflow.requiredSkillFiles) {
    write(join(root, 'skills/lycheedev', path), `synthetic ${path}\n`);
    write(join(packageRoot, 'payload/skill', path), `synthetic ${path}\n`);
  }
  write(join(packageRoot, 'payload/tool/luac/luac.exe'), 'synthetic compiler');
  write(join(packageRoot, 'native/windows-amd64/lycheedev.exe'), binary);
  json(join(packageRoot, 'package.json'), {version: '0.0.0-test', private: true});
  const resources = inventory(join(packageRoot, 'payload')).map(row => ({path: row.path, bytes: row.bytes, sha256: row.sha256}));
  const artifact = {binarySHA256: sha256(binary), inputHashes: {sourceTree: 'a'.repeat(64)}};
  const manifest = {schema: 'lycheedev.release.v1', runtime: 'rust', version: '0.0.0-test', commit: 'b'.repeat(40),
    binaries: {'windows-amd64': {path: 'native/windows-amd64/lycheedev.exe', bytes: binary.length, sha256: artifact.binarySHA256}}, resources};
  json(join(packageRoot, 'release.json'), manifest);
  const report = {schema: 'lycheedev.rust-npm-smoke.v1', state: 'assembled', developmentOnly: true, packageRoot, binarySHA256: artifact.binarySHA256,
    sourceTreeSHA256: artifact.inputHashes.sourceTree, releaseManifestSHA256: sha256(readFileSync(join(packageRoot, 'release.json'))), commit: manifest.commit,
    resourceInventorySHA256: sha256(Buffer.from(JSON.stringify(resources))), compilerSHA256: sha256(Buffer.from('synthetic compiler'))};
  json(reportPath, report);
  return {root, packageRoot, reportPath, artifact, manifest, report, verify: () => verifyCandidate(root, reportPath, artifact, workflow), rewrite: () => json(reportPath, report)};
}

test('workflow binds fixed Go contract and complete original Skill entry/reference tree', () => {
  assert.equal(workflow.goContract, '3057a9666ab078270b63c7f06de748069b0d1f4b');
  assert.equal(workflow.requiredSkillFiles.length, 23);
  assert.equal(new Set(workflow.requiredSkillFiles).size, 23);
  for (const name of ['SKILL.md', 'scripts/lycheedev.mjs', 'references/source-research.md', 'references/source-relations.md', 'references/source-preparation.md', 'references/live-investigation.md', 'references/data-investigation.md']) assert(workflow.requiredSkillFiles.includes(name));
  assert(workflow.prompts.every(row => typeof row.text === 'string' && row.text.length > 20));
});
test('relative paths reject traversal, rooted paths and links', t => {
  const f = fixture(t);
  for (const name of ['../escape', '/absolute', 'C:/absolute', 'safe/../escape', 'safe\\escape', './candidate', 'candidate//package']) assert.throws(() => within(f.root, name));
  if (process.platform !== 'win32') { symlinkSync(join(f.root, 'candidate'), join(f.root, 'linked')); assert.throws(() => within(f.root, 'linked/package'), /linked/); }
});
test('inventory stable order and bounded bytes/files/depth', t => {
  const f = fixture(t), first = inventory(f.packageRoot);
  assert.equal(inventoryHash(first), inventoryHash(inventory(f.packageRoot)));
  assert.throws(() => inventory(f.packageRoot, {maxBytes: 10}), /byte budget/);
  assert.throws(() => inventory(f.packageRoot, {maxFiles: 1}), /budget|file count/);
  write(join(f.packageRoot, ...Array(26).fill('nested'), 'file'), 'x');
  assert.throws(() => inventory(f.packageRoot), /directory depth/);
});
test('complete manifest-bound candidate checker accepts only synthetic fixture identity', t => {
  const f = fixture(t), verified = f.verify();
  assert.equal(verified.reportSHA256, sha256(readFileSync(f.reportPath)));
  assert.equal(verified.packageSHA256, inventoryHash(inventory(f.packageRoot)));
});
test('different sealed binary, source or report manifest is rejected', t => {
  const f = fixture(t);
  for (const field of ['binarySHA256', 'sourceTreeSHA256', 'releaseManifestSHA256', 'resourceInventorySHA256']) {
    const old = f.report[field]; f.report[field] = '0'.repeat(64); f.rewrite(); assert.throws(f.verify); f.report[field] = old; f.rewrite();
  }
});
test('candidate cannot resolve a package outside its owned repository', t => {
  const f = fixture(t); f.report.packageRoot = dirname(f.root); f.rewrite(); assert.throws(f.verify);
});
test('actual packaged native bytes and launcher are both verified', t => {
  const f = fixture(t); write(join(f.packageRoot, 'bin/lycheedev.mjs'), '// substituted'); assert.throws(f.verify, /launcher/);
  write(join(f.packageRoot, 'native/windows-amd64/lycheedev.exe'), 'substituted'); assert.throws(f.verify);
});
test('a self-consistent reduced manifest cannot drop a fixed Skill reference', t => {
  const f = fixture(t);
  f.manifest.resources = f.manifest.resources.filter(row => row.path !== 'skill/references/source-relations.md');
  json(join(f.packageRoot, 'release.json'), f.manifest);
  f.report.releaseManifestSHA256 = sha256(readFileSync(join(f.packageRoot, 'release.json')));
  f.report.resourceInventorySHA256 = sha256(Buffer.from(JSON.stringify(f.manifest.resources))); f.rewrite();
  assert.throws(f.verify, /fixed Go Skill resource missing/);
});
test('unmanifested payload file and stale packaged Skill are rejected', t => {
  const f = fixture(t); write(join(f.packageRoot, 'payload/unmanifested.txt'), 'extra'); assert.throws(f.verify, /unmanifested/);
  rmSync(join(f.packageRoot, 'payload/unmanifested.txt'));
  write(join(f.root, 'skills/lycheedev/SKILL.md'), 'new Skill'); assert.throws(f.verify, /packaged Skill source/);
});
test('duplicate or traversing resource records fail even with updated report digests', t => {
  const f = fixture(t); f.manifest.resources.push({...f.manifest.resources[0]});
  json(join(f.packageRoot, 'release.json'), f.manifest);
  f.report.releaseManifestSHA256 = sha256(readFileSync(join(f.packageRoot, 'release.json')));
  f.report.resourceInventorySHA256 = sha256(Buffer.from(JSON.stringify(f.manifest.resources))); f.rewrite();
  assert.throws(f.verify, /unique resource path/);
});
test('installed describe controls command and flag use; absent batch cannot pass', () => {
  const directory = [{path: 'source query', flags: ['--snapshot <pin>', '--limit <1-200>']}];
  commandFlags(directory, 'source query', ['--snapshot', '--limit']);
  assert.throws(() => commandFlags(directory, 'source batch', ['--file']), /lacks required Go Skill command/);
  assert.throws(() => commandFlags(directory, 'source query', ['--offline']), /lacks --offline/);
});
test('truncation and unavailable semantics cannot be reported complete', () => {
  for (const value of [{complete: true, truncated: true, coverage: {}}, {complete: true, truncated: false, coverage: {complete: false}}, {complete: true, truncated: false, coverage: {semantic: 'unavailable'}}]) {
    assert.throws(() => assertBoundedCoverage(value)); value.complete = false; assertBoundedCoverage(value);
  }
  assert.throws(() => assertBoundedCoverage({complete: true, truncated: false}));
});
function nativeResult() {
  const analysis = {engine: 'native_common_facts', state: 'partial', inputIdentity: 'a'.repeat(64), sourceFiles: oracle.counts['.lua'], sourceExecution: false};
  return {complete: false, truncated: false, coverage: {semantic: 'partial'}, semanticRuntime: {engine: 'native-common-source', rulesVersion: 'synthetic-checker-rules'}, semanticAnalysis: structuredClone(analysis),
    relations: [{path: workflow.nativeCallsite.path, line: workflow.nativeCallsite.line, target: workflow.nativeCallsite.target, kind: 'call', state: 'static_candidate',
      basis: ['native_common_source_facts'], site: structuredClone(workflow.nativeCallsite.site), proof: 'synthetic-reference-identity', lexical: {state: 'unavailable', reason: 'synthetic partial fixture'}, observations: []}],
    nativeResearch: {schema: 'lycheedev.source-native-research.v1', entryEvidence: {sourceExecution: false}, analysis, inputIdentity: analysis.inputIdentity,
      lifecycle: {owner: 'source_owned_process', mode: 'one_shot', retired: true}}};
}
const verifyNative = result => assertNativeEvidence(result, oracle, workflow.nativeCallsite);
test('ordinary native check accepts honest partial callsite facts and actual one-shot retirement', () => {
  verifyNative(nativeResult());
  const resolved = nativeResult(); resolved.relations[0].state = 'resolved'; resolved.relations[0].observations = [{proof: 'synthetic-proof', occurrence: {kind: 'entry'}, selected: {state: 'known', value: {}}}];
  verifyNative(resolved);
});
test('ordinary native metadata rejects schema-only stubs and mismatched engine/input/lifecycle', () => {
  const stub = {complete: false, truncated: false, coverage: {semantic: 'partial'}, nativeResearch: {schema: 'lycheedev.source-native-research.v1', entryEvidence: {sourceExecution: false}, analysis: {sourceFiles: 1}}};
  assert.throws(() => verifyNative(stub));
  const mutations = [
    row => { delete row.nativeResearch; }, row => { row.nativeResearch.entryEvidence.sourceExecution = true; },
    row => { delete row.nativeResearch.lifecycle; }, row => { row.nativeResearch.lifecycle.owner = 'unknown'; },
    row => { row.nativeResearch.lifecycle.mode = 'session'; }, row => { row.nativeResearch.lifecycle.retired = false; },
    row => { row.semanticRuntime.engine = 'static'; }, row => { delete row.semanticRuntime.rulesVersion; },
    row => { row.nativeResearch.analysis.engine = 'static-extraction'; }, row => { row.nativeResearch.analysis.state = 'unavailable'; },
    row => { row.nativeResearch.analysis.sourceExecution = true; }, row => { row.nativeResearch.inputIdentity = 'not-a-digest'; },
    row => { row.nativeResearch.analysis.inputIdentity = 'b'.repeat(64); }, row => { row.semanticAnalysis.sourceFiles = 1; },
    row => { row.nativeResearch.analysis.sourceFiles = 1; row.semanticAnalysis.sourceFiles = 1; }, row => { row.coverage.semantic = 'complete'; },
    row => { row.nativeResearch.relations = row.relations; },
  ];
  for (const mutate of mutations) { const row = nativeResult(); mutate(row); assert.throws(() => verifyNative(row)); }
});
test('native metadata plus structural-only rows cannot impersonate native-origin callsite evidence', () => {
  const mutations = [
    row => { row.relations = [{...oracle.staticCall, state: 'static_candidate', basis: ['static-extraction']}]; },
    row => { row.relations = []; }, row => { row.relations[0].basis = ['static-extraction']; },
    row => { row.relations[0].line++; }, row => { row.relations[0].kind = 'reference'; }, row => { row.relations[0].target = 'Other'; },
    row => { row.relations[0].site.start++; }, row => { row.relations[0].site.end++; }, row => { row.relations[0].site.file = 'other.lua'; },
    row => { delete row.relations[0].proof; }, row => { delete row.relations[0].lexical; }, row => { delete row.relations[0].observations; },
    row => { row.relations[0].state = 'resolved'; },
    row => { row.relations[0].state = 'resolved'; row.relations[0].observations = [{proof: 'fixture-proof', occurrence: {kind: 'entry'}, selected: {state: 'unavailable'}}]; },
  ];
  for (const mutate of mutations) { const row = nativeResult(); mutate(row); assert.throws(() => verifyNative(row)); }
});
test('API and native-site fixture windows retain independently authenticated original identities', () => {
  const window = workflow.apiOriginalWindow;
  assert.equal(window.textSHA256, 'e20df7e8a9e30988ebc68f92cd2715db42915d4beecb1faf371cc7a669e3ca12');
  assert.equal(sha256(Buffer.from(window.text)), window.textSHA256);
  assert.equal(window.archiveSHA256, oracle.archiveSHA256); assert.equal(window.fullFileSHA256, oracle.anchors[2].sha256);
  assert.equal(workflow.nativeCallsite.fullFileSHA256, oracle.anchors[0].sha256);
  assert.deepEqual(workflow.nativeCallsite.site, {file: oracle.staticCall.path, start: 3940, end: 3960});
  assert.equal(workflow.nativeCallsite.siteText, 'GetActionButtonForID');
});
test('API answer requires the exact requested 10–21 original window, not matching substrings', () => {
  const window = workflow.apiOriginalWindow, row = {path: window.path, line: window.firstLine};
  const result = {repository: oracle.repository, commit: oracle.commit, path: window.path, firstLine: window.firstLine, lastLine: window.lastLine, totalLines: window.totalLines, blob: {sha256: window.fullFileSHA256}, text: window.text};
  const verify = value => assertApiAnswer(value, row, oracle, window);
  const answer = verify(result); assert.equal(answer.secretArgumentsLine, 13); assert.equal(answer.windowSHA256, window.textSHA256);
  for (const changed of [
    {...result, commit: '0'.repeat(40)}, {...result, firstLine: 11}, {...result, lastLine: 22}, {...result, totalLines: 82},
    {...result, text: oracle.anchors[2].text}, // The previous 11–22 text with claimed 10–21 coordinates.
    {...result, text: window.text.replaceAll('\n', '\r\n')}, {...result, text: window.text + '\n'},
    {...result, text: window.text.replace('AllowedWhenUntainted', 'Allowed')}, {...result, blob: {sha256: '0'.repeat(64)}},
  ]) assert.throws(() => verify(changed));
  assert.throws(() => assertApiAnswer(result, {...row, line: 11}, oracle, window));
});
