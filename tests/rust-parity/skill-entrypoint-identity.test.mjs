// Internal machine oracle identities; packaged Skill instructions have no comparison preamble.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {CURRENT_GO_COMMIT as COMMAND_GO_COMMIT, CURRENT_FIXTURE} from './oracle-3.2.2.mjs';
const CURRENT_GO_COMMIT = '29bdbdaff5f064ec212ff2064380b1ccac652eda';
import {GO_COMMIT} from './oracle.mjs';
import {loadCurrentWorkflow} from '../rust-skill-usage/assertions.mjs';
const root = resolve(import.meta.dirname, '../..');
const read = path => readFileSync(resolve(root, path), 'utf8');
test('current internal workflow and frozen command oracle retain their exact machine identities', () => {
  const workflow = loadCurrentWorkflow(root);
  assert.equal(workflow.goContract, CURRENT_GO_COMMIT);
  assert.equal(workflow.goVersion, '3.2.4');
  const oracle = JSON.parse(read(CURRENT_FIXTURE));
  assert.equal(oracle.oracle.commit, COMMAND_GO_COMMIT);
  // The generated command table keeps its authenticated 3.2.2 identity.
  // Prove its source is unchanged in 3.2.4 rather than relabeling the fixture.
  const source = commit => execFileSync('git', ['--no-replace-objects', 'cat-file', 'blob', `${commit}:internal/command/command_contract.go`], {cwd: root, timeout: 10000, maxBuffer: 2 * 1024 * 1024});
  assert.deepEqual(source(CURRENT_GO_COMMIT), source(COMMAND_GO_COMMIT));
  assert.ok(read('tests/rust-skill-usage/baselines/go3.2.2-commands.md').includes(COMMAND_GO_COMMIT));
});
test('current internal workflow does not relabel the independently pinned historical fixture', () => {
  const oracle = JSON.parse(read(CURRENT_FIXTURE));
  const historical = readFileSync(resolve(root, 'tests/rust-parity/go-3.2.0.manifest.json'));
  assert.equal(JSON.parse(historical).oracle.commit, GO_COMMIT);
  assert.equal(oracle.historicalReference.commit, GO_COMMIT);
  assert.equal(oracle.historicalReference.fixtureSHA256, createHash('sha256').update(historical).digest('hex'));
  assert.notEqual(GO_COMMIT, CURRENT_GO_COMMIT);
});
