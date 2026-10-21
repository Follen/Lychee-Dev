// Fixed Skill identities shared by Source/Data checker and replay paths.
// This is test evidence, never shipped instructions or behavioral acceptance.
import assert from 'node:assert/strict';
import {lstatSync, readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const gitObject = (kind, bytes) => createHash('sha1').update(`${kind} ${bytes.length}\0`).update(bytes).digest('hex');
export const CURRENT_SKILL_MANIFEST = 'tests/rust-skill-usage/baselines/rust-go324-skill.json';
export const AGENT_CUTOVER_SKILL_MANIFEST = 'tests/rust-skill-usage/baselines/rust-agent-cutover-recovered-skill.json';
export const AGENT_NATIVE_SKILL_MANIFEST = 'tests/rust-skill-usage/baselines/rust-agent-native-skill.json';
export const SIMPLIFIED_SKILL_MANIFEST = 'tests/rust-skill-usage/baselines/rust-simplified-skill.json';
export const HISTORICAL_SKILL_MANIFEST = 'tests/rust-skill-usage/baselines/rust-candidate-skill.json';
const GO_COMMIT = '3766f9cacb52e93a803a626187e2a1d54a445468';
const GO_TREE = 'b9e162ace7abe7ea4a2feecef822283767eb1210';
const HISTORICAL_ADAPTED = ['SKILL.md', 'references/addon-validation.md', 'references/commands.md',
  'references/installation.md', 'references/source-preparation.md', 'references/source-relations.md',
  'references/source-research.md', 'references/source-value-flow.md'];
const CURRENT_ADAPTED = ['SKILL.md', 'references/addon-validation.md', 'references/asset-export.md',
  'references/commands.md', 'references/data-hotfix.md', 'references/data-investigation.md',
  'references/data-query-recipes.md', 'references/data-recovery.md', 'references/data-tables.md',
  'references/data-targets.md', 'references/error-diagnosis.md', 'references/installation.md',
  'references/live-investigation.md', 'references/live-probes.md', 'references/live-recovery.md',
  'references/live-startup.md', 'references/runtime-investigations.md', 'references/source-preparation.md',
  'references/source-relations.md', 'references/source-research.md', 'references/source-value-flow.md'];
const ADDED = ['references/rust-candidate-entry.md', 'references/source-candidate.md'];
const IDENTITIES = [
  {file: CURRENT_SKILL_MANIFEST, commit: 'bd8196aa68ba1570652bb9540a01bbcdb4628638', tree: '4eace74fd90ebc4cc12b4628bb58c6f74262ccb5', adapted: CURRENT_ADAPTED, goCommit: '29bdbdaff5f064ec212ff2064380b1ccac652eda', goManifest: 'go3.2.4-skill.json'},
  {file: AGENT_CUTOVER_SKILL_MANIFEST, commit: '75cd762e31df0dfd614c8687f9fd3fdb26b6a7f7', tree: 'c1768f92d55b454cafb202eb6c2835af1ecc2811', adapted: CURRENT_ADAPTED},
  {file: AGENT_NATIVE_SKILL_MANIFEST, commit: '3ae03c6bd7f13260629e2f92218f8d9e262b1ebb', tree: 'e1c9f992da62a048d24c62843636230783ff6928', adapted: CURRENT_ADAPTED},
  {file: SIMPLIFIED_SKILL_MANIFEST, commit: '6187ec6581ac9dc9301a26d79fa62a520214fe6d', tree: '231818eb48f81cf927542bb19d9a8964dfbe4baf', adapted: CURRENT_ADAPTED},
  {file: HISTORICAL_SKILL_MANIFEST, commit: '0002ce02bec5fb7c9c31ae40fd96eadf0da17dc6', tree: '5edd025cae923fb2235086f250862696db7a2483', adapted: HISTORICAL_ADAPTED},
];
function readManifest(path) {
  const url = new URL(`./baselines/${path.split('/').at(-1)}`, import.meta.url), stat = lstatSync(url);
  assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= 1 << 20, 'bounded fixed Skill manifest');
  return JSON.parse(readFileSync(url, 'utf8'));
}
export function reviewedSkillIdentity(contract) {
  return {goContract: contract.goContract, goTree: contract.goTree,
    candidateSourceCommit: contract.sourceCommit, candidateTree: contract.tree};
}
export function loadReviewedSkillContract(identity) {
  const selected = identity === undefined ? IDENTITIES[0] : IDENTITIES.find(row =>
    identity && identity.candidateSourceCommit === row.commit && identity.candidateTree === row.tree);
  assert(selected, 'unknown reviewed Skill identity');
  const candidate = readManifest(selected.file), go = readManifest(selected.goManifest ?? 'go3.2.3-skill.json');
  const goCommit = selected.goCommit ?? GO_COMMIT;
  assert.equal(candidate.schema, 'lycheedev.reviewed-rust-skill.v1');
  assert.equal(candidate.sourceCommit, selected.commit); assert.equal(candidate.sourcePath, 'skills/lycheedev');
  assert.equal(candidate.goContract, goCommit); assert.equal(go.commit, goCommit);
  assert.equal(candidate.goTree, GO_TREE); assert.equal(go.tree, GO_TREE); assert.equal(skillTreeIdentity(go.files), GO_TREE);
  assert.equal(candidate.tree, selected.tree); assert.equal(skillTreeIdentity(candidate.files), selected.tree);
  assert.deepEqual(candidate.files.map(row => row.path), [...go.files.map(row => row.path), ...ADDED].sort());
  const changed = [];
  for (const original of go.files) {
    const row = candidate.files.find(row => row.path === original.path);
    if (row.gitBlob === original.gitBlob) assert.deepEqual(row, original, 'unchanged fixed Go file identity');
    else changed.push(original.path);
  }
  assert.deepEqual(changed, selected.adapted, 'only this reviewed Skill adaptation');
  if (identity !== undefined) assert.deepEqual(identity, reviewedSkillIdentity(candidate), 'exact known Skill identity');
  return candidate;
}

// Reconstruct the original Git tree from all file identities. This authenticates
// the full inventory without fetching Go or executing its launcher in acceptance.
export function skillTreeIdentity(files) {
  const tree = {};
  for (const row of files) {
    assert.equal(row.mode, '100644', 'fixed Skill regular-file mode');
    assert.match(row.gitBlob, /^[a-f0-9]{40}$/); assert.match(row.sha256, /^[a-f0-9]{64}$/);
    assert(Number.isSafeInteger(row.bytes) && row.bytes > 0 && row.bytes <= 1 << 20);
    assert(typeof row.path === 'string' && row.path.split('/').every(part => /^[A-Za-z0-9_.-]+$/.test(part) && part !== '.' && part !== '..'), 'fixed Skill path');
    const parts = row.path.split('/'); let parent = tree;
    for (const part of parts.slice(0, -1)) {
      assert(!Object.hasOwn(parent, part) || typeof parent[part] === 'object', 'unique file/directory path');
      if (!Object.hasOwn(parent, part)) Object.defineProperty(parent, part, {value: {}, enumerable: true});
      parent = parent[part];
    }
    const name = parts.at(-1); assert(!Object.hasOwn(parent, name), 'unique fixed Skill path');
    Object.defineProperty(parent, name, {value: row.gitBlob, enumerable: true});
  }
  const digest = entries => gitObject('tree', Buffer.concat(Object.entries(entries)
    .sort(([a, av], [b, bv]) => Buffer.compare(Buffer.from(a + (typeof av === 'object' ? '/' : '')), Buffer.from(b + (typeof bv === 'object' ? '/' : ''))))
    .map(([name, value]) => Buffer.concat([Buffer.from(`${typeof value === 'object' ? '40000' : '100644'} ${name}\0`), Buffer.from(typeof value === 'object' ? digest(value) : value, 'hex')]))));
  return digest(tree);
}

