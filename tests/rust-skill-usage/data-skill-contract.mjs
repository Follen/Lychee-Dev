// Fixed Go is the semantic oracle, not the byte identity of the adapted Rust
// package. Authenticate the entire separately reviewed candidate, not a hash
// learned from the installed package or from the report being checked.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {inventory, sha256, within} from './assertions.mjs';

export {CURRENT_SKILL_MANIFEST as DATA_SKILL_MANIFEST} from './skill-identity.mjs';
export {loadReviewedSkillContract as loadDataSkillContract} from './skill-identity.mjs';
import {loadReviewedSkillContract as loadDataSkillContract, reviewedSkillIdentity} from './skill-identity.mjs';
const rowIdentity = ({path, bytes, sha256}) => ({path, bytes, sha256});

export function assertDataSkillInventory(packageFiles, identity) {
  const contract = loadDataSkillContract(identity);
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

export function assertDataSkillDirectory(directory, identity) {
  const contract = loadDataSkillContract(identity);
  assert.deepEqual(inventory(directory, {maxBytes: 4 << 20, maxFiles: 100}), contract.files.map(rowIdentity), 'exact candidate Skill source inventory');
  for (const row of contract.files) assertDataSkillBytes(readFileSync(within(directory, row.path)), row);
  return contract;
}

export function dataSkillIdentity(contract = loadDataSkillContract()) {
  return reviewedSkillIdentity(contract);
}
