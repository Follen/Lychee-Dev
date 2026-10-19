// Independent acceptance helpers. Unit fixtures prove the checker, not the CLI.
import assert from 'node:assert/strict';
import {lstatSync, readFileSync, readdirSync, realpathSync} from 'node:fs';
import {isAbsolute, join, relative, resolve, sep} from 'node:path';
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {hashFile} from '../../tools/rust-build.mjs';
import {CURRENT_SKILL_MANIFEST, HISTORICAL_SKILL_MANIFEST, loadReviewedSkillContract, reviewedSkillIdentity, skillTreeIdentity} from './skill-identity.mjs';
export {skillTreeIdentity} from './skill-identity.mjs';

export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
export const POST_SEMANTIC_REQUIREMENTS = Object.freeze(['ordinary-native-callers', 'ordinary-native-context', 'final-same-pin-api-query']);
export const CURRENT_WORKFLOW = 'tests/rust-skill-usage/workflow-go323.json';
export const CURRENT_GO_CONTRACT = '3766f9cacb52e93a803a626187e2a1d54a445468';
const HISTORICAL_GO_CONTRACT = '3057a9666ab078270b63c7f06de748069b0d1f4b';
const INCREMENT_FILES = Object.freeze({
  'g321-keyboard-recovery': 'references/live-recovery.md',
  'g322-slot-startup': 'references/live-startup.md',
  'g323-data-query': 'references/data-tables.md',
  'g323-data-pressure-recovery': 'references/data-recovery.md',
});
const gitObject = (kind, bytes) => createHash('sha1').update(`${kind} ${bytes.length}\0`).update(bytes).digest('hex');

export function loadCurrentWorkflow(root) {
  const base = within(root, 'tests/rust-skill-usage');
  const workflow = jsonFile(within(root, CURRENT_WORKFLOW));
  assert.equal(workflow.schema, 'lycheedev.skill-usage-workflow.v2');
  assert.equal(workflow.goContract, CURRENT_GO_CONTRACT); assert.equal(workflow.goVersion, '3.2.3');
  assert.equal(workflow.historical?.workflow, 'workflow.json');
  assert.equal(workflow.historical.goContract, HISTORICAL_GO_CONTRACT);
  assert.equal(workflow.historical.sha256, 'ebf400880b52b7929e5614281b1ce90f77e5764bea09a8d9671daedeef6a2d9b');
  const originalBytes = readFileSync(within(base, workflow.historical.workflow));
  assert.equal(sha256(originalBytes), workflow.historical.sha256, 'historical workflow bytes are immutable');
  const original = JSON.parse(originalBytes);
  assert.equal(original.goContract, HISTORICAL_GO_CONTRACT);
  for (const name of ['requiredSkillFiles', 'prompts', 'apiOriginalWindow', 'nativeCallsite']) assert.deepEqual(workflow[name], original[name], `original Source workflow retained: ${name}`);
  assert.equal(workflow.historical.manifest, 'baselines/go3.2.0-skill.json');
  assert.equal(workflow.baselineManifest, 'baselines/go3.2.3-skill.json');
  const historical = jsonFile(within(base, workflow.historical.manifest)), current = jsonFile(within(base, workflow.baselineManifest));
  assert.equal(historical.tree, '7145d71201e7b1b63a3fbcd880ec78d6a1589e26');
  assert.equal(current.tree, 'b9e162ace7abe7ea4a2feecef822283767eb1210');
  for (const [manifest, commit, version] of [[historical, HISTORICAL_GO_CONTRACT, '3.2.0'], [current, CURRENT_GO_CONTRACT, '3.2.3']]) {
    assert.equal(manifest.schema, 'lycheedev.fixed-go-skill.v1'); assert.equal(manifest.commit, commit); assert.equal(manifest.version, version);
    assert.deepEqual(manifest.files.map(row => row.path), workflow.requiredSkillFiles, 'full original 23-file Git inventory');
    assert.equal(skillTreeIdentity(manifest.files), manifest.tree, 'full fixed Go Skill tree identity');
  }
  const changed = current.files.filter((row, index) => row.gitBlob !== historical.files[index].gitBlob).map(row => row.path).sort();
  assert.deepEqual(changed, Object.values(INCREMENT_FILES).sort(), 'exact cumulative 320 to 323 Skill delta');
  assert.deepEqual(workflow.increments?.map(row => row.id), Object.keys(INCREMENT_FILES), 'all four cumulative tasks retained');
  for (const increment of workflow.increments) {
    assert.equal(increment.file, INCREMENT_FILES[increment.id]);
    assert.equal(increment.fixture, `baselines/go3.2.3/${increment.file}`);
    const [since, sourceCommit] = increment.id.startsWith('g321-') ? ['3.2.1', '10b561db5706702cd42e047eaeed4ccb5e825351']
      : increment.id.startsWith('g322-') ? ['3.2.2', '0bec60e4406f5d5dc155ee25cef44f9a710e37e8'] : ['3.2.3', CURRENT_GO_CONTRACT];
    assert.equal(increment.since, since); assert.equal(increment.sourceCommit, sourceCommit);
    assert(typeof increment.prompt === 'string' && increment.prompt.length > 30);
    assert(Array.isArray(increment.requires) && increment.requires.length >= 2 && increment.requires.every(value => typeof value === 'string' && value.length > 10));
    assert(typeof increment.reason === 'string' && increment.reason.length > 30);
    const identity = current.files.find(row => row.path === increment.file), bytes = readFileSync(within(base, increment.fixture));
    assert.equal(bytes.length, identity.bytes, `fixed increment byte length: ${increment.file}`);
    assert.equal(sha256(bytes), identity.sha256, `fixed increment SHA256: ${increment.file}`);
    assert.equal(gitObject('blob', bytes), identity.gitBlob, `fixed increment original Git blob: ${increment.file}`);
  }
  return workflow;
}

export function currentWorkflowInputs(workflow) {
  return [CURRENT_WORKFLOW, CURRENT_SKILL_MANIFEST, HISTORICAL_SKILL_MANIFEST, 'tests/rust-skill-usage/skill-identity.mjs', 'tests/rust-skill-usage/workflow.json', workflow.historical.manifest, workflow.baselineManifest, ...workflow.increments.map(row => row.fixture)]
    .map(path => path.startsWith('tests/') ? path : `tests/rust-skill-usage/${path}`);
}

export function assertPackagedIncrement(skillRoot, workflow, increment, selectedIdentity) {
  assert.equal(workflow.goContract, CURRENT_GO_CONTRACT);
  assert.equal(increment.file, INCREMENT_FILES[increment.id]);
  const reviewed = selectedIdentity === undefined ? null : loadReviewedSkillContract(selectedIdentity);
  const manifest = reviewed ?? jsonFile(new URL('./baselines/go3.2.3-skill.json', import.meta.url));
  const identity = manifest.files.find(row => row.path === increment.file), bytes = readFileSync(within(skillRoot, increment.file));
  const label = reviewed ? 'packaged reviewed candidate Skill' : 'packaged Go323 increment';
  assert.equal(bytes.length, identity.bytes, `${label} byte length: ${increment.file}`);
  assert.equal(sha256(bytes), identity.sha256, `${label} SHA256: ${increment.file}`);
  assert.equal(gitObject('blob', bytes), identity.gitBlob, `${label} Git blob: ${increment.file}`);
  return {taskId: increment.id, file: increment.file, gitBlob: identity.gitBlob, sha256: identity.sha256,
    basis: reviewed ? 'exact_reviewed_candidate_skill_bytes_only' : 'exact_fixed_go323_skill_bytes_only',
    ...(reviewed ? {skillIdentity: reviewedSkillIdentity(reviewed)} : {}), runtimeVerified: false};
}

export function pendingIncrementTasks(workflow) {
  return workflow.increments.map(({id, prompt, requires, reason}) => ({taskId: id, prompt, state: 'not_run', reason, requires, runtimeVerified: false}));
}

export function assertCurrentSelectedReport(report, workflow, expectedIdentity) {
  if (expectedIdentity !== undefined) {
    assert.equal(report?.schema, 'lycheedev.skill-real-usage.v2', 'current owned Source run cannot reuse historical report');
    assert.deepEqual(report.skillIdentity, expectedIdentity, 'same selected Source Skill identity');
  }
  assert.equal(report?.goContract, CURRENT_GO_CONTRACT, 'owned result must identify the actual current workflow');
  assert.equal(report.goVersion, '3.2.3'); assert.equal(report.state, 'passed_selected_skill_workflows');
  assert.equal(report.completeSkillAcceptance, false, 'selected Source checks cannot complete all Skill acceptance');
  assert.deepEqual(report.prompts, workflow.prompts);
  assert.deepEqual(report.pendingTasks, pendingIncrementTasks(workflow), 'unexecuted increment tasks retain their explicit gaps');
  assert(Array.isArray(report.checks) && report.checks.every(row => row.state === 'passed'), 'every selected check passed');
  for (const caseId of POST_SEMANTIC_REQUIREMENTS) assert.equal(report.checks.filter(row => row.caseId === caseId).length, 1, 'actual native calls and final fixed query remain required');
  assert(['lycheedev.skill-real-usage.v1', 'lycheedev.skill-real-usage.v2'].includes(report.schema), 'known Source report schema');
  if (report.schema === 'lycheedev.skill-real-usage.v2') assert(report.skillIdentity, 'current Source report requires reviewed Skill identity');
  else assert.equal(report.skillIdentity, undefined, 'historical Source schema cannot relabel candidate bytes');
  const reviewed = report.skillIdentity === undefined ? null : loadReviewedSkillContract(report.skillIdentity);
  const manifest = reviewed ?? jsonFile(new URL('./baselines/go3.2.3-skill.json', import.meta.url));
  for (const increment of workflow.increments) {
    const rows = report.checks.filter(row => row.caseId === `fixed-skill-contract-${increment.id}`);
    assert.equal(rows.length, 1, 'each current contract checked exactly once');
    const row = rows[0], identity = manifest.files.find(file => file.path === increment.file);
    assert.equal(row.taskId, increment.id); assert.equal(row.file, increment.file);
    assert.equal(row.gitBlob, identity.gitBlob); assert.equal(row.sha256, identity.sha256);
    assert.equal(row.basis, reviewed ? 'exact_reviewed_candidate_skill_bytes_only' : 'exact_fixed_go323_skill_bytes_only');
    if (reviewed) assert.deepEqual(row.skillIdentity, reviewedSkillIdentity(reviewed));
    else assert.equal(row.skillIdentity, undefined);
    assert.equal(row.runtimeVerified, false);
  }
  return true;
}

// Evaluate only an admitted workflow check. Prerequisite failures remain in the
// caller's report unchanged; dependent checks neither execute nor gain a pass.
export function evaluateWorkflowCheck({caseId, work, priorChecks = [], requires = [], fatalProcessFailure = false}) {
  const started = performance.now();
  const blockedBy = requires.flatMap(required => {
    const matches = priorChecks.filter(row => row.caseId === required);
    if (matches.length !== 1) return [{caseId: required, state: matches.length ? 'ambiguous' : 'not_run', reason: 'Required preceding check did not complete exactly once'}];
    const check = matches[0];
    return check.state === 'passed' ? [] : [{caseId: required, state: check.state, reason: check.reason ?? 'Required preceding check did not pass'}];
  });
  if (fatalProcessFailure || blockedBy.length) return {caseId, state: 'not_run', reason: fatalProcessFailure
    ? 'Prior process/controller budget failure requires outer Job retirement; no further child launched'
    : 'Required preceding workflow checks did not complete successfully', ...(blockedBy.length ? {blockedBy} : {}), wallMillis: performance.now() - started};
  try {
    const detail = work() ?? {};
    return {...detail, ...(Object.hasOwn(detail, 'state') ? {observedState: detail.state} : {}), caseId, state: 'passed', wallMillis: performance.now() - started};
  } catch (error) {
    return {caseId, state: 'failed', reason: error.message, wallMillis: performance.now() - started};
  }
}

export function within(root, name) {
  assert.equal(typeof name, 'string', 'relative file name');
  assert(name && !isAbsolute(name) && !name.includes('\\') && !name.includes(':') && !name.includes('\0'), 'safe relative file name');
  assert(name.split('/').every(part => part && part !== '.' && part !== '..'), 'safe path components');
  const base = realpathSync(root), path = resolve(base, name);
  const rel = relative(base, path);
  assert(rel && !isAbsolute(rel) && rel !== '..' && !rel.startsWith(`..${sep}`), 'contained path');
  let cursor = base;
  for (const part of name.split('/')) {
    cursor = join(cursor, part);
    assert(!lstatSync(cursor).isSymbolicLink(), 'no linked candidate path');
  }
  return path;
}
export function jsonFile(path, limit = 8 << 20) {
  const stat = lstatSync(path);
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= limit, 'bounded regular JSON input');
  return JSON.parse(readFileSync(path, 'utf8'));
}
export function inventory(root, {maxBytes = 512 << 20, maxFiles = 10000} = {}) {
  assert(lstatSync(root).isDirectory() && !lstatSync(root).isSymbolicLink(), 'regular input directory');
  const rows = []; let total = 0, entries = 0, bytes = 0;
  function visit(directory, prefix) {
    for (const entry of readdirSync(directory, {withFileTypes: true}).sort((a, b) => Buffer.compare(Buffer.from(a.name), Buffer.from(b.name)))) {
      const name = prefix ? `${prefix}/${entry.name}` : entry.name;
      assert(++entries <= maxFiles * 4, 'candidate entry budget');
      const path = within(root, name), stat = lstatSync(path);
      assert(!entry.isSymbolicLink(), 'no candidate symlinks');
      if (stat.isDirectory()) { assert(name.split('/').length <= 24, 'candidate directory depth'); visit(path, name); }
      else {
        assert(stat.isFile() && ++total <= maxFiles, 'candidate regular-file count');
        rows.push({path: name, bytes: stat.size, sha256: hashFile(path, 128 << 20)});
        bytes += stat.size; assert(bytes <= maxBytes, 'candidate total byte budget');
      }
    }
  }
  visit(root, ''); return rows;
}
export const inventoryHash = rows => sha256(Buffer.from(JSON.stringify(rows)));

export function verifyCandidate(root, reportPath, artifact, workflow) {
  const report = jsonFile(reportPath, 1 << 20);
  assert.equal(report.schema, 'lycheedev.rust-npm-smoke.v1');
  assert(['assembled', 'partial_passed'].includes(report.state), 'successful candidate assembly');
  assert.equal(report.developmentOnly, true, 'unpublished candidate only');
  assert.equal(report.binarySHA256, artifact.binarySHA256, 'candidate and sealed EXE identity');
  assert.equal(report.sourceTreeSHA256, artifact.inputHashes?.sourceTree, 'candidate and sealed source identity');
  const packageRelative = relative(realpathSync(root), resolve(report.packageRoot)).split(sep).join('/');
  const packageRoot = within(root, packageRelative);
  const manifestPath = within(packageRoot, 'release.json');
  assert.equal(hashFile(manifestPath), report.releaseManifestSHA256, 'candidate report binds release manifest');
  const manifest = jsonFile(manifestPath), pkg = jsonFile(within(packageRoot, 'package.json'));
  assert.equal(manifest.schema, 'lycheedev.release.v1');
  assert.equal(manifest.runtime, 'rust');
  assert.equal(manifest.version, pkg.version); assert.equal(pkg.private, true);
  assert.equal(manifest.commit, report.commit);
  assert.equal(manifest.binaries?.['windows-amd64']?.path, 'native/windows-amd64/lycheedev.exe');
  const binary = within(packageRoot, manifest.binaries['windows-amd64'].path);
  assert.equal(hashFile(binary), artifact.binarySHA256);
  assert.equal(lstatSync(binary).size, manifest.binaries['windows-amd64'].bytes);
  assert.equal(manifest.binaries['windows-amd64'].sha256, artifact.binarySHA256);
  assert(Array.isArray(manifest.resources) && manifest.resources.length <= 10000, 'bounded resource inventory');
  assert.equal(sha256(Buffer.from(JSON.stringify(manifest.resources))), report.resourceInventorySHA256);
  const seen = new Set();
  for (const resource of manifest.resources) {
    assert(!seen.has(resource.path), 'unique resource path'); seen.add(resource.path);
    const file = within(packageRoot, `payload/${resource.path}`);
    assert.equal(lstatSync(file).size, resource.bytes, `resource bytes: ${resource.path}`);
    assert.equal(hashFile(file, 64 << 20), resource.sha256, `resource digest: ${resource.path}`);
  }
  for (const name of workflow.requiredSkillFiles) assert(seen.has(`skill/${name}`), `fixed Go Skill resource missing: ${name}`);
  assert.equal(workflow.goContract, CURRENT_GO_CONTRACT, 'current acceptance cannot relabel a historical workflow');
  // Bind the full reviewed candidate and the sealed source independently.
  // Historical Go prose remains authenticated in its own fixtures.
  const skill = loadReviewedSkillContract(), skillRoot = within(packageRoot, 'payload/skill');
  const packagedSkill = inventory(skillRoot, {maxBytes: 4 << 20, maxFiles: 100});
  assert.deepEqual(packagedSkill, skill.files.map(({path, bytes, sha256}) => ({path, bytes, sha256})), 'independently reviewed candidate Skill inventory');
  for (const row of skill.files) assert.equal(gitObject('blob', readFileSync(within(skillRoot, row.path))), row.gitBlob, `reviewed candidate Skill Git blob: ${row.path}`);
  assert.deepEqual(packagedSkill, inventory(within(root, 'skills/lycheedev'), {maxBytes: 4 << 20, maxFiles: 100}), 'entire packaged Skill equals sealed candidate source');
  assert.equal(hashFile(within(packageRoot, 'payload/tool/luac/luac.exe')), report.compilerSHA256);
  assert.equal(hashFile(within(packageRoot, 'bin/lycheedev.mjs')), hashFile(within(root, 'packages/npm/lycheedev/bin/lycheedev-rust.mjs')), 'packaged launcher is selected candidate launcher');
  // Do not silently test a stale or reduced Skill while evaluating today's tree.
  for (const row of inventory(within(root, 'skills/lycheedev'), {maxBytes: 4 << 20, maxFiles: 100})) {
    assert.equal(hashFile(within(packageRoot, `payload/skill/${row.path}`)), row.sha256, `packaged Skill source: ${row.path}`);
  }
  const files = inventory(packageRoot);
  const payload = files.filter(row => row.path.startsWith('payload/'));
  assert.equal(payload.length, seen.size, 'no unmanifested payload resources');
  return {packageRoot, report, manifest, files, packageSHA256: inventoryHash(files), reportSHA256: hashFile(reportPath)};
}

export function commandFlags(directory, path, flags) {
  const command = directory.find(row => row.path === path);
  assert(command, `installed describe lacks required Go Skill command: ${path}`);
  for (const flag of flags) assert(command.flags?.some(value => value === flag || value.startsWith(`${flag} `)), `installed ${path} lacks ${flag}`);
  return command;
}
export function assertBoundedCoverage(result) {
  assert.equal(typeof result.complete, 'boolean', 'explicit completion');
  assert.equal(typeof result.truncated, 'boolean', 'explicit truncation');
  assert(result.coverage && typeof result.coverage === 'object', 'explicit coverage');
  if (result.truncated || result.coverage.complete === false || result.coverage.semantic === 'unavailable') assert.equal(result.complete, false, 'limits cannot imply complete semantics');
}

// Rust hashes the typed structs in declaration order, while JSON Value members
// use serde_json's sorted map order. Hashing the parsed output object's order
// would therefore produce a different identity. Keep these projections aligned
// with native_research/plan.rs and native/graph.rs, not alphabetized structs.
function rustValue(value) {
  if (Array.isArray(value)) return value.map(rustValue);
  if (value !== null && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b))).map(key => [key, rustValue(value[key])]));
  if (typeof value === 'number') assert(Number.isSafeInteger(value), 'exact integer JSON evidence required for native plan identity');
  assert(value === null || ['string', 'number', 'boolean'].includes(typeof value), 'serializable native plan value');
  return value;
}
function typedOperation(operation) {
  assert(['references', 'type', 'definition'].includes(operation?.kind), 'typed semantic operation kind');
  const target = operation.target, site = target?.site;
  assert(site && typeof site.file === 'string' && Number.isSafeInteger(site.start) && Number.isSafeInteger(site.end) && site.start >= 0 && site.end > site.start, 'typed source site');
  assert.match(target.file_sha256, /^[a-f0-9]{64}$/, 'authenticated operation file identity');
  return {kind: operation.kind, target: {site: {file: site.file, start: site.start, end: site.end}, file_sha256: target.file_sha256, position: rustValue(target.position), material: rustValue(target.material)}};
}
export const nativeOperationDigest = operation => sha256(Buffer.from(JSON.stringify(typedOperation(operation))));
export function nativePlanDigest(plan) {
  return sha256(Buffer.from(JSON.stringify({schema: plan.schema, source_identity: plan.source_identity, query_identity: plan.query_identity,
    operations: plan.operations.map(typedOperation), batch_sizes: plan.batch_sizes, unavailable: plan.unavailable, outgoing_truncated: plan.outgoing_truncated,
    scanner_diagnostics: rustValue(plan.scanner_diagnostics), scanner_diagnostics_omitted: plan.scanner_diagnostics_omitted})));
}

function assertContextOperationExecution(evidence, oracle) {
  const planned = evidence.semanticPlan, plan = planned?.plan, execution = evidence.semanticExecution, receipt = execution?.receipt;
  assert(['executed', 'execution_incomplete'].includes(planned?.state), 'context must contain execution evidence, not just a plan');
  assert.equal(plan?.schema, 'lycheedev.native-semantic-plan.v1');
  assert.equal(plan.source_identity, evidence.inputIdentity, 'plan belongs to actual native inputs');
  assert.match(plan.query_identity, /^[a-f0-9]{64}$/);
  assert(Array.isArray(plan.operations) && plan.operations.length > 0 && plan.operations.length <= 130, 'bounded nonempty native operation plan');
  const batchSizes = Array.from({length: Math.ceil(plan.operations.length / 128)}, (_, index) => Math.min(128, plan.operations.length - index * 128));
  assert.deepEqual(plan.batch_sizes, batchSizes, 'plan operation batches');
  assert(Array.isArray(plan.unavailable) && plan.unavailable.every(reason => typeof reason === 'string'));
  assert.equal(typeof plan.outgoing_truncated, 'boolean');
  assert(Array.isArray(plan.scanner_diagnostics)); assert(Number.isSafeInteger(plan.scanner_diagnostics_omitted) && plan.scanner_diagnostics_omitted >= 0);
  assert.equal(execution?.schema, 'lycheedev.native-semantic-execution.v1');
  assert.equal(receipt?.plan_identity, nativePlanDigest(plan), 'receipt binds typed native plan');
  assert.equal(receipt.planned_batches, batchSizes.length);
  for (const field of ['completed_batches', 'scopes', 'unavailable_scopes', 'uncertain_answers', 'unexecuted_operations']) assert(Number.isSafeInteger(receipt[field]) && receipt[field] >= 0, `bounded receipt ${field}`);
  assert(Array.isArray(execution.answers) && execution.answers.length > 0, 'native scope answers');
  const anchor = oracle.anchors[0];
  // Authenticated original ActionButton.lua has this exact declaration token at
  // byte 2915..2935, one-based line 97 (UTF-16 zero-based 96:9..96:29).
  const selected = new Set(plan.operations.flatMap((operation, index) => {
    if (operation.kind !== 'references') return [];
    const target = operation.target;
    assert.deepEqual(target.site, {file: anchor.path, start: 2915, end: 2935}, 'references plan targets the selected original declaration token');
    assert.equal(target.file_sha256, anchor.sha256, 'references plan targets authenticated original file');
    assert.deepEqual(target.material, {kind: 'source'});
    assert.deepEqual(target.position, {path: anchor.path, range: {start: {line: 96, character: 9}, end: {line: 96, character: 29}},
      coveringRange: {start: {line: 96, character: 9}, end: {line: 96, character: 29}}, bytes: {start: 2915, end: 2935}, positionAccuracy: 'utf16-character'}, 'references plan original declaration coordinates');
    return [index];
  }));
  assert(selected.size > 0, 'context plan includes selected declaration references operation');
  let scopes = 0, unavailable = 0, unexecuted = 0, uncertain = 0, executedReferences = 0;
  const completed = batchSizes.map(() => true);
  for (const scope of execution.answers) {
    if (scope.state === 'unavailable') {
      assert.equal(scope.operationsExecuted, false, 'unavailable scope does not credit execution'); assert.equal(scope.operations, undefined); unavailable++; continue;
    }
    assert(scope.scope && typeof scope.scope === 'object', 'execution scope identity');
    assert(Array.isArray(scope.operations) && scope.operations.length === plan.operations.length, 'scope answers correspond to every planned operation'); scopes++;
    for (const [index, answer] of scope.operations.entries()) {
      const operation = plan.operations[index], batch = Math.floor(index / 128);
      assert.equal(answer.operation, index); assert.equal(answer.batch, batch); assert.equal(answer.kind, operation.kind);
      assert.equal(answer.operationIdentity, nativeOperationDigest(operation), 'answer binds typed operation identity');
      assert.equal(typeof answer.executed, 'boolean', 'answer states actual execution'); assert.equal(typeof answer.state, 'string');
      if (!answer.executed) { unexecuted++; completed[batch] = false; }
      if (!['known', 'known_declaration'].includes(answer.state)) uncertain++;
      if (selected.has(index) && answer.executed) {
        assert(['known', 'partial'].includes(answer.state), 'executed references retain actual answer state');
        assert.match(answer.result?.relationsSHA256, /^[a-f0-9]{64}$/, 'actual references result digest');
        assert(Number.isSafeInteger(answer.result.relationCount) && answer.result.relationCount >= 0, 'actual references result count');
        const analysis = answer.result.analysis;
        assert.equal(analysis?.engine, 'native_common_facts'); assert.equal(analysis.inputIdentity, evidence.inputIdentity);
        assert.equal(analysis.sourceExecution, false); assert(['complete', 'partial'].includes(analysis.state));
        assert(Number.isSafeInteger(analysis.sourceFiles) && analysis.sourceFiles >= oracle.counts['.lua']); executedReferences++;
      }
    }
  }
  assert.equal(receipt.scopes, scopes); assert.equal(receipt.unavailable_scopes, unavailable); assert.equal(receipt.unexecuted_operations, unexecuted); assert.equal(receipt.uncertain_answers, uncertain);
  assert.equal(receipt.completed_batches, scopes > 0 && unavailable === 0 ? completed.filter(Boolean).length : 0, 'completion counts do not credit unavailable/unexecuted scopes');
  assert.equal(planned.resourceAccounting?.executedSemanticBatches, receipt.completed_batches);
  assert.equal(planned.state, unavailable > 0 || unexecuted > 0 ? 'execution_incomplete' : 'executed');
  if (plan.unavailable.length || unavailable || uncertain || unexecuted) assert.equal(evidence.analysis.state, 'partial', 'incomplete plan/scope/answers cannot claim complete native analysis');
  assert(executedReferences > 0, 'context requires an actually executed selected-declaration references answer; partial scopes remain allowed');
}

function assertNativeExecution(result, oracle) {
  assertBoundedCoverage(result);
  const evidence = result.nativeResearch;
  assert.equal(evidence?.schema, 'lycheedev.source-native-research.v1', 'ordinary semantic route must return actual native evidence');
  assert.equal(evidence.entryEvidence?.sourceExecution, false, 'studied source was not executed');
  assert.equal(evidence.lifecycle?.owner, 'source_owned_process', 'owned native worker');
  assert.equal(evidence.lifecycle?.mode, 'one_shot', 'ordinary call has one-shot lifecycle');
  assert.equal(evidence.lifecycle?.retired, true, 'one-shot worker actually retired');
  assert.equal(result.semanticRuntime?.engine, 'native-common-source', 'selected native semantic engine');
  assert.equal(typeof result.semanticRuntime.rulesVersion, 'string'); assert(result.semanticRuntime.rulesVersion.length > 0, 'native rules identity');
  assert.equal(evidence.analysis?.engine, 'native_common_facts', 'actual native fact projection');
  assert(['complete', 'partial'].includes(evidence.analysis.state), 'explicit native analysis state');
  assert.equal(evidence.analysis.sourceExecution, false, 'native analysis did not execute studied source');
  assert.match(evidence.inputIdentity, /^[a-f0-9]{64}$/, 'authenticated native input identity');
  assert.equal(evidence.analysis.inputIdentity, evidence.inputIdentity, 'native input identity is consistent');
  assert(Number.isInteger(evidence.analysis.sourceFiles) && evidence.analysis.sourceFiles >= oracle.counts['.lua'], 'native request includes fixed Retail Lua inputs; this does not claim complete semantic resolution');
  for (const field of ['engine', 'state', 'inputIdentity', 'sourceFiles', 'sourceExecution']) assert.deepEqual(result.semanticAnalysis?.[field], evidence.analysis[field], `outer/native analysis ${field}`);
  assert.equal(result.coverage.semantic, evidence.analysis.state, 'coverage preserves actual native state');
  if (evidence.analysis.state === 'partial') assert.equal(result.complete, false, 'partial native facts stay partial');
  assert.equal(evidence.relations, undefined, 'native evidence does not bypass public pagination');
  return evidence;
}
export function assertNativeCaller(row, callsite) {
  assert.equal(row.source, callsite.source, 'native call source is its authenticated caller');
  assert.equal(row.referenceExpression, callsite.referenceExpression, 'original reference expression retained');
  assert.equal(row.caller?.kind, callsite.caller.kind, 'explicit caller kind');
  assert.equal(row.caller?.name, callsite.caller.name, 'authenticated callable name');
  assert.equal(row.caller?.position?.path, callsite.path, 'caller original source path');
  assert.equal(row.caller?.position?.coveringRange?.start?.line, callsite.caller.line - 1, 'caller original declaration line');
  assert.equal(row.caller?.syntaxClosure?.file, callsite.site.file, 'caller admitted syntax file');
  assert(row.caller?.syntaxClosure && row.caller.syntaxClosure.start < row.site.start && row.caller.syntaxClosure.end >= row.site.end, 'nearest callable syntax contains callsite');
}
export function assertNativeEvidence(result, oracle, callsite) {
  const evidence = assertNativeExecution(result, oracle);
  // Production intentionally removes nativeResearch.relations and projects
  // those rows into the bounded public refs page. Check the native-origin row
  // there, never an independently present static-extraction row at that site.
  assert(Array.isArray(result.relations), 'public native relation page');
  const rows = result.relations.filter(row => ['path', 'line', 'kind', 'target'].every(key => row[key] === callsite[key]) && row.basis?.includes('native_common_source_facts'));
  assert(rows.length > 0, 'exact original callsite must have native-origin evidence');
  for (const row of rows) {
    assertNativeCaller(row, callsite);
    assert.deepEqual(row.site, callsite.site, 'native callsite exact original byte range');
    assert(['resolved', 'static_candidate'].includes(row.state), 'native relation strength remains explicit');
    assert.equal(typeof row.proof, 'string'); assert(row.proof.length > 0, 'native reference proof identity');
    assert(['known', 'known_absent', 'unavailable', 'not_applicable', 'selector_required'].includes(row.lexical?.state), 'native lexical result retained');
    assert(Array.isArray(row.observations), 'native observations retained, even when empty/partial');
    for (const observation of row.observations) {
      assert.equal(typeof observation.proof, 'string'); assert(observation.proof.length > 0, 'native observation proof identity');
      assert.equal(typeof observation.occurrence?.kind, 'string', 'native observation occurrence');
      assert(['known', 'known_absent', 'unavailable', 'not_applicable', 'selector_required'].includes(observation.selected?.state), 'native selected-value result');
    }
    if (row.state === 'resolved') assert(row.observations.length > 0 && row.observations.every(observation => observation.selected.state === 'known'), 'resolved call requires selected-value observations');
  }
  return evidence;
}
export function assertNativeContext(result, oracle) {
  const evidence = assertNativeExecution(result, oracle), anchor = oracle.anchors[0];
  assertContextOperationExecution(evidence, oracle);
  // Fixed Go context.go exposes relations only at depth > 0. This check is for
  // our actual --depth 0 request, so empty relations must not fail native work.
  assert.deepEqual(result.relations, [], 'depth-zero context excludes relation traversal');
  assert(Array.isArray(result.snippets) && result.snippets.length === 1, 'depth-zero context returns the selected definition');
  const snippet = result.snippets[0], lines = anchor.text.replace(/\n+$/, '').split('\n');
  const expectedText = lines.map((line, index) => `${anchor.line + index}: ${line}`).join('\n');
  assert.equal(snippet.path, anchor.path, 'context original definition path');
  assert.equal(snippet.firstLine, anchor.line, 'context original definition first line');
  assert.equal(snippet.lastLine, anchor.line + lines.length - 1, 'context original definition last line');
  assert.equal(snippet.contentHash, anchor.sha256, 'context original whole-file hash');
  assert.equal(snippet.basis, 'definition', 'context definition basis');
  assert.equal(snippet.text, expectedText, 'context exact original numbered definition text');
  return evidence;
}
export function assertApiAnswer(result, row, oracle, window) {
  assert.equal(window.commit, oracle.commit); assert.equal(window.archiveSHA256, oracle.archiveSHA256);
  assert.equal(window.fullFileSHA256, oracle.anchors[2].sha256); assert.equal(window.path, oracle.anchors[2].path);
  assert.equal(result.repository, oracle.repository); assert.equal(result.commit, oracle.commit);
  assert.equal(row.path, window.path); assert.equal(row.line, window.firstLine, 'returned declaration starts the authenticated requested window');
  for (const field of ['path', 'firstLine', 'lastLine', 'totalLines', 'text']) assert.equal(result[field], window[field], `exact API original window ${field}`);
  assert.equal(result.blob?.sha256, window.fullFileSHA256);
  assert.equal(sha256(Buffer.from(result.text)), window.textSHA256, 'exact API window bytes');
  assert.equal(window.lastLine - window.firstLine + 1, window.count);
  return {symbol: 'C_Timer.After', commit: result.commit, path: result.path, declarationLines: window.declarationLines, argumentLines: window.argumentLines,
    secretArgumentsLine: window.secretArgumentsLine, windowFirstLine: result.firstLine, windowLastLine: result.lastLine, windowSHA256: window.textSHA256, fullFileSHA256: result.blob.sha256,
    answer: 'C_Timer.After(seconds, callback): seconds is a non-nil number; callback is a non-nil TimerCallback. The exact declaration marks SecretArguments = AllowedWhenUntainted. This is a static declaration contract; it does not establish live taint state, loaded-client behavior, or implementation details.'};
}
