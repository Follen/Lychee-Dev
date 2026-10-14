// Linux-capable checker/fault tests. These are never real CLI/Skill acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import os from 'node:os';
import {assertApiAnswer, assertBoundedCoverage, assertNativeContext, assertNativeEvidence, commandFlags, evaluateWorkflowCheck, inventory, inventoryHash, jsonFile, nativeOperationDigest, nativePlanDigest, POST_SEMANTIC_REQUIREMENTS, sha256, verifyCandidate, within} from './assertions.mjs';

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
function nativeContextResult() {
  const result = nativeResult(), anchor = oracle.anchors[0];
  result.relations = [];
  result.snippets = [{path: anchor.path, firstLine: 97, lastLine: 107, contentHash: anchor.sha256, basis: 'definition',
    text: anchor.text.replace(/\n+$/, '').split('\n').map((line, index) => `${97 + index}: ${line}`).join('\n')}];
  const target = {site: {file: anchor.path, start: 2915, end: 2935}, file_sha256: anchor.sha256,
    position: {path: anchor.path, range: {start: {line: 96, character: 9}, end: {line: 96, character: 29}},
      coveringRange: {start: {line: 96, character: 9}, end: {line: 96, character: 29}}, bytes: {start: 2915, end: 2935}, positionAccuracy: 'utf16-character'}, material: {kind: 'source'}};
  const plan = {schema: 'lycheedev.native-semantic-plan.v1', source_identity: result.nativeResearch.inputIdentity, query_identity: 'b'.repeat(64),
    operations: [{kind: 'references', target}, {kind: 'type', target: structuredClone(target)}], batch_sizes: [2], unavailable: [], outgoing_truncated: false, scanner_diagnostics: [], scanner_diagnostics_omitted: 0};
  result.nativeResearch.semanticPlan = {state: 'execution_incomplete', plan, resourceAccounting: {executedSemanticBatches: 0}};
  result.nativeResearch.semanticExecution = {schema: 'lycheedev.native-semantic-execution.v1',
    receipt: {planned_batches: 1, completed_batches: 0, scopes: 1, unavailable_scopes: 0, uncertain_answers: 2, unexecuted_operations: 1, plan_identity: nativePlanDigest(plan)},
    answers: [{scope: {kind: 'synthetic-checker-scope'}, operations: [
      {operation: 0, batch: 0, kind: 'references', operationIdentity: nativeOperationDigest(plan.operations[0]), state: 'partial', executed: true,
        result: {relationsSHA256: 'c'.repeat(64), relationCount: 1, analysis: structuredClone(result.nativeResearch.analysis)}},
      {operation: 1, batch: 0, kind: 'type', operationIdentity: nativeOperationDigest(plan.operations[1]), state: 'unavailable', executed: false},
    ]}]};
  return result;
}
function refreshContextPlanIdentities(context) {
  const plan = context.nativeResearch.semanticPlan.plan, execution = context.nativeResearch.semanticExecution;
  execution.receipt.plan_identity = nativePlanDigest(plan);
  for (const scope of execution.answers) for (const [index, answer] of (scope.operations ?? []).entries()) answer.operationIdentity = nativeOperationDigest(plan.operations[index]);
}
test('real native depth-zero context requires exact definition evidence without requiring refs traversal', () => {
  const context = nativeContextResult();
  assertNativeContext(context, oracle);
  assert.throws(() => verifyNative(context), /callsite/, 'refs still requires native callsite rows');
  const mixed = nativeContextResult(); mixed.relations = nativeResult().relations;
  assert.throws(() => assertNativeContext(mixed, oracle), /depth-zero/, 'depth-zero context cannot be replaced by a refs page');
});
test('depth-zero native context rejects static substitution, unretired execution and altered original snippets', () => {
  for (const mutate of [
    row => { delete row.nativeResearch; }, row => { row.nativeResearch.lifecycle.retired = false; },
    row => { row.nativeResearch.analysis.inputIdentity = 'b'.repeat(64); }, row => { row.semanticRuntime.engine = 'static'; },
    row => { row.snippets = []; }, row => { row.snippets[0].path = 'wrong.lua'; }, row => { row.snippets[0].firstLine = 98; },
    row => { row.snippets[0].lastLine = 108; }, row => { row.snippets[0].contentHash = '0'.repeat(64); },
    row => { row.snippets[0].basis = 'relation:call'; }, row => { row.snippets[0].text = oracle.anchors[0].declarationLine; },
    row => { row.snippets[0].text = row.snippets[0].text.replace('97:', '98:'); },
  ]) { const context = nativeContextResult(); mutate(context); assert.throws(() => assertNativeContext(context, oracle)); }
});
test('context accepts executed references with partial type or unavailable scopes even when completed batches is zero', () => {
  const context = nativeContextResult(), execution = context.nativeResearch.semanticExecution;
  assert.equal(execution.receipt.completed_batches, 0);
  assertNativeContext(context, oracle);
  execution.answers.push({scope: {reason: 'another scope unavailable'}, state: 'unavailable', operationsExecuted: false});
  execution.receipt.unavailable_scopes = 1;
  assertNativeContext(context, oracle);
});
test('incomplete native execution cannot relabel both analyses and public coverage as complete', () => {
  const context = nativeContextResult();
  assertNativeContext(context, oracle);
  assert.equal(context.nativeResearch.semanticExecution.receipt.unexecuted_operations, 1);
  assert.equal(context.nativeResearch.semanticPlan.state, 'execution_incomplete');
  assert.equal(context.nativeResearch.semanticExecution.receipt.completed_batches, 0);
  context.nativeResearch.analysis.state = 'complete'; context.semanticAnalysis.state = 'complete';
  context.coverage.semantic = 'complete'; context.complete = true;
  assert.throws(() => assertNativeContext(context, oracle), /incomplete plan\/scope\/answers/);
});
test('context rejects missing plan/execution and a planned or explicitly zero-execution summary', () => {
  const mutations = [
    row => { delete row.nativeResearch.semanticPlan; }, row => { delete row.nativeResearch.semanticExecution; },
    row => { row.nativeResearch.semanticPlan.state = 'planned_not_executed'; },
    row => {
      const execution = row.nativeResearch.semanticExecution;
      execution.answers = [{scope: {reason: 'never started'}, state: 'unavailable', operationsExecuted: false}];
      Object.assign(execution.receipt, {scopes: 0, unavailable_scopes: 1, uncertain_answers: 0, unexecuted_operations: 0});
    },
    row => {
      const execution = row.nativeResearch.semanticExecution;
      execution.answers[0].operations[0].executed = false; execution.receipt.unexecuted_operations = 2;
    },
  ];
  for (const mutate of mutations) { const context = nativeContextResult(); mutate(context); assert.throws(() => assertNativeContext(context, oracle)); }
});
test('context execution binds plan schemas, actual inputs, ordered operations and receipt totals', () => {
  for (const mutate of [
    row => { row.nativeResearch.semanticPlan.plan.schema = 'wrong'; }, row => { row.nativeResearch.semanticExecution.schema = 'wrong'; },
    row => { row.nativeResearch.semanticPlan.plan.source_identity = 'd'.repeat(64); refreshContextPlanIdentities(row); },
    row => { row.nativeResearch.semanticPlan.plan.operations = []; }, row => { row.nativeResearch.semanticPlan.plan.batch_sizes = [1]; },
    row => { row.nativeResearch.semanticExecution.receipt.plan_identity = 'd'.repeat(64); },
    row => { row.nativeResearch.semanticExecution.receipt.planned_batches = 2; },
    row => { row.nativeResearch.semanticExecution.receipt.completed_batches = 1; },
    row => { row.nativeResearch.semanticExecution.receipt.scopes = 2; }, row => { row.nativeResearch.semanticExecution.receipt.uncertain_answers = 0; },
    row => { row.nativeResearch.semanticExecution.receipt.unexecuted_operations = 0; },
    row => { row.nativeResearch.semanticExecution.answers[0].operations.pop(); },
    row => { row.nativeResearch.semanticExecution.answers[0].operations[0].operation = 1; },
    row => { row.nativeResearch.semanticExecution.answers[0].operations[0].batch = 1; },
    row => { row.nativeResearch.semanticExecution.answers[0].operations[0].kind = 'type'; },
    row => { row.nativeResearch.semanticExecution.answers[0].operations[0].operationIdentity = 'e'.repeat(64); },
    row => { row.nativeResearch.semanticExecution.answers[0].operations[0].result.relationsSHA256 = ''; },
    row => { row.nativeResearch.semanticExecution.answers[0].operations[0].result.relationCount = -1; },
    row => { row.nativeResearch.semanticExecution.answers[0].operations[0].result.analysis.inputIdentity = 'e'.repeat(64); },
  ]) { const context = nativeContextResult(); mutate(context); assert.throws(() => assertNativeContext(context, oracle)); }
});
test('self-consistent operation hashes cannot retarget the authenticated selected declaration', () => {
  for (const mutate of [
    target => { target.site.start++; }, target => { target.site.end++; }, target => { target.site.file = 'other.lua'; },
    target => { target.file_sha256 = 'd'.repeat(64); }, target => { target.position.range.start.line = 97; },
    target => { target.position.coveringRange.start.character = 10; }, target => { target.position.bytes.start++; },
    target => { target.material.kind = 'api_definition'; },
  ]) {
    const context = nativeContextResult(); mutate(context.nativeResearch.semanticPlan.plan.operations[0].target);
    refreshContextPlanIdentities(context); assert.throws(() => assertNativeContext(context, oracle));
  }
});
test('typed plan hashes are independent of output JSON key order and differ from sorted struct JSON', () => {
  const plan = nativeContextResult().nativeResearch.semanticPlan.plan;
  const sort = value => Array.isArray(value) ? value.map(sort) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, sort(value[key])])) : value;
  // Independently recomputed with Python OrderedDict from the Rust struct field
  // declarations; these are checker fixtures, not product execution receipts.
  assert.equal(nativePlanDigest(plan), '179ddd49791918d300f39d8e3f8075a41c1c4347dbf32ba571f2babc87797523');
  assert.equal(nativeOperationDigest(plan.operations[0]), '3475607b929a962729b7e29b14d1655fa7376f68820e1b83db1fbc5f9156b7a8');
  assert.equal(nativePlanDigest(plan), nativePlanDigest(sort(plan)));
  assert.equal(nativeOperationDigest(plan.operations[0]), nativeOperationDigest(sort(plan.operations[0])));
  assert.notEqual(nativePlanDigest(plan), sha256(Buffer.from(JSON.stringify(sort(plan)))));
  assert.notEqual(nativeOperationDigest(plan.operations[0]), sha256(Buffer.from(JSON.stringify(sort(plan.operations[0])))));
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

function postSemanticFixture(nativeWork) {
  const checks = [], calls = [];
  for (const [index, caseId] of POST_SEMANTIC_REQUIREMENTS.slice(0, 2).entries()) {
    checks.push(evaluateWorkflowCheck({caseId, priorChecks: checks, work: () => nativeWork(index, calls)}));
  }
  checks.push(evaluateWorkflowCheck({caseId: 'final-same-pin-api-query', priorChecks: checks, work: () => { calls.push('query-final'); return {pin: 'PIN-fixture'}; }}));
  const original = structuredClone(checks);
  const dependent = evaluateWorkflowCheck({caseId: 'post-semantic-source-read-still-usable', priorChecks: checks, requires: POST_SEMANTIC_REQUIREMENTS, work: () => { calls.push('post-semantic-assertion'); return {verifiedChecks: [...POST_SEMANTIC_REQUIREMENTS]}; }});
  assert.deepEqual(checks, original, 'dependency evaluation preserves every original failure/result');
  checks.push(dependent);
  return {checks, calls, dependent, overallPassed: checks.every(row => row.state === 'passed')};
}
test('both native calls not requested cannot turn an independent final query into post-semantic success', () => {
  const result = postSemanticFixture(index => { throw new Error(`installed describe lacks source ${index ? 'context' : 'refs'}`); });
  assert.deepEqual(result.calls, ['query-final']);
  assert.equal(result.checks[2].state, 'passed', 'ordinary final same-pin query retains its own accurate success');
  assert.equal(result.dependent.state, 'not_run');
  assert.deepEqual(result.dependent.blockedBy, [
    {caseId: 'ordinary-native-callers', state: 'failed', reason: 'installed describe lacks source refs'},
    {caseId: 'ordinary-native-context', state: 'failed', reason: 'installed describe lacks source context'},
  ]);
  assert.equal(result.overallPassed, false);
});
test('one native execution or evidence failure blocks post-semantic success without swallowing it', () => {
  const result = postSemanticFixture((index, calls) => {
    calls.push(index ? 'context-native' : 'refs-native');
    if (index) throw new Error('native worker failed: original retained failure');
    return {native: {lifecycle: {mode: 'one_shot', retired: true}}};
  });
  assert.deepEqual(result.calls, ['refs-native', 'context-native', 'query-final']);
  assert.equal(result.checks[0].state, 'passed'); assert.equal(result.checks[1].state, 'failed');
  assert.equal(result.dependent.state, 'not_run');
  assert.deepEqual(result.dependent.blockedBy, [{caseId: 'ordinary-native-context', state: 'failed', reason: 'native worker failed: original retained failure'}]);
  assert.equal(result.overallPassed, false);
});
test('completed and verified native calls followed by successful fixed query admit post-semantic success', () => {
  // Synthetic checker execution only; this test is not Windows product evidence.
  const result = postSemanticFixture((index, calls) => {
    calls.push(index ? 'context-native' : 'refs-native');
    const verified = index ? assertNativeContext(nativeContextResult(), oracle) : verifyNative(nativeResult());
    return {native: {lifecycle: verified.lifecycle, inputIdentity: verified.inputIdentity}};
  });
  assert.deepEqual(result.calls, ['refs-native', 'context-native', 'query-final', 'post-semantic-assertion']);
  assert.equal(result.dependent.state, 'passed'); assert.equal(result.overallPassed, true);
});
test('missing, not-run or failed final-query prerequisites never execute the dependent check', () => {
  const passed = POST_SEMANTIC_REQUIREMENTS.map(caseId => ({caseId, state: 'passed'}));
  for (const priorChecks of [[], passed.slice(0, 2), [{...passed[0], state: 'not_run', reason: 'transport retired'}, ...passed.slice(1)], [...passed.slice(0, 2), {...passed[2], state: 'failed', reason: 'query failed'}]]) {
    let invoked = false;
    const result = evaluateWorkflowCheck({caseId: 'post-semantic-source-read-still-usable', priorChecks, requires: POST_SEMANTIC_REQUIREMENTS, work: () => { invoked = true; }});
    assert.equal(result.state, 'not_run'); assert.equal(invoked, false); assert(result.blockedBy.length > 0);
  }
});
test('fatal transport state prohibits all check callbacks and observed business state cannot overwrite check status', () => {
  let invoked = false;
  const stopped = evaluateWorkflowCheck({caseId: 'final-same-pin-api-query', fatalProcessFailure: true, work: () => { invoked = true; }});
  assert.equal(stopped.state, 'not_run'); assert.equal(invoked, false);
  const closed = evaluateWorkflowCheck({caseId: 'release-owned-session', work: () => ({state: 'closed'})});
  assert.equal(closed.state, 'passed'); assert.equal(closed.observedState, 'closed');
});
