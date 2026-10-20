// Controller for tools/rust-skill-usage.mjs only. Its complete process tree is
// held in one Windows Job; never run this file as an unowned acceptance process.
import assert from 'node:assert/strict';
import {assertHelp, assertCommandSchema, assertSearch} from './agent-discovery.mjs';
import {spawnSync} from 'node:child_process';
import {copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, delimiter, join, resolve} from 'node:path';
import os from 'node:os';
import {performance} from 'node:perf_hooks';
import {assertApiAnswer, assertBoundedCoverage, assertNativeContext, assertNativeEvidence, assertPackagedIncrement, commandFlags, evaluateWorkflowCheck, inventory, inventoryHash, jsonFile, loadCurrentWorkflow, pendingIncrementTasks, POST_SEMANTIC_REQUIREMENTS, sha256, within} from './assertions.mjs';
import {assertDataSkillDirectory, dataSkillIdentity} from './data-skill-contract.mjs';
import {assertAnchor, assertEnvelope, assertPageContinuation, assertQuery, assertStaticCall} from '../rust-source-full-corpus/assertions.mjs';

const [packageArg, scratchArg, evidenceArg, packageSHA256, ...extra] = process.argv.slice(2);
assert(packageArg && scratchArg && evidenceArg && /^[a-f0-9]{64}$/.test(packageSHA256) && !extra.length, 'owned controller needs package, scratch, evidence, package digest');
assert.equal(process.platform, 'win32', 'Windows acceptance only');
const root = resolve('.'), sourcePackage = resolve(packageArg), scratch = resolve(scratchArg), out = resolve(evidenceArg);
const owner = jsonFile(join(out, 'receipt.json'));
assert.equal(owner.owner, 'skill-real-usage'); assert.equal(owner.state, 'active');
assert.equal(resolve(owner.directory), scratch, 'controlled scratch owner');
assert.equal(readdirSync(scratch).length, 0, 'fresh controlled scratch');
const workflow = loadCurrentWorkflow(root);
const oracle = jsonFile(new URL('../rust-source-full-corpus/retail-oracle.json', import.meta.url));
const report = {
  schema: 'lycheedev.skill-real-usage.v2', state: 'running', startedAt: new Date().toISOString(),
  goContract: workflow.goContract, goVersion: workflow.goVersion, historicalWorkflow: workflow.historical,
  skillIdentity: dataSkillIdentity(), oracle, prompts: workflow.prompts, sourceTaskScope: workflow.sourceTaskScope, pendingTasks: pendingIncrementTasks(workflow), inputMode: 'full-fixed-retail-public-source-sync',
  packageSHA256, machine: {platform: process.platform, release: os.release(), arch: process.arch, node: process.version, cpu: os.cpus()[0]?.model, cores: os.cpus().length},
  checks: [], commands: [], answers: [], discovery: [],
  boundaries: ['staged npm-prefix resolution fixture, not npm installation acceptance', 'selected full-Retail tasks; complete byte-authenticated corpus gate is separate', 'Go321 keyboard, Go322 slot startup and Go323 Data requirements retain their historical fixtures; selected candidate Skill bytes are authenticated separately and runtime tasks remain not_run', 'ordinary semantic refs/context do not establish complete native lifecycle or flow coverage', 'no game, source/config execution, release publication or performance claim', 'capture IDs belong to retired isolated home; their actual returned envelopes and excerpts remain in sealed evidence'],
};
const save = () => writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
save();
let fatalProcessFailure = false, sequence = 0, outputBytes = 0, stagedReady = false, directory, pin, api, action;
const started = performance.now(), deadline = started + 570000;
const prefix = join(scratch, '空间 prefix'), staged = join(prefix, 'node_modules', 'lycheedev'), home = join(scratch, '研究 home');
const launcher = join(staged, 'payload', 'skill', 'scripts', 'lycheedev.mjs');
const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !['PATH', 'LYCHEEDEV_BIN', 'LYCHEEDEV_HOME', 'HOME', 'USERPROFILE', 'TEMP', 'TMP'].includes(name.toUpperCase())));
Object.assign(env, {PATH: `${prefix}${delimiter}${process.env.PATH ?? process.env.Path ?? ''}`, LYCHEEDEV_HOME: home, HOME: join(scratch, 'user'), USERPROFILE: join(scratch, 'user'), TEMP: join(scratch, 'temp'), TMP: join(scratch, 'temp')});
for (const path of [env.HOME, env.TEMP]) mkdirSync(path, {recursive: true});

function invoke(caseId, args, {expectedExit = 0, timeout = 210000, phase = 'local-fixed-source'} = {}) {
  assert(!fatalProcessFailure, 'prior process failure requires outer Job retirement');
  const metadataOnly=['version','describe','help','query-help','query-schema','query-search'].includes(caseId);
  const paths=[home,join(env.USERPROFILE,'.lycheedev')],before=metadataOnly?paths.map(path=>existsSync(path)):null;
  if(metadataOnly)assert.deepEqual(before,[false,false]);
  const remaining = Math.floor(deadline - performance.now());
  if (remaining < 1000 || sequence >= 60 || outputBytes >= 192 << 20) { fatalProcessFailure = true; throw new Error('skill-usage.total_controller_budget'); }
  const index = String(sequence++).padStart(3, '0'), fullArgs = [launcher, ...args, ...(['cli','schema'].includes(args[0]) ? [] : ['--home',home]), '--format', 'json'];
  const time = performance.now(), startedAt = new Date().toISOString();
  const run = spawnSync(process.execPath, fullArgs, {cwd: scratch, env, windowsHide: true, shell: false, timeout: Math.min(timeout, remaining), maxBuffer: 12 << 20});
  const stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0);
  outputBytes += stdout.length + stderr.length;
  const stdoutName = `${index}-${caseId}.stdout.json`, stderrName = `${index}-${caseId}.stderr.bin`;
  writeFileSync(join(out, stdoutName), stdout); writeFileSync(join(out, stderrName), stderr);
  let envelope; try { envelope = JSON.parse(stdout.toString('utf8')); } catch {}
  const observation = {caseId, executable: process.execPath, args: fullArgs, cwd: scratch, phase, startedAt, endedAt: new Date().toISOString(), wallMillis: performance.now() - time,
    exit: run.status, signal: run.signal, error: run.error?.message, stdout: stdoutName, stderr: stderrName, stdoutBytes: stdout.length, stderrBytes: stderr.length, stdoutSHA256: sha256(stdout), stderrSHA256: sha256(stderr)};
  report.commands.push(observation);
  if (run.error || run.signal || outputBytes > 192 << 20) fatalProcessFailure = true;
  save();
  const result = assertEnvelope({...observation, envelope}, expectedExit);
  assert.equal(run.signal, null, 'no process signal');
  if(metadataOnly){const after=paths.map(path=>existsSync(path));assert.deepEqual(after,[false,false]);report.discovery.push({id:caseId,paths,before,after});save();}
  return {result, envelope, observation};
}
function check(caseId, work, {requires = []} = {}) {
  report.checks.push(evaluateWorkflowCheck({caseId, work, requires, priorChecks: report.checks, fatalProcessFailure}));
  save();
}
function supported(path, flags = []) { assert(directory, 'installed command directory unavailable'); return commandFlags(directory, path, flags); }
function fixed(args) { assert(pin, 'fixed source pin unavailable'); return [...args, '--snapshot', pin]; }
function queried(caseId, anchor, topic) {
  supported('source query', ['--snapshot', '--mode', '--topic', '--limit']);
  const {result, envelope} = invoke(caseId, fixed(['source', 'query', anchor.symbol, '--mode', 'precise', '--topic', topic, '--limit', '10']));
  return {row: assertAnchor(result, anchor, oracle, pin), captures: envelope.captures};
}
function relationArgs(command, semantic = false) {
  supported(`source ${command}`, ['--snapshot', '--symbol-id', '--limit']); assert(action, 'returned action symbol identity unavailable');
  const args = fixed(['source', command, '--symbol-id', action.row.symbolId, '--limit', command === 'refs' ? '25' : '5']);
  if (command === 'refs') args.push('--direction', 'incoming');
  else args.push('--depth', '0', '--max-lines', '80', '--max-bytes', '16384');
  if (!semantic) args.push('--static-only');
  return args;
}
function checkRelationIdentity(result) {
  assert.equal(result.commit, oracle.commit); assert.equal(result.symbol?.id, action.row.symbolId); assertBoundedCoverage(result);
}
function nativeSummary(native) {
  return {schema: native.schema, analysisState: native.analysis?.state, engine: native.analysis?.engine, sourceFiles: native.analysis?.sourceFiles, cacheHit: native.analysis?.cacheHit,
    gapCount: native.gaps?.length, sourceExecution: native.entryEvidence?.sourceExecution, inputIdentity: native.inputIdentity, lifecycle: native.lifecycle};
}

try {
  check('stage-authenticated-packaged-skill', () => {
    const files = inventory(sourcePackage); assert.equal(inventoryHash(files), packageSHA256);
    for (const row of files) { const target = join(staged, row.path); mkdirSync(dirname(target), {recursive: true}); copyFileSync(within(sourcePackage, row.path), target); }
    assert.equal(inventoryHash(inventory(staged)), packageSHA256, 'exact staged package bytes');
    // Only an npm-prefix resolution marker. The Skill detects this file and
    // directly invokes node_modules/lycheedev/bin/lycheedev.mjs with Node; no
    // shell or shim contents execute. This is explicitly not an npm install.
    writeFileSync(join(prefix, 'lycheedev.cmd'), '@echo off\r\nexit /b 99\r\n');
    for (const name of workflow.requiredSkillFiles) assert(existsSync(join(staged, 'payload', 'skill', name)), `complete fixed Skill resource ${name}`);
    assertDataSkillDirectory(join(staged, 'payload', 'skill'));
    report.launch = {skill: launcher, packageLauncher: join(staged, 'bin', 'lycheedev.mjs'), prefix, explicitNativeOverride: false, mode: 'Skill npm-prefix discovery to packaged verified native handoff'};
    stagedReady = true;
    return {files: files.length, requiredSkillFiles: workflow.requiredSkillFiles.length};
  });
  assert(stagedReady, 'authenticated package staging failed; no launcher run');
  for (const increment of workflow.increments) check(`fixed-skill-contract-${increment.id}`, () => assertPackagedIncrement(join(staged, 'payload', 'skill'), workflow, increment, report.skillIdentity));
  check('packaged-version', () => { const {result} = invoke('version', ['version']); assert.equal(result.platform, 'windows/amd64'); assert.equal(typeof result.rust, 'string'); assert(result.rust.length > 0); return {version: result.version, rust: result.rust}; });
  check('installed-command-discovery', () => { const {result} = invoke('describe', ['describe']); assert(Array.isArray(result.commands)); directory = result.commands; return {commands: directory.length}; });
  check('installed-help', () => { const {result} = invoke('help', ['--help']); assertHelp(result); });
  check('source-query-help', () => { supported('source query'); const {result} = invoke('query-help', ['source', 'query', '--help']); assertHelp(result,'source query'); assertCommandSchema(invoke('query-schema',['schema','source','query']).result,'source query'); assertSearch(invoke('query-search',['cli','search','source query']).result,'source query','source query'); });
  check('new-home-initialization', () => { supported('init'); invoke('init', ['init']); });
  check('discover-repository-and-track', () => {
    supported('source list'); const {result} = invoke('catalog', ['source', 'list']);
    const repository = result.find(row => row.key === oracle.repository); assert.equal(repository?.url, oracle.url); assert.equal(repository.tracks?.retail, 'live');
    return {repository: repository.key, track: 'retail', branch: repository.tracks.retail};
  });
  check('sync-exact-fixed-source', () => {
    supported('source sync', ['--source', '--product', '--ref']);
    const {result} = invoke('sync', ['source', 'sync', '--source', oracle.repository, '--product', oracle.product, '--ref', oracle.commit], {phase: 'input-preparation-network-and-local'});
    assert.equal(result.source?.exactCommit, oracle.commit); assert.equal(result.source?.requestedRef, oracle.commit);
    assert.equal(result.source?.repository, oracle.repository); assert.equal(result.source?.product, oracle.product); assert.match(result.id, /^PIN-/);
    pin = result.id; report.pin = pin; return {pin, source: result.source};
  });
  check('confirm-returned-target', () => { supported('target show'); assert(pin, 'returned pin unavailable'); const {result} = invoke('target', ['target', 'show', pin]); assert.equal(result.id, pin); assert.equal(result.source?.exactCommit, oracle.commit); });
  check('api-query-prepares-own-mapping', () => { api = queried('query-api', oracle.anchors[2], 'api'); return {symbol: api.row, captures: api.captures, explicitIndexWarmup: false}; });
  check('api-original-source-answer', () => {
    assert(api, 'returned API source location unavailable'); supported('source inspect', ['--path', '--line', '--count']);
    const {result, envelope} = invoke('inspect-api', fixed(['source', 'inspect', '--path', api.row.path, '--line', String(api.row.line), '--count', String(workflow.apiOriginalWindow.count)]));
    const answer = {...assertApiAnswer(result, api.row, oracle, workflow.apiOriginalWindow), captures: envelope.captures}; report.answers.push(answer); return answer;
  });
  check('action-query-returned-symbol-id', () => { action = queried('query-action', oracle.anchors[0], 'lua'); return {symbol: action.row, captures: action.captures}; });
  check('inspect-symbol-candidates', () => {
    supported('source inspect', ['--symbol']); const {result} = invoke('inspect-symbol', fixed(['source', 'inspect', '--symbol', oracle.anchors[0].symbol]));
    const row = assertAnchor(result, oracle.anchors[0], oracle, pin); assert.equal(row.symbolId, action?.row.symbolId); return {symbolId: row.symbolId};
  });
  check('inspect-returned-implementation', () => {
    assert(action, 'returned action location unavailable'); const anchor = oracle.anchors[0];
    const {result} = invoke('inspect-action', fixed(['source', 'inspect', '--path', action.row.path, '--line', String(action.row.line), '--count', String(anchor.count)]));
    assert.equal(result.text, anchor.text); assert.equal(result.blob?.sha256, anchor.sha256); assert.equal(result.commit, oracle.commit); return {path: result.path, firstLine: result.firstLine, lastLine: result.lastLine, sha256: result.blob.sha256};
  });
  check('static-callers-preserve-evidence-strength', () => {
    const {result} = invoke('refs-static', relationArgs('refs')); checkRelationIdentity(result); assertStaticCall(result, oracle.staticCall);
    assert.equal(result.coverage.semantic, 'unavailable'); return {coverage: result.coverage, complete: result.complete, truncated: result.truncated};
  });
  check('static-context-original-snippet', () => {
    const {result} = invoke('context-static', relationArgs('context')); checkRelationIdentity(result);
    assert(result.snippets?.some(row => row.path === action.row.path && row.text?.includes(oracle.anchors[0].declarationLine))); assert.equal(result.coverage.semantic, 'unavailable');
    return {coverage: result.coverage, snippets: result.snippets.length, complete: result.complete, truncated: result.truncated};
  });
  check('literal-chinese-query-keeps-pin-and-uncertainty', () => {
    const {result} = invoke('query-chinese', fixed(['source', 'query', oracle.chineseQuery.text, '--limit', '10'])); assertQuery(result, oracle, pin);
    assert.deepEqual(result.results, []); assert.equal(result.complete, true); assert.equal(result.truncated, false); assert(result.suggestions?.length > 0);
    return {basis: oracle.chineseQuery.basis, decision: 'Literal UTF-8 miss only. Next step is a different term in the same pin; no claim that cooldown functionality is absent.'};
  });
  check('empty-result-is-actionable', () => {
    const {result} = invoke('query-absent', fixed(['source', 'query', 'LYCHEEDEV_SkillUsage_Absent_31c7f7b', '--limit', '10'])); assertQuery(result, oracle, pin);
    assert.deepEqual(result.results, []); assert.equal(result.complete, true); assert.equal(result.truncated, false); assert(result.suggestions?.length > 0);
  });
  check('large-result-bounded-continuation-and-cursor-rejection', () => {
    const args = fixed(['source', 'query', 'Cooldown', '--mode', 'exploratory', '--topic', 'lua', '--limit', '3']);
    const first = assertQuery(invoke('large-page-1', args).result, oracle, pin); assert(first.nextCursor, 'page continuation required');
    const second = assertQuery(invoke('large-page-2', [...args, '--cursor', first.nextCursor]).result, oracle, pin); assertPageContinuation(first, second, 3);
    const error = invoke('wrong-cursor-scope', fixed(['source', 'query', oracle.anchors[0].symbol, '--mode', 'exploratory', '--topic', 'lua', '--limit', '3', '--cursor', first.nextCursor]), {expectedExit: 2}).result;
    assert.equal(error.code, 'codebase.invalid_search_cursor'); return {pages: 2, limit: 3, firstCoverage: first.matchCoverage, secondCoverage: second.matchCoverage, recovery: 'Restart changed query without originating cursor; do not merge page sets.'};
  });
  check('recoverable-original-line-error', () => {
    assert(action); const error = invoke('bad-line', fixed(['source', 'inspect', '--path', action.row.path, '--line', '1000000', '--count', '1']), {expectedExit: 2}).result;
    assert.equal(error.code, 'codebase.line_out_of_range'); return {code: error.code, retainedPin: pin};
  });
  check('recoverable-invalid-limit', () => { invoke('bad-limit', fixed(['source', 'query', 'Cooldown', '--limit', '201']), {expectedExit: 2}); });
  check('same-pin-query-after-errors', () => { const again = queried('query-recovered', oracle.anchors[0], 'lua'); assert.equal(again.row.symbolId, action?.row.symbolId); return {pin, symbolId: again.row.symbolId}; });
  check('documented-batch-capability-and-ordered-results', () => {
    // Discover the installed capability first. Missing command is a concrete
    // failed required workflow, never permission to fabricate an invocation.
    supported('source batch', ['--snapshot', '--file', '--static-only']);
    const requests = {requests: [{id: 'definition', query: {text: 'C_Timer.After', topic: 'api', limit: 10}}, {id: 'callers', refs: {symbolId: action?.row.symbolId, direction: 'incoming', limit: 25}}, {id: 'implementation', context: {symbolId: action?.row.symbolId, depth: 0, limit: 5, maxBytes: 16384, maxLines: 80}}]};
    assert(action); const file = join(scratch, 'requests.json'); writeFileSync(file, JSON.stringify(requests)); writeFileSync(join(out, 'batch-input.json'), JSON.stringify(requests, null, 2) + '\n');
    const {result} = invoke('batch', fixed(['source', 'batch', '--file', file, '--static-only']));
    assert.equal(result.snapshot, pin); assert.deepEqual(result.results.map(row => row.id), ['definition', 'callers', 'implementation']);
    assertAnchor(result.results[0].query, oracle.anchors[2], oracle, pin); assertStaticCall(result.results[1].refs, oracle.staticCall); assertBoundedCoverage(result.results[2].context);
    assert.equal(result.complete, false, 'static partial semantics remain partial in aggregate');
  });
  check('ordinary-native-callers', () => {
    const {result} = invoke('refs-native', relationArgs('refs', true)); checkRelationIdentity(result); const native = assertNativeEvidence(result, oracle, workflow.nativeCallsite);
    assert(result.relations?.some(row => ['source', 'target', 'kind', 'path', 'line'].every(key => row[key] === oracle.staticCall[key])), 'original call remains visible in ordinary relation route');
    return {coverage: result.coverage, native: nativeSummary(native), complete: result.complete, truncated: result.truncated};
  });
  check('ordinary-native-context', () => {
    const {result} = invoke('context-native', relationArgs('context', true)); checkRelationIdentity(result); const native = assertNativeContext(result, oracle);
    return {coverage: result.coverage, native: nativeSummary(native), complete: result.complete, truncated: result.truncated};
  });
  // This independent fixed-source read is useful even when native capabilities
  // are missing. It proves post-semantic usability only after both actual native
  // calls completed and their evidence checks passed, which the next check gates.
  check('final-same-pin-api-query', () => { const again = queried('query-final', oracle.anchors[2], 'api'); assert.equal(again.row.symbolId, api?.row.symbolId); return {pin, symbolId: again.row.symbolId}; });
  check('post-semantic-source-read-still-usable', () => ({verifiedChecks: [...POST_SEMANTIC_REQUIREMENTS]}), {requires: POST_SEMANTIC_REQUIREMENTS});
  check('no-opt-in-session-left-behind', () => {
    supported('source session status'); const {result} = invoke('session-status', ['source', 'session', 'status']); assert.equal(result.state, 'closed', 'ordinary one-shot calls must not retain a session');
    return {state: result.state, backend: result.backend};
  });
} catch (error) {
  report.blocker = error.message;
} finally {
  // Explicit read/close is safe only while child transport remains controlled.
  // After a timeout or output failure, launch no more processes; the outer Job
  // must retire the tree and prove reaping before its scratch can be reclaimed.
  if (!fatalProcessFailure && directory?.some(row => row.path === 'source session close') && existsSync(home)) {
    check('release-owned-session', () => { const {result} = invoke('session-close', ['source', 'session', 'close'], {timeout: 30000}); assert.equal(result.state, 'closed'); return {state: result.state}; });
  }
  try { report.stagedPackageStable = inventoryHash(inventory(staged)) === packageSHA256; } catch (error) { report.stagedPackageStable = false; report.stagedPackageError = error.message; }
  report.finishedAt = new Date().toISOString(); report.wallMillis = performance.now() - started; report.outputBytes = outputBytes;
  report.state = !report.blocker && report.stagedPackageStable && report.checks.every(row => row.state === 'passed') ? 'passed_selected_skill_workflows' : 'failed';
  report.completeSkillAcceptance = false;
  report.remaining = ['independent review of this real output trail', ...report.pendingTasks.map(row => `${row.taskId}: ${row.reason}`), 'full Data/addon/Live Skill branches', 'complete fixed-Go differential', 'native lifecycle/corpus/performance gates'];
  save();
}
console.log(JSON.stringify({state: report.state, goContract: report.goContract, completeSkillAcceptance: false, pendingTasks: report.pendingTasks,
  report: join(out, 'report.json'), passed: report.checks.filter(row => row.state === 'passed').length, failed: report.checks.filter(row => row.state === 'failed').length}));
if (report.state !== 'passed_selected_skill_workflows') process.exitCode = 1;
