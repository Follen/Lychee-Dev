// Targeted public parity for the real selected encoded/decoded byte boundary.
import assert from 'node:assert/strict';
import {cpSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {isDeepStrictEqual as equal} from 'node:util';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {hashFile} from '../../tools/rust-build.mjs';

const [rustArg, fixtureArg, outArg] = process.argv.slice(2);
assert(rustArg && fixtureArg && outArg, 'usage: content-boundary-public.mjs <cli.exe> <fixture> <new-output>');
const fixture = resolve(fixtureArg), out = resolve(outArg);
assert(!existsSync(out)); mkdirSync(out, {recursive: true});
const identity = JSON.parse(readFileSync(join(fixture, 'input-identity.json')));
const boundary = JSON.parse(readFileSync(join(fixture, 'content-boundaries.json')));
const binaries = {go: resolve('.tmp/lycheedev-oracle.exe'), rust: resolve(rustArg)};
const processes = [], results = [], counters = [];
function execute(engine, home, maximum, format, name) {
  const evidence = join(out, name + '-' + engine); mkdirSync(evidence);
  const args = ['asset', 'inspect', '--file-id', String(boundary.fileDataID),
    '--max-bytes', String(maximum), '--snapshot', identity.snapshot,
    '--installation', identity.installation, '--home', home, '--format', format, '--offline'];
  const run = ownedSpawnSync(binaries[engine], args, {cwd: process.cwd(), evidence, timeout: 30000});
  processes.push({engine, args, exit: run.status, handlesReaped: run.handlesReaped, proof: run.proof});
  writeFileSync(join(evidence, 'stdout'), run.stdout); writeFileSync(join(evidence, 'stderr'), run.stderr);
  assert(!run.error && run.handlesReaped); assert.equal(run.stderr.toString(), '');
  const frames = run.stdout.toString().trim().split(/\r?\n/).map(JSON.parse);
  const envelope = format === 'json' ? frames[0] : frames.at(-1).envelope;
  if (format === 'jsonl') assert.deepEqual(frames.map(row => row.type), run.status === 0 ? ['begin', 'record', 'end'] : ['begin', 'error']);
  const normalized = structuredClone(envelope);
  if (envelope.ok) {
    assert.equal(envelope.result.content.bytes, boundary.decodedBytes);
    const content = join(home, 'blobs', envelope.result.content.sha256.slice(0, 2), envelope.result.content.sha256.slice(2));
    assert.equal(statSync(content).size, boundary.decodedBytes);
    assert.equal(hashFile(content), envelope.result.content.sha256);
    assert.equal(envelope.captures.length, 1);
    const capture = envelope.captures[0];
    assert.equal(capture.mediaType, 'application/octet-stream');
    assert(equal(capture.blob, envelope.result.content));
    normalized.captures[0].id = '<capture>'; normalized.captures[0].capturedAt = '<time>';
    counters.push({engine, name, cache: envelope.result.decodedCache, accounting: envelope.result.resourceAccounting});
    // Counters describe different implementations; preserve them separately.
    delete normalized.result.decodedCache;
    if (engine === 'rust') {
      assert.equal(envelope.result.verification.contentWholeDecodedCKey, true);
      assert.equal(envelope.result.verification.rootWholeDecodedCKey, true);
      delete normalized.result.verification; delete normalized.result.encodingIO;
      delete normalized.result.resourceAccounting;
    }
  }
  return {exit: run.status, envelope: normalized};
}
for (const test of boundary.cases) for (const format of ['json', 'jsonl']) for (const state of ['cold', 'warm']) {
  const name = `${test.name}-${format}-${state}`, runs = {};
  for (const engine of ['go', 'rust']) {
    const home = join(out, 'homes', name + '-' + engine); cpSync(identity.home, home, {recursive: true});
    const db = new DatabaseSync(join(home, 'state/toolkit.sqlite'));
    db.prepare("DELETE FROM documents WHERE key LIKE 'decoded-v1/%'").run(); db.close();
    if (state === 'warm') assert.equal(execute(engine, home, 536870912, 'json', name + '-prime').exit, 0);
    runs[engine] = execute(engine, home, test.maximum, format, name);
  }
  const passed = runs.go.exit === test.expectedExit && runs.rust.exit === test.expectedExit && equal(runs.go, runs.rust);
  results.push({name, passed, runs});
}
const passed = results.every(row => row.passed);
const report = {passed, results, counters, processes, scope: 'Public JSON/JSONL cold/warm encoded/decoded boundary; full stable envelopes and raw capture bytes'};
writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify({passed, cases: results.length, failed: results.filter(row => !row.passed).map(row => row.name), report: join(out, 'report.json')}));
if (!passed) process.exitCode = 1;
