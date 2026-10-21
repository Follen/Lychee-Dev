// Current target declarations only. Historical oracle evidence keeps its own pin.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {CURRENT_GO_COMMIT as COMMAND_GO_COMMIT, CURRENT_FIXTURE} from './oracle-3.2.2.mjs';
const CURRENT_GO_COMMIT = '29bdbdaff5f064ec212ff2064380b1ccac652eda';
import {GO_COMMIT} from './oracle.mjs';
const root = resolve(import.meta.dirname, '../..');
const read = path => readFileSync(resolve(root, path), 'utf8');
const entries = [
  ['skills/lycheedev/SKILL.md', /This Rust migration candidate reuses the Go ([^\n]+) workflow contract at\n`([0-9a-f]{40})`\./g],
  ['skills/lycheedev/references/source-candidate.md', /This candidate follows the fixed Go ([^\n]+) Skill at\n`([0-9a-f]{40})` and complete Lycheedev mailbox\nprotocol v1 contract\./g],
  ['skills/lycheedev/references/rust-candidate-entry.md', /reference tree as the fixed Go ([^\n]+) workflow at\n`([0-9a-f]{40})`\. Read that entry, then the relevant\nordinary workflow\./g],
];
function checkEntry(text, pattern) {
  const declarations = [...text.matchAll(pattern)];
  assert.equal(declarations.length, 1, 'exactly one active target declaration');
  assert.equal(declarations[0][1], '3.2.4', 'current released Go version');
  assert.equal(declarations[0][2], CURRENT_GO_COMMIT, 'current peeled release commit');
}
test('all three active Skill entrypoints name the exact current machine oracle', () => {
  const oracle = JSON.parse(read(CURRENT_FIXTURE));
  assert.equal(oracle.oracle.commit, COMMAND_GO_COMMIT);
  // The generated command table keeps its authenticated 3.2.2 identity.
  // Prove its source is unchanged in 3.2.4 rather than relabeling the fixture.
  const source = commit => execFileSync('git', ['--no-replace-objects', 'cat-file', 'blob', `${commit}:internal/command/command_contract.go`], {cwd: root, timeout: 10000, maxBuffer: 2 * 1024 * 1024});
  assert.deepEqual(source(CURRENT_GO_COMMIT), source(COMMAND_GO_COMMIT));
  for (const [path, pattern] of entries) checkEntry(read(path), pattern);
  assert.ok(read('tests/rust-skill-usage/baselines/go3.2.2-commands.md').includes(COMMAND_GO_COMMIT));
});
test('every entrypoint rejects stale, mixed, missing or duplicate target identity', () => {
  for (const [path, pattern] of entries) {
    const text = read(path);
    for (const changed of [
      text.replace('Go 3.2.4', 'Go 3.2.0'),
      text.replace('Go 3.2.4', 'Go 3.2.2'),
      text.replace('Go 3.2.4', 'Go 3.2.3'),
      text.replace(CURRENT_GO_COMMIT, '3766f9cacb52e93a803a626187e2a1d54a445468'),
      text.replace(CURRENT_GO_COMMIT, COMMAND_GO_COMMIT),
      text.replace(CURRENT_GO_COMMIT, GO_COMMIT),
      text.replace(CURRENT_GO_COMMIT, 'f'.repeat(40)),
      text.replace(CURRENT_GO_COMMIT, ''),
      `${text}\n${text}`,
    ]) assert.throws(() => checkEntry(changed, pattern), undefined, path);
  }
});
test('entrypoint alignment does not relabel the independently pinned historical fixture', () => {
  const oracle = JSON.parse(read(CURRENT_FIXTURE));
  const historical = readFileSync(resolve(root, 'tests/rust-parity/go-3.2.0.manifest.json'));
  assert.equal(JSON.parse(historical).oracle.commit, GO_COMMIT);
  assert.equal(oracle.historicalReference.commit, GO_COMMIT);
  assert.equal(oracle.historicalReference.fixtureSHA256, createHash('sha256').update(historical).digest('hex'));
  assert.notEqual(GO_COMMIT, CURRENT_GO_COMMIT);
});
