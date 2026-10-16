// Compare only the five recorded private-root observations. No process starts here.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {resolve} from 'node:path';

export const NAMES = Object.freeze(['example.test', 'example.test.', 'example.test..', 'example.test...', 'wrong.test']);
export const MARKER = 'LYCHEE_TLS_NAME_OBSERVATION ';
export function rustObservations(stdout) {
  assert(Buffer.byteLength(stdout, 'utf8') <= 16 * 1024 * 1024, 'Rust stdout budget');
  return stdout.split(/\r?\n/).flatMap(line => {
    const at = line.indexOf(MARKER);
    return at < 0 ? [] : [JSON.parse(line.slice(at + MARKER.length))];
  });
}
export function compare(go, rust) {
  for (const [label, rows] of [['go', go], ['rust', rust]]) {
    assert(Array.isArray(rows), `${label}: array required`);
    assert.equal(rows.length, NAMES.length, `${label}: all finite cases required`);
    assert.deepEqual(rows.map(row => row.input).sort(), [...NAMES].sort());
    for (const row of rows) {
      assert.equal(row.trust, 'private test root only');
      assert.equal(typeof row.accepted, 'boolean');
      assert.equal(typeof row.sni, 'string');
    }
  }
  const differences = [];
  for (const name of NAMES) {
    const g = go.find(row => row.input === name), r = rust.find(row => row.input === name);
    assert.equal(g.goVersion, 'go1.27.1', 'use the fixed official Go toolchain');
    assert.equal(g.goos, 'windows', 'Windows observation required');
    assert.equal(r.os, 'windows', 'Windows Rust observation required');
    for (const field of ['sni', 'accepted']) {
      if (g[field] !== r[field]) differences.push({input: name, field, go: g[field], rust: r[field]});
    }
    if (!g.accepted && g.phase !== 'tls') differences.push({input: name, stage: 'go', phase: g.phase});
    if (!r.accepted && r.error?.details?.phase !== 'tls') differences.push({input: name, stage: 'rust', error: r.error});
    if (r.stop !== null) differences.push({input: name, stage: 'rust-control', stop: r.stop});
  }
  return {schema: 'lycheedev.private-tls-name-comparison.v1', cases: NAMES.length, differences,
    scope: 'private-root loopback; not Windows default-system-root verification'};
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const [goFile, rustFile] = process.argv.slice(2);
  assert(goFile && rustFile, 'usage: node compare.mjs go-results.json rust-stdout.bin');
  const result = compare(JSON.parse(fs.readFileSync(goFile, 'utf8')), rustObservations(fs.readFileSync(rustFile, 'utf8')));
  console.log(JSON.stringify(result, null, 2));
  if (result.differences.length) process.exitCode = 1;
}
