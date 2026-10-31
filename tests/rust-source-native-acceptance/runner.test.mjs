// These fault fixtures test the checker only, never report product acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync, mkdtempSync, mkdirSync, readdirSync, rmSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {ownedUserEnvironment} from '../../tools/rust-build.mjs';
import {assertApiType, assertContextSnippets, assertExecutedContext, assertExecutionAccounting, assertFullNative, assertNestedControllerRetired, extractXmlMaps, rebuildXmlInventory, semanticOperationDigest, semanticPlanDigest, assertMaterial, assertPrerequisite, assertReuse, fixturePin, groupBudget} from './assertions.mjs';
import {corpusDigest, sha256} from '../rust-source-full-corpus/assertions.mjs';
import {GO_CONTRACT, GO_VERSION, NATIVE_CALLSITE} from './runner-inputs.mjs';
const commit = 'a'.repeat(40), callsite = {source: 'Caller', referenceExpression: 'Helper', caller: {kind: 'named', name: 'Caller', line: 1}, path: 'a.lua', line: 2, kind: 'call', target: 'Helper', site: {file: 'a.lua', start: 20, end: 26}};
const source = {'a.lua': 'function Caller()\n   Helper()\nend\n', 'Interface/AddOns/Blizzard_APIDocumentationGenerated/API.lua': 'local API = {}\n', 'Main.toc': 'a.lua\nui.xml\n', 'ui.xml': '<Ui><Frame><Scripts><OnLoad>print(1)</OnLoad></Scripts></Frame></Ui>\n'};
const files = Object.entries(source).map(([path, text]) => ({path, bytes: Buffer.byteLength(text), sha256: sha256(Buffer.from(text))}));
const originals = new Map(Object.entries(source).map(([path, text]) => [path, Buffer.from(text)]));
const manifest = {commit, files}; manifest.xmlMappings = rebuildXmlInventory(originals, manifest);
const oracle = {commit, repository: 'wow-ui-source', product: 'retail', archiveSHA256: 'e'.repeat(64), allFiles: 4, allBytes: files.reduce((n, row) => n + row.bytes, 0), allPathSHA256SizeDigest: corpusDigest(files), documents: 4, documentBytes: files.reduce((n, row) => n + row.bytes, 0), documentPathSHA256SizeDigest: corpusDigest(files), counts: {'.lua': 2, '.xml': 1, '.toc': 1}};
function sample(mode = 'session') {
  const analysis = {engine: 'native_common_facts', state: 'partial', sourceFiles: 3, sourceExecution: false, inputIdentity: 'b'.repeat(64), cacheHit: false};
  const native = {schema: 'lycheedev.source-native-research.v1', inputIdentity: analysis.inputIdentity, entryEvidence: {sourceExecution: false}, analysis,
    sourceMaps: structuredClone(manifest.xmlMappings.maps), loadEvidence: [{manifest: 'Main.toc'}],
    environmentManifest: {identity: {repository: oracle.repository, commit, client: 'retail'}, documents: 1},
    semanticPlan: {state: 'execution_incomplete', plan: {schema: 'lycheedev.native-semantic-plan.v1', source_identity: analysis.inputIdentity, query_identity: 'd'.repeat(64), operations: ['references', 'type'].map(kind => ({kind, target: {site: {file: 'a.lua', start: 9, end: 15}, file_sha256: files[0].sha256, position: {}, material: {kind: 'source'}}})), batch_sizes: [2], unavailable: [], outgoing_truncated: false, scanner_diagnostics: [], scanner_diagnostics_omitted: 0}, resourceAccounting: {executedSemanticBatches: 0}},
    semanticExecution: {schema: 'lycheedev.native-semantic-execution.v1', receipt: {planned_batches: 1, completed_batches: 0, scopes: 1, uncertain_answers: 2, unavailable_scopes: 0, unexecuted_operations: 1}},
    nativeSession: {workerPid: 12, batches: 1, admittedSemanticBatches: 1, analysisRetired: false, cacheHit: false, snapshotTransfer: 'full'}};
  native.semanticExecution.receipt.plan_identity = semanticPlanDigest(native.semanticPlan.plan);
  native.semanticExecution.answers = [{scope: {manifest: 'Main.toc', snapshot: 'e'.repeat(64)}, operations: native.semanticPlan.plan.operations.map((operation, index) => ({kind: operation.kind, operation: index, batch: 0, executed: index === 0, state: index === 0 ? 'partial' : 'unavailable', operationIdentity: semanticOperationDigest(operation)}))}];
  if (mode === 'one_shot') { delete native.nativeSession; native.lifecycle = {owner: 'source_owned_process', mode: 'one_shot', retired: true}; }
  return {commit, repository: oracle.repository, product: oracle.product, complete: false, truncated: false, coverage: {semantic: 'partial'}, semanticRuntime: {engine: 'native-common-source', rulesVersion: 'fixture'}, semanticAnalysis: structuredClone(analysis), nativeResearch: native,
    relations: [{...structuredClone(callsite), caller: {kind: 'named', name: 'Caller', position: {path: 'a.lua', coveringRange: {start: {line: 0}}}, syntaxClosure: {file: 'a.lua', start: 15, end: 32}}, basis: ['native_common_source_facts'], state: 'static_candidate', proof: 'proof', lexical: {state: 'known'}, observations: []}]};
}
function material() { return {schema: 'lycheedev.source-research-material.v3', snapshot: {source: {exactCommit: commit, repository: oracle.repository, product: oracle.product}, files: Object.fromEntries(Object.entries(source).filter(([path]) => path.endsWith('.lua'))), load_documents: {'Main.toc': source['Main.toc'], 'ui.xml': source['ui.xml']}, environment_documents: {[files[1].path]: source[files[1].path]}, environment_manifest: {documents: 1}}}; }
test('groups are bounded and never accept unknown or combined work', () => {
  assert.equal(groupBudget('full'), 900000); assert.equal(groupBudget('idle'), 150000); assert.equal(groupBudget('lifetime'), 720000); assert.throws(() => groupBudget('all')); assert.throws(() => groupBudget('__proto__'));
});
test('real native evidence preserves partial semantics and actual execution counts', () => {
  assertFullNative(sample(), oracle, manifest, callsite, {mode: 'session'});
  assertFullNative(sample('one_shot'), oracle, manifest, callsite);
});
for (const [name, change] of [
  ['subset Lua inputs', r => { r.nativeResearch.analysis.sourceFiles--; r.semanticAnalysis.sourceFiles--; }],
  ['invented extra Lua inputs', r => { r.nativeResearch.analysis.sourceFiles++; r.semanticAnalysis.sourceFiles++; }],
  ['forged original XML bytes', r => { r.nativeResearch.sourceMaps[0].xmlSHA256 = '0'.repeat(64); }],
  ['omitted TOC planning', r => { r.nativeResearch.loadEvidence = []; }],
  ['changed API environment', r => { r.nativeResearch.environmentManifest.identity.commit = '0'.repeat(40); }],
  ['static substitute', r => { r.relations[0].basis = ['static-extraction']; }],
  ['false native self caller', r => { r.relations[0].source = 'Helper'; }],
  ['missing native expression', r => { delete r.relations[0].referenceExpression; }],
  ['wrong native caller location', r => { r.relations[0].caller.position.coveringRange.start.line++; }],
  ['wrong native byte site', r => { r.relations[0].site.start++; }],
  ['false resolved relation', r => { r.relations[0].state = 'resolved'; }],
  ['planned batches called executed', r => { r.nativeResearch.semanticPlan.resourceAccounting.executedSemanticBatches = 1; }],
  ['wrong semantic admission', r => { r.nativeResearch.nativeSession.admittedSemanticBatches = 2; }],
  ['uncertainty called complete', r => { r.complete = true; }],
  ['source execution', r => { r.nativeResearch.entryEvidence.sourceExecution = true; }],
]) test(`checker rejects ${name}`, () => { const result = sample(); change(result); assert.throws(() => assertFullNative(result, oracle, manifest, callsite, {mode: 'session'})); });
test('cached material authenticates every document, not just the selected symbol', () => {
  assert.equal(assertMaterial(material(), manifest, oracle).sourceDocuments, 4);
  for (const mutate of [m => { delete m.snapshot.load_documents['ui.xml']; }, m => { m.snapshot.files['a.lua'] += '-- forged'; }, m => { m.snapshot.environment_documents[files[1].path] += '-- forged'; }, m => { m.snapshot.files['a.lua'] = 2; }, m => { m.snapshot.environment_documents = {}; m.snapshot.environment_manifest.documents = 0; }, m => { m.snapshot.source.product = 'classic'; }]) {
    const m = material(); mutate(m); assert.throws(() => assertMaterial(m, manifest, oracle));
  }
});
test('same unchanged original documents under synthetic commit remain separately identified', () => {
  const m = material(), changed = 'c'.repeat(40); m.snapshot.source.exactCommit = changed;
  assert.throws(() => assertMaterial(m, manifest, oracle)); assertMaterial(m, manifest, oracle, changed);
});
test('reuse binds owner, input, transfer and hit; identity change requires full rebuild', () => {
  const first = sample().nativeResearch, next = structuredClone(first); next.analysis.cacheHit = true; Object.assign(next.nativeSession, {cacheHit: true, snapshotTransfer: 'identity_only'});
  assertReuse(first, next); next.nativeSession.workerPid++; assert.throws(() => assertReuse(first, next));
  const changed = structuredClone(first); changed.inputIdentity = 'c'.repeat(64); assertReuse(first, changed, {changed: true}); assert.throws(() => assertReuse(first, changed));
});
test('prerequisite cannot borrow old or partial full-corpus proof', () => {
  const report = {state: 'passed_selected_static_cases', oracle, pin: `PIN-${'d'.repeat(64)}`, checks: Array.from({length: 21}, () => ({state: 'passed'})), commands: Array.from({length: 25}, () => ({signal: null}))};
  assertPrerequisite(report, manifest, oracle);
  for (const mutate of [r => { r.state = 'running'; }, r => { r.checks[0].state = 'not_run'; }, r => { r.commands[0].signal = 'SIGTERM'; }, r => { r.commands[0].error = 'timeout'; }, r => { r.pin = 'PIN-old'; }]) {
    const r = structuredClone(report); mutate(r); assert.throws(() => assertPrerequisite(r, manifest, oracle));
  }
});
test('fixture pin uses exact Rust/Go serialization order and escapes', () => {
  const source = {repository: 'wow-ui-source', product: 'retail', requestedRef: 'a<&>\u2028\u2029', exactCommit: commit, parserRevision: 'source-v1'};
  const pin = fixturePin(source), unsigned = {...pin, id: ''};
  const expected = JSON.stringify(unsigned).replaceAll('&', '\\u0026').replaceAll('<', '\\u003c').replaceAll('>', '\\u003e').replaceAll('\u2028', '\\u2028').replaceAll('\u2029', '\\u2029');
  assert.equal(pin.id, `PIN-${sha256(Buffer.from(expected))}`); assert.notEqual(pin.id, fixturePin({...source, exactCommit: 'c'.repeat(40)}).id);
});
test('wrapper binds source digest and never changes frozen ownership support', () => {
  const text = readFileSync(new URL('../../tools/rust-source-native-acceptance.mjs', import.meta.url), 'utf8');
  assert(text.includes('sourceDigest(root)')); assert(text.includes('verifyCandidate(')); assert(text.includes('ownedSpawnSync(')); assert(text.includes('handlesReaped')); assert(text.includes('job.seal(')); assert(text.includes('job.cleanup()'));
  assert.match(text, /import \{[^}]*ownedUserEnvironment[^}]*\} from '\.\/rust-build\.mjs'/);
  assert(text.includes('env: ownedUserEnvironment(job.directory), evidence: job.evidence, timeout,'));
  assert(!text.includes('env: {...process.env}'));
});

test('controller admits the real owner user directories and rejects business or foreign state', t => {
  const scratch = mkdtempSync(join(tmpdir(), 'source-native-fresh-'));
  t.after(() => rmSync(scratch, {recursive: true, force: true}));
  ownedUserEnvironment(scratch);
  const source = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  const statement = source.match(/^assert\.[^\n]*'fresh controlled scratch'[^\n]*$/m)?.[0];
  assert(statement, 'execute the actual controller freshness assertion');
  const check = new Function('assert', 'readdirSync', 'scratch', statement);
  check(assert, readdirSync, scratch);
  for (const entry of ['home', 'foreign']) {
    mkdirSync(join(scratch, entry));
    assert.throws(() => check(assert, readdirSync, scratch), /fresh controlled scratch/);
    rmSync(join(scratch, entry), {recursive: true});
  }
});

test('current performance label takes the fixed external Go identity without claiming a Go measurement', () => {
  const source = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  assert.equal(GO_CONTRACT, '29bdbdaff5f064ec212ff2064380b1ccac652eda');
  assert.equal(GO_VERSION, '3.2.4');
  assert(source.includes('goOracle: GO_CONTRACT'));
  assert(source.includes("goDifferential: 'not_run'"));
  assert(!source.includes('rust-skill' + '-usage'));
});

function apiType() {
  const native = sample().nativeResearch; native.environmentManifest.definitionsSHA256 = 'd'.repeat(64);
  native.semanticExecution.answers = [{operations: [{kind: 'type', executed: true, state: 'known', result: {sourceExecution: false, type: 'fun(seconds: number, callback: TimerCallback)', bindingSelection: 'authenticated_environment_declaration', coverage: [{state: 'bound'}], target: {site: {file: '@native/api-definitions.lua'}, file_sha256: 'd'.repeat(64), material: {manifest: structuredClone(native.environmentManifest), mapping: {callable: 'C_Timer.After', sourcePath: 'timer.lua', sourceLine: 10}}}}}]}];
  return native;
}
test('API type requires actual bound inference and exact original mapping, not a descriptor', () => {
  const anchor = {symbol: 'C_Timer.After', path: 'timer.lua', line: 11, queryLine: 10, count: 12};
  assert.equal(assertApiType(apiType(), anchor).length, 1);
  for (const mutate of [
    a => { a.executed = false; }, a => { a.state = 'unavailable'; }, a => { a.result.type = 'unknown'; },
    a => { a.result.coverage = []; }, a => { a.result.bindingSelection = 'descriptor'; },
    a => { a.result.target.file_sha256 = 'wrong'; }, a => { a.result.target.material.mapping.sourceLine = 11; },
    a => { a.result.target.material.mapping.sourcePath = 'foreign.lua'; },
  ]) { const native = apiType(); mutate(native.semanticExecution.answers[0].operations[0]); assert.throws(() => assertApiType(native, anchor)); }
});

test('self-consistent missing XML maps and adjusted sourceFiles cannot impersonate full native input', () => {
  const result = sample(); result.nativeResearch.sourceMaps = []; result.nativeResearch.analysis.sourceFiles = 2; result.semanticAnalysis.sourceFiles = 2;
  assert.throws(() => assertFullNative(result, oracle, manifest, callsite, {mode: 'session'}), /independently reconstructed/);
});
test('independent XML oracle rejects duplicates, omitted maps and wrong output/original coordinates', () => {
  for (const mutate of [
    maps => maps.push(structuredClone(maps[0])), maps => { maps[0].element.start++; },
    maps => { maps[0].segments[1].raw.start++; }, maps => { maps[0].segments[1].output.end--; },
    maps => { maps[0].compilerSHA256 = 'f'.repeat(64); }, maps => { maps[0].segments[0].anchor.end++; },
  ]) { const result = sample(); mutate(result.nativeResearch.sourceMaps); assert.throws(() => assertFullNative(result, oracle, manifest, callsite, {mode: 'session'})); }
  const incompleteOriginals = new Map(originals); incompleteOriginals.delete('ui.xml'); assert.throws(() => rebuildXmlInventory(incompleteOriginals, manifest));
});
test('independent XML reconstruction authenticates BOM, Unicode, entities, CRLF, CDATA and synthetic anchors', () => {
  const prefix = '\uFEFF<Ui><Scripts><OnLoad>', body = 'local s=&quot;🙂&quot;\r\n<![CDATA[x=1]]>&#13;', suffix = '</OnLoad></Scripts></Ui>';
  const raw = Buffer.from(prefix + body + suffix), [map] = extractXmlMaps('ui.xml', raw);
  const compiler = 'return function(self, ...)\nlocal s="🙂"\nx=1\r\nend\n';
  assert.equal(map.compilerSHA256, sha256(Buffer.from(compiler))); assert.equal(map.compilerBytes, Buffer.byteLength(compiler));
  assert.equal(map.element.start, Buffer.byteLength('\uFEFF<Ui><Scripts>'));
  assert.equal(map.element.end, raw.indexOf('</OnLoad>') + Buffer.byteLength('</OnLoad>'));
  assert.deepEqual(map.segments[0].anchor, {start: map.element.start, end: map.element.start});
  assert.deepEqual(map.segments.at(-1).anchor, {start: raw.indexOf('</OnLoad>'), end: map.element.end});
  const entity = map.segments.find(row => row.origin === 'Entity'); assert.equal(raw.subarray(entity.raw.start, entity.raw.end).toString(), '&quot;');
  const normalized = map.segments.find(row => row.origin === 'NewlineNormalization'); assert.equal(raw.subarray(normalized.raw.start, normalized.raw.end).toString(), '\r\n');
  const cdata = map.segments.find(row => row.origin === 'Exact' && raw.subarray(row.raw.start, row.raw.end).toString().includes('x=1')); assert.equal(raw.subarray(cdata.raw.start, cdata.raw.end).toString(), 'x=1');
});
test('independent XML inventory counts bodies rather than tags or historical Retail constants', () => {
  const raw = Buffer.from('<Bindings><Binding name="empty"></Binding><Binding name="self"/><Binding name="work">Run()</Binding></Bindings>');
  const maps = extractXmlMaps('Bindings.xml', raw); assert.equal(maps.length, 1); assert.equal(maps[0].path, 'Bindings.xml/__inline_0.lua');
  assert.equal(maps[0].compilerSHA256, sha256(Buffer.from('return function(...)\nRun()\nend\n')));
  assert.equal(extractXmlMaps('ui.xml', Buffer.from('<Ui><Script>Top()</Script><Scripts><Unknown>Call()</Unknown><Empty/></Scripts></Ui>')).length, 2);
  for (const raw of ['<Ui><Script>bad &unknown;</Script></Ui>', '<!DOCTYPE Ui><Ui/>', '<Ui><Script><Nested/></Script></Ui>', '<Ui><Scripts></Ui>']) assert.throws(() => extractXmlMaps('bad.xml', Buffer.from(raw)));
});
function contextFixture() {
  const text = source['a.lua'].trimEnd().split('\n').map((line, index) => `${index + 1}: ${line}`).join('\n');
  return {snippets: [{path: 'a.lua', firstLine: 1, lastLine: 3, contentHash: files[0].sha256, text, basis: 'definition'}]};
}
const contextOptions = {maxBytes: 16384, maxLines: 80, limit: 50, anchor: {path: 'a.lua', line: 1, text: source['a.lua']}};
test('both ordinary and session context use complete exact numbered original snippets', () => {
  const summary = assertContextSnippets(contextFixture(), originals, manifest, contextOptions); assert.equal(summary.lines, 3); assert.equal(summary.exactOriginals, true);
  const code = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8'); assert.equal((code.match(/const snippets = contextEvidence\(result\)/g) ?? []).length, 2);
  for (const mutate of [
    s => { s.contentHash = 'f'.repeat(64); }, s => { s.firstLine++; }, s => { s.lastLine--; },
    s => { s.text = s.text.replace('Helper()', 'Forged()'); }, s => { s.text = s.text.replace('2:', '9:'); },
    s => { s.path = 'ui.xml'; }, s => { s.text = '1: function Caller()'; },
  ]) { const result = contextFixture(); mutate(result.snippets[0]); assert.throws(() => assertContextSnippets(result, originals, manifest, contextOptions)); }
  assert.throws(() => assertContextSnippets(contextFixture(), originals, manifest, {...contextOptions, maxBytes: 5}));
  assert.throws(() => assertContextSnippets(contextFixture(), originals, manifest, {...contextOptions, maxLines: 2}));
});
test('nested timeout may leave descendants: suppress every subsequent launch including final close', () => {
  // Control-flow fault model only, not an actual Windows process-reaping pass.
  for (const [outer, nested] of [
    [{exit: 1, signal: null}, {commands: [{exit: null, signal: 'SIGTERM', error: 'ETIMEDOUT'}], descendantsStillActive: 1}],
    [{exit: 0, signal: null}, {commands: [{exit: null, signal: null, error: 'ETIMEDOUT'}], descendantsStillActive: 1}],
    [{exit: 0, signal: null}, {commands: [{exit: 0, signal: null}], fatalProcessFailure: true}],
    [{exit: 0, signal: null}, null],
  ]) {
    let fatal = false; const launched = [];
    try { assertNestedControllerRetired(outer, nested); launched.push('next-native-query'); } catch (error) { assert.equal(error.fatalProcessFailure, true); fatal = true; }
    if (!fatal) launched.push('final-close'); assert.deepEqual(launched, []);
  }
  assertNestedControllerRetired({exit: 0, signal: null}, {commands: [{exit: 0, signal: null}]});
  const code = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8'); assert(code.includes('assertNestedControllerRetired(run.observation, previous)')); assert(code.includes('if (!fatal && existsSync(home))'));
});

function executedContextFixture() {
  const native = sample().nativeResearch, site = {file: 'a.lua', start: 9, end: 15}, range = {start: {line: 0, character: 9}, end: {line: 0, character: 15}};
  const operation = {kind: 'references', target: {site, file_sha256: files[0].sha256, position: {path: 'a.lua', bytes: {start: 9, end: 15}, range, coveringRange: range, positionAccuracy: 'utf16-character'}, material: {kind: 'source'}}};
  native.semanticPlan.plan = {schema: 'lycheedev.native-semantic-plan.v1', source_identity: native.inputIdentity, query_identity: 'd'.repeat(64), operations: [operation], batch_sizes: [1], unavailable: [], outgoing_truncated: false, scanner_diagnostics: [], scanner_diagnostics_omitted: 0};
  Object.assign(native.semanticExecution.receipt, {plan_identity: semanticPlanDigest(native.semanticPlan.plan), scopes: 1, uncertain_answers: 1, unexecuted_operations: 0, unavailable_scopes: 1});
  native.semanticExecution.answers = [{scope: {manifest: 'Main.toc', snapshot: 'e'.repeat(64)}, operations: [{kind: 'references', operation: 0, batch: 0, executed: true, state: 'partial', operationIdentity: semanticOperationDigest(operation), result: {relationsSHA256: 'f'.repeat(64), relationCount: 1, analysis: structuredClone(native.analysis)}}]}, {scope: {manifest: 'Main.toc', code: 'fixture_unavailable_scope'}, state: 'unavailable', operationsExecuted: false}];
  return native;
}
test('context requires actual identity-bound executed references while partial batch completion remains legal', () => {
  const anchor = {path: 'a.lua', line: 1, symbol: 'Caller', sha256: files[0].sha256};
  const valid = executedContextFixture(); assert.equal(valid.semanticExecution.receipt.completed_batches, 0); assert.equal(assertExecutedContext(valid, originals, anchor).executedReferences.length, 1);
  for (const mutate of [
    n => { n.semanticExecution.answers = []; }, n => { n.semanticExecution.answers[0].operations[0].executed = false; },
    n => { n.semanticExecution.answers[0].operations[0].operationIdentity = '0'.repeat(64); },
    n => { n.semanticExecution.receipt.plan_identity = '0'.repeat(64); },
    n => { n.semanticExecution.answers[0].scope.manifest = 'foreign.toc'; },
    n => { n.semanticExecution.answers[0].operations[0].result.analysis.inputIdentity = '0'.repeat(64); },
    n => { n.semanticPlan.plan.source_identity = '0'.repeat(64); },
    n => { n.semanticPlan.plan.operations[0].target.site.start++; n.semanticExecution.receipt.plan_identity = semanticPlanDigest(n.semanticPlan.plan); },
  ]) { const n = executedContextFixture(); mutate(n); assert.throws(() => assertExecutedContext(n, originals, anchor)); }
});
function completeContextResult() {
  const result = sample(), native = executedContextFixture();
  native.semanticExecution.answers.pop();
  native.semanticExecution.answers[0].operations[0].state = 'known';
  native.semanticExecution.answers[0].operations[0].result.analysis.state = 'complete';
  native.analysis.state = 'complete'; native.semanticPlan.state = 'executed';
  native.semanticPlan.resourceAccounting.executedSemanticBatches = 1;
  Object.assign(native.semanticExecution.receipt, {completed_batches: 1, uncertain_answers: 0, unavailable_scopes: 0});
  Object.assign(result, {nativeResearch: native, semanticAnalysis: structuredClone(native.analysis), coverage: {semantic: 'complete'}, complete: true});
  return result;
}
test('nonempty plan unavailable cannot be hidden behind complete execution receipts', () => {
  const result = completeContextResult(), anchor = {path: 'a.lua', line: 1, symbol: 'Caller', sha256: files[0].sha256};
  assertFullNative(result, oracle, manifest, null, {mode: 'session'}); assertExecutedContext(result.nativeResearch, originals, anchor);
  result.nativeResearch.semanticPlan.plan.unavailable.push('fixture_incomplete_scanner');
  result.nativeResearch.semanticExecution.receipt.plan_identity = semanticPlanDigest(result.nativeResearch.semanticPlan.plan);
  assert.throws(() => assertFullNative(result, oracle, manifest, null, {mode: 'session'}), /incomplete plan/);
  assert.throws(() => assertExecutedContext(result.nativeResearch, originals, anchor), /incomplete plan/);
});
test('actual partial reference answers cannot be masked by forged zero uncertainty and complete top-level state', () => {
  const result = completeContextResult(), native = result.nativeResearch;
  const anchor = {path: 'a.lua', line: 1, symbol: 'Caller', sha256: files[0].sha256};
  const answer = native.semanticExecution.answers[0].operations[0]; answer.state = 'partial'; answer.result.analysis.state = 'partial';
  assert.throws(() => assertFullNative(result, oracle, manifest, null, {mode: 'session'}));
  assert.throws(() => assertExecutedContext(native, originals, anchor));
  // Even an honest uncertainty counter cannot make partial answers complete.
  native.semanticExecution.receipt.uncertain_answers = 1;
  assert.throws(() => assertFullNative(result, oracle, manifest, null, {mode: 'session'}), /incomplete plan/);
  answer.state = 'known'; native.semanticExecution.receipt.uncertain_answers = 0;
  assert.throws(() => assertExecutedContext(native, originals, anchor), /incomplete plan/);
  const honest = executedContextFixture(); assert.equal(assertExecutionAccounting(honest).completedBatches, 0);
  assert.equal(assertExecutedContext(honest, originals, anchor).executedReferences.length, 1);
});
test('receipt counts and each batch completion are recomputed across all actual scopes', () => {
  const native = sample().nativeResearch, operation = native.semanticPlan.plan.operations[1];
  native.semanticPlan.plan.operations = Array.from({length: 129}, () => structuredClone(operation)); native.semanticPlan.plan.batch_sizes = [128, 1];
  native.semanticExecution.answers = [0, 1].map(scope => ({scope: {manifest: 'Main.toc', snapshot: 'e'.repeat(64)}, operations: native.semanticPlan.plan.operations.map((operation, index) => ({kind: operation.kind, operation: index, batch: Math.floor(index / 128), operationIdentity: semanticOperationDigest(operation), executed: scope === 0 || index !== 128, state: scope === 0 || index !== 128 ? 'known' : 'unavailable'}))}));
  Object.assign(native.semanticExecution.receipt, {plan_identity: semanticPlanDigest(native.semanticPlan.plan), planned_batches: 2, completed_batches: 1, scopes: 2, uncertain_answers: 1, unexecuted_operations: 1});
  native.semanticPlan.resourceAccounting.executedSemanticBatches = 1; assert.equal(assertExecutionAccounting(native).completedBatches, 1);
  for (const field of ['completed_batches', 'scopes', 'unavailable_scopes', 'uncertain_answers', 'unexecuted_operations']) {
    const forged = structuredClone(native); forged.semanticExecution.receipt[field]++; assert.throws(() => assertExecutionAccounting(forged));
  }
  native.semanticExecution.answers.push({scope: {manifest: 'Main.toc'}, state: 'unavailable', operationsExecuted: false});
  native.semanticExecution.receipt.unavailable_scopes = 1; native.semanticExecution.receipt.completed_batches = 0; native.semanticPlan.resourceAccounting.executedSemanticBatches = 0;
  assert.equal(assertExecutionAccounting(native).completedBatches, 0);
});
test('semantic hashes reconstruct declared Rust struct order and sorted nested Value maps', () => {
  // Literal wire bytes independently fix the serialization contract; using the
  // production helper to manufacture both expected and actual would hide drift.
  const operationWire = '{"kind":"references","target":{"site":{"file":"a.lua","start":9,"end":15},"file_sha256":"hash","position":{"bytes":{"end":15,"start":9},"path":"a.lua"},"material":{"kind":"source"}}}';
  const operation = {target: {material: {kind: 'source'}, position: {path: 'a.lua', bytes: {start: 9, end: 15}}, file_sha256: 'hash', site: {end: 15, file: 'a.lua', start: 9}}, kind: 'references'};
  assert.equal(semanticOperationDigest(operation), sha256(Buffer.from(operationWire)));
  const plan = {scanner_diagnostics_omitted: 0, scanner_diagnostics: [{path: 'a.lua', line: 1}], outgoing_truncated: false, unavailable: [], batch_sizes: [1], operations: [operation], query_identity: 'query', source_identity: 'input', schema: 'lycheedev.native-semantic-plan.v1'};
  const planWire = '{"schema":"lycheedev.native-semantic-plan.v1","source_identity":"input","query_identity":"query","operations":[' + operationWire + '],"batch_sizes":[1],"unavailable":[],"outgoing_truncated":false,"scanner_diagnostics":[{"line":1,"path":"a.lua"}],"scanner_diagnostics_omitted":0}';
  assert.equal(semanticPlanDigest(plan), sha256(Buffer.from(planWire)));
  assert.notEqual(semanticPlanDigest(plan), sha256(Buffer.from(JSON.stringify(plan))));
});

test('native wrapper and controller use the self-contained current runner inputs', () => {
  const outer = readFileSync(new URL('../../tools/rust-source-native-acceptance.mjs', import.meta.url), 'utf8');
  const inner = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  for (const source of [outer, inner]) {
    assert(!source.includes('rust-skill' + '-usage'), 'retired migration workflow input must not remain');
    assert(!source.includes('loadCurrent' + 'Workflow'));
    assert(source.includes('GO_CONTRACT'));
  }
  assert(inner.includes('goContract: GO_CONTRACT'));
  assert(inner.includes('NATIVE_CALLSITE'));
  assert.equal(NATIVE_CALLSITE.target, 'GetActionButtonForID');
  assert.equal(NATIVE_CALLSITE.site.file, NATIVE_CALLSITE.path);
  assert(outer.includes('verifyCandidate(root, candidatePath, artifact)'));
  assert(outer.includes("...readdirSync(join(runnerRoot, 'tests/rust-source-native-acceptance'))"));
});
test('separate native runner keeps product identity and hashes every executing workflow input', () => {
  const outer = readFileSync(new URL('../../tools/rust-source-native-acceptance.mjs', import.meta.url), 'utf8');
  const inner = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  assert(outer.includes("const runnerRoot = resolve(import.meta.dirname, '..');"));
  assert(outer.includes('artifact = verifyArtifact(root, artifactPath)'));
  assert(outer.includes("assert.equal(artifact.inputHashes?.sourceTree, tree"));
  assert(outer.includes('sourceDigest(root) === tree'));
  assert(outer.includes('hashFile(join(runnerRoot, path)) === hash'));
  assert(outer.includes("join(runnerRoot, 'tests/rust-source-native-acceptance/run.mjs')"));
  assert(inner.includes("join(runnerRoot, 'tests/rust-source-full-corpus/run.mjs')"));
  assert(outer.includes('inventoryHash(inventory(candidate.packageRoot)) === candidate.packageSHA256'));
  assert(outer.includes('hashFile(candidatePath) === candidate.reportSHA256'));
});
