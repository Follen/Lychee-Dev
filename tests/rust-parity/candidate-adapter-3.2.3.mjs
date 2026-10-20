// Separately reviewed candidate adapter. Historical oracle.mjs is unchanged.
import { RUST_FILES, readBounded, sha256, parseRegistrations, validateCommands,
  extractRustProducts, extractSourceTracks, addonInventory } from './oracle.mjs';

// Reviewed against c508b10's 640f49… contract: implemented() is byte-identical;
// takes_value adds only --data-derived=false. Remaining changes are Rust tests.
// A later source edit, including tests/comments, requires a fresh review.
export const HISTORICAL_CONTRACT_SHA256 = '66240b8ebb9d2920d7308e5bce38ff859c58fcb9192b57ee7ee1b015b9e836c8';
export const HISTORICAL_CONTRACT_BLOB = '1060a12cbd68e3913deb52c3d5610ee336751c99';
// Agent successor: implemented() is unchanged; the only new parser arm is
// --result-ttl-seconds=true. Full approved tests/metadata are separately pinned.
export const AGENT_CONTRACT_SHA256 = 'da8ed5418cf1de015e9f24c8012bd2eae3eb7ce0356cb4fb0f51d2dde744274f';
export const AGENT_CONTRACT_BLOB = 'f3f72aa6290c6f15298508ea0bbc9af80c6bf561';
// Protected Data inventory adds only --data=false to the production parser.
// The independently pinned Agent predecessor remains historical evidence.
export const INVENTORY_CONTRACT_SHA256 = 'a783204b5b9aabfd0a4de82be36e825f0e59b3af8da457d6f0ad8503e9a3e403';
export const INVENTORY_CONTRACT_BLOB = 'bbca7908fa1f3fe347b862b00e5839a9c0f547b1';
// Reconstructed Agent-only clap contract; full source requires independent review.
export const CANDIDATE_CONTRACT_SHA256 = '0b17cb207532c72e3c968584c63289645875b846d791cd3d992dd436f3c98f3f';
export const CANDIDATE_CONTRACT_BLOB = 'b279772481f7e9ec3d58be03530447345dca5569';
const reviewedContract = source => [HISTORICAL_CONTRACT_SHA256, AGENT_CONTRACT_SHA256, INVENTORY_CONTRACT_SHA256, CANDIDATE_CONTRACT_SHA256].includes(sha256(source));
const assert = (condition, message) => { if (!condition) throw new Error(message); };
export function extractCandidateAdvertised(inventory, texts) {
  if (!reviewedContract(texts.filter)) return { state: 'unverified', reason: 'contract.rs changed; review the static advertised-filter adapter', commands: null };
  const registrations = Object.fromEntries(['source', 'data', 'live', 'delivery', 'workspace', 'evidence', 'cache'].map(key => [key, parseRegistrations(texts[key].toString('utf8'))]));
  const commands = inventory.filter(d => {
    if (sha256(texts.filter) === CANDIDATE_CONTRACT_SHA256 && ['cli search','schema'].includes(d.path)) return true;
    if (['version', 'describe', 'project init', 'project status', 'project lock', 'target available'].includes(d.path)) return true;
    const family = d.path.startsWith('source ') ? 'source' : /^(data|asset) /.test(d.path) ? 'data' : d.path.startsWith('live ') ? 'live' : /^(addon|skill) /.test(d.path) || d.path === 'update' ? 'delivery' : null;
    return family ? registrations[family].includes(d.path) : ['workspace', 'evidence', 'cache'].some(key => registrations[key].includes(d.path));
  });
  return { state: 'static-extracted', commands, registrations, behavior: 'unverified: registration does not establish handler correctness or availability' };
}
// The whole containing source is authenticated before this narrow match adapter.
export function extractCandidateArities(source) {
  if (!reviewedContract(source)) return null;
  const text = source.toString('utf8');
  const body = text.match(/pub fn takes_value\(&self\) -> bool \{[\s\S]*?match self\.name\(\) \{([\s\S]*?)\n        \}/)?.[1];
  assert(body, 'explicit arity implementation missing');
  const pattern = /((?:\s*"--[a-z-]+"\s*\|?)+)\s*=>\s*(?:\{\s*(false|true)\s*\}|(false|true)),?/g;
  const arms = [...body.matchAll(pattern)];
  assert(arms.length === 2, 'explicit arity arms changed');
  const result = Object.create(null);
  for (const [, names, braced, plain] of arms) for (const [, name] of names.matchAll(/"(--[a-z-]+)"/g)) {
    assert(!Object.hasOwn(result, name), 'duplicate explicit arity');
    result[name] = (braced ?? plain) === 'true';
  }
  assert(body.replace(pattern, '').trim() === 'name => panic!("flag missing explicit contract arity: {name}"),', 'unreviewed arity fallback');
  return result;
}
export function loadRelease323Candidate(root) {
  const texts = Object.fromEntries(Object.entries(RUST_FILES).map(([key, path]) => [key, readBounded(root, path)]));
  const inventory = JSON.parse(texts.inventory.toString('utf8'));
  validateCommands(inventory);
  return {
    inventory, advertised: extractCandidateAdvertised(inventory, texts), arities: extractCandidateArities(texts.filter),
    products: extractRustProducts(texts.products.toString('utf8')),
    sourceTracks: extractSourceTracks(texts.sourceCatalog.toString('utf8')), addon: addonInventory(root),
    sources: Object.entries(RUST_FILES).map(([key, path]) => ({ path, bytes: texts[key].length, sha256: sha256(texts[key]) })),
  };
}
