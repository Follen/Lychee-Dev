// Independent acceptance helpers. Unit fixtures prove the checker, not the CLI.
import assert from 'node:assert/strict';
import {lstatSync, readFileSync, readdirSync, realpathSync} from 'node:fs';
import {isAbsolute, join, relative, resolve, sep} from 'node:path';
import {createHash} from 'node:crypto';
import {hashFile} from '../../tools/rust-build.mjs';

export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
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
export function assertNativeEvidence(result, oracle, callsite) {
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
  // Production intentionally removes nativeResearch.relations and projects
  // those rows into the bounded public page. Check the native-origin row there,
  // never an independently present static-extraction row at the same location.
  assert.equal(evidence.relations, undefined, 'native evidence does not bypass public pagination');
  assert(Array.isArray(result.relations), 'public native relation page');
  const rows = result.relations.filter(row => ['path', 'line', 'kind', 'target'].every(key => row[key] === callsite[key]) && row.basis?.includes('native_common_source_facts'));
  assert(rows.length > 0, 'exact original callsite must have native-origin evidence');
  for (const row of rows) {
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
