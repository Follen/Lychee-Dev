// Identity and coverage checks only. This does not execute either parser.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const read = path => readFileSync(new URL(path, import.meta.url));
const sha = bytes => createHash('sha256').update(bytes).digest('hex');

test('generated additions preserve both original corpora and the fixed-Go overlay', () => {
  assert.equal(sha(read('../source-facts-malformed/cases.json')), 'feeed5637b4d3d3c6c979192b7b4a0ced17bd99f349945e1adae5810dcb4c36f');
  assert.equal(sha(read('../source-facts-xml-diagnostics/cases.json')), 'ff830cc7690eae7b93e2867a4e6ae22d9789b5420a27cea6df93ddc0b8405c09');
  assert.equal(sha(read('../source-facts-malformed/oracle_test.go')), '67efd745fa1c30a0f821bb847fe34b5d7515e3b2d95750a360f8b4311f2e816b');
});

test('all 20 complete generated inputs use the pinned oracle and original worker limit', () => {
  const bytes = read('./cases.json');
  assert.equal(sha(bytes), 'f26bddac5eeb449da21f748e9dda2c1acee626223974098298cfc8994458aa36');
  const fixture = JSON.parse(bytes);
  assert.equal(fixture.schema, 'lycheedev.source-facts-malformed-cases.v1');
  assert.equal(fixture.oracleCommit, '3766f9cacb52e93a803a626187e2a1d54a445468');
  assert.equal(fixture.cases.length, 20);
  assert.equal(new Set(fixture.cases.map(row => row.id)).size, 20);
  for (const row of fixture.cases) {
    assert.equal(row.path, `Fixture/Blizzard_APIDocumentationGenerated/${row.id}.lua`);
    assert.equal(typeof row.source, 'string');
    assert.ok(Buffer.byteLength(row.source) <= 64 * 1024);
  }
});

test('first-invalid, repeated roots and potential historical differences stay explicit', () => {
  const cases = new Map(JSON.parse(read('./cases.json')).cases.map(row => [row.id, row.source]));
  assert.ok(cases.get('first_invalid_type_masks_system').includes('Type=false, Type="System"'));
  assert.ok(cases.get('first_invalid_name_masks_later').includes('Name=false,Name="Hidden"'));
  assert.ok(cases.get('first_invalid_section_masks_later').includes('Functions=false,Functions='));
  assert.ok(cases.get('nested_system_repeated_roots').includes('Namespace="C_Inner"'));
  assert.ok(cases.get('parenthesized_field_and_entry').includes('({Type="Function",Name="Ignored"})'));
  assert.ok(cases.get('unsupported_clears_all').includes('return 1 // 2'));
});


test('independent boundary inputs pin complete current contracts without replacing original 20', () => {
  const bytes = read('./boundary-cases.json');
  assert.equal(sha(bytes), '8c557b8581fbfe8644b134fb98abef10aa1a27139314705b684fd1ac4e7abb9b');
  const fixture = JSON.parse(bytes);
  assert.equal(fixture.schema, 'lycheedev.source-facts-malformed-cases.v1');
  assert.equal(fixture.oracleCommit, '3766f9cacb52e93a803a626187e2a1d54a445468');
  assert.equal(fixture.cases.length, 11);
  assert.equal(new Set(fixture.cases.map(row => row.id)).size, 11);
  const originalIds = new Set(JSON.parse(read('./cases.json')).cases.map(row => row.id));
  for (const row of fixture.cases) {
    assert.ok(!originalIds.has(row.id));
    assert.equal(row.path, `Fixture/Blizzard_APIDocumentationGenerated/${row.id}.lua`);
    assert.ok(Buffer.byteLength(row.source) <= 64 * 1024);
    assert.deepEqual(Object.keys(row.expectedRustFacts), ['declarations', 'relationships', 'loads', 'headers', 'diagnostics']);
  }
  assert.equal(fixture.cases.filter(row => row.expectedRustFacts.diagnostics.length === 0).length, 6);
  assert.equal(fixture.cases.filter(row => row.expectedRustFacts.diagnostics.length === 1).length, 5);
});

test('historical generated reference function bodies remain byte-identical', () => {
  const source = read('../../../crates/lycheedev-source/src/lua_tree/generated_tests.rs').toString('utf8');
  const start = source.indexOf('impl SyntaxDocument {');
  assert.ok(start >= 0);
  assert.equal(sha(source.slice(start)), '01ced830a4fec6eadd4872133d4a4483bf743125ca5dedc6d7f39e987410056f');
});
