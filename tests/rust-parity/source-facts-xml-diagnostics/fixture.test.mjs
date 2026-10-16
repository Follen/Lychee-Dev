// Static fixture identity checks only, never a Go/Rust parser parity result.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const read = path => readFileSync(new URL(path, import.meta.url));
const sha = bytes => createHash('sha256').update(bytes).digest('hex');

test('the existing 52-case input and fixed-Go overlay remain byte-identical', () => {
  const input = read('../source-facts-malformed/cases.json');
  assert.equal(sha(input), 'feeed5637b4d3d3c6c979192b7b4a0ced17bd99f349945e1adae5810dcb4c36f');
  assert.equal(JSON.parse(input).cases.length, 52);
  assert.equal(sha(read('../source-facts-malformed/oracle_test.go')), '67efd745fa1c30a0f821bb847fe34b5d7515e3b2d95750a360f8b4311f2e816b');
});

test('the independent 32-case input includes raw newlines and Go tolerance controls', () => {
  const bytes = read('./cases.json');
  assert.equal(sha(bytes), 'ff830cc7690eae7b93e2867a4e6ae22d9789b5420a27cea6df93ddc0b8405c09');
  const input = JSON.parse(bytes);
  assert.equal(input.schema, 'lycheedev.source-facts-malformed-cases.v1');
  assert.equal(input.oracleCommit, '3766f9cacb52e93a803a626187e2a1d54a445468');
  assert.equal(input.cases.length, 32);
  assert.equal(new Set(input.cases.map(row => row.id)).size, 32);
  assert.ok(input.cases.every(row => row.path === 'Fixture.xml' && typeof row.source === 'string'));
  const cases = new Map(input.cases.map(row => [row.id, row.source]));
  assert.equal(cases.get('bang_comment_open_crlf'), '<Ui><!-\r\n-->');
  assert.equal(cases.get('end_name_leading_lf'), '<Ui></\nUi>');
  assert.equal(cases.get('namespace_empty_prefix_control'), '<Ui><:Frame name="F"></:Frame></Ui>');
  assert.equal(cases.get('namespace_empty_suffix_control'), '<Ui><Frame: name="F"></Frame:></Ui>');
  assert.equal(cases.get('attribute_no_space_control'), '<Ui><Frame name="F"inherits="Base"method="Shown"/></Ui>');
  assert.equal(cases.get('duplicate_attribute_no_space_control'), '<Ui><Frame name="Old"name="F"/></Ui>');
});
