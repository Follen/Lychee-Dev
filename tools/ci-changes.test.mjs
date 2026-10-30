import test from 'node:test';
import assert from 'node:assert/strict';
import { classify } from './ci-changes.mjs';

test('only non-product documentation skips product CI', () => {
  assert.equal(classify({files: ['DESIGN.md', 'handover.md', 'docs/usage.md']}).native, false);
  for (const path of [
    'crates/lycheedev-cli/README.md', 'tests/fixtures/source/README.md',
    'release/licenses/NOTICE.md', 'vendor/fixture/README.md',
    'skills/lycheedev/SKILL.md', 'packages/npm/lycheedev/README.md',
    'addon/README.md', '.github/workflows/ci.yml', 'tools/ci-changes.mjs',
  ]) assert.equal(classify({files: [path]}).native, true, path);
});

test('unknown, empty and mixed changes keep full product validation', () => {
  for (const files of [[], ['todo.md', 'Cargo.lock'], ['new-input.bin']]) {
    assert.equal(classify({files}).native, true);
  }
  assert.equal(classify({event: undefined}).native, true);
  assert.equal(classify({event: {}, name: 'workflow_dispatch'}).native, true);
});
