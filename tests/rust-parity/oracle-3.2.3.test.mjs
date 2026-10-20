// Pure static oracle fault tests. No Rust, Windows, or handler execution claim.
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { RUST_FILES, LIMITS, sha256, readBounded, extractArities, extractAdvertised } from './oracle.mjs';
import { CANDIDATE_CONTRACT_BLOB, CANDIDATE_CONTRACT_SHA256, HISTORICAL_CONTRACT_BLOB, HISTORICAL_CONTRACT_SHA256, extractCandidateArities,
  extractCandidateAdvertised, loadRelease323Candidate } from './candidate-adapter-3.2.3.mjs';
import { GO_323, RELEASE_323_FIELDS, buildRelease323Fixture, compareRelease323 } from './oracle-3.2.3.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const oracle = buildRelease323Fixture(root);
const contract = execFileSync('git', ['--no-replace-objects', 'cat-file', 'blob', CANDIDATE_CONTRACT_BLOB],
  {cwd: root, maxBuffer: LIMITS.fileBytes, timeout: 10000});
const clone = value => structuredClone(value);
function candidate() {
  const inventory = oracle.acceptedCommands.map(({parserFlags, ...row}) => clone(row));
  return {inventory, arities: clone(oracle.expectedArities), advertised: {state: 'static-extracted', commands: clone(inventory)},
    products: oracle.products.map(p => ({id: p.product, family: p.family, catalog_product: p.productCode, folder: p.folder,
      series: p.buildSeries, reused_slot: p.reusedSlot})),
    addon: oracle.addon.map(row => {
      const allowed = RELEASE_323_FIELDS.find(allowed => allowed.path === row.path);
      return allowed ? {path: row.path, ...allowed.retained} : clone(row);
    }), sourceTracks: {forever: 'forever'}};
}
function fails(mutate) {
  const c = candidate(); mutate(c);
  assert.notEqual(compareRelease323(oracle, c).acceptedStaticStatus, 'pass');
}
test('fixed 3.2.3 source provenance retains 84 original commands and exact independent deltas', () => {
  assert.equal(oracle.oracle.commit, GO_323.commit);
  assert.equal(oracle.commands.length, 84); assert.equal(oracle.acceptedCommands.length, 87);
  assert.equal(oracle.products.length, 7); assert.equal(oracle.addon.length, 55);
  assert.equal(oracle.provenance.commandCount, 84);
  assert.equal(oracle.provenance.historicalFixtureSHA256, sha256(readBounded(root, 'tests/rust-parity/go-3.2.0.manifest.json')));
  const c = candidate(), before = clone(c), report = compareRelease323(oracle, c);
  assert.equal(report.acceptedStaticStatus, 'pass'); assert.equal(report.rawExactStatus, 'fail');
  assert.deepEqual(report.rawExact.commands.extra, ['data agent schema', 'data agent serve', 'data cache prepare']);
  assert.equal(report.behavioralParity, 'unverified'); assert.equal(report.execution, 'NOT_RUN');
  assert.deepEqual(c, before);
});
test('reviewed current parser contains exactly frozen 115 arities plus Data prune and Agent TTL', () => {
  assert.equal(sha256(contract), CANDIDATE_CONTRACT_SHA256);
  const arities = extractCandidateArities(contract);
  assert.equal(Object.keys(arities).length, 117); assert.equal(arities['--data-derived'], false);
  assert.deepEqual(arities, oracle.expectedArities);
  assert.equal(extractArities(contract), null, 'historical adapter must not accept later candidate');
  assert.equal(extractAdvertised([], {filter: contract}).state, 'unverified');
});
test('current advertised adapter preserves family routing, builtins, order and inventory intersection', () => {
  const registrations = {
    source: ['source approved', 'registered only'], data: ['data approved', 'asset approved'], live: ['live approved'],
    delivery: ['addon approved', 'skill approved', 'update', 'update extra'],
    workspace: ['target approved', 'source', 'data', 'asset', 'live', 'addon', 'skill',
      'source wrong-family', 'data wrong-family', 'asset wrong-family', 'live wrong-family', 'addon wrong-family', 'skill wrong-family'],
    evidence: ['evidence approved'], cache: ['cache approved'],
  };
  const texts = {filter: contract};
  for (const [key, values] of Object.entries(registrations)) texts[key] = Buffer.from(`pub fn commands() { &[${values.map(JSON.stringify).join(',')}] }`);
  const accepted = ['version', 'describe', 'project init', 'project status', 'project lock', 'target available',
    'source approved', 'data approved', 'asset approved', 'live approved', 'addon approved', 'skill approved', 'update',
    'target approved', 'evidence approved', 'cache approved', 'source', 'data', 'asset', 'live', 'addon', 'skill'];
  const rejected = ['source wrong-family', 'data wrong-family', 'asset wrong-family', 'live wrong-family',
    'addon wrong-family', 'skill wrong-family', 'update extra', 'not registered'];
  const inventory = [...accepted, ...rejected].reverse().map(path => ({path}));
  const actual = extractCandidateAdvertised(inventory, texts);
  assert.equal(actual.state, 'static-extracted');
  assert.deepEqual(actual.commands.map(row => row.path), [...accepted].reverse());
  assert.deepEqual(actual.registrations, registrations);
  texts.delivery = Buffer.from('pub fn commands() { &[] }');
  assert.ok(!extractCandidateAdvertised(inventory, texts).commands.some(row => row.path === 'update'));
});
test('all complete-source changes invalidate both current adapters, including tests and comments', () => {
  const source = contract.toString('utf8');
  const mutations = [
    source.replace('"--data-derived" | ', '').replace('"--after-id"', '"--data-derived" | "--after-id"'),
    source.replace('"--data-derived" | ', ''),
    source.replace('"target available" => true,', '"target available" => false,'),
    source.replace('"--allow-partial"', '"--unreviewed" | "--allow-partial"'),
    source.replace('name => panic!("flag missing explicit contract arity: {name}"),', '_ => false,'),
    source.replace('assert_eq!(current.len(), 87);', 'assert_eq!(current.len(), 84);'),
    source + '\n// unreviewed trailing edit\n',
  ];
  for (const mutated of mutations) {
    assert.notEqual(mutated, source);
    const filter = Buffer.from(mutated);
    assert.equal(extractCandidateArities(filter), null);
    assert.equal(extractCandidateAdvertised([], {filter}).state, 'unverified');
  }
});
test('all 117 parser flags, advertised omissions, profile fields and addon bytes remain gates', () => {
  for (const [flag, value] of Object.entries(oracle.expectedArities)) fails(c => { c.arities[flag] = !value; });
  fails(c => { c.arities['--unreviewed'] = false; });
  fails(c => { c.arities = null; c.advertised = {state: 'unverified', commands: null}; });
  fails(c => { c.advertised.commands.pop(); });
  for (const field of ['id', 'family', 'catalog_product', 'folder', 'series', 'reused_slot']) fails(c => { c.products[0][field] = 'wrong'; });
  fails(c => { c.products.pop(); });
  fails(c => { c.addon.pop(); });
  fails(c => { c.addon.push({path: 'addon/extra.lua', bytes: 0, sha256: '0'.repeat(64)}); });
  for (const row of oracle.addon) for (const field of ['bytes', 'sha256']) fails(c => {
    c.addon.find(item => item.path === row.path)[field] = field === 'bytes' ? 0 : '0'.repeat(64);
  });
});
test('exact CLI fields, additions, flag order, command order and unknown properties cannot be normalized', () => {
  for (let i = 0; i < oracle.acceptedCommands.length; i++) {
    for (const field of ['summary', 'mutates', 'arguments', 'flags']) fails(c => {
      c.inventory[i][field] = field === 'mutates' ? !c.inventory[i][field] : field === 'summary' ? 'wrong' : field === 'flags' ? [] : ['wrong'];
    });
    fails(c => { c.inventory[i].unapproved = true; });
  }
  fails(c => { c.inventory.reverse(); });
  fails(c => { c.inventory.splice(c.inventory.findIndex(row => row.path === 'data cache prepare'), 1); });
  fails(c => { c.inventory.find(row => row.path === 'cache prune').flags.pop(); });
  fails(c => { c.inventory.find(row => row.path === 'cache prune').flags.reverse(); });
});
test('candidate loader authenticates its own worktree, never substituting reviewed bytes', () => {
  const temp = mkdtempSync(join(tmpdir(), 'lycheedev-current-contract-'));
  try {
    for (const path of Object.values(RUST_FILES)) {
      mkdirSync(dirname(join(temp, path)), {recursive: true});
      writeFileSync(join(temp, path), readBounded(root, path));
    }
    mkdirSync(join(temp, 'addon')); writeFileSync(join(temp, 'addon/fixture.lua'), 'return "not executed"');
    const changed = Buffer.concat([contract, Buffer.from('\n// changed\n')]);
    writeFileSync(join(temp, RUST_FILES.filter), changed);
    const c = loadRelease323Candidate(temp);
    assert.equal(c.sources.find(row => row.path === RUST_FILES.filter).sha256, sha256(changed));
    assert.equal(c.advertised.state, 'unverified'); assert.equal(c.advertised.commands, null); assert.equal(c.arities, null);
  } finally { rmSync(temp, {recursive: true, force: true}); }
});
test('current CLI rejects invalid invocation without relaxing historical runners', () => {
  const result = spawnSync(process.execPath, ['tests/rust-parity/run-3.2.3.mjs', '--accept-gaps'], {cwd: root, encoding: 'utf8', timeout: 20000});
  assert.equal(result.status, 2); assert.match(result.stderr, /Usage:/);
});

test('historical parser custody remains distinct from the Agent successor', () => {
  const bytes = execFileSync('git', ['--no-replace-objects', 'cat-file', 'blob', HISTORICAL_CONTRACT_BLOB], {cwd:root});
  assert.equal(sha256(bytes), HISTORICAL_CONTRACT_SHA256);
  const arities = extractCandidateArities(bytes);
  assert.equal(Object.keys(arities).length, 116);
  assert.equal(Object.hasOwn(arities, '--result-ttl-seconds'), false);
  assert.notDeepEqual(arities, oracle.expectedArities);
  assert.equal(extractCandidateArities(Buffer.concat([bytes, Buffer.from('\n// tampered') ])), null);
});
