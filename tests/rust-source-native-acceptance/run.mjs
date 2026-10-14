// Called only by the owned wrapper. One Job owns this controller, the reused
// full-corpus controller, every CLI/Git/PowerShell child and retained owner.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {closeSync, existsSync, lstatSync, mkdirSync, openSync, readFileSync, readSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {gzipSync} from 'node:zlib';
import {performance} from 'node:perf_hooks';
import os from 'node:os';
import {assertEnvelope, assertAnchor, sha256} from '../rust-source-full-corpus/assertions.mjs';
import {jsonFile} from '../rust-skill-usage/assertions.mjs';
import {assertApiType, assertContextSnippets, assertExecutedContext, assertFullNative, assertMaterial, assertNestedControllerRetired, assertPrerequisite, assertReuse, fixturePin, groupBudget, rebuildXmlInventory} from './assertions.mjs';

const [binaryArg, releaseArg, scratchArg, evidenceArg, group, ...extra] = process.argv.slice(2);
assert(binaryArg && releaseArg && scratchArg && evidenceArg && group && !extra.length, 'owned controller arguments');
assert.equal(process.platform, 'win32');
const root = resolve('.'), binary = resolve(binaryArg), release = resolve(releaseArg), scratch = resolve(scratchArg), out = resolve(evidenceArg), home = join(scratch, 'home');
const owner = jsonFile(join(out, 'receipt.json'));
assert.equal(owner.owner, `source-native-${group}`); assert.equal(owner.state, 'active'); assert.equal(resolve(owner.directory), scratch);
assert.equal(readdirSync(scratch).length, 0, 'fresh controlled scratch');
const oracle = jsonFile(new URL('../rust-source-full-corpus/retail-oracle.json', import.meta.url));
const workflow = jsonFile(new URL('../rust-skill-usage/workflow.json', import.meta.url));
const started = performance.now(), deadline = started + groupBudget(group) - 15000;
const report = {schema: 'lycheedev.source-native-acceptance.v1', group, state: 'running', startedAt: new Date().toISOString(), oracle,
  binarySHA256: sha256(readFileSync(binary)), checks: [], commands: [], resources: [],
  machine: {platform: process.platform, arch: process.arch, release: os.release(), node: process.version, cpu: os.cpus()[0]?.model, cores: os.cpus().length, totalMemory: os.totalmem()},
  performance: {state: 'not_evaluated', targetRatio: 0.30, sampleKind: 'single correctness execution; independent cold/warm observations only', goOracle: '3057a9666ab078270b63c7f06de748069b0d1f4b', goDifferential: 'not_run', cpu: 'not_measured', io: 'not_measured', aggregateRSS: 'not_measured', reason: 'No same-machine Go candidate/repeated paired samples. Source sync includes network and local validation; no guessed download subtraction.'},
  boundaries: ['No studied Lua/configuration execution, game or publication', 'Passed selected cases never mean complete semantic resolution or complete Source acceptance', 'Quota/idle/lifetime are separate real small-material IPC groups, never substitutes for this full Retail group']};
let sequence = 0, outputBytes = 0, fatal = false, pin, corpus, originals;
const save = () => writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n'); save();
const env = {...process.env, LYCHEEDEV_SOURCE_PHASE_PROFILE: '1'};
function command(caseId, executable, args, {timeout = 210000, phase = 'local', input, maxBuffer = 32 << 20, cwd = scratch, environment = env} = {}) {
  assert(!fatal, 'prior process failure requires outer Job retirement');
  const remaining = Math.floor(deadline - performance.now());
  if (remaining < 1000 || sequence >= 100 || outputBytes >= 384 << 20) { fatal = true; throw new Error('native-acceptance.controller_budget'); }
  const prefix = `${String(sequence++).padStart(3, '0')}-${caseId}`, time = performance.now();
  const run = spawnSync(executable, args, {cwd, env: environment, input, windowsHide: true, shell: false, timeout: Math.min(timeout, remaining), maxBuffer});
  const stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0);
  outputBytes += stdout.length + stderr.length;
  const preserve = (suffix, bytes) => {
    const files = [];
    for (let offset = 0; offset < Math.max(1, bytes.length); offset += 8 << 20) {
      const name = `${prefix}.${suffix}.part${files.length}`, part = bytes.subarray(offset, offset + (8 << 20));
      writeFileSync(join(out, name), part); files.push({path: name, bytes: part.length, sha256: sha256(part)});
    } return files;
  };
  const stdoutFiles = preserve('stdout', stdout), stderrFiles = preserve('stderr', stderr);
  const observation = {caseId, executable, args, cwd, phase, wallMillis: performance.now() - time, exit: run.status, signal: run.signal, error: run.error?.message,
    stdoutFiles, stderrFiles, stdoutSHA256: sha256(stdout), stderrSHA256: sha256(stderr), stdoutBytes: stdout.length, stderrBytes: stderr.length};
  if (input !== undefined) observation.inputSHA256 = sha256(Buffer.from(input));
  report.commands.push(observation); if (run.error || run.signal || outputBytes > 384 << 20) fatal = true; save();
  assert(!run.error && run.signal === null, `owned command failed: ${caseId}: ${run.error?.message ?? run.signal}`);
  return {stdout, stderr, observation};
}
function invoke(caseId, args, options = {}) {
  const run = command(caseId, binary, [...args, '--home', home, '--format', 'json'], options);
  let envelope; try { envelope = JSON.parse(run.stdout); } catch {}
  const result = assertEnvelope({...run.observation, envelope}, options.expectedExit ?? 0);
  const phases = run.stderr.toString('utf8').split(/\r?\n/).flatMap(line => { try { const event = JSON.parse(line); return event.schema === 'lycheedev.source-phase.v1' ? [event] : []; } catch { return []; } });
  run.observation.phases = phases; save(); return {result, envelope, observation: run.observation};
}
async function check(caseId, work) {
  const time = performance.now();
  if (fatal) { report.checks.push({caseId, state: 'not_run', reason: 'Prior fatal process/budget failure; outer Job retirement required'}); save(); return false; }
  try { const detail = await work(); report.checks.push({caseId, state: 'passed', wallMillis: performance.now() - time, ...detail}); save(); return true; }
  catch (error) { if (error.fatalProcessFailure) fatal = true; report.checks.push({caseId, state: 'failed', wallMillis: performance.now() - time, reason: error.message}); save(); return false; }
}
function fixed(args) { assert(pin); return [...args, '--snapshot', pin]; }
function relation(command, {reuse = false, project} = {}) {
  const args = ['source', command, '--symbol', oracle.anchors[0].symbol, '--limit', command === 'refs' ? '25' : '50', '--release', release];
  if (command === 'refs') args.push('--direction', 'incoming'); else args.push('--depth', '0', '--max-lines', '80', '--max-bytes', '16384');
  if (reuse) args.push('--session-reuse');
  return project ? [...args, '--project', project] : fixed(args);
}
function diagnostic(caseId, expectedPid) {
  const path = join(home, 'tmp/source-native-last-request.json');
  assert(existsSync(path), 'actual native owner diagnostic required');
  const raw = readFileSync(path); assert(raw.length <= 1 << 20);
  writeFileSync(join(out, `${caseId}.diagnostic.json`), raw);
  const value = JSON.parse(raw.toString().replace(/("(?:workerCreated|created)"\s*:\s*)(\d+)/g, '$1"$2"'));
  assert.equal(value.schema, 'lycheedev.native-source-request-diagnostic.v1'); assert.equal(value.phase, 'replying'); assert.equal(value.workerPid, expectedPid);
  assert.match(value.workerCreated, /^\d+$/); assert.equal(value.processMemory?.identity?.pid, expectedPid); assert.equal(value.processMemory.identity.created, value.workerCreated);
  for (const field of ['private_commit', 'working_set', 'peak_working_set', 'peak_private_commit']) assert(Number.isSafeInteger(value.processMemory[field]) && value.processMemory[field] >= 0);
  assert.equal(value.memory?.job_memory_limit, 2 ** 31);
  report.resources.push({caseId, diagnostic: `${caseId}.diagnostic.json`, sha256: sha256(raw), ...value}); save();
  return {pid: value.workerPid, created: value.workerCreated};
}
function materialEvidence(caseId, commit = oracle.commit) {
  const cache = join(home, 'cache/objects'); let examined = 0, found = 0, detail;
  for (const bucket of readdirSync(cache, {withFileTypes: true})) {
    assert(++examined <= 10000 && bucket.isDirectory() && /^[a-f0-9]{2}$/.test(bucket.name), 'bounded regular cache inventory');
    for (const entry of readdirSync(join(cache, bucket.name), {withFileTypes: true})) {
      assert(++examined <= 10000 && entry.isFile() && /^[a-f0-9]{62}$/.test(entry.name));
      const path = join(cache, bucket.name, entry.name), stat = lstatSync(path); assert(!stat.isSymbolicLink());
      const fd = openSync(path, 'r'), header = Buffer.alloc(Math.min(stat.size, 96));
      try { readSync(fd, header, 0, header.length, 0); } finally { closeSync(fd); }
      if (!header.toString().startsWith('{"schema":"lycheedev.source-research-material.v3"')) continue;
      assert(stat.size <= 160 << 20, 'bounded full Retail material audit');
      const raw = readFileSync(path), material = JSON.parse(raw);
      if (material.snapshot.source.exactCommit !== commit) continue;
      assert.equal(sha256(raw), bucket.name + entry.name, 'content addressed actual material');
      detail = assertMaterial(material, corpus, oracle, commit); found++;
      assert.equal(found, 1, 'unambiguous exact source material');
      const compressed = gzipSync(raw), chunks = [];
      for (let offset = 0; offset < compressed.length; offset += 8 << 20) {
        const name = `${caseId}.material.gz.part${chunks.length}`, bytes = compressed.subarray(offset, offset + (8 << 20));
        writeFileSync(join(out, name), bytes); chunks.push({path: name, bytes: bytes.length, sha256: sha256(bytes)});
      }
      detail = {...detail, rawBytes: raw.length, rawSHA256: sha256(raw), encoding: 'concatenate chunks then gzip-decompress', chunks};
    }
  }
  assert.equal(found, 1, 'complete persisted native input required'); return detail;
}
function git(caseId, args, input) {
  const environment = Object.fromEntries(Object.entries(env).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE'));
  Object.assign(environment, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: 'NUL', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1'});
  const result = command(caseId, 'git', ['--no-optional-locks', '-c', 'core.hooksPath=NUL', '--git-dir', join(home, 'mirrors', `${oracle.repository}.git`), ...args], {input, environment, timeout: 60000, phase: 'local-identity-fixture'});
  assert.equal(result.observation.exit, 0); return result.stdout.toString('utf8').trim();
}
function authenticateNativeOriginals() {
  const documents = corpus.files.filter(row => /\.(lua|xml|toc)$/i.test(row.path));
  const environment = Object.fromEntries(Object.entries(env).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE'));
  Object.assign(environment, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: 'NUL', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1'});
  const run = command('native-original-blobs', 'git', ['--no-optional-locks', '--git-dir', join(home, 'mirrors', `${oracle.repository}.git`), 'cat-file', '--batch'], {environment, input: documents.map(row => `${row.gitSHA1}\n`).join(''), maxBuffer: 48 << 20, timeout: 60000, phase: 'independent-original-native-input-audit'});
  assert.equal(run.observation.exit, 0); const bytes = run.stdout, found = new Map(); let cursor = 0;
  for (const row of documents) {
    const end = bytes.indexOf(10, cursor); assert(end >= cursor);
    assert.equal(bytes.subarray(cursor, end).toString('ascii'), `${row.gitSHA1} blob ${row.bytes}`);
    const raw = Buffer.from(bytes.subarray(end + 1, end + 1 + row.bytes));
    assert.equal(raw.length, row.bytes); assert.equal(sha256(raw), row.sha256); assert.equal(bytes[end + 1 + row.bytes], 10);
    assert(!found.has(row.path)); found.set(row.path, raw); cursor = end + row.bytes + 2;
  }
  assert.equal(cursor, bytes.length); assert.equal(found.size, oracle.documents); originals = found;
  corpus.xmlMappings = rebuildXmlInventory(originals, corpus);
  const inventory = Buffer.from(JSON.stringify(corpus.xmlMappings)); assert(inventory.length <= 16 << 20, 'independent XML oracle evidence budget');
  writeFileSync(join(out, 'independent-xml-maps.json'), inventory);
  return {documents: originals.size, xmlDocuments: corpus.xmlMappings.xmlDocuments, generatedScripts: corpus.xmlMappings.maps.length, mappingSHA256: corpus.xmlMappings.sha256, inventory: 'independent-xml-maps.json', inventorySHA256: sha256(inventory)};
}
function contextEvidence(result) {
  assert.deepEqual(result.relations, [], 'depth-zero context excludes relation rows');
  const snippets = assertContextSnippets(result, originals, corpus, {maxBytes: 16384, maxLines: 80, limit: 50, anchor: oracle.anchors[0]});
  return {...snippets, execution: assertExecutedContext(result.nativeResearch, originals, oracle.anchors[0])};
}
function makeIdentityVariant() {
  // An isolated synthetic descendant; original Retail documents are unchanged.
  // Never publish its ref or identify it as a second upstream Retail revision.
  const note = 'Isolated native acceptance identity variant; all original source documents are unchanged.\n';
  const blob = git('variant-note', ['hash-object', '-w', '--stdin'], note);
  const listing = git('variant-root-tree', ['ls-tree', oracle.commit]);
  const tree = git('variant-tree', ['mktree'], `${listing}\n100644 blob ${blob}\tlycheedev-acceptance-identity.txt\n`);
  const commitRaw = `tree ${tree}\nparent ${oracle.commit}\nauthor Native acceptance <fixture@example.invalid> 0 +0000\ncommitter Native acceptance <fixture@example.invalid> 0 +0000\n\nIsolated same-document identity variant\n`;
  const commit = git('variant-commit', ['hash-object', '-t', 'commit', '-w', '--stdin'], commitRaw);
  assert.match(commit, /^[a-f0-9]{40}$/); assert.notEqual(commit, oracle.commit);
  const project = join(scratch, 'identity-variant'); mkdirSync(project);
  const selection = fixturePin({repository: oracle.repository, product: oracle.product, requestedRef: commit, exactCommit: commit, parserRevision: 'source-v1'});
  const declaration = {schema: 'lycheedev.project.v1', product: oracle.product}, lock = {schema: 'lycheedev.project-lock.v1', selection};
  writeFileSync(join(project, 'lycheedev.json'), JSON.stringify(declaration)); writeFileSync(join(project, 'lycheedev.lock.json'), JSON.stringify(lock));
  writeFileSync(join(out, 'identity-variant.json'), JSON.stringify({scope: 'Synthetic local descendant, not upstream product evidence', base: oracle.commit, commit, tree, note, blob, declaration, lock, commitRaw}, null, 2) + '\n');
  invoke('variant-project-validate', ['project', 'status', '--path', project]);
  return {project, commit};
}
try {
  if (group === 'full') {
    const prerequisite = join(out, 'full-corpus'); mkdirSync(prerequisite);
    const ready = await check('fresh-complete-retail-prerequisite', () => {
      const run = command('full-corpus-controller', process.execPath, [join(root, 'tests/rust-source-full-corpus/run.mjs'), binary, home, prerequisite], {cwd: root, timeout: 900000, phase: 'complete-fresh-corpus-and-static-quality'});
      let previous;
      try { previous = jsonFile(join(prerequisite, 'report.json')); } catch (error) { error.fatalProcessFailure = true; throw error; }
      assertNestedControllerRetired(run.observation, previous);
      corpus = jsonFile(join(prerequisite, 'verified-corpus-manifest.json'));
      pin = assertPrerequisite(previous, corpus, oracle); report.pin = pin;
      report.performance.inputPreparationWallMillis = previous.performance.inputPreparationWallMillis;
      return {pin, files: corpus.files.length, documents: oracle.documents, prerequisite: 'full-corpus/report.json', independentlyAuthenticatedManifest: 'full-corpus/verified-corpus-manifest.json'};
    });
    assert(ready, 'Full fixed corpus prerequisite failed; native full-corpus cases not_run');
    assert(await check('independently-rebuild-native-xml-inventory', authenticateNativeOriginals), 'Independent original/XML audit failed; native cases not_run');
    let ordinary, first, second, firstOwner, reopenedOwner;
    await check('ordinary-native-cold-refs', () => {
      const {result, observation} = invoke('ordinary-cold-refs', relation('refs'));
      ordinary = assertFullNative(result, oracle, corpus, workflow.nativeCallsite);
      assert.equal(ordinary.analysis.cacheHit, false); assert(observation.phases.some(row => row.phase === 'source.material.write' && row.status === 'ok'), 'cold material preparation');
      return {inputIdentity: ordinary.inputIdentity, coverage: result.coverage, analysis: ordinary.analysis, semanticReceipt: ordinary.semanticExecution.receipt};
    });
    await check('audit-actual-full-native-material', () => materialEvidence('retail'));
    await check('ordinary-native-warm-material-context', () => {
      const {result, observation} = invoke('ordinary-warm-context', relation('context'));
      const native = assertFullNative(result, oracle, corpus, null);
      assert(ordinary); assert.equal(native.inputIdentity, ordinary.inputIdentity); assert.equal(native.analysis.cacheHit, false, 'one-shot analysis remains fresh');
      assert(observation.phases.some(row => row.phase === 'source.material.admit' && row.status === 'ok'), 'actual warm material cache admission');
      const snippets = contextEvidence(result);
      assert.equal(invoke('ordinary-status', ['source', 'session', 'status']).result.state, 'closed');
      return {analysis: native.analysis, coverage: result.coverage, semanticReceipt: native.semanticExecution.receipt, snippets};
    });
    await check('full-retail-authenticated-api-type', () => {
      const anchor = oracle.anchors[2];
      const queried = invoke('api-type-query', fixed(['source', 'query', anchor.symbol, '--topic', 'api', '--limit', '10'])).result;
      const symbol = assertAnchor(queried, anchor, oracle, pin);
      const result = invoke('api-native-type', fixed(['source', 'refs', '--symbol-id', symbol.symbolId, '--direction', 'incoming', '--limit', '25', '--release', release])).result;
      const native = assertFullNative(result, oracle, corpus, null);
      const types = assertApiType(native, anchor);
      return {inputIdentity: native.inputIdentity, analysis: native.analysis, semanticReceipt: native.semanticExecution.receipt, types};
    });
    await check('full-retail-session-cold', () => {
      assert.equal(invoke('session-before', ['source', 'session', 'status']).result.state, 'closed');
      first = assertFullNative(invoke('session-cold-refs', relation('refs', {reuse: true})).result, oracle, corpus, workflow.nativeCallsite, {mode: 'session'});
      assert.equal(first.analysis.cacheHit, false); assert.equal(first.nativeSession.snapshotTransfer, 'full'); firstOwner = diagnostic('session-cold', first.nativeSession.workerPid);
      return {inputIdentity: first.inputIdentity, nativeSession: first.nativeSession, owner: firstOwner};
    });
    await check('full-retail-cross-cli-warm-context', () => {
      assert(first); const result = invoke('session-warm-context', relation('context', {reuse: true})).result;
      second = assertFullNative(result, oracle, corpus, null, {mode: 'session'});
      const snippets = contextEvidence(result);
      assertReuse(first, second); assert.deepEqual(diagnostic('session-warm', second.nativeSession.workerPid), firstOwner);
      assert.equal(second.nativeSession.batches, first.nativeSession.batches + second.nativeSession.admittedSemanticBatches);
      const status = invoke('session-ready', ['source', 'session', 'status']).result;
      assert.equal(status.ownerPid, firstOwner.pid); assert.equal(status.workerPid, firstOwner.pid); assert.equal(status.state, 'ready'); assert.equal(status.batches, second.nativeSession.batches);
      return {nativeSession: second.nativeSession, status, snippets};
    });
    await check('changed-full-corpus-identity-reprepares-and-restores', () => {
      assert(second); const variant = makeIdentityVariant();
      const changed = assertFullNative(invoke('session-changed-identity', relation('refs', {reuse: true, project: variant.project})).result, oracle, corpus, workflow.nativeCallsite, {mode: 'session', commit: variant.commit});
      assertReuse(second, changed, {changed: true}); assert.deepEqual(diagnostic('session-changed', changed.nativeSession.workerPid), firstOwner);
      const actualMaterial = materialEvidence('variant', variant.commit);
      const restored = assertFullNative(invoke('session-restored-retail', relation('refs', {reuse: true})).result, oracle, corpus, workflow.nativeCallsite, {mode: 'session'});
      assertReuse(changed, restored, {changed: true}); assert.equal(restored.inputIdentity, first.inputIdentity);
      return {scope: 'Local synthetic commit with one non-source note; all 4036 exact upstream documents retained', variant, actualMaterial, changed: changed.nativeSession, restored: restored.nativeSession};
    });
    await check('session-close-real-exit-and-new-owner', () => {
      assert(firstOwner); assert.equal(invoke('session-close', ['source', 'session', 'close']).result.state, 'closed');
      assert.equal(invoke('session-closed', ['source', 'session', 'status']).result.state, 'closed');
      const reopened = assertFullNative(invoke('session-reopened', relation('refs', {reuse: true})).result, oracle, corpus, workflow.nativeCallsite, {mode: 'session'});
      assert.equal(reopened.analysis.cacheHit, false); assert.equal(reopened.nativeSession.snapshotTransfer, 'full');
      reopenedOwner = diagnostic('session-reopened', reopened.nativeSession.workerPid); assert.notDeepEqual(reopenedOwner, firstOwner);
      return {firstOwner, reopenedOwner, closureProof: 'Public close waits for the authenticated held process handle to exit; outer Job independently reaps all descendants'};
    });
    await check('same-pin-recovery-after-native-lifecycle', () => {
      const result = invoke('query-after-native', fixed(['source', 'query', oracle.anchors[0].symbol, '--topic', 'lua', '--limit', '10'])).result;
      assertAnchor(result, oracle.anchors[0], oracle, pin);
    });
  } else {
    const {runSessionGroup} = await import('./session-ipc.mjs');
    await check(`real-ipc-${group}`, async () => await runSessionGroup({group, binary, home, release, scratch, evidenceDir: out, invoke, report, deadline}));
  }
} catch (error) { report.blocker = error.message; }
finally {
  if (!fatal && existsSync(home)) await check('final-public-session-close', () => {
    assert.equal(invoke('final-close', ['source', 'session', 'close'], {timeout: 10000}).result.state, 'closed');
    assert.equal(invoke('final-status', ['source', 'session', 'status'], {timeout: 10000}).result.state, 'closed');
  });
  report.fatalProcessFailure = fatal;
  report.finishedAt = new Date().toISOString(); report.wallMillis = performance.now() - started; report.outputBytes = outputBytes;
  report.state = !report.blocker && !fatal && report.checks.length > 1 && report.checks.every(row => row.state === 'passed') ? `passed_native_${group}` : 'failed';
  report.completeSourceAcceptance = false; report.remaining = ['same-machine fixed Go differential', 'paired independent cold/warm 20-repeat performance', 'other separate acceptance groups and complete product/Skill coverage']; save();
}
console.log(JSON.stringify({state: report.state, report: join(out, 'report.json'), passed: report.checks.filter(row => row.state === 'passed').length, failed: report.checks.filter(row => row.state === 'failed').length}));
if (report.state !== `passed_native_${group}`) process.exitCode = 1;
