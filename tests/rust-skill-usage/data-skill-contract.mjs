// Fixed Go is the semantic oracle, not the byte identity of the adapted Rust
// package. Authenticate the entire separately reviewed candidate, not a hash
// learned from the installed package or from the report being checked.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {CURRENT_GO_CONTRACT, inventory, jsonFile, sha256, skillTreeIdentity, within} from './assertions.mjs';

export const DATA_SKILL_MANIFEST = 'tests/rust-skill-usage/baselines/rust-candidate-skill.json';
const SOURCE_COMMIT = '0002ce02bec5fb7c9c31ae40fd96eadf0da17dc6';
const CANDIDATE_TREE = '5edd025cae923fb2235086f250862696db7a2483';
const GO_TREE = 'b9e162ace7abe7ea4a2feecef822283767eb1210';
// These are existing, reviewed engine/installation and acceptance-boundary
// adaptations. All Data, Live, launcher and other original bytes stay fixed.
const ADAPTED = ['SKILL.md', 'references/addon-validation.md', 'references/commands.md',
  'references/installation.md', 'references/source-preparation.md',
  'references/source-relations.md', 'references/source-research.md', 'references/source-value-flow.md'];
const ADDED = ['references/rust-candidate-entry.md', 'references/source-candidate.md'];
const rowIdentity = ({path, bytes, sha256}) => ({path, bytes, sha256});

export function loadDataSkillContract() {
  const candidate = jsonFile(new URL('./baselines/rust-candidate-skill.json', import.meta.url));
  const go = jsonFile(new URL('./baselines/go3.2.3-skill.json', import.meta.url));
  assert.equal(candidate.schema, 'lycheedev.reviewed-rust-skill.v1');
  assert.equal(candidate.sourceCommit, SOURCE_COMMIT); assert.equal(candidate.sourcePath, 'skills/lycheedev');
  assert.equal(candidate.goContract, CURRENT_GO_CONTRACT); assert.equal(go.commit, CURRENT_GO_CONTRACT);
  assert.equal(candidate.goTree, GO_TREE); assert.equal(go.tree, GO_TREE); assert.equal(skillTreeIdentity(go.files), GO_TREE);
  assert.equal(candidate.tree, CANDIDATE_TREE); assert.equal(skillTreeIdentity(candidate.files), CANDIDATE_TREE);
  assert.deepEqual(candidate.files.map(row => row.path), [...go.files.map(row => row.path), ...ADDED].sort());
  const changed = [];
  for (const original of go.files) {
    const selected = candidate.files.find(row => row.path === original.path);
    if (selected.gitBlob === original.gitBlob) assert.deepEqual(selected, original, 'unchanged fixed Go file identity');
    else changed.push(original.path);
  }
  assert.deepEqual(changed, ADAPTED, 'only the reviewed migration adaptations');
  return candidate;
}

export function assertDataSkillInventory(packageFiles) {
  const contract = loadDataSkillContract();
  assert.deepEqual(packageFiles.filter(row => row.path.startsWith('payload/skill/'))
    .map(row => ({...row, path: row.path.slice('payload/skill/'.length)})),
  contract.files.map(rowIdentity), 'entire installed Skill equals independently fixed candidate');
  return contract;
}

export function assertDataSkillBytes(bytes, expected) {
  assert.equal(bytes.length, expected.bytes, `candidate Skill bytes: ${expected.path}`);
  assert.equal(sha256(bytes), expected.sha256, `candidate Skill SHA256: ${expected.path}`);
  const blob = createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
  assert.equal(blob, expected.gitBlob, `candidate Skill original Git blob: ${expected.path}`);
}

export function assertDataSkillDirectory(directory) {
  const contract = loadDataSkillContract();
  assert.deepEqual(inventory(directory, {maxBytes: 4 << 20, maxFiles: 100}), contract.files.map(rowIdentity), 'exact candidate Skill source inventory');
  for (const row of contract.files) assertDataSkillBytes(readFileSync(within(directory, row.path)), row);
  return contract;
}

export function dataSkillIdentity(contract = loadDataSkillContract()) {
  return {goContract: contract.goContract, goTree: contract.goTree,
    candidateSourceCommit: contract.sourceCommit, candidateTree: contract.tree};
}
